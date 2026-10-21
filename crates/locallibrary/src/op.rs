//! Planned operations: every change locallibrary makes is worked out first, shown, and then made
//! exactly as it was shown.
//!
//! A [`Planned`] wraps an operation's plan with what a front end needs to ask for confirmation:
//! an id, the operation's name, summary lines, warnings, whether it destroys anything, and the
//! drives (or files on this computer) it touches. For each of those it keeps a hash of the state
//! the plan was made from, taken at plan time and never serialised. [`Planned::apply`] measures
//! them again first and refuses with [`Error::StalePlan`] (`stale_plan`) if anything changed, so
//! a dialog that said "remove 3 tracks" can't remove 5. It also refuses to write to an archive or
//! retired drive unless the plan was made to (see [`crate::fleet::check_target`]). That check
//! runs before the drive is locked; the write checks the drive's databases again once it holds
//! the lock, so another write that slipped in between isn't overwritten either.
//!
//! A write to a library reaches all of it: a plan made with `Planned::reaching_clones` names the
//! library's other mounted clones, and applying it copies the drive it wrote onto them.
//!
//! Plans live in memory: the CLI applies the one it showed, the TUI the one in its dialog, and the
//! JSON-RPC sidecar keeps recent ones by id (`*.apply {"plan_id": ...}`). [`AnyPlanned`] lets a
//! front end hold plans of every kind together.

use std::any::Any;
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Serialize, Serializer};

use crate::device::{Device, DeviceState};
use crate::hash::Hash;
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result, fsutil, paths};

thread_local! {
    static ARCHIVE_WRITES: Cell<bool> = const { Cell::new(false) };
    /// The drives the plans being applied on this thread write, with their databases when the
    /// plans were made (see [`planned_databases`]); `None` once a write has locked the drive.
    static PLANNED: RefCell<Vec<(PathBuf, Option<Hash>)>> = const { RefCell::new(Vec::new()) };
    /// Whether plans made on this thread reach their library's other clones (see [`alone`]).
    static REACH_CLONES: Cell<bool> = const { Cell::new(true) };
}

/// Run `f` with the plans made in it writing their own drive only, not their library's other
/// clones (see `Planned::reaching_clones`): a job plans the operations it runs this way, and
/// copies onto the clones itself.
pub(crate) fn alone<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            REACH_CLONES.with(|r| r.set(self.0));
        }
    }
    let _restore = Restore(REACH_CLONES.with(|r| r.replace(false)));
    f()
}

/// The databases the drive at `root` held when the plan being applied on this thread was made,
/// the first time a write locks the drive while it's applied (later locks are the plan's own
/// next writes). [`Planned::apply`] checks a plan before anything is locked;
/// [`crate::commit::lock`] checks this once the drive is.
pub(crate) fn planned_databases(root: &Path) -> Option<Hash> {
    PLANNED.with(|planned| {
        let mut planned = planned.borrow_mut();
        let found = planned.iter().rev().find(|(r, _)| r == root).and_then(|(_, h)| *h);
        for (_, h) in planned.iter_mut().filter(|(r, _)| r == root) {
            *h = None;
        }
        found
    })
}

/// A plan being applied on this thread, until it's done (a panic included): the drives it
/// writes, noted for [`planned_databases`], and its archive-write permission, which what it runs
/// (a provision's format and push) shares. Both are put back as they were when it's dropped.
struct Applying {
    planned: usize,
    archive_writes: bool,
}

impl Applying {
    /// `archive`: archive and retired drives may be written.
    fn start(drives: &[Touched], archive: bool) -> Applying {
        let archive_writes = archive_writes_allowed();
        allow_archive_writes(archive);
        PLANNED.with(|planned| {
            let mut planned = planned.borrow_mut();
            let before = planned.len();
            let written = drives.iter().filter(|t| t.writes && t.drive);
            planned.extend(written.filter_map(|t| t.databases.map(|h| (t.root.clone(), Some(h)))));
            Applying { planned: before, archive_writes }
        })
    }
}

impl Drop for Applying {
    fn drop(&mut self) {
        PLANNED.with(|planned| planned.borrow_mut().truncate(self.planned));
        allow_archive_writes(self.archive_writes);
    }
}

