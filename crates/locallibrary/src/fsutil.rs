//! Small filesystem helpers: atomic writes, mtime-preserving copies, free space.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use filetime::FileTime;

use crate::error::{Action, IoContext, Reason};
use crate::hash::{self, Hash};
use crate::paths;
use crate::progress::{Ctx, Phase};
use crate::{Error, Result};

/// Write `bytes` to `path` atomically (temp file in the same directory, fsync, rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = stage_bytes(path, bytes)?;
    tmp.persist(path).map_err(|e| Error::Io { path: path.to_path_buf(), source: e.error })?;
    Ok(())
}

/// [`write_atomic`] for secrets: the file is readable by its owner only, whatever `path` was.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = stage(path, bytes, true)?;
    tmp.persist(path).map_err(|e| Error::Io { path: path.to_path_buf(), source: e.error })?;
    Ok(())
}

/// A temp file in `dir` (named to end in `suffix`) to write `path` through and rename over it:
/// made as a new file is (0666 less the umask), with `path`'s permissions when it's there.
/// tempfile's own 0600 would leave an export, or a file it replaces, readable by its owner only.
pub fn temp_for(dir: &Path, path: &Path, suffix: &str) -> Result<tempfile::NamedTempFile> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(paths::TEMP_PREFIX).suffix(suffix);
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o666));
    let tmp = builder.tempfile_in(dir).at(dir)?;
    // Best effort: FAT keeps none (Linux refuses to set them there). Not on Windows, where a
    // read-only temp file couldn't be cleaned up.
    #[cfg(unix)]
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(tmp.path(), meta.permissions());
    }
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

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).at(path)?;
    serde_json::from_slice(&bytes).map_err(|e| Error::parse(path, e))
}

/// Read JSON if the file exists.
pub fn read_json_opt<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
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
    tmp.persist(dst).map_err(|e| Error::Io { path: dst.to_path_buf(), source: e.error })?;
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
        true => tempfile::Builder::new().prefix(paths::TEMP_PREFIX).tempfile_in(dir).at(dir)?,
        false => temp_for(dir, path, "")?,
    };
    tmp.write_all(bytes).at(tmp.path())?;
    tmp.as_file().sync_all().at(tmp.path())?;
    Ok(tmp)
}

/// `path` made absolute against the current folder, `.` and `..` resolved lexically (as a browser
/// resolves a link: symlinks aren't followed).
pub fn absolute(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()).components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// `path` made [`absolute`], with symlinks resolved as far as it exists (macOS's `/tmp` is
/// `/private/tmp`), so two paths compare, and relative paths between them lead where they say;
/// without Windows' `\\?\` prefix.
pub fn resolved(path: &Path) -> PathBuf {
    let abs = absolute(path);
    let mut existing = abs.as_path();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                existing = parent;
            }
            _ => return abs,
        }
    }
    let mut out = crate::device::dunce_canonicalize(existing);
    out.extend(rest.iter().rev());
    out
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
    tmp.persist(path).map_err(|e| Error::Io { path: path.to_path_buf(), source: e.error })?;
    Ok(())
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

/// FAT32 stores mtimes with 2 second granularity, so allow that much drift.
pub fn mtime_eq(a: i64, b: i64) -> bool {
    (a - b).abs() <= 2
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
        let file = FileLock::open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(FileLock(file))),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(e)) => Err(Error::Io { path: path.to_path_buf(), source: e }),
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
mod tests {
    use super::*;

    #[test]
    fn paths_are_made_absolute_lexically() {
        #[cfg(unix)]
        assert_eq!(absolute(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
        assert!(absolute(Path::new("x")).is_absolute());
    }

    /// The part of a path that exists is resolved (symlinks too), the rest appended; `..` below a
    /// folder that isn't there yet is folded first. Regression: export's own copy of this left
    /// such a path unresolved, so `/tmp/new/../out` and `/private/tmp/out` didn't compare.
    #[test]
    fn the_part_that_exists_is_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let real = crate::device::dunce_canonicalize(dir.path());
        assert_eq!(resolved(&dir.path().join("new/../out/x")), real.join("out/x"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real, real.join("link")).unwrap();
            assert_eq!(resolved(&real.join("link/missing/../there")), real.join("there"));
        }
    }

    /// Regression: a lock was released by closing its file, so it stayed held while a child
    /// process another thread was starting still shared the file (until it ran its program), and
    /// taking it again right away failed.
    #[test]
    fn a_lock_is_released_while_a_child_still_shares_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        let lock = FileLock::try_take(&path).unwrap().unwrap();
        assert!(FileLock::try_take(&path).unwrap().is_none(), "held");
        // What a child being started holds: the same open file.
        let shared = lock.0.try_clone().unwrap();
        drop(lock);
        assert!(FileLock::try_take(&path).unwrap().is_some());
        drop(shared);
    }

    /// Flushing a copied file works on every platform (on Windows it once failed with "Access is
    /// denied"), leaves its contents and modification time alone, and skips files that are gone.
    #[test]
    fn flush_then_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("a.mp3"), dir.path().join("Contents/b.mp3"));
        fs::write(&src, b"audio").unwrap();
        copy_file(&src, &dst, None, None, false, &Ctx::silent()).unwrap();
        flush(&[dst.clone(), dir.path().join("gone.mp3")]).unwrap();
        set_mtime(&dst, 1_600_000_000).unwrap();
        flush(std::slice::from_ref(&dst)).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"audio");
        assert_eq!(mtime_secs(&fs::metadata(&dst).unwrap()), 1_600_000_000);
    }

    /// Regression: files written through a temp file got tempfile's 0600, so an export (or a file
    /// it replaced) was readable by its owner only.
    #[cfg(unix)]
    #[test]
    fn written_files_get_the_usual_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        // What a new file gets here (the umask is the process's).
        let plain = dir.path().join("plain.txt");
        fs::write(&plain, b"x").unwrap();
        let (new, copied) = (dir.path().join("new.m3u8"), dir.path().join("Contents/a.mp3"));
        write_atomic(&new, b"#EXTM3U").unwrap();
        copy_file(&plain, &copied, None, None, true, &Ctx::silent()).unwrap();
        assert_eq!((mode(&new), mode(&copied)), (mode(&plain), mode(&plain)));
        fs::set_permissions(&new, fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&new, b"#EXTM3U again").unwrap();
        assert_eq!(mode(&new), 0o640, "a file replaced keeps its permissions");
    }
}
