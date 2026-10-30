//! Small filesystem helpers: atomic writes, mtime-preserving copies, free space.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use filetime::FileTime;

use crate::error::{Action, IoContext, Reason};
use crate::hash::{self, Hash};
use crate::paths;
use crate::progress::{Ctx, Phase};
use crate::{Error, Result};

/// Whether a file is a cloud storage placeholder whose contents aren't on this computer (an
/// online-only file in Dropbox or Google Drive): reading it downloads it first.
pub fn is_online_only(meta: &fs::Metadata) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        // SF_DATALESS: the file's data is fetched when it's first read.
        const SF_DATALESS: u32 = 0x4000_0000;
        meta.st_flags() & SF_DATALESS != 0
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
        const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
        meta.file_attributes() & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = meta;
        false
    }
}

/// Held exclusively while [`replace`] renames a file into place, and shared by [`unreplaced`].
static REPLACING: RwLock<()> = RwLock::new(());

/// Rename the temp file `tmp` over `dst`. On Windows a rename over a file that another handle
/// has open fails ("Access is denied"), as do two renames onto one file at once, so this waits
/// for the other renames this process makes and for the looks [`unreplaced`] takes.
pub fn replace(tmp: tempfile::TempPath, dst: &Path) -> Result<()> {
    let _held = REPLACING.write().unwrap_or_else(PoisonError::into_inner);
    tmp.persist(dst).map_err(|e| Error::Io { path: dst.to_path_buf(), source: e.error })
}

/// Run `look` (a few `stat` calls, a small file read) while this process renames nothing into
/// place (see [`replace`]): a watcher polling a drive that another thread is writing. `look`
/// mustn't call this (or [`read`]) again: std's lock can deadlock on a second read while a rename
/// waits for the first.
pub fn unreplaced<T>(look: impl FnOnce() -> T) -> T {
    let _held = REPLACING.read().unwrap_or_else(PoisonError::into_inner);
    look()
}

/// [`fs::read`] between this process's renames (see [`unreplaced`]): for a file that one thread
/// reads while another may rename a new one over it (a drive's database or state, settings).
pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    unreplaced(|| fs::read(path))
}

/// Write `bytes` to `path` atomically (temp file in the same directory, fsync, rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    replace(stage_bytes(path, bytes)?.into_temp_path(), path)
}

/// [`write_atomic`] for secrets: the file is readable by its owner only, whatever `path` was.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    replace(stage(path, bytes, true)?.into_temp_path(), path)
}

/// A temp file `builder` makes in `dir`, an error said of `dir`: tempfile adds the random name it
/// tried (`... at path "<dir>/.locallibrary-tmp-x1Yz"`), which differs each time and means nothing
/// to a person.
pub fn temp_in(builder: &tempfile::Builder, dir: &Path) -> Result<tempfile::NamedTempFile> {
    builder.tempfile_in(dir).map_err(|e| {
        let source = match e.to_string().split_once(" at path ") {
            Some((cause, _)) => std::io::Error::new(e.kind(), cause),
            None => e,
        };
        Error::Io { path: dir.to_path_buf(), source }
    })
}

/// A temp file in `dir` (named to end in `suffix`) to write `path` through and rename over it:
/// made as a new file is (0666 less the umask), with `path`'s permissions when it's there.
/// tempfile's own 0600 would leave an export, or a file it replaces, readable by its owner only.
pub fn temp_for(dir: &Path, path: &Path, suffix: &str) -> Result<tempfile::NamedTempFile> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(paths::TEMP_PREFIX).suffix(suffix);
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o666));
    let tmp = temp_in(&builder, dir)?;
    // Best effort: FAT keeps none (Linux refuses to set them there). Not on Windows, where a
    // read-only temp file couldn't be cleaned up.
    #[cfg(unix)]
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(tmp.path(), meta.permissions());
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(tmp)
}

/// Unless `replace`, refuse to replace any of the files a command would write on this computer
/// (an export, an XML library, an image) that are there already: a `conflict` (`exists`) naming
/// the first, whose action is the same request with `replace`.
pub fn refuse_to_replace(paths: &[&Path], replace: bool) -> Result<()> {
    let existing: Vec<&Path> = paths.iter().copied().filter(|p| !replace && p.exists()).collect();
    let Some(first) = existing.first() else { return Ok(()) };
    let (them, more) = match existing.len() {
        1 => ("it", String::new()),
        n => ("them", format!(" (and {} more)", n - 1)),
    };
    Err(Error::conflict(
        Reason::Exists,
        format!("{} is already there{more}; choose another place, or replace {them}", first.display()),
    )
    .with("path", first)
    .action(Action::retry(serde_json::json!({ "replace": true }), format!("Replace {them}"))))
}