/// Let plans made or applied on this thread write to archive and retired drives (the CLI's
/// `--overwrite-archive`, an RPC request's `overwrite_archive`). A plan made while this is set
/// keeps the permission wherever it's applied.
pub fn allow_archive_writes(yes: bool) {
    ARCHIVE_WRITES.with(|a| a.set(yes));
}

/// Whether [`allow_archive_writes`] is set on this thread.
pub fn archive_writes_allowed() -> bool {
    ARCHIVE_WRITES.with(Cell::get)
}

/// Run `f` with archive and retired drives refused on this thread, whatever
/// [`allow_archive_writes`] says (put back after): the plans made in it never write to one.
pub(crate) fn refusing_archive_writes<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            allow_archive_writes(self.0);
        }
    }
    let _restore = Restore(ARCHIVE_WRITES.with(|a| a.replace(false)));
    f()
}

/// What of a drive (or a folder on this computer) a plan depends on. [`Touched`] hashes these
/// when the plan is made and again before it is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// The library's databases (`export.pdb`, `exportExt.pdb`, `exportLibrary.db` and its WAL), by
    /// content: rekordbox exporting again, or another edit, changes them.
    Databases,
    /// The drive's identity and the state locallibrary last recorded for it (another drive
    /// mounted at the same place, or a locallibrary write in between). A drive without an
    /// identity is known by its volume: the root folder's device and inode (volume serial and
    /// file index on Windows) and the volume's size.
    Identity,
    /// These files (keys below the root) by content.
    Content(Vec<String>),
    /// These files by size and modification time (audio, recordings, update files: too big to
    /// read twice).
    Stamp(Vec<String>),
    /// The files directly in a folder (a key, `""` for the root) by name, size and time.
    Listing(String),
}

/// A drive, folder or file an operation reads or writes, and its state when it was planned.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Touched {
    /// The drive's root, or the folder or file on this computer.
    pub root: PathBuf,
    pub name: String,
    /// Whether the operation writes to it (a sync's source is only read).
    pub writes: bool,
    /// A drive (archive and retired drives are refused unless the plan allows it), rather than
    /// files on this computer.
    pub drive: bool,
    #[serde(skip)]
    probes: Vec<Probe>,
    /// It was there when planned: a folder an operation makes (an export's) may not be yet.
    #[serde(skip)]
    existed: bool,
    #[serde(skip)]
    state: Hash,
    /// Its [`Probe::Databases`] alone, if probed, for the write to check again under the lock.
    #[serde(skip)]
    databases: Option<Hash>,
}

impl Touched {
    /// A drive, its state measured now.
    pub fn drive(device: &Device, writes: bool, probes: Vec<Probe>) -> Touched {
        let root = device.root().to_path_buf();
        let databases = probes.contains(&Probe::Databases).then(|| crate::commit::database_state_at(&root));
        Touched {
            state: measure(&root, &probes, databases),
            name: device.name(),
            root,
            writes,
            drive: true,
            probes,
            databases,
            existed: true,
        }
    }

    /// A drive whose library an operation reads or edits: its databases and identity.
    pub fn library(device: &Device, writes: bool) -> Touched {
        Touched::drive(device, writes, vec![Probe::Databases, Probe::Identity])
    }

    /// A folder or file on this computer (keys in `probes` are below it; `""` is `path` itself).
    pub fn path(path: &Path, writes: bool, probes: Vec<Probe>) -> Touched {
        Touched {
            state: measure(path, &probes, None),
            name: path.display().to_string(),
            root: path.to_path_buf(),
            writes,
            drive: false,
            probes,
            databases: None,
            existed: path.exists(),
        }
    }

    /// Whether it changed since it was measured.
    pub fn changed(&self) -> bool {
        measure(&self.root, &self.probes, None) != self.state
    }

    /// The drive, opened again (refused if it's gone).
    pub fn open(&self) -> Result<Device> {
        Device::open(&self.root)
    }
}

