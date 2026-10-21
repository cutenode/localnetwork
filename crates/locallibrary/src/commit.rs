//! One routine for writing to a drive in place, which every operation that edits a drive goes
//! through.
//!
//! 1. [`lock`]: take the drive's lock and check it still holds the databases the plan was made
//!    from ([`Error::StalePlan`] otherwise).
//! 2. The caller builds the new databases and files in memory and verifies them by reading them
//!    back; a failure there leaves nothing behind.
//! 3. [`commit`]: back up the databases and the files about to be overwritten or removed, mark the
//!    drive as being written (an interrupted write is reported by `info` and `doctor`, and refuses
//!    the next one, see [`Device::begin`]), copy the new files next to where they go (without an
//!    fsync each, then flushed together, and only then given their modification time), and swap
//!    everything in back to back: the files, then the databases, OneLibrary's old WAL removed
//!    first. Then delete what nothing references any more, note in the backup what the write left
//!    (so `undo` can tell whether anything changed the drive since, see [`backup::seal`]), record
//!    the new state in the drive's lineage, add the edit to the journal and prune old backups.
//!
//! Until the swap, the only changes on the drive are the pending marker, the backup and temp files
//! next to where the new files go: a failure or cancellation before then takes them away again,
//! so the drive is left byte-identical (locallibrary's lock file aside). There's one exception: a
//! write that takes over a cut-off write's marker removes the temp files that write left first
//! (see [`Device::begin`]), and they stay removed. From the swap on, it isn't cancellable: once
//! the drive is changed, what records the change runs to the end.
//!
//! Refusing to write to an archive or retired drive is [`crate::op`]'s part: every plan checks it
//! before it is applied. Sync, push and pack copy whole libraries incrementally with their own
//! staged copy, which a run that's stopped resumes ([`crate::sync`], [`crate::pack::apply`]).

use std::cell::Cell;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tempfile::TempPath;
use uuid::Uuid;

use crate::backup;
use crate::device::{Device, DriveLock, LineageEntry, LineageOp};
use crate::edit::Edit;
use crate::error::IoContext;
use crate::hash::{self, Hash};
use crate::progress::{Ctx, Event, Phase};
use crate::{Error, Result, fsutil, paths};

thread_local! {
    static FAIL_BEFORE_SWAP: Cell<bool> = const { Cell::new(false) };
    static FAIL_AFTER_SWAP: Cell<bool> = const { Cell::new(false) };
}

/// For tests: make every commit on this thread fail just before its swap, as a crash or a full
/// disk would, to check what it leaves behind.
pub fn fail_before_swap(yes: bool) {
    FAIL_BEFORE_SWAP.with(|f| f.set(yes));
}

/// For tests: make every commit on this thread fail just after its swap, the drive changed, as a
/// full disk can while it deletes or records what it did.
pub fn fail_after_swap(yes: bool) {
    FAIL_AFTER_SWAP.with(|f| f.set(yes));
}

/// Hash of a drive's database files, to notice a change between planning and applying.
pub(crate) fn database_state(device: &Device) -> Hash {
    database_state_at(device.root())
}

/// [`database_state`] of the drive (or folder laid out like one) at `root`. A missing file counts
/// as an empty one: SQLite leaves OneLibrary's WAL empty or removes it as a connection closes,
/// which changes nothing, and an empty database file holds no library either.
pub(crate) fn database_state_at(root: &Path) -> Hash {
    let mut hasher = blake3::Hasher::new();
    for key in paths::DATABASE_FILES {
        hasher.update(key.as_bytes());
        if let Ok(bytes) = fsutil::read(&paths::to_fs(root, key)) {
            hasher.update(&bytes);
        }
    }
    hasher.finalize().into()
}

/// Take the drive's lock. With `expected` (the [`database_state`] the plan was made from), a drive
/// whose databases changed since is refused. So is one whose databases changed since the plan
/// being applied was made (see [`crate::op::planned_databases`]): that plan was checked before
/// the lock was taken, when another write could still slip in.
pub(crate) fn lock(device: &Device, expected: Option<Hash>) -> Result<DriveLock> {
    let lock = device.lock()?;
    let wanted: Vec<Hash> = expected.into_iter().chain(crate::op::planned_databases(device.root())).collect();
    if !wanted.is_empty() {
        let now = database_state(device);
        if wanted.iter().any(|h| *h != now) {
            return Err(Error::stale(device.name(), "changed"));
        }
    }
    Ok(lock)
}

