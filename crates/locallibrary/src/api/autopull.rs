//! Saving drives into the store as they're plugged in (`locallibrary watch`, RPC `watch.start`).
//!
//! rekordbox deletes the history lists players recorded on a drive when it imports them, so a
//! snapshot in the store is the only copy that lasts. [`AutoPull`] looks at each drive a
//! [`VolumeWatcher`] sees plugged in, or whose databases change, once it has stopped changing.
//! A drive of a library in the store holding a state the store lacks is pulled; with
//! `sync_set`, the library's other mounted drives are then synced from it when it's the newest
//! with confidence. A drive back from a set after a newer export was pulled is saved beside the
//! library's newest snapshot ([`crate::sync::pull_beside`]). It never starts a library, and
//! leaves archive and retired drives alone.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::Serialize;

use super::{SyncManyRequest, TargetResult};
use crate::compare::{self, Relation, Side};
use crate::device::Device;
use crate::fleet::{self, Role};
use crate::hash::Hash;
use crate::progress::Ctx;
use crate::store::{LibraryRecord, Snapshot, SnapshotInfo, Store};
use crate::sync::{self, PullOptions};
use crate::watch::{DriveChange, MountedDrive, VolumeWatcher, WatchEvent};
use crate::{Error, Result};

/// How long a drive's databases must stay unchanged before it's looked at: rekordbox, a player
/// or another program may still be writing it.
pub const WATCH_SETTLE: Duration = Duration::from_secs(5);

/// How often drives are polled.
pub const WATCH_POLL_EVERY: Duration = Duration::from_secs(1);

/// How long `watch --once` waits for another operation (the TUI, the RPC sidecar, a pull) to let
/// go of the store or a drive before giving up.
pub const WATCH_ONCE_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy)]
pub struct WatchOptions {
    /// Pull drives holding a state the store lacks; without it, only say so.
    pub pull: bool,
    /// After a drive is pulled (or found saved), sync its library's other mounted drives from it
    /// when it's the newest with confidence.
    pub sync_set: bool,
    /// See [`WATCH_SETTLE`].
    pub settle: Duration,
}

impl Default for WatchOptions {
    fn default() -> Self {
        WatchOptions { pull: false, sync_set: false, settle: WATCH_SETTLE }
    }
}

/// What watching did about a drive.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "event")]
pub enum WatchAction {
    /// The store holds the drive's state already.
    #[serde(rename = "watch.up_to_date")]
    UpToDate {
        #[serde(flatten)]
        drive: MountedDrive,
        library: String,
    },
    /// The drive holds a state the store lacks, and it wasn't pulled (watching without `pull`).
    #[serde(rename = "watch.pending")]
    Pending {
        #[serde(flatten)]
        drive: MountedDrive,
        library: String,
        /// History lists the store lacks.
        history_lists: Vec<String>,
        /// It would be saved beside the library's newest snapshot (back from a set).
        beside: bool,
    },
    #[serde(rename = "watch.pulled")]
    Pulled {
        #[serde(flatten)]
        drive: MountedDrive,
        library: String,
        snapshot: Box<SnapshotInfo>,
        /// History lists the store lacked.
        history_lists: Vec<String>,
        /// Saved beside the library's newest snapshot, which stays the newest.
        beside: bool,
    },
    /// The library's other mounted drives were synced from this one.
    #[serde(rename = "watch.synced")]
    Synced {
        #[serde(flatten)]
        drive: MountedDrive,
        library: String,
        targets: Vec<TargetResult>,
    },
    /// Another operation holds the store or the drive: it's looked at again once that's done.
    /// Said once per wait.
    #[serde(rename = "watch.waiting")]
    Waiting {
        #[serde(flatten)]
        drive: MountedDrive,
        reason: String,
    },
    /// Left alone, and why.
    #[serde(rename = "watch.skipped")]
    Skipped {
        #[serde(flatten)]
        drive: MountedDrive,
        reason: String,
    },
    #[serde(rename = "watch.failed")]
    Failed {
        #[serde(flatten)]
        drive: MountedDrive,
        kind: &'static str,
        message: String,
    },
}