/// A mounted clone of the library a plan writes, which applying the plan copies the drive it wrote
/// onto unless it's left alone (see `Planned::reaching_clones`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CloneTarget {
    pub name: String,
    pub root: PathBuf,
    /// Why it isn't copied onto (it's newer, diverged, or back from a set), also among the
    /// plan's warnings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_alone: Option<String>,
}

/// Names for summaries: `a`, `a and b`, `a, b and c`.
pub(crate) fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [a] => a.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A size for summaries: `840 B`, `12.5 MB`.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    // 999.95 and up rounds to 1000.0 at one decimal: that's the next unit's 1.0.
    while v >= 999.95 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

fn at(root: &Path, key: &str) -> PathBuf {
    if key.is_empty() { root.to_path_buf() } else { paths::to_fs(root, key) }
}

/// Hash `probes` of the drive or folder at `root` (`databases`: its [`Probe::Databases`], when
/// that's known already). Files that can't be read count as absent.
fn measure(root: &Path, probes: &[Probe], databases: Option<Hash>) -> Hash {
    let mut h = blake3::Hasher::new();
    h.update(root.to_string_lossy().as_bytes());
    let content = |h: &mut blake3::Hasher, key: &str| {
        h.update(b"\0content\0");
        h.update(key.as_bytes());
        match std::fs::File::open(at(root, key)) {
            Ok(mut f) => {
                let mut buf = vec![0u8; 1 << 20];
                loop {
                    match f.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            h.update(&buf[..n]);
                        }
                        Err(_) => {
                            h.update(b"\0unreadable");
                            break;
                        }
                    }
                }
            }
            Err(_) => {
                h.update(b"\0absent");
            }
        }
    };
    let stamp = |h: &mut blake3::Hasher, path: &Path| match std::fs::metadata(path) {
        Ok(m) => {
            h.update(&m.len().to_le_bytes());
            h.update(&fsutil::mtime_secs(&m).to_le_bytes());
            h.update(&[u8::from(m.is_dir())]);
        }
        Err(_) => {
            h.update(b"\0absent");
        }
    };
    for p in probes {
        match p {
            Probe::Databases => {
                h.update(b"\0databases");
                h.update(databases.unwrap_or_else(|| crate::commit::database_state_at(root)).as_bytes());
            }
            Probe::Identity => {
                h.update(b"\0identity");
                let state: Option<DeviceState> =
                    fsutil::read_json_opt(&root.join(paths::STATE_DIR).join("state.json")).ok().flatten();
                match state {
                    Some(s) => {
                        h.update(s.id.as_bytes());
                        if let Some(head) = s.head() {
                            h.update(head.fingerprint.as_bytes());
                        }
                        if let Some(p) = &s.pending {
                            h.update(format!("{:?} {}", p.op, p.started).as_bytes());
                        }
                    }
                    // Blank sticks mount at the same few paths (`/Volumes/NO NAME`): one planned
                    // onto mustn't pass for another mounted there since.
                    None => {
                        if let Ok(volume) = same_file::Handle::from_path(root) {
                            let mut v = std::hash::DefaultHasher::new();
                            std::hash::Hash::hash(&volume, &mut v);
                            h.update(&std::hash::Hasher::finish(&v).to_le_bytes());
                        }
                        h.update(&fsutil::total_space(root).unwrap_or(0).to_le_bytes());
                    }
                }
            }
            Probe::Content(keys) => {
                for key in keys {
                    content(&mut h, key);
                }
            }
            Probe::Stamp(keys) => {
                for key in keys {
                    h.update(b"\0stamp\0");
                    h.update(key.as_bytes());
                    stamp(&mut h, &at(root, key));
                }
            }
            Probe::Listing(dir) => {
                h.update(b"\0listing\0");
                h.update(dir.as_bytes());
                let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(at(root, dir))
                    .map(|rd| rd.flatten().map(|e| e.file_name()).collect())
                    .unwrap_or_default();
                names.sort();
                for n in names {
                    h.update(n.to_string_lossy().as_bytes());
                    stamp(&mut h, &at(root, dir).join(&n));
                }
            }
        }
    }
    h.finalize().into()
}