/// A drive's databases as an operation leaves them, edited and read back in memory (`None`: left
/// as they are).
#[derive(Debug, Default)]
pub(crate) struct NewDatabases {
    pub pdb: Option<Vec<u8>>,
    pub ext: Option<Vec<u8>>,
    pub plus: Option<Vec<u8>>,
}

impl NewDatabases {
    fn keys(&self) -> impl Iterator<Item = (&'static str, &Vec<u8>)> {
        [(paths::EXPORT_PDB, &self.pdb), (paths::EXPORT_EXT_PDB, &self.ext), (paths::LIBRARY_PLUS_DB, &self.plus)]
            .into_iter()
            .filter_map(|(k, b)| b.as_ref().map(|b| (k, b)))
    }
}

/// Where a new file's bytes come from.
pub(crate) enum Content<'a> {
    Bytes(Vec<u8>),
    /// A file (on this or another drive, or in the store), verified against `expected` if given.
    Copy {
        path: PathBuf,
        expected: Option<Hash>,
    },
    /// Made when it's written, so a large commit doesn't hold every file in memory.
    Make(Box<dyn FnOnce() -> Result<Vec<u8>> + 'a>),
}

/// A file a commit writes (or overwrites).
pub(crate) struct NewFile<'a> {
    pub key: String,
    pub content: Content<'a>,
    /// Modification time to give it, in unix seconds (`None`: the time it's written).
    pub mtime: Option<i64>,
    /// Size, for progress.
    pub size: u64,
    /// One that can't be written is reported in [`Committed::failed`] instead of stopping the
    /// commit (a repair writes what it can).
    pub optional: bool,
}

impl<'a> NewFile<'a> {
    pub fn bytes(key: impl Into<String>, bytes: Vec<u8>) -> Self {
        let size = bytes.len() as u64;
        NewFile { key: key.into(), content: Content::Bytes(bytes), mtime: None, size, optional: false }
    }

    pub fn copy(key: impl Into<String>, from: PathBuf, size: u64, mtime: Option<i64>) -> Self {
        NewFile { key: key.into(), content: Content::Copy { path: from, expected: None }, mtime, size, optional: false }
    }

    pub fn make(key: impl Into<String>, size: u64, make: impl FnOnce() -> Result<Vec<u8>> + 'a) -> Self {
        NewFile { key: key.into(), content: Content::Make(Box::new(make)), mtime: None, size, optional: false }
    }
}

/// What a commit writes.
#[derive(Default)]
pub(crate) struct Changes<'a> {
    pub databases: NewDatabases,
    /// Swapped in before the databases that name them.
    pub files: Vec<NewFile<'a>>,
    /// Removed once the swap is done (nothing references them any more).
    pub deletes: Vec<String>,
    /// Removed as the swap begins, before anything goes in (OneLibrary's WAL, which SQLite would
    /// replay into a database put back as a file).
    pub remove_first: Vec<String>,
}

/// How a commit is recorded.
pub(crate) struct Options<'a> {
    pub op: LineageOp,
    /// For the pending marker an interrupted write leaves.
    pub description: String,
    /// Back up first (the databases and every file overwritten or removed), giving this reason.
    /// `None` when there's nothing to undo (a repair writes back good copies of the same state;
    /// copies of firmware or recordings would only fill the drive) or the caller has made one (a
    /// replay backs up once for all its steps).
    pub backup: Option<String>,
    /// The backup this write puts back, when it's an undo (see [`backup::BackupInfo::undoes`]).
    pub undoes: Option<String>,
    /// Record the drive's new library state in its lineage (`false` when the library didn't
    /// change: settings files, the player name, files outside the library).
    pub lineage: bool,
    /// The drive the content came from (merge).
    pub source: Option<Uuid>,
    /// Edits to add to the drive's journal, with what they did.
    pub journal: Option<(&'a [Edit], &'a [String])>,
}

