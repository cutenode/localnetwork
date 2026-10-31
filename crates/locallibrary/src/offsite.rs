//! Copies of the store somewhere else: a NAS, an external disk, a cloud-synced folder.
//!
//! A copy has the store's own layout (and id), so `--store DIR` reads it as it would the store,
//! to push a snapshot from it after losing this computer, say. Copying is incremental and can
//! be stopped and started again: stored files are named by their content, so only the ones the
//! copy lacks are copied, each hashed on the way against its name and renamed into place once
//! whole. Snapshot manifests follow when every file they list is there and flushed, and
//! `libraries.json` (the latest snapshot of each library, which makes the rest reachable) goes
//! last. A copy stopped halfway still holds the state its last finished copy left, whole; its
//! `copy.json` says when a copy last started and last finished. The store lists its copies in
//! `copies.json` (`store copies`).
//!
//! A snapshot whose files are damaged or missing in the store is left out of the copy (and
//! reported) rather than copied broken; a library whose latest snapshot is left out keeps the
//! latest the copy had.
//!
//! A copy is only written by `store copy`. One written to since (opened with `--store` and
//! pulled into, labelled, given player models...) isn't copied over unless forced: `copy.json`
//! keeps a hash of what the last finished copy left there.
//!
//! A plugin's store destination (see [`Destination`]) is copied to the same way, in the same
//! order ([`copy_to`]), and what it holds is brought back into a store with [`restore`].

use indexmap::IndexMap;

use rustc_hash::{FxHashMap, FxHashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::catalog::{self, Catalog, CatalogSnapshot, FetchReport};
use crate::error::IoContext;
use crate::error::{Action, Reason};
use crate::fsutil;
use crate::hash::{self, Hash};
use crate::progress::{Ctx, Event, Phase};
use crate::store::{LibraryRecord, SnapshotLabel, Store};
use crate::{Error, Result};

/// The store's own files a copy takes besides stored files, snapshots and `libraries.json`. Not
/// the accounts (`atmosphere*.json`, `accounts.json` and their secrets, which stay on this
/// computer), the locks, caches or reports.
const METADATA: &[&str] = &[
    "drives.json",
    "settings.json",
    "smart.json",
    "players.json",
    crate::profile::PREFS_FILE,
    crate::sets::SETS_FILE,
    "pruned.json",
];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CopyOptions {
    /// Also remove from the copy what the store no longer has (pruned or collected snapshots,
    /// and files nothing in the store uses).
    #[serde(default)]
    pub mirror: bool,
    /// Re-hash the files the copy has already, copying again any that changed.
    #[serde(default)]
    pub verify: bool,
    /// Copy over a copy that was written to since it was last copied to (with `--store`), or that
    /// has snapshots the store doesn't.
    #[serde(default)]
    pub force: bool,
    /// Say what would be copied, and copy nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct CopyReport {
    /// The folder copied to, or `<plugin>:<destination>` for a plugin's destination.
    pub path: PathBuf,
    /// Snapshots the copy holds now.
    pub snapshots: usize,
    pub snapshots_copied: usize,
    pub objects_copied: usize,
    pub bytes_copied: u64,
    /// Stored files the copy had already.
    pub objects_present: usize,
    /// With `verify`: files the copy had that had changed, copied again.
    pub objects_repaired: usize,
    /// With `mirror`: what was removed from the copy.
    pub removed_snapshots: usize,
    pub removed_objects: usize,
    /// Snapshots left out because their files are damaged or missing in the store.
    pub problems: Vec<String>,
    /// Whether the copy now holds every snapshot.
    pub complete: bool,
    pub dry_run: bool,
}

/// A copy of a store: kept in the copy as `copy.json`, and in the store's `copies.json`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CopyRecord {
    pub path: PathBuf,
    /// The store copied: its id, and where it was.
    pub store: Uuid,
    pub from: PathBuf,
    /// When the latest copy started.
    pub started: DateTime<Utc>,
    /// When the latest copy finished; `None` while one runs, or after one was stopped.
    pub finished: Option<DateTime<Utc>>,
    /// When a copy last finished with every snapshot: the copy holds the store as it was then.
    pub last_complete: Option<DateTime<Utc>>,
    pub snapshots: usize,
    pub objects: usize,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<String>,
    /// A hash of what the copy held besides stored files (snapshots, libraries, labels, the
    /// store's other records) when the latest copy finished: one that changed since was written
    /// to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<Hash>,
}