/// An operation's plan, ready to be confirmed and applied (see the module docs).
///
/// It serialises as the plan's own fields with these added: `plan_id`, `operation`, `summary`,
/// `warnings` (every warning, the plan's own included), `destructive`, `noop`, `drives` (each
/// `{root, name, writes, drive}`), `clones` (when it has any) and `planned_at`.
#[derive(Debug)]
pub struct Planned<P, R = ()> {
    /// Names this plan in `*.apply {"plan_id": ...}`.
    pub id: String,
    /// `merge`, `clean`, `edit`...: the RPC method's name without `.plan` (a plugin's
    /// operation's name, for its edits).
    pub operation: Cow<'static, str>,
    pub plan: P,
    /// What will change, in a few lines, for a confirmation dialog.
    pub summary: Vec<String>,
    pub warnings: Vec<String>,
    /// The plugin's guard (`<plugin>.<check>`) the user made blocking that found something about
    /// its edits, among its warnings: it isn't applied (see [`Planned::unblocked`]).
    pub blocked: Option<String>,
    /// It deletes or overwrites something (files, tracks, a library, a disk); a dialog should
    /// say so.
    pub destructive: bool,
    /// Applying it changes nothing.
    pub noop: bool,
    pub drives: Vec<Touched>,
    /// The library's other mounted clones, which applying copies the drive it writes onto, but
    /// those left alone (see `Self::reaching_clones`).
    pub clones: Vec<CloneTarget>,
    pub planned_at: DateTime<Utc>,
    /// What was asked for, as applying needs it (not shown).
    pub(crate) request: R,
    /// Archive and retired drives may be written.
    pub(crate) overwrite_archive: bool,
    /// Archive and retired drives are never written, whatever this or the applying thread
    /// allows (a plugin's plan).
    pub(crate) never_archive: bool,
}

impl<P, R> Planned<P, R> {
    pub(crate) fn new(operation: impl Into<Cow<'static, str>>, plan: P, request: R) -> Self {
        Planned {
            id: uuid::Uuid::new_v4().to_string(),
            operation: operation.into(),
            plan,
            summary: Vec::new(),
            warnings: Vec::new(),
            blocked: None,
            destructive: false,
            noop: false,
            drives: Vec::new(),
            clones: Vec::new(),
            planned_at: Utc::now(),
            request,
            overwrite_archive: archive_writes_allowed(),
            never_archive: false,
        }
    }

    pub(crate) fn summary(mut self, lines: impl IntoIterator<Item = String>) -> Self {
        self.summary.extend(lines);
        self
    }

    pub(crate) fn line(mut self, line: impl Into<String>) -> Self {
        self.summary.push(line.into());
        self
    }

    pub(crate) fn warnings(mut self, warnings: impl IntoIterator<Item = String>) -> Self {
        for w in warnings {
            if !self.warnings.contains(&w) {
                self.warnings.push(w);
            }
        }
        self
    }

    pub(crate) fn destructive(mut self, yes: bool) -> Self {
        self.destructive = yes;
        self
    }

    pub(crate) fn noop(mut self, yes: bool) -> Self {
        self.noop = yes;
        self
    }

    pub(crate) fn touches(mut self, touched: Touched) -> Self {
        self.drives.push(touched);
        self
    }

    pub(crate) fn overwrite_archive(mut self, yes: bool) -> Self {
        self.overwrite_archive |= yes;
        self
    }

    /// Never write to an archive or retired drive, whatever the applying thread allows.
    pub(crate) fn never_archive(mut self) -> Self {
        self.never_archive = true;
        self
    }

    /// Whether applying it may write to archive and retired drives, on this thread.
    fn archive_allowed(&self) -> bool {
        !self.never_archive && (self.overwrite_archive || archive_writes_allowed())
    }