impl Options<'_> {
    pub fn new(op: LineageOp, description: impl Into<String>) -> Self {
        Options {
            op,
            description: description.into(),
            backup: None,
            undoes: None,
            lineage: true,
            source: None,
            journal: None,
        }
    }

    pub fn backup(mut self, reason: impl Into<String>) -> Self {
        self.backup = Some(reason.into());
        self
    }
}

/// What a commit did.
#[derive(Debug, Default)]
pub(crate) struct Committed {
    pub backup: Option<String>,
    pub fingerprint: Option<Hash>,
    /// Files written, as keys (not the databases).
    pub written: Vec<String>,
    /// The databases written, by file name.
    pub databases: Vec<String>,
    pub copied_bytes: u64,
    pub deleted: usize,
    pub freed: u64,
    /// Optional files that couldn't be written, and why.
    pub failed: Vec<(String, Error)>,
}

/// One step of a [`swap`].
pub(crate) enum Swap {
    /// Remove this file, if it's there.
    Remove(PathBuf),
    /// Rename this staged file over that one.
    Place(TempPath, PathBuf),
}

impl Swap {
    /// `bytes` staged next to `dst` (written and flushed), to go over it.
    pub(crate) fn bytes(dst: PathBuf, bytes: &[u8]) -> Result<Swap> {
        Ok(Swap::Place(fsutil::stage_bytes(&dst, bytes)?.into_temp_path(), dst))
    }
}

/// Make `steps`, staged beforehand, in order and back to back, then flush the folders they
/// changed: how a write puts a drive's databases in, which must match each other. Nothing in here
/// checks for cancellation, and a process exiting at once waits for it
/// ([`crate::progress::Critical`]).
pub(crate) fn swap(steps: Vec<Swap>) -> Result<()> {
    let _swapping = crate::progress::Critical::enter();
    let mut folders = std::collections::BTreeSet::new();
    for step in steps {
        let path = match step {
            Swap::Remove(path) => {
                fsutil::remove_if_exists(&path)?;
                path
            }
            Swap::Place(tmp, dst) => {
                fsutil::replace(tmp, &dst)?;
                dst
            }
        };
        folders.extend(path.parent().map(Path::to_path_buf));
    }
    for folder in &folders {
        fsutil::sync_dir(folder);
    }
    Ok(())
}

/// A file staged next to where it goes, not yet in place.
struct Staged {
    tmp: TempPath,
    dst: PathBuf,
    key: String,
    mtime: Option<i64>,
}

/// What a commit made before its swap, taken away again if it fails before then.
struct Undo {
    /// `state.json` as it was (`None`: there was none).
    state: Option<Vec<u8>>,
    state_path: PathBuf,
    backup: Option<PathBuf>,
    /// Whether `.locallibrary/backups/` was there before.
    had_backups: bool,
    /// Folders made for new files, outermost first.
    dirs: Vec<PathBuf>,
}

impl Undo {
    fn new(device: &Device) -> Self {
        let state_path = device.state_dir().join("state.json");
        Undo {
            state: std::fs::read(&state_path).ok(),
            state_path,
            backup: None,
            had_backups: device.state_dir().join("backups").is_dir(),
            dirs: Vec::new(),
        }
    }

    /// Make `dir` (and the folders above it), remembering which were new.
    fn make_dir(&mut self, dir: &Path) -> Result<()> {
        let mut missing: Vec<PathBuf> = dir.ancestors().take_while(|d| !d.exists()).map(Path::to_path_buf).collect();
        missing.reverse();
        std::fs::create_dir_all(dir).at(dir)?;
        self.dirs.extend(missing);
        Ok(())
    }

    /// Put the drive back as it was. Best effort: the error that got here is what's reported.
    fn rollback(self, device: &mut Device) {
        if let Some(dir) = &self.backup {
            let _ = std::fs::remove_dir_all(dir);
            if !self.had_backups {
                let _ = std::fs::remove_dir(device.state_dir().join("backups"));
            }
        }
        for d in self.dirs.iter().rev() {
            let _ = std::fs::remove_dir(d);
        }
        let _ = match &self.state {
            Some(bytes) => fsutil::write_atomic(&self.state_path, bytes),
            None => fsutil::remove_if_exists(&self.state_path).map(|_| ()),
        };
        let _ = device.reload_state();
    }
}