/// A copy the store made, and how it stands now.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CopyStatus {
    #[serde(flatten)]
    pub record: CopyRecord,
    /// Whether the copy is there now (its disk mounted, its share reachable).
    pub available: bool,
    /// Whether it holds every library's latest snapshot, when available.
    pub up_to_date: Option<bool>,
}

/// The copy record a store holds when it is itself a copy.
pub fn marker(root: &Path) -> Result<Option<CopyRecord>> {
    fsutil::read_json_opt(&root.join("copy.json"))
}

/// What to say before a change is made to the store at `root` when it's a copy `store copy`
/// made: the next copy to it refuses what changed unless forced. `None` for any other store.
pub fn written_copy_warning(root: &Path) -> Option<String> {
    let copy = marker(root).ok()??;
    Some(format!(
        "{} is a copy of the store at {}: a change written to it makes the next `store copy` there refuse unless \
         forced (--force)",
        root.display(),
        copy.from.display()
    ))
}

/// The copies a store has made, most recent first.
pub fn copies(store: &Store) -> Result<Vec<CopyStatus>> {
    let id = store.id()?;
    let heads: IndexMap<Uuid, Option<Hash>> = store.libraries()?.into_iter().map(|l| (l.id, l.head)).collect();
    let mut out: Vec<CopyStatus> = records(store)?
        .into_iter()
        .map(|record| {
            let copy = Store::open(&record.path).ok().filter(|c| c.id().is_ok_and(|c| c == id));
            let up_to_date = copy.as_ref().map(|c| {
                c.libraries()
                    .map(|libs| libs.into_iter().map(|l| (l.id, l.head)).collect::<IndexMap<_, _>>() == heads)
                    .unwrap_or(false)
            });
            CopyStatus { available: copy.is_some(), up_to_date, record }
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.record.started));
    Ok(out)
}

fn records(store: &Store) -> Result<Vec<CopyRecord>> {
    Ok(fsutil::read_json_opt(&store.root().join("copies.json"))?.unwrap_or_default())
}

fn save_record(store: &Store, record: &CopyRecord) -> Result<()> {
    let mut all = records(store)?;
    all.retain(|r| r.path != record.path);
    all.push(record.clone());
    fsutil::write_json_atomic(&store.root().join("copies.json"), &all)
}