/// Write a file a command makes on this computer (`-o FILE`) atomically, replacing one that's
/// there only with `replace` (see [`refuse_to_replace`]).
pub fn write_output(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    refuse_to_replace(&[path], replace)?;
    write_atomic(path, bytes)
}

/// Serialise `value` as pretty JSON and write it atomically.
pub fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| Error::invalid(e.to_string()))?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

/// Read JSON (with [`read`]).
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = read(path).at(path)?;
    serde_json::from_slice(&bytes).map_err(|e| Error::parse(path, e))
}

/// Read JSON (with [`read`]) if the file exists.
pub fn read_json_opt<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| Error::parse(path, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io { path: path.to_path_buf(), source: e }),
    }
}

/// Copy `src` to `dst` via a temp file + rename, hashing on the way and stamping `mtime`
/// (unix seconds) on the result. If `expected` is given the copy is verified against it.
///
/// With `durable`, the data is fsynced before the rename. Bulk copies pass `false` and call
/// [`flush`] once on everything they wrote before their commit point: an fsync right after each
/// write costs a device round trip per file (~10x slower for thousands of analysis files),
/// while a deferred flush finds most data already written back. Copies that a later run judges
/// by size and modification time pass no `mtime` either, and [`set_mtime`] after the flush: a
/// file renamed into place can reach the disk before its data does (a pulled plug leaves it
/// torn), and it must not look like a finished copy until the data is there.
pub fn copy_file(
    src: &Path,
    dst: &Path,
    mtime: Option<i64>,
    expected: Option<Hash>,
    durable: bool,
    ctx: &Ctx<'_>,
) -> Result<Hash> {
    let dir = dst.parent().ok_or_else(|| Error::invalid(format!("{} has no parent", dst.display())))?;
    fs::create_dir_all(dir).at(dir)?;
    let mut tmp = temp_for(dir, dst, "")?;
    let (hash, _) = hash::copy_hashing(src, tmp.as_file_mut(), dst, ctx, Phase::Copy)?;
    if let Some(expected) = expected
        && expected != hash
    {
        return Err(Error::Corrupt { path: src.to_path_buf(), expected: expected.to_hex(), actual: hash.to_hex() });
    }
    if durable {
        tmp.as_file().sync_all().at(tmp.path())?;
    }
    if let Some(secs) = mtime {
        filetime::set_file_handle_times(tmp.as_file(), None, Some(FileTime::from_unix_time(secs, 0))).at(tmp.path())?;
    }
    replace(tmp.into_temp_path(), dst)?;
    Ok(hash)
}

/// Copy `src` into a durable temp file next to `dst` (mtime stamped, hash verified) without
/// replacing `dst` yet. Persist the returned file to commit it.
pub fn stage_copy(
    src: &Path,
    dst: &Path,
    mtime: Option<i64>,
    expected: Option<Hash>,
    ctx: &Ctx<'_>,
) -> Result<(tempfile::NamedTempFile, Hash)> {
    let dir = dst.parent().ok_or_else(|| Error::invalid(format!("{} has no parent", dst.display())))?;
    fs::create_dir_all(dir).at(dir)?;
    let mut tmp = temp_for(dir, dst, "")?;
    let (hash, _) = hash::copy_hashing(src, tmp.as_file_mut(), dst, ctx, Phase::Copy)?;
    if let Some(expected) = expected
        && expected != hash
    {
        return Err(Error::Corrupt { path: src.to_path_buf(), expected: expected.to_hex(), actual: hash.to_hex() });
    }
    tmp.as_file().sync_all().at(tmp.path())?;
    if let Some(secs) = mtime {
        filetime::set_file_handle_times(tmp.as_file(), None, Some(FileTime::from_unix_time(secs, 0))).at(tmp.path())?;
    }
    Ok((tmp, hash))
}

/// Write `bytes` into a durable temp file next to `path` without replacing `path` yet. Persist
/// the returned file to commit it.
pub fn stage_bytes(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
    stage(path, bytes, false)
}