/// Write `changes` to the drive (locked by the caller, see [`lock`]) as the module docs describe.
pub(crate) fn commit(device: &mut Device, changes: Changes<'_>, opts: Options<'_>, ctx: &Ctx<'_>) -> Result<Committed> {
    ctx.check()?;
    let mut undo = Undo::new(device);
    let mut done = Committed::default();
    let Changes { databases, files, deletes, remove_first } = changes;
    let staged = match prepare(device, &databases, files, &deletes, &opts, &mut undo, &mut done, ctx) {
        Ok(s) => s,
        Err(e) => {
            undo.rollback(device);
            return Err(e);
        }
    };

    // The swap: the files, then the databases, back to back. Not cancellable from here on.
    let ctx = &ctx.uncancellable();
    let (files, dbs) = staged;
    let mut steps: Vec<Swap> =
        remove_first.iter().map(|k| device.checked_path(k).map(Swap::Remove)).collect::<Result<_>>()?;
    done.written.extend(files.iter().map(|s| s.key.clone()));
    steps.extend(files.into_iter().map(|s| Swap::Place(s.tmp, s.dst)));
    if databases.plus.is_some() {
        // SQLite would replay the old WAL into the new database.
        steps.extend([paths::LIBRARY_PLUS_WAL, paths::LIBRARY_PLUS_SHM].map(|key| Swap::Remove(device.path(key))));
    }
    done.databases.extend(dbs.iter().map(|s| s.key.rsplit('/').next().unwrap_or_default().to_string()));
    steps.extend(dbs.into_iter().map(|s| Swap::Place(s.tmp, s.dst)));
    device.swapping();
    swap(steps)?;
    if FAIL_AFTER_SWAP.with(Cell::get) {
        return Err(Error::Io {
            path: device.root().to_path_buf(),
            source: std::io::Error::other("failed after the swap (test)"),
        });
    }

    // What nothing references any more.
    if !deletes.is_empty() {
        let bytes = deletes.iter().map(|k| std::fs::metadata(device.path(k)).map(|m| m.len()).unwrap_or(0)).sum();
        ctx.phase_start(Phase::Delete, deletes.len() as u64, bytes);
        for key in &deletes {
            let p = device.checked_path(key)?;
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            ctx.emit(Event::Item { phase: Phase::Delete, path: key.clone(), bytes: size });
            match std::fs::remove_file(&p) {
                Ok(()) => {
                    done.deleted += 1;
                    done.freed += size;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::Io { path: p, source: e }),
            }
            if let Some(parent) = p.parent() {
                fsutil::prune_empty_dirs(parent, device.root());
            }
        }
        ctx.phase_end(Phase::Delete);
    }

    // What the write left, the new state, the journal, and the backups kept.
    if let Some(id) = &done.backup {
        backup::seal(device, id)?;
    }
    if opts.lineage {
        let library = device.library()?;
        let scanned = device.scan_fingerprinted(Some(&library), ctx)?;
        done.fingerprint = scanned.fingerprint;
        device.finish(scanned.fingerprint.map(|fp| LineageEntry {
            fingerprint: fp,
            at: Utc::now(),
            op: opts.op,
            snapshot: None,
            source: opts.source,
            tracks: library.tracks.len(),
        }))?;
    } else {
        device.finish(None)?;
    }
    if let Some((edits, changes)) = opts.journal
        && !edits.is_empty()
    {
        crate::edit::journal::append(device, edits, changes, done.backup.as_deref())?;
    }
    if done.backup.is_some() {
        backup::prune(device)?;
    }
    Ok(done)
}