/// Copy the store to `dest` (see the module docs).
pub fn copy(store: &Store, dest: &Path, opts: &CopyOptions, ctx: &Ctx<'_>) -> Result<CopyReport> {
    let _lock = store.lock()?;
    let id = store.id()?;
    let dest = fsutil::resolved(dest);
    let root = fsutil::resolved(store.root());
    if dest.starts_with(&root) || root.starts_with(&dest) {
        return Err(Error::invalid(format!(
            "{} and the store ({}) can't be inside one another",
            dest.display(),
            root.display()
        )));
    }
    let existing = match Store::open(&dest) {
        Ok(copy) if copy.id()? == id => Some(copy),
        Ok(_) => {
            return Err(Error::conflict(
                Reason::DifferentStore,
                format!("{} holds a different store; copy somewhere else", dest.display()),
            )
            .with("path", &dest));
        }
        Err(_) if dest.is_dir() && fs::read_dir(&dest).at(&dest)?.next().is_some() => {
            return Err(Error::invalid(format!("{} isn't empty and isn't a copy of this store", dest.display())));
        }
        Err(_) => None,
    };

    let libraries = store.libraries()?;
    let pruned = store.pruned()?;
    let (snapshots, wanted) = holding(store)?;
    // A copy that moved on by itself (pulled into with --store) would lose that.
    let copy_libraries = match &existing {
        Some(copy) => copy.libraries()?,
        None => Vec::new(),
    };
    if !opts.force {
        let finished = marker(&dest)?.filter(|m| m.finished.is_some()).and_then(|m| m.contents);
        if finished.is_some_and(|c| Some(c) != contents(&dest).ok()) {
            return Err(Error::conflict(
                Reason::CopyAhead,
                format!(
                    "{} changed since it was last copied to (was it written to with --store?); force the copy to \
                     replace what changed",
                    dest.display()
                ),
            )
            .with("path", &dest)
            .action(Action::retry(serde_json::json!({ "force": true }), "Replace what changed")));
        }
        behind(&snapshots, &pruned, &copy_libraries)?;
    }
    let present: FxHashMap<Hash, u64> = match &existing {
        Some(copy) => copy.objects()?.into_iter().collect(),
        None => FxHashMap::default(),
    };
    let copy_snapshots: FxHashSet<Hash> = match &existing {
        Some(copy) => copy.snapshot_ids()?.into_iter().collect(),
        None => FxHashSet::default(),
    };
    let mut report = CopyReport { path: dest.clone(), dry_run: opts.dry_run, ..Default::default() };

    if opts.dry_run {
        for (h, (size, _)) in &wanted {
            if present.get(h) == Some(size) {
                report.objects_present += 1;
            } else {
                report.objects_copied += 1;
                report.bytes_copied += size;
            }
        }
        report.snapshots_copied = snapshots.keys().filter(|s| !copy_snapshots.contains(s)).count();
        report.snapshots = snapshots.len();
        if opts.mirror {
            report.removed_snapshots = copy_snapshots.iter().filter(|s| !snapshots.contains_key(*s)).count();
            report.removed_objects = present.keys().filter(|h| !wanted.contains_key(*h)).count();
        }
        return Ok(report);
    }

    // Start (or carry on) the copy. Its folder is made, but not the ones above it: a missing
    // `/Volumes/NAS` means the share isn't mounted, not a place to start a copy.
    if existing.is_none() {
        if !dest.is_dir() {
            // Only a missing parent means an unmounted disk: Windows also says a path through a
            // file isn't there.
            let parent_missing = dest.parent().is_none_or(|p| !p.exists());
            fs::create_dir(&dest).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound if parent_missing => Error::NotFound(format!(
                    "{}, the folder to make the copy in (is its disk or share mounted?)",
                    dest.parent().unwrap_or(&dest).display()
                )),
                _ => Error::Io { path: dest.clone(), source: e },
            })?;
        }
        for d in ["objects", "snapshots"] {
            fs::create_dir_all(dest.join(d)).at(dest.join(d))?;
        }
        fsutil::write_atomic(&dest.join("store.json"), &fs::read(store.root().join("store.json")).at(store.root())?)?;
    }
    let copy = Store::open(&dest)?;
    let _copy_lock = copy.lock()?;
    // What a stopped copy left half-written.
    for (p, _) in copy.leftovers()? {
        fsutil::remove_if_exists(&p)?;
    }
    let mut record = CopyRecord {
        path: dest.clone(),
        store: id,
        from: root.clone(),
        started: Utc::now(),
        finished: None,
        last_complete: marker(&dest)?.and_then(|m| m.last_complete),
        snapshots: 0,
        objects: wanted.len(),
        bytes: wanted.values().map(|(size, _)| size).sum(),
        problems: Vec::new(),
        contents: None,
    };
    fsutil::write_json_atomic(&dest.join("copy.json"), &record)?;
    save_record(store, &record)?;

    // Stored files, each checked against its name on the way.
    let mut todo = Vec::new();
    for (h, (size, path)) in &wanted {
        match present.get(h) {
            Some(s) if s == size && !opts.verify => report.objects_present += 1,
            Some(s) if s == size => {
                ctx.check()?;
                if hash::hash_file(&copy.object_path(h), ctx, Phase::Verify)? == *h {
                    report.objects_present += 1;
                } else {
                    report.objects_repaired += 1;
                    todo.push((h, *size, path));
                }
            }
            _ => todo.push((h, *size, path)),
        }
    }
    ctx.phase_start(Phase::Copy, todo.len() as u64, todo.iter().map(|(_, size, _)| size).sum());
    let mut broken: FxHashMap<Hash, String> = FxHashMap::default();
    let mut written = Vec::new();
    for (h, size, path) in todo {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Copy, path: path.clone(), bytes: size });
        let src = store.object_path(h);
        if !src.is_file() {
            broken.insert(*h, format!("{path} is missing from the store"));
            continue;
        }
        let dst = copy.object_path(h);
        match fsutil::copy_file(&src, &dst, None, Some(*h), false, ctx) {
            Ok(_) => {
                report.objects_copied += 1;
                report.bytes_copied += size;
                written.push(dst);
            }
            Err(Error::Corrupt { .. }) => {
                broken.insert(*h, format!("{path} is damaged in the store (see `store verify`)"));
            }
            Err(e) => return Err(e),
        }
    }
    fsutil::flush(&written)?;
    ctx.phase_end(Phase::Copy);

    // Snapshots whose files are all there.
    let mut copied: FxHashSet<Hash> = FxHashSet::default();
    for (sid, hashes) in &snapshots {
        if let Some(why) = hashes.iter().find_map(|h| broken.get(h)) {
            report.problems.push(format!("snapshot {} left out: {why}", sid.short()));
            continue;
        }
        if !copy_snapshots.contains(sid) {
            let bytes = fsutil::read(&store.snapshot_path(sid)).at(store.snapshot_path(sid))?;
            fsutil::write_atomic(&copy.snapshot_path(sid), &bytes)?;
            report.snapshots_copied += 1;
        }
        copied.insert(*sid);
    }

    // The rest of the store's records, then the libraries' latest snapshots.
    for name in METADATA {
        match fsutil::read(&store.root().join(name)) {
            Ok(bytes) => fsutil::write_atomic(&dest.join(name), &bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fsutil::remove_if_exists(&dest.join(name))?;
            }
            Err(e) => return Err(Error::Io { path: store.root().join(name), source: e }),
        }
    }
    fsutil::write_json_atomic(&dest.join("labels.json"), &labels(store, &copied)?)?;
    fsutil::write_json_atomic(&dest.join("libraries.json"), &heads(libraries, &copied, &copy_libraries))?;

    // What the store no longer has.
    if opts.mirror && report.problems.is_empty() {
        for sid in copy_snapshots.iter().filter(|s| !snapshots.contains_key(*s)) {
            fsutil::remove_if_exists(&copy.snapshot_path(sid))?;
            report.removed_snapshots += 1;
        }
        for h in present.keys().filter(|h| !wanted.contains_key(*h)) {
            fsutil::remove_if_exists(&copy.object_path(h))?;
            report.removed_objects += 1;
        }
    }

    report.snapshots = copy.snapshot_ids()?.len();
    report.complete = report.problems.is_empty();
    let now = Utc::now();
    record.finished = Some(now);
    if report.complete {
        record.last_complete = Some(now);
    }
    record.snapshots = report.snapshots;
    record.problems = report.problems.clone();
    record.contents = Some(contents(&dest)?);
    fsutil::write_json_atomic(&dest.join("copy.json"), &record)?;
    save_record(store, &record)?;
    Ok(report)
}