    /// Once it's applied, copy the drive it writes (the first it touches as written) onto the other
    /// mounted clones of that drive's library, as a group sync would: a write to a library is one
    /// plan, and one confirmation, for all of it. A clone that can't take the copy (newer than the
    /// drive, diverged from it, back from a set) is left alone, which a warning says. Not for a plan
    /// that changes nothing, nor one made inside [`alone`].
    pub(crate) fn reaching_clones(mut self, ctx: &Ctx<'_>) -> Result<Self> {
        if self.noop || !REACH_CLONES.with(Cell::get) {
            return Ok(self);
        }
        let Some(written) = self.written() else { return Ok(self) };
        let touched: Vec<&Path> = self.drives.iter().map(|t| t.root.as_path()).collect();
        for (clone, left_alone) in crate::api::clones_of(&written.open()?, &touched, ctx)? {
            match &left_alone {
                Some(why) => self = self.warnings([why.clone()]),
                None => self.drives.push(Touched::library(&clone, true)),
            }
            self.clones.push(CloneTarget { name: clone.name(), root: clone.root().to_path_buf(), left_alone });
        }
        self.summary.extend(self.copied_onto());
        Ok(self)
    }

    /// "then copied onto Backup and SD": the clones applying copies the drive it writes onto, if
    /// any (see `Self::reaching_clones`).
    pub fn copied_onto(&self) -> Option<String> {
        let mut names: Vec<&str> = self.copied().map(|c| c.name.as_str()).collect();
        let last = names.pop()?;
        Some(match names.is_empty() {
            true => format!("then copied onto {last}"),
            false => format!("then copied onto {} and {last}", names.join(", ")),
        })
    }

    /// The clones it's copied onto: those not left alone.
    fn copied(&self) -> impl Iterator<Item = &CloneTarget> {
        self.clones.iter().filter(|c| c.left_alone.is_none())
    }

    /// The drive it writes that its clones are copied from: the first it touches as written.
    fn written(&self) -> Option<&Touched> {
        self.drives.iter().find(|t| t.writes && t.drive)
    }

    /// What was asked for.
    pub fn request(&self) -> &R {
        &self.request
    }

    /// Refuse a plan that writes to an archive or retired drive (unless it was made to). Run when
    /// planning, so nobody is asked to confirm what would be refused, and again by [`Self::check`].
    pub(crate) fn guard(self, store: Option<&Store>) -> Result<Self> {
        self.guard_targets(store)?;
        Ok(self)
    }

    fn guard_targets(&self, store: Option<&Store>) -> Result<()> {
        if self.archive_allowed() {
            return Ok(());
        }
        let registry;
        let store = match store {
            Some(s) => Some(s),
            None => {
                registry = crate::api::registry();
                registry.as_ref()
            }
        };
        let refused: Result<()> = refusing_archive_writes(|| {
            for t in self.drives.iter().filter(|t| t.writes && t.drive) {
                if let Ok(d) = Device::open(&t.root) {
                    crate::fleet::check_target(store, &d, false)?;
                }
            }
            Ok(())
        });
        // A plan that never writes to one can't be applied again to write to it anyway.
        refused.map_err(|e| if self.never_archive { e.without_retries() } else { e })
    }

    /// Whether the plan can still be applied: every drive and file it touches is as it was when
    /// planned, there or not ([`Error::StalePlan`] otherwise), and none it writes is an archive or
    /// retired drive unless the plan was made to write to one.
    pub fn check(&self, store: Option<&Store>) -> Result<()> {
        for t in &self.drives {
            match (t.existed, t.root.exists()) {
                (true, false) => return Err(Error::stale(&t.name, "is gone")),
                (false, true) => return Err(Error::stale(&t.name, "is there now")),
                _ => {}
            }
            if t.changed() {
                return Err(Error::stale(&t.name, "changed"));
            }
        }
        self.guard_targets(store)
    }

    /// Refused when a plugin's guard the user made blocking found something about it
    /// ([`Planned::blocked`]).
    pub fn unblocked(&self) -> Result<()> {
        let Some(check) = &self.blocked else { return Ok(()) };
        let message = format!(
            "{check} found something about these changes (see the warnings), so they weren't made; \
             `locallibrary plugin checks {check} --no-blocking` lets it warn without stopping them"
        );
        Err(Error::conflict(crate::error::Reason::Blocked, message).with("check", check))
    }

    /// The touched drive at `index`, opened again.
    pub(crate) fn drive(&self, index: usize) -> Result<Device> {
        self.drives.get(index).ok_or_else(|| Error::invalid("the plan names no such drive"))?.open()
    }
}