/// Everything before the swap: the pending marker, the backup, and the files and databases staged.
#[allow(clippy::too_many_arguments)]
fn prepare(
    device: &mut Device,
    databases: &NewDatabases,
    files: Vec<NewFile<'_>>,
    deletes: &[String],
    opts: &Options<'_>,
    undo: &mut Undo,
    done: &mut Committed,
    ctx: &Ctx<'_>,
) -> Result<(Vec<Staged>, Vec<Staged>)> {
    // Every path is checked before anything is written.
    let mut targets = Vec::new();
    for f in &files {
        targets.push(device.checked_path(&f.key)?);
    }
    for key in deletes {
        device.checked_path(key)?;
    }
    // The backup: the databases, and what's overwritten or removed.
    if let Some(reason) = &opts.backup {
        let mut extra: Vec<String> =
            files.iter().zip(&targets).filter(|(_, p)| p.is_file()).map(|(f, _)| f.key.clone()).collect();
        extra.extend(deletes.iter().cloned());
        let info = backup::create(device, reason, &extra, opts.undoes.as_deref())?;
        undo.backup = Some(backup::dir(device, &info.id));
        done.backup = Some(info.id);
    }
    device.begin(opts.op, opts.description.clone(), done.backup.clone())?;

    // New files, staged next to where they go: written without an fsync each, then flushed
    // together, and only then given their modification time.
    let mut staged = Vec::new();
    if !files.is_empty() {
        let total = files.iter().map(|f| f.size).sum();
        ctx.phase_start(Phase::Copy, files.len() as u64, total);
        for (f, dst) in files.into_iter().zip(targets) {
            ctx.check()?;
            ctx.emit(Event::Item { phase: Phase::Copy, path: f.key.clone(), bytes: f.size });
            let (key, mtime, optional) = (f.key.clone(), f.mtime, f.optional);
            match stage(f, &dst, undo, ctx) {
                Ok((tmp, bytes)) => {
                    done.copied_bytes += bytes;
                    staged.push(Staged { tmp, dst, key, mtime });
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) if optional => done.failed.push((key, e)),
                Err(e) => return Err(e),
            }
        }
        ctx.phase_end(Phase::Copy);
        let temps: Vec<PathBuf> = staged.iter().map(|s| s.tmp.to_path_buf()).collect();
        fsutil::flush(&temps)?;
        for s in &staged {
            if let Some(secs) = s.mtime {
                fsutil::set_mtime(&s.tmp, secs)?;
            }
        }
    }
    let mut dbs = Vec::new();
    for (key, bytes) in databases.keys() {
        let dst = device.checked_path(key)?;
        let tmp = fsutil::stage_bytes(&dst, bytes)?.into_temp_path();
        dbs.push(Staged { tmp, dst, key: key.to_string(), mtime: None });
    }

    // The last moment to stop.
    if FAIL_BEFORE_SWAP.with(Cell::get) {
        return Err(Error::Io {
            path: device.root().to_path_buf(),
            source: std::io::Error::other("failed before the swap (test)"),
        });
    }
    ctx.check()?;
    Ok((staged, dbs))
}

/// Write one new file into a temp file next to `dst`. Returns it and the bytes written.
fn stage(f: NewFile<'_>, dst: &Path, undo: &mut Undo, ctx: &Ctx<'_>) -> Result<(TempPath, u64)> {
    let dir = dst.parent().ok_or_else(|| Error::invalid(format!("{} has no parent", dst.display())))?;
    if !dir.is_dir() {
        undo.make_dir(dir)?;
    }
    let mut tmp = fsutil::temp_for(dir, dst, "")?;
    let n = match f.content {
        Content::Bytes(bytes) => {
            tmp.write_all(&bytes).at(tmp.path())?;
            bytes.len() as u64
        }
        Content::Make(make) => {
            let bytes = make()?;
            tmp.write_all(&bytes).at(tmp.path())?;
            bytes.len() as u64
        }
        Content::Copy { path, expected } => {
            let (h, n) = hash::copy_hashing(&path, tmp.as_file_mut(), dst, ctx, Phase::Copy)?;
            if let Some(expected) = expected
                && expected != h
            {
                return Err(Error::Corrupt { path, expected: expected.to_hex(), actual: h.to_hex() });
            }
            n
        }
    };
    Ok((tmp.into_temp_path(), n))
}

#[cfg(test)]
mod tests;