/// What the store holds: each snapshot and the stored files it lists, and each stored file
/// (once) with its size and a path it's at.
#[allow(clippy::type_complexity)]
fn holding(store: &Store) -> Result<(IndexMap<Hash, Vec<Hash>>, IndexMap<Hash, (u64, String)>)> {
    let mut snapshots: IndexMap<Hash, Vec<Hash>> = IndexMap::new();
    let mut wanted: IndexMap<Hash, (u64, String)> = IndexMap::new();
    for sid in store.snapshot_ids()? {
        let s = store.load_snapshot(&sid)?;
        let mut hashes = Vec::new();
        for f in s.files {
            let h =
                f.hash.ok_or_else(|| Error::invalid(format!("snapshot {}: {} has no hash", sid.short(), f.path)))?;
            wanted.entry(h).or_insert((f.size, f.path));
            hashes.push(h);
        }
        snapshots.insert(sid, hashes);
    }
    Ok((snapshots, wanted))
}

/// Refuses copying over a copy whose libraries' latest snapshots (`copy_libraries`) the store
/// has neither among its `snapshots` nor pruned: it was pulled into.
fn behind(
    snapshots: &IndexMap<Hash, Vec<Hash>>,
    pruned: &IndexMap<Hash, crate::store::PrunedSnapshot>,
    copy_libraries: &[LibraryRecord],
) -> Result<()> {
    let foreign: Vec<String> = copy_libraries
        .iter()
        .filter_map(|l| l.head.map(|h| (l, h)))
        .filter(|(_, h)| !snapshots.contains_key(h) && !pruned.contains_key(h))
        .map(|(l, h)| format!("{} ({})", l.name, h.short()))
        .collect();
    if foreign.is_empty() {
        return Ok(());
    }
    Err(Error::conflict(
        Reason::CopyAhead,
        format!(
            "the copy has snapshots this store doesn't: {}; was it pulled into? Force the copy to replace them",
            foreign.join(", ")
        ),
    )
    .with("snapshots", &foreign)
    .action(Action::retry(serde_json::json!({ "force": true }), "Replace them")))
}