/// A kind of plan that can be applied. [`crate::api`] implements it for the plans its `*_plan`
/// functions return.
pub trait Operation: Sized {
    /// What applying needs besides the plan (kept in [`Planned`]).
    type Request;
    type Report: Serialize;

    /// Make the change. Called by [`Planned::apply`] once the plan is checked.
    fn apply(planned: &Planned<Self, Self::Request>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<Self::Report>;
}

/// A `Planned` reads as its plan (`planned.tracks`, `planned.is_noop()`).
impl<P, R> std::ops::Deref for Planned<P, R> {
    type Target = P;

    fn deref(&self) -> &P {
        &self.plan
    }
}

impl<P: Operation> Planned<P, P::Request> {
    /// Apply exactly this plan: refused with [`Error::StalePlan`] if anything it touches changed
    /// since it was made (see [`Self::check`]). Then copy the drive it wrote onto its clones (see
    /// `Self::reaching_clones`): the write is done, so a clone that isn't copied onto is a warning.
    pub fn apply(&self, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<P::Report> {
        self.unblocked()?;
        self.check(store)?;
        let _applying = Applying::start(&self.drives, self.archive_allowed());
        let report = P::apply(self, store, ctx)?;
        let clones: Vec<&CloneTarget> = self.copied().collect();
        if let Some(written) = self.written().filter(|_| !clones.is_empty()) {
            crate::api::copy_onto_clones(&written.root, &clones, store, ctx);
        }
        if let Some(tell) = APPLIED.get() {
            let (operation, summary, drives) =
                (self.operation.as_ref(), self.summary.as_slice(), self.drives.as_slice());
            tell(&Applied { operation, summary, destructive: self.destructive, drives }, store);
        }
        Ok(report)
    }
}

/// A plan that was applied, as [`on_applied`] tells of it.
#[derive(Debug, Clone, Copy)]
pub struct Applied<'a> {
    pub operation: &'a str,
    pub summary: &'a [String],
    pub destructive: bool,
    pub drives: &'a [Touched],
}

type Tell = Box<dyn Fn(&Applied<'_>, Option<&Store>) + Send + Sync>;

static APPLIED: OnceLock<Tell> = OnceLock::new();

/// Has `tell` told of each plan applied from now on, as it's applied, on the thread that applied
/// it (with the store it was applied with): what an app's plugins hear (`plan.applied`). Set once:
/// another call does nothing.
pub fn on_applied(tell: impl Fn(&Applied<'_>, Option<&Store>) + Send + Sync + 'static) {
    let _ = APPLIED.set(Box::new(tell));
}

/// What every plan says besides its own fields, as `*.plan` returns it (see [`Planned`]).
#[derive(Serialize, JsonSchema)]
pub struct PlanFields<'a> {
    /// Names the plan in `*.apply {"plan_id": ...}`.
    pub plan_id: &'a str,
    /// The RPC method's name without `.plan`, except `undo` for `drive.restore.plan`, `eject` for
    /// `drive.eject.plan` and `set` for `prep.plan`; for a plugin's operation, its name.
    pub operation: &'a str,
    /// What will change, in a few lines, for a confirmation dialog.
    pub summary: &'a [String],
    /// Every warning, the plan's own included.
    pub warnings: &'a [String],
    /// The plugin's guard (`<plugin>.<check>`) the user made blocking that found something about
    /// it: applying it is refused (`blocked`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<&'a str>,
    /// It deletes or overwrites something (files, tracks, a library, a disk): a dialog should
    /// say so.
    pub destructive: bool,
    /// Applying it changes nothing.
    pub noop: bool,
    /// The drives (and files on this computer) it reads or writes, its clones included.
    pub drives: &'a [Touched],
    /// The library's other mounted clones, which applying copies the drive it writes onto, but
    /// those `left_alone`.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub clones: &'a [CloneTarget],
    pub planned_at: DateTime<Utc>,
}

impl<P: Serialize, R> Serialize for Planned<P, R> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = match json::<_, S::Error>(&self.plan)? {
            serde_json::Value::Object(map) => map,
            serde_json::Value::Null => serde_json::Map::new(),
            other => {
                let mut map = serde_json::Map::new();
                map.insert("plan".into(), other);
                map
            }
        };
        let fields = PlanFields {
            plan_id: &self.id,
            operation: &self.operation,
            summary: &self.summary,
            warnings: &self.warnings,
            blocked: self.blocked.as_deref(),
            destructive: self.destructive,
            noop: self.noop,
            drives: &self.drives,
            clones: &self.clones,
            planned_at: self.planned_at,
        };
        if let serde_json::Value::Object(fields) = json::<_, S::Error>(&fields)? {
            map.extend(fields);
        }
        map.serialize(serializer)
    }
}

