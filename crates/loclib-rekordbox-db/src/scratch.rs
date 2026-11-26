//! Private folders for decrypted working copies, which no ended process leaves behind for long.
//!
//! A decrypted `master.db` or `exportLibrary.db` holds the person's whole library, and
//! `master.db` can hold the cloud agent's sign-in too. Reads keep it in memory where they can
//! (see [`crate::masterdb::open`]); what SQLite has to have as a file (a plain database's WAL to
//! replay, `VACUUM INTO`, a drive's database being edited) goes in a [`Folder`] instead.
//!
//! A folder is made in the temporary folder, named `locallibrary-decrypted-` and something
//! random, readable by its owner only (`0700` on Unix; on Windows the temporary folder is the
//! user's own), and removed when the [`Folder`] is dropped. A process that's killed, crashes or
//! calls `process::exit` never drops it, so each folder holds a file (`.lock`) its process keeps
//! locked while it lives. The OS lets go of a lock when its process ends, however it ends, on
//! macOS, Linux and Windows alike, so the next [`sweep`] (each process runs one before its first
//! copy) can tell an abandoned folder from one in use without trusting process ids, which get
//! reused. Removing a file while it's open, the other way to have nothing left, doesn't work on
//! Windows, and a kill while it's being written still leaves it.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Duration, SystemTime};

use crate::error::IoContext;
use crate::{Error, Result};

/// What the folders' names start with.
pub const PREFIX: &str = "locallibrary-decrypted-";

/// The file a folder's process keeps locked.
const LOCK: &str = ".lock";

/// What the lock file is called until it's locked.
const MAKING: &str = ".lock-making";

/// How old a folder without its lock file has to be to be taken as abandoned: one is briefly
/// without it while it's made, and nothing here takes an hour.
const GRACE: Duration = Duration::from_secs(60 * 60);

/// A private folder for decrypted copies, removed with everything in it when dropped.
pub struct Folder {
    // Fields drop in order: the lock is let go of before the folder goes (Windows can't remove
    // a folder holding an open file).
    _lock: File,
    dir: tempfile::TempDir,
}

impl Folder {
    /// Where it is.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl std::fmt::Debug for Folder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Folder").field(&self.path()).finish()
    }
}

/// A new private folder in the temporary folder, after a [`sweep`] when this process hasn't run
/// one yet.
pub fn folder() -> Result<Folder> {
    sweep();
    folder_in(&std::env::temp_dir())
}

/// A new private folder in `root`.
fn folder_in(root: &Path) -> Result<Folder> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(PREFIX);
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
    let dir = builder.tempdir_in(root).map_err(|e| Error::Io { path: root.to_path_buf(), source: e })?;
    // Locked under another name first and only then named `.lock`: a sweep that found `.lock`
    // there but not yet locked would take the folder as abandoned and remove it while it's used.
    let making = dir.path().join(MAKING);
    let lock = File::create_new(&making).at(&making)?;
    lock.try_lock().map_err(|e| Error::Io { path: making.clone(), source: e.into() })?;
    let path = dir.path().join(LOCK);
    std::fs::rename(&making, &path).at(&path)?;
    Ok(Folder { _lock: lock, dir })
}

/// Removes, once per process, what ended processes left in the temporary folder: [`Folder`]s
/// nobody holds the lock of, and decrypted copies of `master.db` earlier versions left in
/// folders of their own (`tempfile`'s `.tmp` folders holding only those files). Whatever can't be read or removed is left as it is.
pub fn sweep() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        sweep_in(&std::env::temp_dir(), GRACE);
    });
}

/// [`sweep`] of `root`, taking folders older than `grace` without a lock file as abandoned.
/// Answers what it removed.
pub(crate) fn sweep_in(root: &Path, grace: Duration) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        // Never a link: only folders made here are looked into.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let gone = if name.starts_with(PREFIX) {
            abandoned(&path, grace)
        } else {
            name.starts_with(".tmp") && legacy(&path, grace)
        };
        if gone && std::fs::remove_dir_all(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

/// Whether the [`Folder`] at `dir` was left by a process that has ended: nothing holds its lock,
/// or it has none and is older than `grace`. One owned by someone else (a shared `/tmp`) can't
/// be opened, so it stays.
fn abandoned(dir: &Path, grace: Duration) -> bool {
    match File::open(dir.join(LOCK)) {
        // The lock is let go of when `lock` is dropped, before the folder's removed.
        Ok(lock) => lock.try_lock().is_ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => older_than(dir, grace),
        Err(_) => false,
    }
}

/// The files earlier versions decrypted `master.db` into, each read in a temporary folder of its
/// own (`tempfile`'s `.tmp` and six characters), and what an export left beside them.
const LEGACY: [&str; 4] = ["master.db", "master.db.enc", "master.db-wal", "export.db"];

/// Whether `dir` is a folder an earlier version left a copy of `master.db` in: it holds files of
/// those names ([`LEGACY`]) and nothing else, `master.db` or `master.db.enc` among them, and it
/// hasn't changed for `grace`. Any other program's temporary folders hold something else, or
/// are newer.
fn legacy(dir: &Path, grace: Duration) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    let mut copy = false;
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|n| LEGACY.contains(n)) else { return false };
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            return false;
        }
        copy |= name == "master.db" || name == "master.db.enc";
    }
    copy && older_than(dir, grace)
}

/// Whether `path` was last changed more than `grace` ago.
fn older_than(path: &Path, grace: Duration) -> bool {
    let changed = std::fs::symlink_metadata(path).and_then(|m| m.modified());
    changed.is_ok_and(|t| SystemTime::now().duration_since(t).is_ok_and(|age| age >= grace))
}

#[cfg(test)]
mod tests;