/// The store's labels of the snapshots `copied`.
fn labels(store: &Store, copied: &FxHashSet<Hash>) -> Result<Vec<SnapshotLabel>> {
    Ok(store.labels()?.into_iter().filter(|l| copied.contains(&l.snapshot)).collect())
}

/// The libraries a copy gets: each one's latest snapshot when it was `copied`, else the one the
/// copy had (`copy_libraries`), if any.
fn heads(
    libraries: Vec<LibraryRecord>,
    copied: &FxHashSet<Hash>,
    copy_libraries: &[LibraryRecord],
) -> Vec<LibraryRecord> {
    libraries
        .into_iter()
        .filter_map(|l| match l.head {
            Some(h) if !copied.contains(&h) => copy_libraries.iter().find(|c| c.id == l.id).cloned(),
            _ => Some(l),
        })
        .collect()
}

/// A hash of what a copy holds besides stored files: its snapshots (by id), libraries, labels
/// and the store's other records. Anything that writes to the copy changes one of them.
fn contents(dest: &Path) -> Result<Hash> {
    let mut h = blake3::Hasher::new();
    let dir = dest.join("snapshots");
    let mut ids: Vec<String> = fs::read_dir(&dir)
        .at(&dir)?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    ids.sort();
    for id in &ids {
        h.update(id.as_bytes());
    }
    for name in ["libraries.json", "labels.json"].iter().chain(METADATA) {
        h.update(b"\0");
        h.update(name.as_bytes());
        match fsutil::read(&dest.join(name)) {
            Ok(bytes) => {
                h.update(&(bytes.len() as u64).to_le_bytes());
                h.update(&bytes);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                h.update(b"\0absent");
            }
            Err(e) => return Err(Error::Io { path: dest.join(name), source: e }),
        }
    }
    Ok(h.finalize().into())
}