fn json<T: Serialize, E: serde::ser::Error>(value: &T) -> std::result::Result<serde_json::Value, E> {
    serde_json::to_value(value).map_err(E::custom)
}

/// A plan's schema: the plan's own fields and [`PlanFields`]. (A plan that isn't a JSON object
/// is under `plan`; one that is `null` adds nothing.)
impl<P: JsonSchema, R> JsonSchema for Planned<P, R> {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("Planned_{}", P::schema_name()).into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        format!("locallibrary::op::Planned<{}>", P::schema_id()).into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        use crate::schema::{Shape, shape};
        let plan = generator.subschema_for::<P>();
        let fields = generator.subschema_for::<PlanFields<'static>>();
        match shape(generator, &plan) {
            Shape::Null => fields,
            Shape::Object => schemars::json_schema!({ "allOf": [plan, fields] }),
            Shape::Other => schemars::json_schema!({
                "allOf": [
                    { "type": "object", "properties": { "plan": plan }, "required": ["plan"] },
                    fields,
                ]
            }),
        }
    }
}

/// A plan of any kind, for front ends that hold plans of several kinds (the RPC sidecar keeps
/// them by id, the TUI shows one in its dialog).
pub trait AnyPlanned: Send + Sync {
    fn id(&self) -> &str;
    fn operation(&self) -> &str;
    fn summary(&self) -> &[String];
    fn warnings(&self) -> &[String];
    /// [`Planned::blocked`] (none for a plan no guard is asked about).
    fn blocked(&self) -> Option<&str> {
        None
    }
    /// [`Planned::unblocked`].
    fn unblocked(&self) -> Result<()> {
        Ok(())
    }
    fn destructive(&self) -> bool;
    fn is_noop(&self) -> bool;
    fn drives(&self) -> &[Touched];
    fn planned_at(&self) -> DateTime<Utc>;
    /// The plan as `*.plan` returns it.
    fn to_json(&self) -> serde_json::Value;
    /// [`Planned::check`].
    fn check(&self, store: Option<&Store>) -> Result<()>;
    /// [`Planned::apply`], with the report as JSON.
    fn apply_json(&self, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<serde_json::Value>;
    /// For a front end that shows some kinds of plan in detail.
    fn as_any(&self) -> &dyn Any;
}

impl<P, R> AnyPlanned for Planned<P, R>
where
    P: Operation<Request = R> + Serialize + Send + Sync + 'static,
    R: Send + Sync + 'static,
{
    fn id(&self) -> &str {
        &self.id
    }

    fn operation(&self) -> &str {
        &self.operation
    }

    fn summary(&self) -> &[String] {
        &self.summary
    }

    fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn blocked(&self) -> Option<&str> {
        self.blocked.as_deref()
    }

    fn unblocked(&self) -> Result<()> {
        Planned::unblocked(self)
    }

    fn destructive(&self) -> bool {
        self.destructive
    }

    fn is_noop(&self) -> bool {
        self.noop
    }

    fn drives(&self) -> &[Touched] {
        &self.drives
    }

    fn planned_at(&self) -> DateTime<Utc> {
        self.planned_at
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }))
    }

    fn check(&self, store: Option<&Store>) -> Result<()> {
        Planned::check(self, store)
    }

    fn apply_json(&self, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<serde_json::Value> {
        let report = self.apply(store, ctx)?;
        serde_json::to_value(report).map_err(|e| Error::invalid(e.to_string()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests;