/// [`stage_bytes`], the temp file readable by its owner only when `private` (tempfile's own
/// 0600), else made as [`temp_for`] makes it.
fn stage(path: &Path, bytes: &[u8], private: bool) -> Result<tempfile::NamedTempFile> {
    let dir = path.parent().ok_or_else(|| Error::invalid(format!("{} has no parent", path.display())))?;
    fs::create_dir_all(dir).at(dir)?;
    let mut tmp = match private {
        true => temp_in(tempfile::Builder::new().prefix(paths::TEMP_PREFIX), dir)?,
        false => temp_for(dir, path, "")?,
    };
    tmp.write_all(bytes).at(tmp.path())?;
    tmp.as_file().sync_all().at(tmp.path())?;
    Ok(tmp)
}

/// `path` made absolute against the current folder, `.` and `..` resolved lexically (as a browser
/// resolves a link: symlinks aren't followed).
pub fn absolute(path: &Path) -> PathBuf {
    loclib_model::paths::absolute_with(path, |_| None)
}

/// `path` made absolute, with symlinks resolved as far as it exists (macOS's `/tmp` is
/// `/private/tmp`), so two paths compare, and relative paths between them lead where they say;
/// without Windows' `\\?\` prefix. A `..` goes where the OS takes it, out of a link's target
/// rather than back to the link's folder, so a path is inside a folder here exactly when what it
/// opens is (once the folders that aren't there yet are made, `..` folded over them).
pub fn resolved(path: &Path) -> PathBuf {
    loclib_model::paths::absolute_with(path, |p| p.canonicalize().ok().map(crate::device::without_verbatim_prefix))
}

/// Delete a file if it exists. Returns whether it did.
pub fn remove_if_exists(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Error::Io { path: path.to_path_buf(), source: e }),
    }
}

/// Allocation unit of the filesystem holding `path` (4 KiB if unknown).
pub fn cluster_size(path: &Path) -> u64 {
    fs4::allocation_granularity(path).ok().filter(|&n| n > 0).unwrap_or(4096)
}

/// Write `bytes` to `path` via temp file + rename, without fsync (see [`copy_file`]).
pub fn write_deferred(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::invalid(format!("{} has no parent", path.display())))?;
    fs::create_dir_all(dir).at(dir)?;
    let mut tmp = temp_for(dir, path, "")?;
    tmp.write_all(bytes).at(tmp.path())?;
    replace(tmp.into_temp_path(), path)
}

/// Make previously written files durable (the barrier before a commit point).
pub fn flush(paths: &[std::path::PathBuf]) -> Result<()> {
    for p in paths {
        match open_to_flush(p) {
            Ok(f) => f.sync_data().at(p)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Io { path: p.clone(), source: e }),
        }
    }
    Ok(())
}