/// What a plugin keeps a copy of the store in (a store destination, see [`crate::plugin`]):
/// stored files by hash, and the store's records by name, the names a copy's folder has them by
/// (`libraries.json`, `snapshots/<id>.json`). Only [`copy_to`] decides what goes there, and only
/// the host writes the store: what comes back is checked first.
pub trait Destination {
    /// Its plugin's id.
    fn plugin(&self) -> &str;
    /// Its name, among its plugin's destinations.
    fn name(&self) -> &str;
    /// Of `hashes` (a [`BATCH`] at most), those it doesn't have.
    fn missing(&self, hashes: &[Hash], ctx: &Ctx<'_>) -> Result<Vec<Hash>>;
    /// Keeps the stored file at `path` (in the store, read only), whose content is `hash`.
    fn put(&self, hash: &Hash, path: &Path, ctx: &Ctx<'_>) -> Result<()>;
    /// Writes its stored file `hash` to `into`, as it has it: whoever asks checks it.
    fn get(&self, hash: &Hash, into: &Path, ctx: &Ctx<'_>) -> Result<()>;
    /// Keeps `bytes` as the record `name`.
    fn put_manifest(&self, name: &str, bytes: &[u8], ctx: &Ctx<'_>) -> Result<()>;
    /// Its record `name`, if it has one.
    fn get_manifest(&self, name: &str, ctx: &Ctx<'_>) -> Result<Option<Vec<u8>>>;

    /// `<plugin>:<destination>`, as it's named to the user.
    fn shown(&self) -> String {
        format!("{}:{}", self.plugin(), self.name())
    }
}

/// The most hashes a destination is asked about at once.
pub const BATCH: usize = 1000;

/// A destination's index of the snapshots it holds, each with its library and parents: what
/// [`restore`] fetches by, put before `libraries.json`.
const INDEX: &str = "snapshots.json";

/// Of `hashes`, those `dest` doesn't have, asked [`BATCH`] at a time.
pub(crate) fn missing_from(dest: &dyn Destination, hashes: &[Hash], ctx: &Ctx<'_>) -> Result<FxHashSet<Hash>> {
    let mut missing = FxHashSet::default();
    for batch in hashes.chunks(BATCH) {
        ctx.check()?;
        missing.extend(dest.missing(batch, ctx)?);
    }
    Ok(missing)
}

/// `dest`'s record `name` as JSON, if it has one.
fn record<T: serde::de::DeserializeOwned>(dest: &dyn Destination, name: &str, ctx: &Ctx<'_>) -> Result<Option<T>> {
    let Some(bytes) = dest.get_manifest(name, ctx)? else { return Ok(None) };
    let damaged = |e: serde_json::Error| Error::External {
        command: format!("the {} destination", dest.shown()),
        message: format!("its {name} can't be read: {e}"),
    };
    serde_json::from_slice(&bytes).map(Some).map_err(damaged)
}

fn put_json(dest: &dyn Destination, name: &str, value: &impl Serialize, ctx: &Ctx<'_>) -> Result<()> {
    dest.put_manifest(name, &serde_json::to_vec_pretty(value).map_err(|e| Error::invalid(e.to_string()))?, ctx)
}

/// Copy the store to a plugin's destination, or bring the copy there up to date, as [`copy`] does
/// a folder (in the same order: stored files, then snapshots once every file they list is there,
/// then the store's other records, and `libraries.json` last), but for what a destination can't
/// do: remove (`mirror`) and re-read (`verify`) what it has. Each stored file is checked against
/// its hash before it goes; the account session never does. A destination holding another
/// store's copy (this one's before it was restored into a new store, say) is only copied over
/// when forced.
pub fn copy_to(store: &Store, dest: &dyn Destination, opts: &CopyOptions, ctx: &Ctx<'_>) -> Result<CopyReport> {
    let shown = dest.shown();
    if opts.mirror || opts.verify {
        return Err(Error::invalid(format!(
            "{shown} keeps what it's given: a copy there can't be mirrored or verified"
        )));
    }
    let _lock = store.lock()?;
    let id = store.id()?;
    let theirs: Option<serde_json::Value> = record(dest, "store.json", ctx)?;
    let ours = theirs.as_ref().is_some_and(|m| m["id"] == id.to_string());
    if theirs.is_some() && !ours && !opts.force {
        let said = format!("{shown} holds a different store's copy; force the copy to replace it");
        return Err(Error::conflict(Reason::DifferentStore, said)
            .with("path", &shown)
            .action(Action::retry(serde_json::json!({ "force": true }), "Replace it")));
    }
    let libraries = store.libraries()?;
    let (snapshots, wanted) = holding(store)?;
    let copy_libraries: Vec<LibraryRecord> = record(dest, "libraries.json", ctx)?.unwrap_or_default();
    if !opts.force {
        behind(&snapshots, &store.pruned()?, &copy_libraries)?;
    }
    let index: Vec<CatalogSnapshot> = record(dest, INDEX, ctx)?.unwrap_or_default();
    let had: FxHashSet<Hash> = index.iter().map(|s| s.id).collect();
    let hashes: Vec<Hash> = wanted.keys().copied().collect();
    let missing = missing_from(dest, &hashes, ctx)?;
    let mut report = CopyReport { path: shown.clone().into(), dry_run: opts.dry_run, ..Default::default() };
    report.objects_present = wanted.len() - wanted.keys().filter(|h| missing.contains(*h)).count();
    let todo: Vec<(&Hash, &(u64, String))> = wanted.iter().filter(|(h, _)| missing.contains(*h)).collect();
    if opts.dry_run {
        report.objects_copied = todo.len();
        report.bytes_copied = todo.iter().map(|(_, (size, _))| size).sum();
        report.snapshots_copied = snapshots.keys().filter(|s| !had.contains(s)).count();
        report.snapshots = snapshots.len();
        return Ok(report);
    }

    // Stored files, each checked against its hash first.
    ctx.phase_start(Phase::Copy, todo.len() as u64, todo.iter().map(|(_, (size, _))| size).sum());
    let mut broken: FxHashMap<Hash, String> = FxHashMap::default();
    for (h, (size, path)) in todo {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Copy, path: path.clone(), bytes: *size });
        let src = store.object_path(h);
        let damaged = || format!("{path} is damaged in the store (see `store verify`)");
        match hash::hash_file(&src, ctx, Phase::Verify) {
            Ok(got) if got == *h => {}
            Ok(_) => {
                broken.insert(*h, damaged());
                continue;
            }
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                broken.insert(*h, format!("{path} is missing from the store"));
                continue;
            }
            Err(e) => return Err(e),
        }
        dest.put(h, &src, ctx)?;
        report.objects_copied += 1;
        report.bytes_copied += size;
    }
    ctx.phase_end(Phase::Copy);

    // Snapshots whose files are all there, then the index of them.
    let mut copied: FxHashSet<Hash> = FxHashSet::default();
    for (sid, hashes) in &snapshots {
        if let Some(why) = hashes.iter().find_map(|h| broken.get(h)) {
            report.problems.push(format!("snapshot {} left out: {why}", sid.short()));
            continue;
        }
        if !had.contains(sid) {
            ctx.check()?;
            let bytes = fsutil::read(&store.snapshot_path(sid)).at(store.snapshot_path(sid))?;
            dest.put_manifest(&format!("snapshots/{}.json", sid.to_hex()), &bytes, ctx)?;
            report.snapshots_copied += 1;
        }
        copied.insert(*sid);
    }
    let mut index: Vec<CatalogSnapshot> = index;
    index.extend(catalog::snapshots(store)?.into_iter().filter(|s| copied.contains(&s.id) && !had.contains(&s.id)));

    // The rest of the store's records, then the libraries' latest snapshots.
    if !ours {
        dest.put_manifest("store.json", &fsutil::read(&store.root().join("store.json")).at(store.root())?, ctx)?;
    }
    for name in METADATA {
        match fsutil::read(&store.root().join(name)) {
            Ok(bytes) => dest.put_manifest(name, &bytes, ctx)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Io { path: store.root().join(name), source: e }),
        }
    }
    put_json(dest, "labels.json", &labels(store, &copied)?, ctx)?;
    put_json(dest, INDEX, &index, ctx)?;
    put_json(dest, "libraries.json", &heads(libraries, &copied, &copy_libraries), ctx)?;
    report.snapshots = index.len();
    report.complete = report.problems.is_empty();
    Ok(report)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RestoreOptions {
    /// Libraries to bring back, by name or id. Empty: all of them.
    #[serde(default)]
    pub libraries: Vec<String>,
    /// Say what would be fetched, and fetch nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// Bring into the store what a copy of it at a plugin's destination has that it lacks (after
/// losing this computer, say), as a fetch from another computer does (see [`crate::catalog`]):
/// snapshots and the stored files they list, each checked against its hash before it goes in,
/// then the libraries and labels; and the store's other records where it has none. Nothing the
/// store has is replaced.
pub fn restore(store: &Store, dest: &dyn Destination, opts: &RestoreOptions, ctx: &Ctx<'_>) -> Result<FetchReport> {
    let _lock = if opts.dry_run { None } else { Some(store.lock()?) };
    let shown = dest.shown();
    let theirs: serde_json::Value =
        record(dest, "store.json", ctx)?.ok_or_else(|| Error::NotFound(format!("a copy of a store at {shown}")))?;
    let damaged = |why: String| Error::External { command: format!("the {shown} destination"), message: why };
    let catalog = Catalog {
        store: serde_json::from_value(theirs["id"].clone()).map_err(|e| damaged(format!("its store.json: {e}")))?,
        libraries: record(dest, "libraries.json", ctx)?.unwrap_or_default(),
        snapshots: record(dest, INDEX, ctx)?.unwrap_or_default(),
        labels: record(dest, "labels.json", ctx)?.unwrap_or_default(),
    };
    let chosen = catalog::choose(&catalog, &opts.libraries)?;
    let lacking = catalog::lacking(store, &catalog, &chosen)?;
    ctx.phase_start(Phase::Scan, lacking.len() as u64, 0);
    let mut manifests = Vec::new();
    for id in &lacking {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: id.short(), bytes: 0 });
        // One it no longer has (removed there since) is left out.
        let Some(bytes) = dest.get_manifest(&format!("snapshots/{}.json", id.to_hex()), ctx)? else { continue };
        manifests.push(catalog::checked(id, &bytes).map_err(damaged)?);
    }
    ctx.phase_end(Phase::Scan);
    let (wanted, present) = catalog::needed(store, &manifests);
    let mut report = FetchReport {
        peer: shown,
        snapshots: manifests.len(),
        files: wanted.len(),
        bytes: wanted.values().sum(),
        present,
        dry_run: opts.dry_run,
        ..Default::default()
    };
    if opts.dry_run {
        report.libraries = catalog::settle(store, &catalog, &chosen, manifests, true)?;
        return Ok(report);
    }
    ctx.phase_start(Phase::Copy, wanted.len() as u64, report.bytes);
    let mut fetched = Vec::new();
    let got: Result<()> = wanted.iter().try_for_each(|(h, size)| {
        ctx.check()?;
        fetch_object(store, dest, h, ctx)?;
        ctx.emit(Event::Item { phase: Phase::Copy, path: h.short(), bytes: *size });
        fetched.push(*h);
        Ok(())
    });
    // What came before a failure is flushed, so the next restore can trust it.
    store.flush_objects(&fetched)?;
    got?;
    ctx.phase_end(Phase::Copy);
    for name in METADATA.iter().filter(|n| !store.root().join(n).exists()) {
        if let Some(bytes) = dest.get_manifest(name, ctx)? {
            fsutil::write_atomic(&store.root().join(name), &bytes)?;
        }
    }
    report.libraries = catalog::settle(store, &catalog, &chosen, manifests, false)?;
    Ok(report)
}

/// Copies `dest`'s stored file `hash` into the store, refused unless it hashes to `hash`. The
/// caller holds the store's lock and flushes it.
pub(crate) fn fetch_object(store: &Store, dest: &dyn Destination, hash: &Hash, ctx: &Ctx<'_>) -> Result<()> {
    let dir = store.root().join("objects").join("tmp");
    fs::create_dir_all(&dir).at(&dir)?;
    let tmp = fsutil::temp_in(tempfile::Builder::new().prefix("fetch-"), &dir)?;
    dest.get(hash, tmp.path(), ctx)?;
    if hash::hash_file(tmp.path(), ctx, Phase::Verify)? != *hash {
        return Err(Error::External {
            command: format!("the {} destination", dest.shown()),
            message: format!("a stored file came back damaged ({})", hash.short()),
        });
    }
    let dst = store.object_path(hash);
    let parent = dst.parent().expect("object path has parent");
    fs::create_dir_all(parent).at(parent)?;
    fsutil::replace(tmp.into_temp_path(), &dst)
}
