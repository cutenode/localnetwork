//! Mirroring library files between drives and store snapshots.
//!
//! A sync is two steps: [`plan`] computes exactly which files to copy and delete (cheap, no
//! writes, suitable for a confirmation dialog), and [`sync_drive_to_drive`] (or [`push`]) carries
//! it out, the target locked from before it's planned again until what it holds is recorded.
//!
//! Carrying it out orders writes so a drive is never left unusable:
//!
//! 1. new/changed audio, analysis and artwork files are copied (temp file + rename each), then
//!    flushed to the device together, and only then given their source's modification time;
//! 2. the database files (`export.pdb`, `exportExt.pdb`, `exportLibrary.db` and its WAL) are
//!    staged, then swapped in back to back;
//! 3. only then are files the new database no longer references deleted: database files the
//!    source lacks first (players may read one of them before the new ones), then the rest.
//!
//! Until step 2 the old database still finds every file it references (a file the sync
//! *changed* may already hold its new contents); after it, the new database finds all of its
//! files. With `delete_first`, stale files go before step 1 while the old database may still
//! use them, so it is taken off the drive first: the drive holds no library until step 2, rather
//! than one missing some of its files. An interruption leaves a playable drive (or, with
//! `delete_first`, one with no library) and a `pending` marker so the next run can report it. A
//! file whose data never reached the disk (a pulled plug) can't carry its source's modification
//! time, so the next run copies it again: while the marker is there, it copies anything modified
//! since the interrupted run started unless its content is known to match.
//!
//! OneLibrary's `exportLibrary.db` and `exportLibrary.db-wal` are one unit: SQLite replays a WAL
//! into whichever database sits next to it, so a new database beside the old WAL would be
//! corrupted on the next open. When either changes, the target's WAL (and its `-shm` index) is
//! removed before the new database is renamed in, and the source's WAL follows it.
//!
//! Settings files, rekordbox's per-device identity and what players and rekordbox keep for one
//! drive are never synced (see [`crate::paths::FileClass`]); settings are handled by
//! [`crate::settings`] so per-drive overrides survive. A drive that gets a whole library from
//! another drive gets the edits recorded on that one with it; from a snapshot, it forgets the
//! edits recorded for the library it held (see [`crate::edit::journal`]).

use indexmap::IndexMap;
use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};

use chrono::Utc;
use schemars::JsonSchema;
use serde::Serialize;

use crate::commit::Swap;
use crate::device::{self, Device, LineageEntry, LineageOp};
use crate::error::IoContext;
use crate::error::{Action, Reason};
use crate::fsutil;
use crate::hash::Hash;
use crate::paths::{self, FileClass, fold};
use crate::progress::{Ctx, Event, Phase, Scope};
use crate::store::{Snapshot, SnapshotSource, Store};
use crate::tree::{FileEntry, FileTree};
use crate::{Error, Result};

/// Where files come from.
pub enum Source<'a> {
    Drive { device: &'a Device, tree: &'a FileTree },
    Snapshot { store: &'a Store, snapshot: &'a Snapshot },
}