/// Open an existing file so that `sync_data` can flush it. On Windows that's `FlushFileBuffers`,
/// which needs a handle with write access ("Access is denied" on a read-only one), so the file is
/// opened for writing, neither created nor truncated. Elsewhere `fsync` works on any handle.
fn open_to_flush(path: &Path) -> std::io::Result<File> {
    #[cfg(windows)]
    {
        File::options().write(true).open(path)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

/// Make the renames into `dir` durable: a rename reaches the disk with its folder, and a crash
/// before then can leave the old file, or (on FAT, where replacing a file deletes it first)
/// none. Best effort: Windows can't open a folder to flush it, and some filesystems refuse to.
pub fn sync_dir(dir: &Path) {
    #[cfg(not(windows))]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(windows)]
    let _ = dir;
}

/// Set a file's modification time (unix seconds).
pub fn set_mtime(path: &Path, secs: i64) -> Result<()> {
    filetime::set_file_mtime(path, FileTime::from_unix_time(secs, 0)).at(path)
}

/// Modification time in unix seconds.
pub fn mtime_secs(meta: &fs::Metadata) -> i64 {
    FileTime::from_last_modification_time(meta).unix_seconds()
}

/// How far apart two mtimes of the same file may be: FAT32 stores them with 2 second
/// granularity.
const MTIME_DRIFT: i64 = 2;

/// FAT32 stores mtimes with 2 second granularity, so allow that much drift.
pub fn mtime_eq(a: i64, b: i64) -> bool {
    (a - b).abs() <= MTIME_DRIFT
}

/// Whether a file's size and modification time `mtime`, recorded at `at` (unix seconds), still
/// tell it apart from any rewrite since: a file rewritten after `at` has an mtime of `at - 1` or
/// later (FAT32 rounds down to 2 s, and `at` is whole seconds), which must be more than
/// [`mtime_eq`]'s drift from `mtime`. A file recorded in the same window it was written in can be
/// rewritten at the same size with an mtime that looks unchanged ("racy", as git calls it), so
/// equal size and mtime say nothing about it.
pub fn settled(mtime: i64, at: i64) -> bool {
    at - 1 > mtime + MTIME_DRIFT
}

/// The file or folder at `path` (whose metadata is `m`): its device, and its id there. The
/// device and inode on Unix; on Windows the volume serial number and file index, through a
/// handle (0s when it can't be opened).
#[cfg(unix)]
pub(crate) fn ids(_: &Path, m: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (m.dev(), m.ino())
}

#[cfg(windows)]
pub(crate) fn ids(path: &Path, _: &fs::Metadata) -> (u64, u64) {
    let info = winapi_util::Handle::from_path_any(path).and_then(winapi_util::file::information);
    info.map_or((0, 0), |i| (i.volume_serial_number(), i.file_index()))
}

/// Whether `a` and `b` are on the same volume, each by its nearest folder that's there (a folder
/// an export makes may not be yet). `false` when either can't be told.
pub(crate) fn same_volume(a: &Path, b: &Path) -> bool {
    let volume = |p: &Path| p.ancestors().find_map(|p| fs::metadata(p).ok().map(|m| ids(p, &m).0)).filter(|v| *v != 0);
    volume(a).is_some_and(|v| volume(b) == Some(v))
}

/// Run `f`, warning `waiting` once if it's still running after `after`: reading an online-only
/// file waits for its download, which can take minutes, and the warning says which file it is.
pub(crate) fn warn_if_slow<T>(ctx: &Ctx<'_>, after: std::time::Duration, waiting: String, f: impl FnOnce() -> T) -> T {
    let (done, finished) = std::sync::mpsc::channel::<()>();
    std::thread::scope(|s| {
        s.spawn(move || {
            if finished.recv_timeout(after) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                ctx.warn(waiting);
            }
        });
        let out = f();
        drop(done);
        out
    })
}

/// Free bytes on the filesystem holding `path`.
pub fn available_space(path: &Path) -> Option<u64> {
    fs4::available_space(path).ok()
}

pub fn total_space(path: &Path) -> Option<u64> {
    fs4::total_space(path).ok()
}

/// Remove `dir` and its now-empty parents up to (not including) `stop`.
pub fn prune_empty_dirs(dir: &Path, stop: &Path) {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d == stop || !d.starts_with(stop) {
            break;
        }
        // remove_dir fails on non-empty directories, which is exactly the stop condition.
        if fs::remove_dir(d).is_err() {
            break;
        }
        cur = d.parent();
    }
}

pub fn file_exists(path: &Path) -> bool {
    File::open(path).is_ok()
}

/// An exclusive lock on a file (made if missing), released on drop.
pub struct FileLock(File);

impl FileLock {
    fn open(path: &Path) -> Result<File> {
        File::options().create(true).truncate(false).write(true).open(path).at(path)
    }

    /// Wait for the lock on `path`.
    pub fn wait(path: &Path) -> Result<FileLock> {
        let file = FileLock::open(path)?;
        file.lock().at(path)?;
        Ok(FileLock(file))
    }

    /// The lock on `path`, or `None` while another holds it.
    pub fn try_take(path: &Path) -> Result<Option<FileLock>> {
        FileLock::try_hold(FileLock::open(path)?).at(path)
    }

    /// [`FileLock::try_take`] for a reader, which leaves things as it found them: the file is
    /// opened only to read, and never made (an error of kind `NotFound` when it isn't there).
    pub fn try_take_existing(path: &Path) -> std::io::Result<Option<FileLock>> {
        FileLock::try_hold(File::open(path)?)
    }

    fn try_hold(file: File) -> std::io::Result<Option<FileLock>> {
        match file.try_lock() {
            Ok(()) => Ok(Some(FileLock(file))),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(e)) => Err(e),
        }
    }
}

impl Drop for FileLock {
    /// Unlocked, not just closed: a child process another thread is starting shares the open
    /// file until it runs its program, and the lock would stay held until then.
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
mod tests;