impl WatchAction {
    pub fn drive(&self) -> &MountedDrive {
        match self {
            WatchAction::UpToDate { drive, .. }
            | WatchAction::Pending { drive, .. }
            | WatchAction::Pulled { drive, .. }
            | WatchAction::Synced { drive, .. }
            | WatchAction::Waiting { drive, .. }
            | WatchAction::Skipped { drive, .. }
            | WatchAction::Failed { drive, .. } => drive,
        }
    }
}

/// Something watching noticed or did, as it happens.
#[derive(Debug, Clone, Copy)]
pub enum Watched<'a> {
    Seen(&'a WatchEvent),
    Did(&'a WatchAction),
}

/// Decides which drives to look at, and looks at them.
#[derive(Debug, Clone)]
pub struct AutoPull {
    pub options: WatchOptions,
    /// Drives to look at, and when they'll have settled.
    due: BTreeMap<PathBuf, Instant>,
    /// Drives found held by another operation, and since when.
    waiting: BTreeMap<PathBuf, Instant>,
    /// How long to wait for a drive another operation holds before failing; `None` waits as long
    /// as it takes.
    pub patience: Option<Duration>,
}

impl AutoPull {
    pub fn new(options: WatchOptions) -> AutoPull {
        AutoPull { options, due: BTreeMap::new(), waiting: BTreeMap::new(), patience: None }
    }

    /// Look at these drives (the ones mounted when watching starts) once they've settled, as
    /// for a drive plugged in: rekordbox may be exporting to one as watching starts.
    pub fn check_now(&mut self, drives: &[MountedDrive]) {
        let settled = Instant::now() + self.options.settle;
        for d in drives.iter().filter(|d| d.has_export) {
            self.due.insert(d.root.clone(), settled);
        }
    }

    /// Note what a poll found: a drive plugged in, or whose databases changed, is looked at once
    /// it has settled; one unplugged is forgotten.
    pub fn observe(&mut self, events: &[WatchEvent]) {
        let settled = Instant::now() + self.options.settle;
        for e in events {
            match e {
                WatchEvent::Attached(d) if d.has_export => {
                    self.due.insert(d.root.clone(), settled);
                }
                WatchEvent::Changed { drive, changed }
                    if drive.has_export && changed.contains(&DriveChange::Databases) =>
                {
                    self.due.insert(drive.root.clone(), settled);
                }
                WatchEvent::Detached(d) => {
                    self.due.remove(&d.root);
                    self.waiting.remove(&d.root);
                }
                _ => {}
            }
        }
    }

    /// Whether any drive is waiting to be looked at.
    pub fn is_idle(&self) -> bool {
        self.due.is_empty()
    }

    /// Looks at the drives that have settled, among `mounted`, with the store at `store_root`
    /// (never created here). Fails only when cancelled, or when another operation held a drive or
    /// the store for longer than `patience`.
    pub fn run(
        &mut self,
        store_root: &Path,
        mounted: &[MountedDrive],
        ctx: &Ctx<'_>,
        on: &mut dyn FnMut(&WatchAction),
    ) -> Result<()> {
        let now = Instant::now();
        let ready: Vec<PathBuf> = self.due.iter().filter(|(_, at)| **at <= now).map(|(r, _)| r.clone()).collect();
        for root in in_save_order(store_root, ready, ctx)? {
            self.due.remove(&root);
            let Some(drive) = mounted.iter().find(|d| d.root == root) else { continue };
            match look(store_root, drive, mounted, &self.options, ctx, on) {
                Ok(()) => {}
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                // Another operation is writing it, or the store: say so once, and look again
                // later.
                Err(e @ Error::Locked(_)) => {
                    let patience = self.patience;
                    let since = *self.waiting.entry(root.clone()).or_insert_with(|| {
                        let limit = patience.map(|p| format!("; giving up after {}s", p.as_secs())).unwrap_or_default();
                        on(&WatchAction::Waiting { drive: drive.clone(), reason: format!("{e}{limit}") });
                        now
                    });
                    if patience.is_some_and(|p| now.duration_since(since) >= p) {
                        return Err(e);
                    }
                    self.due.insert(root, Instant::now() + self.options.settle.max(WATCH_SETTLE));
                    continue;
                }
                Err(e) => on(&WatchAction::Failed { drive: drive.clone(), kind: e.code(), message: e.to_string() }),
            }
            self.waiting.remove(&root);
        }
        Ok(())
    }
}

/// `roots` in the order [`save_order`] saves them in (when there are several): each pull moves
/// the library's newest snapshot, which the next drive is looked at against. The drives it
/// doesn't rank (saved already, left alone, or that can't be read yet) follow in path order.
fn in_save_order(store_root: &Path, roots: Vec<PathBuf>, ctx: &Ctx<'_>) -> Result<Vec<PathBuf>> {
    if roots.len() < 2 {
        return Ok(roots);
    }
    let Ok(store) = Store::open(store_root) else { return Ok(roots) };
    let mut next = Vec::new();
    for (i, root) in roots.iter().enumerate() {
        let Ok(d) = Device::open(root) else { continue };
        let lib = d.state().and_then(|s| s.library_id).and_then(|id| store.library_by_id(id).ok().flatten());
        let Some(lib) = lib else { continue };
        match pull_next(&store, &lib, &d, ctx) {
            Ok(n) => next.push((i, d, n)),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            // Looked at (and reported) on its own turn.
            Err(_) => {}
        }
    }
    let order = match save_order(&store, next.iter().map(|(i, d, n)| (*i, d, Some(n))).collect(), ctx) {
        Ok(order) => order,
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(_) => Vec::new(),
    };
    let mut out: Vec<PathBuf> = order.iter().map(|(_, i)| roots[*i].clone()).collect();
    let rest: Vec<PathBuf> = roots.into_iter().filter(|r| !out.contains(r)).collect();
    out.extend(rest);
    Ok(out)
}

/// Watching: polls drives and pulls them as they settle.
#[derive(Debug, Clone)]
pub struct Watching {
    pub watcher: VolumeWatcher,
    pub auto: AutoPull,
    pub every: Duration,
}

impl Watching {
    /// Watches the mounted volumes; the ones mounted now are looked at first, once they've
    /// settled.
    pub fn new(watcher: VolumeWatcher, options: WatchOptions) -> Watching {
        let mut auto = AutoPull::new(options);
        auto.check_now(&watcher.drives());
        Watching { watcher, auto, every: WATCH_POLL_EVERY }
    }

    /// Looks at the drives due now.
    pub fn once(&mut self, store_root: &Path, ctx: &Ctx<'_>, on: &mut dyn FnMut(Watched<'_>)) -> Result<()> {
        let drives = self.watcher.drives();
        self.auto.run(store_root, &drives, ctx, &mut |a| on(Watched::Did(a)))
    }

    /// Looks at the drives mounted now (and any plugged in meanwhile), each once it has
    /// settled, then returns: `watch --once`. A drive or the store another operation holds is
    /// waited for up to `auto.patience` [default: [`WATCH_ONCE_WAIT`]], then it fails.
    pub fn until_idle(&mut self, store_root: &Path, ctx: &Ctx<'_>, on: &mut dyn FnMut(Watched<'_>)) -> Result<()> {
        self.auto.patience.get_or_insert(WATCH_ONCE_WAIT);
        self.watch(store_root, ctx, &|w| w.auto.is_idle(), on)
    }

    /// Looks at the drives mounted now once they've settled, and keeps watching until `stop`
    /// says so (checked between actions) or `ctx` is cancelled (which stops an action part way,
    /// as it would any pull or sync).
    pub fn run(
        &mut self,
        store_root: &Path,
        ctx: &Ctx<'_>,
        stop: &dyn Fn() -> bool,
        on: &mut dyn FnMut(Watched<'_>),
    ) -> Result<()> {
        self.watch(store_root, ctx, &|_| stop(), on)
    }

    fn watch(
        &mut self,
        store_root: &Path,
        ctx: &Ctx<'_>,
        stop: &dyn Fn(&Watching) -> bool,
        on: &mut dyn FnMut(Watched<'_>),
    ) -> Result<()> {
        loop {
            self.once(store_root, ctx, on)?;
            let until = Instant::now() + self.every;
            while Instant::now() < until {
                if stop(self) {
                    return Ok(());
                }
                ctx.check()?;
                std::thread::sleep(Duration::from_millis(50));
            }
            let events = self.watcher.poll();
            for e in &events {
                on(Watched::Seen(e));
            }
            self.auto.observe(&events);
        }
    }
}

fn skipped(drive: &MountedDrive, reason: impl Into<String>) -> WatchAction {
    WatchAction::Skipped { drive: drive.clone(), reason: reason.into() }
}

/// Looks at one drive: pulls it when the store lacks its state, then syncs its group.
fn look(
    store_root: &Path,
    drive: &MountedDrive,
    mounted: &[MountedDrive],
    opts: &WatchOptions,
    ctx: &Ctx<'_>,
    on: &mut dyn FnMut(&WatchAction),
) -> Result<()> {
    let mut d = Device::open(&drive.root)?;
    if !d.has_export() {
        return Ok(());
    }
    let Some(state) = d.state() else {
        on(&skipped(drive, "not linked to a library in the store (pull it once to start one)"));
        return Ok(());
    };
    let (id, linked) = (state.id, state.library_id);
    let Ok(store) = Store::open(store_root) else {
        on(&skipped(drive, "there's no store yet (pull a drive once to start one)"));
        return Ok(());
    };
    let Some(lib) = linked.map(|l| store.library_by_id(l)).transpose()?.flatten() else {
        on(&skipped(drive, "not linked to a library in the store (pull it once to start one)"));
        return Ok(());
    };
    if let Some(rec) = store.drives()?.remove(&id) {
        if rec.log.retired.is_some() {
            on(&skipped(drive, "retired"));
            return Ok(());
        }
        if rec.role == Some(Role::Archive) {
            on(&skipped(drive, "an archive drive, kept as it is"));
            return Ok(());
        }
    }

    let (ancestor, beside) = match pull_next(&store, &lib, &d, ctx)? {
        PullNext::Skip(reason) => {
            on(&skipped(drive, reason));
            return Ok(());
        }
        PullNext::Saved { beside } => {
            on(&WatchAction::UpToDate { drive: drive.clone(), library: lib.name.clone() });
            if opts.sync_set && !beside {
                sync_set(&store, drive, &lib, mounted, ctx, on)?;
            }
            return Ok(());
        }
        PullNext::Pull { ancestor } => (ancestor, false),
        PullNext::Beside { shared } => (Some(shared), true),
    };
    let history_lists = new_history(&store, &d, ancestor.as_deref())?;
    if !opts.pull {
        on(&WatchAction::Pending { drive: drive.clone(), library: lib.name.clone(), history_lists, beside });
        return Ok(());
    }
    let message = Some(format!(
        "{} plugged in{}",
        d.name(),
        if beside { " (back from a set, beside the newest snapshot)" } else { "" }
    ));
    let opts_pull = PullOptions { library: None, message, force: false };
    let report = match ancestor.filter(|_| beside) {
        Some(shared) => sync::pull_beside(&store, &mut d, &opts_pull, shared.id, ctx)?,
        None => sync::pull(&store, &mut d, &opts_pull, ctx)?,
    };
    let Some(snapshot) = report.snapshot else { return Ok(()) };
    let drive = MountedDrive { id: d.id(), name: d.name(), ..drive.clone() };
    if report.up_to_date {
        on(&WatchAction::UpToDate { drive: drive.clone(), library: lib.name.clone() });
    } else {
        on(&WatchAction::Pulled {
            drive: drive.clone(),
            library: lib.name.clone(),
            snapshot: Box::new(snapshot),
            history_lists,
            beside,
        });
    }
    if opts.sync_set && !beside {
        sync_set(&store, &drive, &lib, mounted, ctx, on)?;
    }
    Ok(())
}

/// How a drive's state would be saved in its library (see [`pull_next`]).
pub(super) enum PullNext {
    /// The store holds it already: as the library's newest snapshot, or beside it.
    Saved { beside: bool },
    /// Pulled on top of the library's newest snapshot (`ancestor`; `None` when it has none).
    Pull { ancestor: Option<Box<Snapshot>> },
    /// Back from a set after a newer export was pulled: saved beside the newest snapshot, from
    /// the state it last shared with the library.
    Beside { shared: Box<Snapshot> },
    /// Left alone, and why.
    Skip(String),
}

/// How to save `d`, a drive of `lib`, in the store: the drive against the library's newest
/// snapshot, as `watch` decides it.
pub(super) fn pull_next(store: &Store, lib: &LibraryRecord, d: &Device, ctx: &Ctx<'_>) -> Result<PullNext> {
    let here = Side::from_device(d, ctx)?;
    let head = lib.head.map(|h| store.load_snapshot(&h)).transpose()?;
    let relation = match &head {
        Some(head) => compare::compare(&Side::from_snapshot(store, head, "store")?, &here).relation,
        None => Relation::RightNewer,
    };
    // Newer by the evidence but not by its lineage: a plain pull would refuse it.
    let descends = head.as_ref().is_none_or(|h| d.state().is_some_and(|s| s.knows(&h.fingerprint)));
    let relation = if relation == Relation::RightNewer && !descends { Relation::Undetermined } else { relation };
    let saved_already = |d: &Device| {
        (d.state().and_then(|s| s.head()))
            .filter(|h| Some(h.fingerprint) == here.fingerprint)
            .and_then(|h| h.snapshot)
            .is_some_and(|s| store.load_snapshot(&s).is_ok_and(|s| s.library_id == lib.id))
    };
    Ok(match relation {
        Relation::Identical => PullNext::Saved { beside: false },
        Relation::RightNewer => PullNext::Pull { ancestor: head.map(Box::new) },
        Relation::LeftNewer => PullNext::Skip(format!("behind {:?}'s newest snapshot: nothing new to save", lib.name)),
        Relation::Unrelated => PullNext::Skip(format!("holds a different library from {:?}", lib.name)),
        Relation::Diverged | Relation::Undetermined if saved_already(d) => PullNext::Saved { beside: true },
        Relation::Diverged | Relation::Undetermined => {
            // Back from a set: kept beside the newest snapshot when players made every change.
            let shared = shared_snapshot(store, lib, d)?;
            let player_only = match &shared {
                Some(s) => crate::after_set::changes_since(store, d, s, ctx)?.player_only(),
                None => false,
            };
            match shared {
                Some(shared) if player_only => PullNext::Beside { shared: Box::new(shared) },
                _ => PullNext::Skip(format!(
                    "both it and {:?} changed since they last matched: merge, or sync it by hand",
                    lib.name
                )),
            }
        }
    })
}

/// The order drives are saved in when several are at once: `(rank, index)`, by rank. Exports
/// go first (0; `None`, a drive starting a library, counts as one), then drives only players
/// changed since their library's newest snapshot (1), then drives back from a set, saved beside
/// it (2). Pulled on top of the newest snapshot ahead of a newer export, a drive played at a
/// set would leave that export unsaveable. Drives saved already or left alone are left out.
pub(super) fn save_order(
    store: &Store,
    drives: Vec<(usize, &Device, Option<&PullNext>)>,
    ctx: &Ctx<'_>,
) -> Result<Vec<(u8, usize)>> {
    let pulls = drives.iter().filter(|(_, _, n)| matches!(n, Some(PullNext::Pull { .. }))).count();
    let mut order = Vec::new();
    for (i, d, next) in drives {
        let rank = match next {
            Some(PullNext::Saved { .. } | PullNext::Skip(_)) => continue,
            Some(PullNext::Beside { .. }) => 2,
            // Only worth reading when another drive could go first.
            Some(PullNext::Pull { ancestor: Some(a) })
                if pulls > 1 && crate::after_set::changes_since(store, d, a, ctx)?.player_only() =>
            {
                1
            }
            Some(PullNext::Pull { .. }) | None => 0,
        };
        order.push((rank, i));
    }
    order.sort();
    Ok(order)
}

/// The newest snapshot of `lib` the drive held: its lineage, newest first, against the
/// library's snapshots.
fn shared_snapshot(store: &Store, lib: &LibraryRecord, d: &Device) -> Result<Option<Snapshot>> {
    let Some(state) = d.state() else { return Ok(None) };
    let in_library = |id: &Hash| store.load_snapshot(id).ok().filter(|s| s.library_id == lib.id);
    // The library's snapshots, read only when an entry doesn't name its own.
    let mut history: Option<Vec<SnapshotInfo>> = None;
    for e in state.lineage.iter().rev() {
        if let Some(s) = e.snapshot.as_ref().and_then(in_library) {
            return Ok(Some(s));
        }
        if history.is_none() {
            history = Some(store.history(lib, None)?);
        }
        if let Some(s) = history.iter().flatten().find(|s| s.fingerprint == e.fingerprint) {
            return store.load_snapshot(&s.id).map(Some);
        }
    }
    Ok(None)
}

/// History lists on the drive that `ancestor` lacks (every one without an ancestor), by name.
pub(super) fn new_history(store: &Store, d: &Device, ancestor: Option<&Snapshot>) -> Result<Vec<String>> {
    let then = ancestor.map(|a| Side::from_snapshot(store, a, "then")).transpose()?.map(|s| s.library);
    let known: HashSet<(String, Vec<u32>)> =
        then.iter().flat_map(|l| &l.history).map(|h| (h.name.clone(), h.track_ids.clone())).collect();
    Ok(d.library()?
        .history
        .iter()
        .filter(|h| !h.is_folder && !known.contains(&(h.name.clone(), h.track_ids.clone())))
        .map(|h| h.name.clone())
        .collect())
}

/// Syncs the library's other mounted drives from `drive` when it's the newest of them with
/// confidence. Anything less certain is left alone, and said.
fn sync_set(
    store: &Store,
    drive: &MountedDrive,
    lib: &LibraryRecord,
    mounted: &[MountedDrive],
    ctx: &Ctx<'_>,
    on: &mut dyn FnMut(&WatchAction),
) -> Result<()> {
    let devices: Vec<Device> = mounted.iter().filter_map(|m| Device::open(&m.root).ok()).collect();
    let Some(set) = fleet::sets(store, &devices)?.into_iter().find(|s| s.library_id == Some(lib.id)) else {
        return Ok(());
    };
    if !set.members.contains(&drive.root) || set.members.len() < 2 {
        return Ok(());
    }
    // This drive first, so it stays the source when another holds the same state.
    let mut drives = vec![drive.root.display().to_string()];
    drives.extend(set.members.iter().filter(|m| **m != drive.root).map(|m| m.display().to_string()));
    let req = SyncManyRequest {
        drives,
        allow_low_confidence: false,
        verify: false,
        keep_extra: false,
        delete_first: false,
        overwrite_archive: false,
    };
    let preview = match super::sync_many_plan(&req, ctx) {
        Ok(p) => p,
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(e) => {
            on(&skipped(drive, format!("not syncing its group: {e}")));
            return Ok(());
        }
    };
    if preview.source != drive.root {
        on(&skipped(drive, format!("not syncing its group: {} is newer", preview.source_name)));
        return Ok(());
    }
    if preview.targets.iter().all(|t| t.plan.as_ref().is_none_or(|p| p.is_noop())) {
        return Ok(());
    }
    let targets = super::sync_many_apply(Some(store), &req, &preview, ctx)?;
    on(&WatchAction::Synced { drive: drive.clone(), library: lib.name.clone(), targets });
    Ok(())
}