impl Source<'_> {
    pub fn label(&self) -> String {
        match self {
            Source::Drive { device, .. } => device.name(),
            Source::Snapshot { snapshot, .. } => format!("snapshot {}", snapshot.id.short()),
        }
    }

    fn entries(&self) -> Vec<&FileEntry> {
        match self {
            Source::Drive { tree, .. } => tree.library_entries().collect(),
            Source::Snapshot { snapshot, .. } => snapshot.files.iter().filter(|f| f.is_library()).collect(),
        }
    }

    fn fs_path(&self, e: &FileEntry) -> Result<PathBuf> {
        match self {
            Source::Drive { tree, .. } => Ok(tree.fs_path(&e.path)),
            Source::Snapshot { store, .. } => {
                let h = e.hash.ok_or_else(|| Error::invalid(format!("{} has no hash", e.path)))?;
                Ok(store.object_path(&h))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Remove files on the target that the source doesn't have, making it an exact mirror.
    /// Default `true`; only files locallibrary manages are ever deleted.
    pub delete: bool,
    /// Compare audio by content hash instead of size + modification time.
    pub verify: bool,
    /// Free space by deleting stale files *before* copying. Needed when the target is nearly
    /// full. The target's old databases go first (they may use the stale files), so if
    /// interrupted the drive holds no library until the next sync finishes the job.
    pub delete_first: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        SyncOptions { delete: true, verify: false, delete_first: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CopyReason {
    New,
    Changed,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlannedCopy {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub class: FileClass,
    pub reason: CopyReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<Hash>,
    /// Existing target key to replace when it differs only in letter case.
    #[serde(skip)]
    target_key: Option<String>,
    /// Size of the file being replaced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlannedDelete {
    pub path: String,
    pub size: u64,
    pub class: FileClass,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ClassCounts {
    pub audio: usize,
    pub analysis: usize,
    pub artwork: usize,
    pub database: usize,
    pub other: usize,
}

impl ClassCounts {
    fn add(&mut self, c: FileClass) {
        match c {
            FileClass::Audio => self.audio += 1,
            FileClass::Analysis => self.analysis += 1,
            FileClass::Artwork => self.artwork += 1,
            FileClass::Database => self.database += 1,
            _ => self.other += 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SyncPlan {
    /// Source drive name, or `snapshot <id>`.
    pub source: String,
    pub target_name: String,
    pub target: PathBuf,
    pub copies: Vec<PlannedCopy>,
    pub deletes: Vec<PlannedDelete>,
    pub unchanged: usize,
    pub copy_bytes: u64,
    pub delete_bytes: u64,
    pub copies_by_class: ClassCounts,
    pub deletes_by_class: ClassCounts,
    pub free_space: Option<u64>,
    /// Allocation unit of the target filesystem, used to estimate space.
    pub cluster_size: u64,
    /// Whether the copies fit in the free space (deletes happen last).
    pub fits: bool,
    pub warnings: Vec<String>,
}

impl SyncPlan {
    pub fn is_noop(&self) -> bool {
        self.copies.is_empty() && self.deletes.is_empty()
    }

    /// Whether two plans copy and delete the same files: a plan made again under the lock must
    /// be the one the user confirmed.
    pub fn same_changes(&self, other: &SyncPlan) -> bool {
        let copies = |p: &SyncPlan| -> BTreeSet<(String, u64)> {
            p.copies.iter().map(|c| (c.path.to_lowercase(), c.size)).collect()
        };
        let deletes = |p: &SyncPlan| -> BTreeSet<String> { p.deletes.iter().map(|d| d.path.to_lowercase()).collect() };
        copies(self) == copies(other) && deletes(self) == deletes(other)
    }

    /// [`Error::StalePlan`] unless this plan is `confirmed`'s (see [`Self::same_changes`]).
    fn check_confirmed(&self, confirmed: Option<&SyncPlan>) -> Result<()> {
        match confirmed {
            Some(c) if !self.same_changes(c) => {
                Err(Error::stale(format!("the files to copy to or delete from {}", self.target_name), "changed"))
            }
            _ => Ok(()),
        }
    }

    /// Whether a database file is replaced or removed.
    fn replaces_databases(&self) -> bool {
        self.copies.iter().any(|c| is_database_file(&c.path)) || self.deletes.iter().any(|d| is_database_file(&d.path))
    }

    /// Whether OneLibrary's database or its WAL is replaced or removed.
    fn touches_library_plus(&self) -> bool {
        self.copies.iter().any(|c| is_library_plus(&c.path)) || self.deletes.iter().any(|d| is_library_plus(&d.path))
    }

    /// Space needed on the target during apply, rounded to the filesystem's cluster size (exFAT
    /// on big drives uses 128 KiB clusters, which matters with thousands of small analysis
    /// files): new files, growth of replaced files, temp headroom for the largest replaced file,
    /// and all database files staged at once.
    pub fn bytes_needed(&self) -> u64 {
        let r = |n: u64| n.div_ceil(self.cluster_size.max(1)) * self.cluster_size.max(1);
        let mut needed = 0;
        let mut largest_replace = 0;
        for c in &self.copies {
            match (c.reason, c.target_size) {
                (CopyReason::New, _) | (_, None) => needed += r(c.size),
                (CopyReason::Changed, Some(old)) => {
                    needed += r(c.size).saturating_sub(r(old));
                    if c.class == FileClass::Database {
                        needed += r(c.size); // staged next to the old one until the commit
                    } else {
                        largest_replace = largest_replace.max(r(c.size));
                    }
                }
            }
        }
        needed + largest_replace
    }
}

/// `exportLibrary.db` or its WAL.
fn is_library_plus(key: &str) -> bool {
    key.eq_ignore_ascii_case(paths::LIBRARY_PLUS_DB) || key.eq_ignore_ascii_case(paths::LIBRARY_PLUS_WAL)
}

/// One of the files that make up a library's database (see [`paths::DATABASE_FILES`]).
fn is_database_file(key: &str) -> bool {
    paths::DATABASE_FILES.iter().any(|k| k.eq_ignore_ascii_case(key))
}

/// Whether `apply` takes the target's databases off the drive before it deletes anything: with
/// `delete_first`, stale files are deleted before the new ones are copied, while the old
/// databases may still use them. Each database file the source has is then copied, changed or
/// not, and the drive holds no library until they go in, rather than one missing its files.
fn takes_library_off_first(opts: &SyncOptions, deletes: &[PlannedDelete]) -> bool {
    opts.delete_first && deletes.iter().any(|d| d.class != FileClass::Database)
}

/// Copy the source's `e` over the target's `t` although they match (see the callers).
fn copy_again(plan: &mut SyncPlan, e: &FileEntry, t: &FileEntry) {
    plan.unchanged = plan.unchanged.saturating_sub(1);
    plan.copy_bytes += e.size;
    plan.copies_by_class.add(e.class);
    plan.copies.push(PlannedCopy {
        path: e.path.clone(),
        size: e.size,
        mtime: e.mtime,
        class: e.class,
        reason: CopyReason::Changed,
        hash: e.hash,
        target_key: Some(t.path.clone()),
        target_size: Some(t.size),
    });
}

/// After the state `copied` was synced or pushed onto `target`: the drive's own fingerprint, when
/// files the copy kept (`--keep-extra`) or had to skip make it differ. The drive then goes by both
/// (see [`device::StateAlias`]), so it doesn't look changed since, or newer than its source.
fn alias_after_copy(target: &Device, copied: Hash, ctx: &Ctx<'_>) -> Result<Option<device::StateAlias>> {
    let library = target.library().ok();
    let own = target.scan_fingerprinted(library.as_ref(), ctx)?.fingerprint;
    Ok(own.filter(|fp| *fp != copied).map(|fingerprint| device::StateAlias { fingerprint, holds: copied }))
}

/// What a sync or push leaves its target holding: the source's history, ending with `entry`
/// (whose `op` says which it is), and the store library it belongs to.
struct Holds<'a> {
    lineage: &'a [LineageEntry],
    entry: LineageEntry,
    library_id: Option<uuid::Uuid>,
}

/// Copy a library onto `target` (locked by the caller) as `plan` says, and record what it holds
/// then. Its player name and colour are put back and its pack record is forgotten: it holds the
/// whole library now. When its databases changed, its recorded edits become the source drive's,
/// which go with the library (a snapshot has none: they're forgotten, made to the library the
/// copy replaced). Once the copy is done, recording it isn't cancellable.
fn copy_library(
    plan: &SyncPlan,
    source: &Source<'_>,
    target: &mut Device,
    opts: &SyncOptions,
    holds: Holds<'_>,
    ctx: &Ctx<'_>,
) -> Result<SyncReport> {
    let databases = crate::commit::database_state(target);
    let mut report = apply(plan, source, target, opts, holds.entry.op, ctx)?;
    let ctx = &ctx.uncancellable();
    report.fingerprint = Some(holds.entry.fingerprint);
    crate::pack::forget(target)?;
    crate::collection_export::forget(target)?;
    if crate::commit::database_state(target) != databases {
        match source {
            Source::Drive { device, .. } => crate::edit::journal::copy(device, target)?,
            Source::Snapshot { .. } => crate::edit::journal::forget(target)?,
        }
    }
    let alias = alias_after_copy(target, holds.entry.fingerprint, ctx)?;
    let state = target.state_mut()?;
    state.alias = alias;
    state.inherit_with(holds.lineage, holds.entry);
    if holds.library_id.is_some() {
        state.library_id = holds.library_id;
    }
    target.finish(None)?;
    Ok(report)
}

/// Compare `source` against the target drive's `target_tree` and decide what to copy/delete.
///
/// Non-audio files must already be hashed on both sides. Audio is compared by size + mtime
/// unless `opts.verify`, in which case both sides must have audio hashes too.
pub fn plan(source: &Source<'_>, target: &Device, target_tree: &FileTree, opts: &SyncOptions) -> Result<SyncPlan> {
    let mut plan = SyncPlan {
        source: source.label(),
        target_name: target.name(),
        target: target.root().to_path_buf(),
        copies: Vec::new(),
        deletes: Vec::new(),
        unchanged: 0,
        copy_bytes: 0,
        delete_bytes: 0,
        copies_by_class: ClassCounts::default(),
        deletes_by_class: ClassCounts::default(),
        free_space: fsutil::available_space(target.root()),
        cluster_size: fsutil::cluster_size(target.root()),
        fits: true,
        warnings: Vec::new(),
    };

    // FAT32/exFAT/HFS+ are case-insensitive: match keys ignoring case so a file whose name only
    // changed case is not "copied" and then deleted as stale.
    let target_by_fold: FxHashMap<String, &FileEntry> =
        target_tree.library_entries().map(|e| (fold(&e.path), e)).collect();
    let mut source_folds = FxHashSet::default();
    let source_entries = source.entries();
    // The DJ's own files where the source keeps audio outside PIONEER/ and Contents/.
    let mut in_the_way = Vec::new();
    let cut_off = cut_off_since(target);
    // Files copied onto the target moments after their source was written (see
    // `fsutil::settled`): their size and mtime don't say they're still the same, so they're
    // compared by content.
    let index = target.load_index();

    for &e in &source_entries {
        if !paths::is_safe_key(&e.path) {
            plan.warnings.push(format!("skipping unsafe path {:?}", e.path));
            continue;
        }
        let f = fold(&e.path);
        if !source_folds.insert(f.clone()) {
            plan.warnings.push(format!("{} collides with another file differing only in case; skipped", e.path));
            continue;
        }
        // What to do, and the file it replaces (its key and size).
        let (reason, replaced) = match target_by_fold.get(&f) {
            Some(t) => {
                let suspect = cut_off.is_some_and(|since| t.mtime >= since);
                let same = match !suspect && index.racy(t) {
                    true => same_by_content(source, e, &target_tree.fs_path(&t.path), t.size)?,
                    false => same_content(e, t, opts, suspect)?,
                };
                ((!same).then_some(CopyReason::Changed), Some((t.path.clone(), t.size)))
            }
            None => match outside_file(source, target, &index, e, cut_off)? {
                Outside::Free => (Some(CopyReason::New), None),
                Outside::Same => (None, None),
                Outside::Ours(size) => (Some(CopyReason::Changed), Some((e.path.clone(), size))),
                Outside::Theirs => {
                    in_the_way.push(e.path.clone());
                    continue;
                }
            },
        };
        match reason {
            None => plan.unchanged += 1,
            Some(reason) => {
                plan.copy_bytes += e.size;
                plan.copies_by_class.add(e.class);
                plan.copies.push(PlannedCopy {
                    path: e.path.clone(),
                    size: e.size,
                    mtime: e.mtime,
                    class: e.class,
                    reason,
                    hash: e.hash,
                    target_size: replaced.as_ref().map(|r| r.1),
                    target_key: replaced.map(|r| r.0),
                });
            }
        }
    }
    if !in_the_way.is_empty() {
        let shown: Vec<&str> = in_the_way.iter().take(3).map(String::as_str).collect();
        let more = if in_the_way.len() > 3 { format!(" and {} more", in_the_way.len() - 3) } else { String::new() };
        return Err(Error::conflict(
            Reason::InTheWay,
            format!(
                "{} has files of its own where {}'s library keeps audio outside Contents/ ({}{more}), and its library \
                 doesn't use them; move or rename them, then sync again",
                target.name(),
                source.label(),
                shown.join(", ")
            ),
        )
        .with("paths", &in_the_way));
    }

    // Only files under PIONEER/ and Contents/ are ever deleted. Audio the database references
    // elsewhere on the drive is copied when missing, never removed. With deletes off
    // (`--keep-extra`) the target's other files stay, but not database files the source lacks:
    // the target's library is the source's, or players would read an old database beside it.
    for t in target_tree.library_entries() {
        let managed = paths::classify(&t.path).is_some_and(FileClass::is_library) && paths::is_safe_key(&t.path);
        if managed && (opts.delete || is_database_file(&t.path)) && !source_folds.contains(&fold(&t.path)) {
            plan.delete_bytes += t.size;
            plan.deletes_by_class.add(t.class);
            plan.deletes.push(PlannedDelete { path: t.path.clone(), size: t.size, class: t.class });
        }
    }

    // With `delete_first`, the target's databases come off the drive before the stale files they
    // may use, so every one the source has goes back in.
    if takes_library_off_first(opts, &plan.deletes) {
        for &e in source_entries.iter().filter(|e| is_database_file(&e.path)) {
            let f = fold(&e.path);
            if let Some(t) = target_by_fold.get(&f)
                && !plan.copies.iter().any(|c| fold(&c.path) == f)
            {
                copy_again(&mut plan, e, t);
            }
        }
    }

    // OneLibrary's database and WAL move together (see the module docs): if either changes, the
    // target's WAL is removed first, so the source's must be copied even when it's unchanged. (A
    // WAL the source doesn't have is deleted above, as a database file, even when deletes are off.)
    if plan.touches_library_plus() {
        let wal = fold(paths::LIBRARY_PLUS_WAL);
        if let Some(e) = source_entries.iter().find(|e| fold(&e.path) == wal)
            && let Some(t) = target_by_fold.get(&wal)
            && !plan.copies.iter().any(|c| fold(&c.path) == wal)
        {
            copy_again(&mut plan, e, t);
        }
    }

    // Database files last, OneLibrary's WAL after its database; deletes of database files (e.g.
    // exportLibrary.db disappearing) too.
    plan.copies.sort_by_key(|c| (c.class == FileClass::Database, c.path.clone()));
    if plan.copies.iter().any(|c| is_library_plus(&c.path)) {
        plan.warnings.extend(crate::player::no_name_note(target));
    }
    if plan.replaces_databases() {
        let from = match source {
            Source::Drive { device, .. } => Some(*device),
            Source::Snapshot { .. } => None,
        };
        plan.warnings.extend(crate::edit::journal::forget_warning(target, from, "this"));
    }
    if let Some(free) = plan.free_space {
        let needed = plan.bytes_needed();
        let available = if opts.delete_first { free + plan.delete_bytes } else { free };
        plan.fits = needed <= available;
        if !plan.fits {
            let hint = if !opts.delete_first && needed <= free + plan.delete_bytes {
                " (it would fit if stale files were deleted first)"
            } else {
                ""
            };
            plan.warnings.push(format!("needs {needed} bytes but only {free} are free on the target{hint}"));
        } else if let Some(w) =
            crate::space::headroom_warning(Some(free), needed, if opts.delete { plan.delete_bytes } else { 0 })
        {
            plan.warnings.push(w);
        }
    }
    Ok(plan)
}

/// How far back, before a run cut off on a drive started, its files are suspect: FAT's clock
/// counts in 2-second steps, and the computer's clock may have moved a little.
const CUT_OFF_SLACK_SECS: i64 = 60;

/// When the drive is marked as being written by a run that never finished (an unplugged drive, a
/// crash): the time from which its files may be torn. Copies get their source's modification time
/// only once their data is on the disk (see [`fsutil::copy_file`]), so a file renamed into place
/// whose data never got there shows a time after the run started.
pub(crate) fn cut_off_since(device: &Device) -> Option<i64> {
    let pending = device.state()?.pending.as_ref()?;
    Some(pending.started.timestamp() - CUT_OFF_SLACK_SECS)
}

/// What's on the target where the source has a file the target's scan doesn't list.
enum Outside {
    /// Nothing: the file is new.
    Free,
    /// The same file (as an earlier sync left it): taken as it is.
    Same,
    /// Written by a run cut off on this drive (see [`cut_off_since`]), with this size: copied again.
    Ours(u64),
    /// A file of the DJ's that the target's library doesn't use: never overwritten.
    Theirs,
}

/// The target's scan lists everything under PIONEER/ and Contents/, and audio its own library
/// keeps elsewhere. Anywhere else, a file at the source's path is the DJ's, unless it's the same
/// or a run cut off on this drive wrote it.
fn outside_file(
    source: &Source<'_>,
    target: &Device,
    index: &crate::tree::HashIndex,
    e: &FileEntry,
    cut_off: Option<i64>,
) -> Result<Outside> {
    if paths::classify(&e.path).is_some() {
        return Ok(Outside::Free);
    }
    let path = target.checked_path(&e.path)?;
    let Ok(meta) = std::fs::symlink_metadata(&path) else { return Ok(Outside::Free) };
    if !meta.is_file() {
        return Ok(Outside::Theirs);
    }
    let ours = cut_off.is_some_and(|since| fsutil::mtime_secs(&meta) >= since);
    let here = FileEntry {
        path: e.path.clone(),
        size: meta.len(),
        mtime: fsutil::mtime_secs(&meta),
        class: e.class,
        hash: None,
    };
    let same = meta.len() == e.size
        && match e.hash {
            Some(h) => crate::hash::hash_file(&path, &Ctx::silent(), crate::progress::Phase::Hash)? == h,
            None if ours => false,
            None if index.racy(&here) => same_by_content(source, e, &path, here.size)?,
            None => fsutil::mtime_eq(here.mtime, e.mtime),
        };
    Ok(match (same, ours) {
        (true, _) => Outside::Same,
        (false, true) => Outside::Ours(meta.len()),
        (false, false) => Outside::Theirs,
    })
}

/// Whether the target's file at `path` (`size` bytes) holds the source's `src`, by content: for a
/// copy made moments after its source was written (see [`crate::tree::HashIndex::racy`]).
fn same_by_content(source: &Source<'_>, src: &FileEntry, path: &Path, size: u64) -> Result<bool> {
    if src.size != size {
        return Ok(false);
    }
    let hash = |p: &Path| crate::hash::hash_file(p, &Ctx::silent(), crate::progress::Phase::Hash);
    let theirs = match src.hash {
        Some(h) => h,
        None => hash(&source.fs_path(src)?)?,
    };
    Ok(hash(path)? == theirs)
}

/// Whether the target's file `dst` already holds the source's `src`. A `suspect` file (see
/// [`cut_off_since`]) is only taken as it is when its content is known to match.
fn same_content(src: &FileEntry, dst: &FileEntry, opts: &SyncOptions, suspect: bool) -> Result<bool> {
    if src.size != dst.size {
        return Ok(false);
    }
    if let (Some(a), Some(b)) = (src.hash, dst.hash) {
        return Ok(a == b);
    }
    if suspect {
        return Ok(false);
    }
    if device::needs_content_hash(src) || opts.verify {
        return Err(Error::invalid(format!("{} must be hashed on both sides before planning", src.path)));
    }
    Ok(fsutil::mtime_eq(src.mtime, dst.mtime))
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct SyncReport {
    pub copied: usize,
    pub copied_bytes: u64,
    pub deleted: usize,
    pub deleted_bytes: u64,
    pub unchanged: usize,
    pub fingerprint: Option<Hash>,
}

/// Execute a plan, the target locked by the caller and marked as being written by `op` (a sync
/// or a push). Hashes of copied files are recorded in the target's hash cache. A new
/// `exportLibrary.db` gets the target's player name and colour back (see [`crate::player::stash`]).
fn apply(
    plan: &SyncPlan,
    source: &Source<'_>,
    target: &mut Device,
    opts: &SyncOptions,
    op: LineageOp,
    ctx: &Ctx<'_>,
) -> Result<SyncReport> {
    if !plan.fits {
        let free = plan.free_space.unwrap_or(0);
        return Err(Error::NoSpace { path: plan.target.clone(), needed: plan.bytes_needed(), available: free });
    }
    let by_key: FxHashMap<&str, &FileEntry> = source.entries().into_iter().map(|e| (e.path.as_str(), e)).collect();
    let mut report = SyncReport { unchanged: plan.unchanged, ..Default::default() };
    let mut index = target.load_index();

    target.begin(op, plan.source.clone(), None)?;
    let identity = crate::player::stash(target, plan.touches_library_plus())?;

    let (db_deletes, file_deletes): (Vec<&PlannedDelete>, Vec<&PlannedDelete>) =
        plan.deletes.iter().partition(|d| d.class == FileClass::Database);
    let (db_copies, file_copies): (Vec<&PlannedCopy>, Vec<&PlannedCopy>) =
        plan.copies.iter().partition(|c| c.class == FileClass::Database);
    if opts.delete_first {
        // The old databases may use the stale files: they go first (see `takes_library_off_first`).
        if takes_library_off_first(opts, &plan.deletes) {
            ctx.check()?;
            take_off_databases(target, &db_copies, &db_deletes, &mut index)?;
        }
        delete_files(target, &file_deletes, &mut index, &mut report, ctx)?;
    }

    // 1. Audio, analysis, artwork: copied without per-file fsync, then flushed together, and only
    //    then given their source's modification time: until a copy's data is on the disk it must
    //    not pass for a finished one (see `cut_off_since`). A run stopped part way (Ctrl-C, an
    //    error) still flushes and stamps what it copied, so the next one doesn't copy it again.
    ctx.phase_start(Phase::Copy, plan.copies.len() as u64, plan.copy_bytes);
    let mut written = Vec::new();
    let mut stopped = None;
    for c in &file_copies {
        match copy_one(c, &by_key, source, target, ctx) {
            Ok(copied) => {
                written.push(copied);
                report.copied += 1;
                report.copied_bytes += c.size;
            }
            Err(e) => {
                stopped = Some(e);
                break;
            }
        }
    }
    // Barrier: everything the new database references is on disk before it's replaced.
    settle(&written, &mut index)?;
    if let Some(e) = stopped {
        target.save_index(&index)?;
        return Err(e);
    }

    // 2. Database files: staged first, then swapped in back to back with no cancellation point in
    //    between (`commit::swap`), so export.pdb / exportExt.pdb / exportLibrary.db always match
    //    each other. OneLibrary's old WAL goes before its new database or WAL is renamed in
    //    (module docs).
    let wal: &[&str] =
        if plan.touches_library_plus() { &[paths::LIBRARY_PLUS_WAL, paths::LIBRARY_PLUS_SHM] } else { &[] };
    let mut steps: Vec<Swap> =
        wal.iter().map(|key| target.checked_path(key).map(Swap::Remove)).collect::<Result<_>>()?;
    let mut staged = Vec::new();
    for c in &db_copies {
        ctx.check()?;
        let src_entry = by_key.get(c.path.as_str()).ok_or_else(|| Error::NotFound(c.path.clone()))?;
        let dst_key = c.target_key.as_deref().unwrap_or(&c.path).to_string();
        let dst = target.checked_path(&dst_key)?;
        ctx.emit(Event::Item { phase: Phase::Copy, path: c.path.clone(), bytes: c.size });
        let (tmp, h) = fsutil::stage_copy(&source.fs_path(src_entry)?, &dst, Some(c.mtime), c.hash, ctx)?;
        steps.push(Swap::Place(tmp.into_temp_path(), dst.clone()));
        staged.push((dst, dst_key, c.class, h, c.size));
    }
    crate::commit::swap(steps)?;
    for key in wal {
        index.forget(key);
    }
    for (dst, dst_key, class, h, size) in staged {
        record(&mut index, &dst, &dst_key, class, h)?;
        report.copied += 1;
        report.copied_bytes += size;
    }
    ctx.phase_end(Phase::Copy);

    // 3. Only now remove what the new database no longer references: first database files the
    //    source lacks (players may read an old export.pdb before a new exportLibrary.db), so none
    //    outlives the files it uses.
    delete_files(target, &db_deletes, &mut index, &mut report, ctx)?;
    if !opts.delete_first {
        delete_files(target, &file_deletes, &mut index, &mut report, ctx)?;
    }
    target.save_index(&index)?;
    crate::player::preserve(target, identity.as_ref())?;
    Ok(report)
}

/// Take the target's databases off the drive before `delete_first` deletes files they may use:
/// each one the plan replaces (`replaced`) or removes (`removed`), OneLibrary's WAL and its index
/// before its database. Until the new ones go in, the drive holds no library.
fn take_off_databases(
    target: &Device,
    replaced: &[&PlannedCopy],
    removed: &[&PlannedDelete],
    index: &mut crate::tree::HashIndex,
) -> Result<()> {
    let mut keys: Vec<&str> = replaced
        .iter()
        .filter_map(|c| c.target_key.as_deref())
        .chain(removed.iter().map(|d| d.path.as_str()))
        .collect();
    if keys.iter().any(|k| is_library_plus(k)) {
        keys.splice(0..0, [paths::LIBRARY_PLUS_WAL, paths::LIBRARY_PLUS_SHM]);
    }
    for key in keys {
        if fsutil::remove_if_exists(&target.checked_path(key)?)? {
            index.forget(key);
        }
    }
    Ok(())
}

/// A file copied onto the target, its data not yet flushed nor its modification time set.
struct Copied {
    path: PathBuf,
    key: String,
    class: FileClass,
    hash: Hash,
    mtime: i64,
}

fn copy_one(
    c: &PlannedCopy,
    by_key: &FxHashMap<&str, &FileEntry>,
    source: &Source<'_>,
    target: &Device,
    ctx: &Ctx<'_>,
) -> Result<Copied> {
    ctx.check()?;
    let src_entry = by_key.get(c.path.as_str()).ok_or_else(|| Error::NotFound(c.path.clone()))?;
    // When the target has the file under a name differing only in case, overwrite it in place:
    // players resolve paths case-insensitively on FAT32/exFAT/HFS+.
    let key = c.target_key.as_deref().unwrap_or(&c.path);
    let path = target.checked_path(key)?;
    // A file that turned up outside the library since the plan is the DJ's (see `outside_file`).
    if c.reason == CopyReason::New && paths::classify(key).is_none() && std::fs::symlink_metadata(&path).is_ok() {
        return Err(Error::conflict(
            Reason::InTheWay,
            format!(
                "{key} appeared on {} since the sync was planned; move or rename it, then sync again",
                target.name()
            ),
        )
        .with("paths", [key]));
    }
    ctx.emit(Event::Item { phase: Phase::Copy, path: c.path.clone(), bytes: c.size });
    let hash = fsutil::copy_file(&source.fs_path(src_entry)?, &path, None, c.hash, false, ctx)?;
    Ok(Copied { path, key: key.to_string(), class: c.class, hash, mtime: c.mtime })
}

/// Flush copied files to the device, then give each its source's modification time and record
/// its hash.
fn settle(copied: &[Copied], index: &mut crate::tree::HashIndex) -> Result<()> {
    fsutil::flush(&copied.iter().map(|c| c.path.clone()).collect::<Vec<_>>())?;
    for c in copied {
        fsutil::set_mtime(&c.path, c.mtime)?;
        record(index, &c.path, &c.key, c.class, c.hash)?;
    }
    Ok(())
}

/// Note in `index` that the file at `path` (`key`) holds `hash`, as it is now.
pub(crate) fn record(
    index: &mut crate::tree::HashIndex,
    path: &Path,
    key: &str,
    class: FileClass,
    hash: Hash,
) -> Result<()> {
    let meta = std::fs::metadata(path).at(path)?;
    index.record(&FileEntry {
        path: key.to_string(),
        size: meta.len(),
        mtime: fsutil::mtime_secs(&meta),
        class,
        hash: Some(hash),
    });
    Ok(())
}

fn delete_files(
    target: &Device,
    deletes: &[&PlannedDelete],
    index: &mut crate::tree::HashIndex,
    report: &mut SyncReport,
    ctx: &Ctx<'_>,
) -> Result<()> {
    if deletes.is_empty() {
        return Ok(());
    }
    ctx.phase_start(Phase::Delete, deletes.len() as u64, deletes.iter().map(|d| d.size).sum());
    for d in deletes {
        ctx.check()?;
        let p = target.checked_path(&d.path)?;
        ctx.emit(Event::Item { phase: Phase::Delete, path: d.path.clone(), bytes: d.size });
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Io { path: p, source: e }),
        }
        index.forget(&d.path);
        if let Some(parent) = p.parent() {
            fsutil::prune_empty_dirs(parent, target.root());
        }
        report.deleted += 1;
        report.deleted_bytes += d.size;
    }
    ctx.phase_end(Phase::Delete);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// High-level operations
// ---------------------------------------------------------------------------------------------

/// Scan both drives with everything hashed that planning needs.
fn scan_pair(source: &Device, target: &Device, opts: &SyncOptions, ctx: &Ctx<'_>) -> Result<(FileTree, FileTree)> {
    let src_lib = source.library().ok();
    let mut src = source.scan_fingerprinted(src_lib.as_ref(), ctx)?.tree;
    let tgt_lib = target.library().ok();
    let mut tgt = target.scan_fingerprinted(tgt_lib.as_ref(), ctx)?.tree;
    if opts.verify {
        let mut si = source.load_index();
        src.ensure_hashes(&mut si, |e| e.class.is_library(), ctx)?;
        source.save_index(&si)?;
        let mut ti = target.load_index();
        tgt.ensure_hashes(&mut ti, |e| e.class.is_library(), ctx)?;
        target.save_index(&ti)?;
    }
    Ok((src, tgt))
}

/// Plan a drive-to-drive mirror.
pub fn plan_drive_to_drive(
    source: &Device,
    target: &Device,
    opts: &SyncOptions,
    ctx: &Ctx<'_>,
) -> Result<(SyncPlan, FileTree)> {
    if !source.has_export() {
        return Err(Error::NotAnExport(source.root().to_path_buf()));
    }
    source.finished()?;
    let (src_tree, tgt_tree) = scan_pair(source, target, opts, ctx)?;
    let plan = plan(&Source::Drive { device: source, tree: &src_tree }, target, &tgt_tree, opts)?;
    Ok((plan, src_tree))
}

/// Mirror `source` onto `target` and hand `target` the source's lineage.
pub fn sync_drive_to_drive(
    source: &mut Device,
    target: &mut Device,
    opts: &SyncOptions,
    ctx: &Ctx<'_>,
) -> Result<SyncReport> {
    sync_pair(source, target, opts, None, ctx)
}

/// [`sync_drive_to_drive`], refused with [`Error::StalePlan`] unless what it would copy and
/// delete is what `confirmed` (the plan the user was shown) does.
pub fn sync_drive_to_drive_as_planned(
    source: &mut Device,
    target: &mut Device,
    opts: &SyncOptions,
    confirmed: &SyncPlan,
    ctx: &Ctx<'_>,
) -> Result<SyncReport> {
    sync_pair(source, target, opts, Some(confirmed), ctx)
}

fn sync_pair(
    source: &mut Device,
    target: &mut Device,
    opts: &SyncOptions,
    confirmed: Option<&SyncPlan>,
    ctx: &Ctx<'_>,
) -> Result<SyncReport> {
    if source.root() == target.root() {
        return Err(Error::invalid("source and target are the same drive"));
    }
    // The source's state file is updated too, so hold it as well, and read it again once held.
    // The target is held from before it's planned again until what it holds is recorded.
    let _source_lock = source.lock()?;
    source.reload_state()?;
    let _target_lock = crate::commit::lock(target, None)?;
    let (plan, src_tree) = plan_drive_to_drive(source, target, opts, ctx)?;
    plan.check_confirmed(confirmed)?;
    let src_fp = source.fingerprint(&src_tree)?.ok_or_else(|| Error::NotAnExport(source.root().into()))?;
    let tracks = source.library().map(|l| l.tracks.len()).unwrap_or(0);

    // The source's current state becomes part of its own recorded history.
    source.state_mut()?.record(LineageEntry {
        fingerprint: src_fp,
        at: Utc::now(),
        op: LineageOp::Observed,
        snapshot: None,
        source: None,
        tracks,
    });
    source.save_state()?;
    let source_id = source.id();
    let source_lineage = source.state().map(|s| s.lineage.clone()).unwrap_or_default();
    let library_id = source.state().and_then(|s| s.library_id);

    let holds = Holds {
        lineage: &source_lineage,
        entry: LineageEntry {
            fingerprint: src_fp,
            at: Utc::now(),
            op: LineageOp::Sync,
            snapshot: None,
            source: source_id,
            tracks,
        },
        library_id,
    };
    copy_library(&plan, &Source::Drive { device: source, tree: &src_tree }, target, opts, holds, ctx)
}

/// Forwards one target's events during a parallel sync: its file names and warnings carry the
/// drive's name, byte counts add to the shared bar, and its own phase boundaries are dropped (the
/// caller reports one phase for all targets). Every event is scoped to the drive's root.
struct TargetProgress<'a> {
    inner: &'a dyn crate::progress::Progress,
    label: String,
    scope: Scope,
}

impl crate::progress::Progress for TargetProgress<'_> {
    fn on_event(&self, event: &Event) {
        match event {
            Event::Item { phase, path, bytes } => self.inner.on_scoped_event(
                &self.scope,
                &Event::Item { phase: *phase, path: format!("{}: {path}", self.label), bytes: *bytes },
            ),
            Event::Warning { message } => self
                .inner
                .on_scoped_event(&self.scope, &Event::Warning { message: format!("{}: {message}", self.label) }),
            Event::Bytes { phase: Phase::Copy, .. } => self.inner.on_scoped_event(&self.scope, event),
            _ => {}
        }
    }
}

/// How a target drive is named in progress events' `target`: its root, as in the results.
pub(crate) fn target_label(d: &Device) -> String {
    d.root().display().to_string()
}

/// Mirror `source` onto several drives at once. The source is scanned once; each target is
/// scanned and planned in turn, then all of them are written in parallel (one thread each), so
/// three sticks take about as long as the slowest. A failing target doesn't stop the others:
/// results come back in `targets` order.
pub fn sync_drive_to_many(
    source: &mut Device,
    targets: &mut [Device],
    opts: &SyncOptions,
    ctx: &Ctx<'_>,
) -> Result<Vec<Result<SyncReport>>> {
    sync_many(source, targets, opts, None, ctx)
}

/// [`sync_drive_to_many`], each target refused with [`Error::StalePlan`] unless what it would
/// copy and delete there is what its plan in `confirmed` (in `targets` order) does.
pub fn sync_drive_to_many_as_planned(
    source: &mut Device,
    targets: &mut [Device],
    opts: &SyncOptions,
    confirmed: &[SyncPlan],
    ctx: &Ctx<'_>,
) -> Result<Vec<Result<SyncReport>>> {
    if confirmed.len() != targets.len() {
        return Err(Error::invalid("one confirmed plan per target"));
    }
    sync_many(source, targets, opts, Some(confirmed), ctx)
}

fn sync_many(
    source: &mut Device,
    targets: &mut [Device],
    opts: &SyncOptions,
    confirmed: Option<&[SyncPlan]>,
    ctx: &Ctx<'_>,
) -> Result<Vec<Result<SyncReport>>> {
    if !source.has_export() {
        return Err(Error::NotAnExport(source.root().to_path_buf()));
    }
    if let Some(t) = targets.iter().find(|t| t.root() == source.root()) {
        return Err(Error::invalid(format!("{} is both the source and a target", t.name())));
    }
    let _source_lock = source.lock()?;
    source.reload_state()?;
    source.finished()?;
    let src_lib = source.library().ok();
    let mut src_tree = source.scan_fingerprinted(src_lib.as_ref(), ctx)?.tree;
    if opts.verify {
        let mut index = source.load_index();
        src_tree.ensure_hashes(&mut index, |e| e.class.is_library(), ctx)?;
        source.save_index(&index)?;
    }
    let src_fp = source.fingerprint(&src_tree)?.ok_or_else(|| Error::NotAnExport(source.root().into()))?;
    let tracks = src_lib.as_ref().map(|l| l.tracks.len()).unwrap_or(0);
    source.state_mut()?.record(LineageEntry {
        fingerprint: src_fp,
        at: Utc::now(),
        op: LineageOp::Observed,
        snapshot: None,
        source: None,
        tracks,
    });
    source.save_state()?;
    let source_id = source.id();
    let lineage = source.state().map(|s| s.lineage.clone()).unwrap_or_default();
    let library_id = source.state().and_then(|s| s.library_id);
    let source: &Device = source;
    let src = Source::Drive { device: source, tree: &src_tree };

    // Plan every target first (scanning reports its own progress), so the copy phase can show
    // one total. Each is held from before it's planned until what it holds is recorded.
    let mut plans: Vec<Result<SyncPlan>> = Vec::new();
    let mut locks = Vec::new();
    for (i, t) in targets.iter().enumerate() {
        let plan = ctx.for_target(target_label(t), |ctx| -> Result<SyncPlan> {
            locks.push(crate::commit::lock(t, None)?);
            let lib = t.library().ok();
            let mut tree = t.scan_fingerprinted(lib.as_ref(), ctx)?.tree;
            if opts.verify {
                let mut index = t.load_index();
                tree.ensure_hashes(&mut index, |e| e.class.is_library(), ctx)?;
                t.save_index(&index)?;
            }
            let plan = plan(&src, t, &tree, opts)?;
            plan.check_confirmed(confirmed.map(|c| &c[i]))?;
            Ok(plan)
        });
        plans.push(plan);
    }
    let (items, bytes) =
        plans.iter().flatten().fold((0u64, 0u64), |(i, b), p| (i + p.copies.len() as u64, b + p.copy_bytes));
    ctx.phase_start(Phase::Copy, items, bytes);
    let results = std::thread::scope(|s| {
        let handles: Vec<_> = targets
            .iter_mut()
            .zip(plans)
            .map(|(t, plan)| {
                let (src, lineage, cancel) = (&src, &lineage, ctx.cancel.clone());
                s.spawn(move || -> Result<SyncReport> {
                    let plan = plan?;
                    let scope = Scope { target: Some(target_label(t)), step: None };
                    let progress = TargetProgress { inner: ctx.progress, label: t.name(), scope };
                    let tctx = Ctx::new(&progress, cancel);
                    let holds = Holds {
                        lineage,
                        entry: LineageEntry {
                            fingerprint: src_fp,
                            at: Utc::now(),
                            op: LineageOp::Sync,
                            snapshot: None,
                            source: source_id,
                            tracks,
                        },
                        library_id,
                    };
                    copy_library(&plan, src, t, opts, holds, &tctx)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|p| {
                    let said = crate::error::panic_text(&*p);
                    Err(Error::invalid(format!("internal error: a sync thread panicked: {said}")))
                })
            })
            .collect()
    });
    ctx.phase_end(Phase::Copy);
    Ok(results)
}

// -- store <-> drive ------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct PullOptions {
    /// Library name to pull into (created if missing). Defaults to the drive's linked library,
    /// else a new library named after the drive.
    pub library: Option<String>,
    pub message: Option<String>,
    /// Record the drive's state on top of the library head even if it isn't a descendant, and
    /// pull a drive whose last write didn't finish (packing saves a drive's state this way before
    /// it replaces it).
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PullReport {
    pub library: String,
    pub library_id: uuid::Uuid,
    pub snapshot: Option<crate::store::SnapshotInfo>,
    pub up_to_date: bool,
    pub imported_files: usize,
    pub imported_bytes: u64,
    pub reused_files: usize,
}

/// Snapshot a drive into the store.
pub fn pull(store: &Store, device: &mut Device, opts: &PullOptions, ctx: &Ctx<'_>) -> Result<PullReport> {
    pull_into(store, device, opts, None, ctx)
}

/// Snapshot a drive the library has moved on from, beside the library's head rather than on
/// top of it: the snapshot's parent is `parent` (the last state the drive shared with the
/// library) and the head stays where it is. A drive back from a set after a newer export was
/// pulled keeps its history lists and cues in the store this way, while the newest library
/// stays the one `push` writes. A drive whose state is saved already (its lineage says so) is
/// up to date. `opts.force` only lets it pull a drive whose last write didn't finish.
pub fn pull_beside(
    store: &Store,
    device: &mut Device,
    opts: &PullOptions,
    parent: Hash,
    ctx: &Ctx<'_>,
) -> Result<PullReport> {
    pull_into(store, device, opts, Some(parent), ctx)
}

fn pull_into(
    store: &Store,
    device: &mut Device,
    opts: &PullOptions,
    beside: Option<Hash>,
    ctx: &Ctx<'_>,
) -> Result<PullReport> {
    let _lock = store.lock()?;
    let _drive_lock = device.lock()?;
    device.reload_state()?;
    if !opts.force {
        device.finished()?;
    }
    let library = device.library()?;
    // Adopt up front so the hash cache built while importing is kept for the next pull.
    device.adopt(None)?;
    let scanned = device.scan_fingerprinted(Some(&library), ctx)?;
    let fp = scanned.fingerprint.ok_or_else(|| Error::NotAnExport(device.root().into()))?;
    let legacy = scanned.legacy_fingerprints.clone();
    let recorded: Vec<Hash> = scanned.fingerprints().collect();
    let mut tree = scanned.tree;

    // Resolve the target library.
    let linked = device.state().and_then(|s| s.library_id);
    let lib = match (&opts.library, linked) {
        (Some(name), _) => match store.find_library(name)? {
            Some(l) => l,
            None => store.create_library(name)?,
        },
        (None, Some(id)) => match store.library_by_id(id)? {
            Some(l) => l,
            None => store.create_library(&store.unused_library_name(&device.name())?)?,
        },
        (None, None) => store.create_library(&store.unused_library_name(&device.name())?)?,
    };

    // Relationship between the drive and the library head.
    if beside.is_some() {
        let saved = (device.state().and_then(|s| s.head()))
            .filter(|h| h.fingerprint == fp)
            .and_then(|h| h.snapshot)
            .and_then(|id| store.load_snapshot(&id).ok())
            .filter(|s| s.library_id == lib.id);
        if let Some(s) = saved {
            return Ok(PullReport {
                library: lib.name.clone(),
                library_id: lib.id,
                snapshot: Some(s.info()),
                up_to_date: true,
                imported_files: 0,
                imported_bytes: 0,
                reused_files: 0,
            });
        }
    } else if let Some(head_id) = lib.head {
        let head = store.load_snapshot(&head_id)?;
        if head.fingerprint == fp || legacy.contains(&head.fingerprint) {
            link_device(store, device, &lib, fp, Some(head_id), LineageOp::Pull, library.tracks.len())?;
            return Ok(PullReport {
                library: lib.name.clone(),
                library_id: lib.id,
                snapshot: Some(head.info()),
                up_to_date: true,
                imported_files: 0,
                imported_bytes: 0,
                reused_files: 0,
            });
        }
        let descends = device.state().is_some_and(|s| s.knows(&head.fingerprint, &head.lineage));
        if !descends && !opts.force {
            let mut older = None;
            for fp in &recorded {
                older = store.find_by_fingerprint(&lib, fp)?;
                if older.is_some() {
                    break;
                }
            }
            // Older when it's saved in the library, or the library's latest went through it
            // after every state the two share (as compare tells), saved or not.
            let lineage = device.state().map_or(&[][..], |s| &s.lineage[..]);
            let behind = older.is_some() || crate::device::has_held(&head.lineage, &recorded, lineage);
            let drive = device.root().display().to_string();
            let refused = if behind {
                let named = older.as_ref().map(|s| format!(" (snapshot {})", s.short_id)).unwrap_or_default();
                let older_than = Error::conflict(
                    Reason::OlderThanStore,
                    format!(
                        "the drive holds an older state of {:?}{named}; push to update the drive, or force the pull to record it as the newest",
                        lib.name
                    ),
                );
                match older {
                    Some(s) => older_than.with("snapshot", &s.short_id),
                    None => older_than,
                }
                .action(Action::call(
                    "push.plan",
                    serde_json::json!({ "drive": drive, "library": lib.name }),
                    format!("Update the drive from {:?}", lib.name),
                ))
                .action(Action::retry(serde_json::json!({ "force": true }), "Record it as the newest"))
            } else {
                Error::conflict(
                    Reason::DivergedFromStore,
                    format!(
                        "the drive's library has diverged from {:?} (or was never synced with it); force the pull to make it the newest state, pull into a different library, or merge",
                        lib.name
                    ),
                )
                .action(Action::retry(serde_json::json!({ "force": true }), "Make it the newest state"))
                .action(Action::retry(serde_json::json!({ "library": null }), "Pull into another library"))
            };
            return Err(refused.with("drive", &drive).with("library", &lib.name));
        }
    }

    // Import every library file. Files with a cached hash that the store already has are not
    // read at all; everything else is read exactly once, hashing while copying.
    let mut index = device.load_index();
    for e in tree.entries.values_mut() {
        if e.hash.is_none() {
            e.hash = index.lookup(e);
        }
    }
    let keys: Vec<String> = tree.library_entries().map(|e| e.path.clone()).collect();
    let total: u64 = keys.iter().map(|k| tree.entries[k].size).sum();
    ctx.phase_start(Phase::Copy, keys.len() as u64, total);
    let mut report_import = (0usize, 0u64, 0usize);
    let mut imported = Vec::new();
    for key in &keys {
        ctx.check()?;
        let path = tree.fs_path(key);
        let e = tree.entries.get_mut(key).expect("key from tree");
        ctx.emit(Event::Item { phase: Phase::Copy, path: key.clone(), bytes: e.size });
        let (h, new) = store.import_file(&path, e.hash, ctx)?;
        e.hash = Some(h);
        index.record(e);
        if new {
            imported.push(h);
            report_import.0 += 1;
            report_import.1 += e.size;
        } else {
            report_import.2 += 1;
        }
    }
    store.flush_objects(&imported)?;
    ctx.phase_end(Phase::Copy);
    device.save_index(&index)?;
    let entries: Vec<FileEntry> = tree.library_entries().cloned().collect();

    let mut lineage = device.state().map(|s| s.lineage.clone()).unwrap_or_default();
    lineage.push(LineageEntry {
        fingerprint: fp,
        at: Utc::now(),
        op: LineageOp::Pull,
        snapshot: None,
        source: device.id(),
        tracks: library.tracks.len(),
    });
    let snap = store.save_snapshot(Snapshot {
        id: Hash::of(b""),
        library_id: lib.id,
        parents: beside.or(lib.head).into_iter().collect(),
        created: Utc::now(),
        message: opts.message.clone(),
        source: Some(SnapshotSource { drive_id: device.id(), drive_name: device.name() }),
        fingerprint: fp,
        summary: library.summary(),
        lineage,
        files: entries,
    })?;
    if beside.is_none() {
        store.set_head(lib.id, snap.id)?;
    }
    link_device(store, device, &lib, fp, Some(snap.id), LineageOp::Pull, library.tracks.len())?;
    Ok(PullReport {
        library: lib.name.clone(),
        library_id: lib.id,
        snapshot: Some(snap.info()),
        up_to_date: false,
        imported_files: report_import.0,
        imported_bytes: report_import.1,
        reused_files: report_import.2,
    })
}

fn link_device(
    store: &Store,
    device: &mut Device,
    lib: &crate::store::LibraryRecord,
    fp: Hash,
    snapshot: Option<Hash>,
    op: LineageOp,
    tracks: usize,
) -> Result<()> {
    let state = device.state_mut()?;
    state.library_id = Some(lib.id);
    state.record(LineageEntry { fingerprint: fp, at: Utc::now(), op, snapshot, source: None, tracks });
    if let Some(head) = state.lineage.last_mut()
        && head.fingerprint == fp
        && head.snapshot.is_none()
    {
        head.snapshot = snapshot;
    }
    device.save_state()?;
    register_drive(store, device, Some(fp))
}

/// Remember a drive in the store's registry. What the registry alone knows (role, logbook) is
/// kept.
pub fn register_drive(store: &Store, device: &Device, fp: Option<Hash>) -> Result<()> {
    let Some(state) = device.state() else { return Ok(()) };
    let capacity = fsutil::total_space(device.root());
    store.update_drives(|drives| {
        // A primary that joins a library which already has one becomes a backup there: a library
        // has one primary, the drive rekordbox exports to.
        let primary_taken = state.library_id.is_some()
            && drives.values().any(|r| {
                r.id != state.id
                    && r.library_id == state.library_id
                    && r.role == Some(crate::fleet::Role::Primary)
                    && r.log.retired.is_none()
            });
        let rec = drives
            .entry(state.id)
            .or_insert_with(|| crate::store::DriveRecord::new(state.id, &state.name, device.root().to_path_buf()));
        if rec.library_id != state.library_id && rec.role == Some(crate::fleet::Role::Primary) && primary_taken {
            rec.role = Some(crate::fleet::Role::Backup);
        }
        rec.name = state.name.clone();
        rec.library_id = state.library_id;
        rec.last_mount = device.root().to_path_buf();
        rec.last_seen = Utc::now();
        rec.last_fingerprint = fp.or_else(|| state.head().map(|h| h.fingerprint));
        rec.capacity = capacity.or(rec.capacity);
        if state.hardware.is_some() {
            rec.hardware = state.hardware.clone();
        }
        rec.log.first_seen.get_or_insert(state.created);
        Ok(())
    })
}

#[derive(Debug, Clone, Default)]
pub struct PushOptions {
    pub library: Option<String>,
    /// Push this snapshot instead of the library head.
    pub snapshot: Option<String>,
    /// Overwrite even if the drive holds changes the store doesn't know about.
    pub force: bool,
    pub sync: SyncOptions,
}

/// Resolve which snapshot a push should write.
pub fn resolve_push_snapshot(store: &Store, device: &Device, opts: &PushOptions) -> Result<Snapshot> {
    if let Some(prefix) = &opts.snapshot {
        return store.load_snapshot(&store.resolve_snapshot(prefix)?);
    }
    let lib = match (&opts.library, device.state().and_then(|s| s.library_id)) {
        (Some(name), _) => store.find_library(name)?.ok_or_else(|| Error::NotFound(format!("library {name:?}")))?,
        (None, Some(id)) => store.library_by_id(id)?.ok_or_else(|| Error::NotFound(format!("library {id}")))?,
        (None, None) => {
            let libs = store.libraries()?;
            match libs.as_slice() {
                [only] => only.clone(),
                [] => return Err(Error::NotFound("the store has no libraries yet; pull a drive first".into())),
                _ => {
                    return Err(Error::invalid("the drive isn't linked to a library; choose one")
                        .because(Reason::ChooseLibrary)
                        .with("libraries", libs.iter().map(|l| &l.name).collect::<Vec<_>>())
                        .action(Action::retry(serde_json::json!({ "library": null }), "Choose the library")));
                }
            }
        }
    };
    let head = lib.head.ok_or_else(|| Error::NotFound(format!("library {:?} has no snapshots", lib.name)))?;
    store.load_snapshot(&head)
}

/// Plan writing a snapshot onto a drive. Returns the plan and the target tree.
pub fn plan_push(
    store: &Store,
    snapshot: &Snapshot,
    device: &Device,
    opts: &PushOptions,
    ctx: &Ctx<'_>,
) -> Result<SyncPlan> {
    let library = device.library().ok();
    let mut scanned = device.scan_fingerprinted(library.as_ref(), ctx)?;
    if !opts.force {
        guard_unsaved_changes(store, device, &scanned.fingerprints().collect::<Vec<_>>(), snapshot)?;
    }
    if opts.sync.verify {
        let mut index = device.load_index();
        scanned.tree.ensure_hashes(&mut index, |e| e.class.is_library(), ctx)?;
        device.save_index(&index)?;
    }
    plan(&Source::Snapshot { store, snapshot }, device, &scanned.tree, &opts.sync)
}

/// Refuse to overwrite a drive whose current library state isn't saved in the store.
///
/// The drive's own lineage isn't enough: merges, artwork edits and drive-to-drive syncs record
/// new states there without snapshotting them. Only a state that exists as a snapshot is safe to
/// overwrite, or one the snapshot written went through (the drive is behind it, as compare tells):
/// its changes are all in the snapshot already.
fn guard_unsaved_changes(store: &Store, device: &Device, fps: &[Hash], snapshot: &Snapshot) -> Result<()> {
    if fps.is_empty() {
        return Ok(()); // empty drive
    }
    let lineage = device.state().map_or(&[][..], |s| &s.lineage[..]);
    if fps.contains(&snapshot.fingerprint) || crate::device::has_held(&snapshot.lineage, fps, lineage) {
        return Ok(());
    }
    let head_snapshot = device
        .state()
        .and_then(|s| s.head())
        .filter(|h| fps.contains(&h.fingerprint))
        .and_then(|h| h.snapshot)
        .and_then(|id| store.load_snapshot(&id).ok())
        .is_some_and(|s| fps.contains(&s.fingerprint));
    let in_library = match store.library_by_id(snapshot.library_id)? {
        Some(lib) => {
            let mut found = false;
            for fp in fps {
                found |= store.find_by_fingerprint(&lib, fp)?.is_some();
            }
            found
        }
        None => false,
    };
    if head_snapshot || in_library {
        return Ok(());
    }
    let drive = device.root().display().to_string();
    Err(Error::conflict(
        Reason::NotSaved,
        format!(
            "{} holds a library state that isn't saved in the store (a rekordbox export, merge or artwork change since the last pull?). \
             Pull it first to keep it, or force the push to overwrite it",
            device.name()
        ),
    )
    .with("drive", &drive)
    .action(Action::call("pull", serde_json::json!({ "drive": drive }), format!("Pull {} into the store", device.name())))
    .action(Action::retry(serde_json::json!({ "force": true }), "Overwrite it")))
}

pub fn push(
    store: &Store,
    snapshot: &Snapshot,
    device: &mut Device,
    opts: &PushOptions,
    ctx: &Ctx<'_>,
) -> Result<SyncReport> {
    push_snapshot(store, snapshot, device, opts, None, ctx)
}

/// [`push`], refused with [`Error::StalePlan`] unless what it would copy and delete is what
/// `confirmed` (the plan the user was shown) does.
pub fn push_as_planned(
    store: &Store,
    snapshot: &Snapshot,
    device: &mut Device,
    opts: &PushOptions,
    confirmed: &SyncPlan,
    ctx: &Ctx<'_>,
) -> Result<SyncReport> {
    push_snapshot(store, snapshot, device, opts, Some(confirmed), ctx)
}

fn push_snapshot(
    store: &Store,
    snapshot: &Snapshot,
    device: &mut Device,
    opts: &PushOptions,
    confirmed: Option<&SyncPlan>,
    ctx: &Ctx<'_>,
) -> Result<SyncReport> {
    let _lock = store.lock()?;
    let _drive_lock = crate::commit::lock(device, None)?;
    let plan = plan_push(store, snapshot, device, opts, ctx)?;
    plan.check_confirmed(confirmed)?;
    let holds = Holds {
        lineage: &snapshot.lineage,
        entry: LineageEntry {
            fingerprint: snapshot.fingerprint,
            at: Utc::now(),
            op: LineageOp::Push,
            snapshot: Some(snapshot.id),
            source: None,
            tracks: snapshot.summary.tracks,
        },
        library_id: Some(snapshot.library_id),
    };
    let report = copy_library(&plan, &Source::Snapshot { store, snapshot }, device, &opts.sync, holds, ctx)?;
    register_drive(store, device, Some(snapshot.fingerprint))?;
    Ok(report)
}

/// Group a plan's copies by top-level folder for compact display.
pub fn summarize_paths(paths: impl Iterator<Item = String>) -> IndexMap<String, usize> {
    let mut out = IndexMap::new();
    for p in paths {
        let top: String = p.split('/').take(2).collect::<Vec<_>>().join("/");
        *out.entry(top).or_insert(0) += 1;
    }
    out.sort_unstable_keys();
    out
}

#[cfg(test)]
mod tests;
