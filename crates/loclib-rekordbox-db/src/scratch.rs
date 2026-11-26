//! Private folders for decrypted working copies, which no ended process leaves behind for long.
//!
//! A decrypted `master.db` or `exportLibrary.db` holds the person's whole library, and
//! `master.db` can hold the cloud agent's sign-in too. Reads keep it in memory where they can
//! (see [`crate::masterdb::open`]); what SQLite has to have as a file (a plain database's WAL to
//! replay, `VACUUM INTO`, a drive's database being edited) goes in a [`Folder`] instead.
//!
//! A folder is made in the temporary folder, named `locallibrary-decrypted-` and six random
//! letters and digits, readable by its owner only (`0700` on Unix; on Windows the temporary
//! folder is the user's own), and removed when the [`Folder`] is dropped. A process that's
//! killed, crashes or calls `process::exit` never drops it, so each folder holds a file (`.lock`)
//! its process keeps locked while it lives. The OS lets go of a lock when its process ends,
//! however it ends, on macOS, Linux and Windows alike, so the next [`sweep`] (each process runs
//! one before its first copy) can tell an abandoned folder from one in use without trusting
//! process ids, which get reused. Removing a file while it's open, the other way to have nothing
//! left, doesn't work on Windows, and a kill while it's being written still leaves it.
//!
//! A sweep only ever looks into folders this user owns (and, for these folders, private ones),
//! so another user on a shared `/tmp` can't plant one to make it hang or remove anything.

use std::fs::{File, Metadata};
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Duration, SystemTime};

use crate::error::IoContext;
use crate::{Error, Result};

#[cfg(windows)]
mod windows;

/// What the folders' names start with.
pub const PREFIX: &str = "locallibrary-decrypted-";

/// What `tempfile` names the folders earlier versions made start with.
const LEGACY_PREFIX: &str = ".tmp";

/// The file a folder's process keeps locked.
const LOCK: &str = ".lock";

/// What the lock file is called until it's locked.
const MAKING: &str = ".lock-making";

/// How old a folder without its lock file has to be to be taken as abandoned: one is briefly
/// without it while it's made, and nothing here takes an hour.
const GRACE: Duration = Duration::from_secs(60 * 60);

/// What a [`Folder`] can hold besides its lock: what [`crate::masterdb`] and
/// [`crate::onelibrary`] write in it, and what SQLite writes beside a database it has open.
const OURS: [&str; 8] = [
    MAKING,
    "master.db",
    "master.db-wal",
    "master.db-shm",
    "master.db-journal",
    "export.db",
    "export.db-journal",
    "exportLibrary.db",
];

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
    folder_in(&root())
}

/// The folder [`Folder`]s are made in and [`sweep`] looks in: the temporary folder.
#[cfg(not(any(test, feature = "test-util")))]
fn root() -> PathBuf {
    std::env::temp_dir()
}

/// The folder [`Folder`]s are made in and [`sweep`] looks in. Tests never sweep the real
/// temporary folder: a thread's own when it set one ([`test_root`]), else
/// `locallibrary-scratch-tests` in the temporary folder, one for every test process.
#[cfg(any(test, feature = "test-util"))]
fn root() -> PathBuf {
    TEST_ROOT.with_borrow(Clone::clone).unwrap_or_else(|| {
        let shared = std::env::temp_dir().join("locallibrary-scratch-tests");
        let _ = std::fs::create_dir_all(&shared);
        shared
    })
}

#[cfg(any(test, feature = "test-util"))]
thread_local! {
    static TEST_ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Makes this thread's folders in `root`, and its first [`sweep`] look there (tests only).
#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub fn test_root(root: impl Into<PathBuf>) {
    TEST_ROOT.set(Some(root.into()));
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
/// nobody holds the lock of, and decrypted copies earlier versions left in folders of their own
/// (`tempfile`'s `.tmp` folders holding only those files). Only this user's folders are looked
/// into, and whatever can't be read or removed is left as it is.
pub fn sweep() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        sweep_in(&root(), GRACE);
    });
}

/// [`sweep`] of `root`, taking folders older than `grace` without a lock file as abandoned.
/// Answers what it removed.
pub(crate) fn sweep_in(root: &Path, grace: Duration) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let ours = named(&name, PREFIX);
        if !ours && !named(&name, LEGACY_PREFIX) {
            continue;
        }
        // Never a link (the metadata isn't followed), and only this user's own folders.
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if !meta.is_dir() || !owned(&path, &meta, ours) {
            continue;
        }
        let gone = if ours { abandoned(&path, grace) } else { legacy(&path, grace) };
        if gone && std::fs::remove_dir_all(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

/// Whether `name` is what `tempfile` names a folder made with `prefix`: it and six letters or
/// digits.
fn named(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|rest| rest.len() == 6 && rest.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// Whether the folder at `path` (`meta`, not followed) is this user's, and private too when
/// `private`, as each [`Folder`] is made.
#[cfg(unix)]
fn owned(_: &Path, meta: &Metadata, private: bool) -> bool {
    // SAFETY: geteuid has no preconditions and can't fail.
    owned_by(meta, unsafe { libc::geteuid() }, private)
}

/// [`owned`], by the user `uid`.
#[cfg(unix)]
fn owned_by(meta: &Metadata, uid: u32, private: bool) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.uid() == uid && (!private || meta.mode() & 0o777 == 0o700)
}

/// Whether the folder at `path` is this user's. Each [`Folder`]'s access list is checked when
/// it's made, and nobody else can change it.
#[cfg(windows)]
fn owned(path: &Path, _: &Metadata, _: bool) -> bool {
    windows::owned(path)
}

/// Whether the [`Folder`] at `dir` was left by a process that has ended: nothing holds its lock,
/// or it has none, is older than `grace` and holds only what one of ours can ([`OURS`]).
fn abandoned(dir: &Path, grace: Duration) -> bool {
    match open_lock(&dir.join(LOCK)) {
        // The lock is let go of when `lock` is dropped, before the folder's removed.
        Ok(lock) => lock.metadata().is_ok_and(|m| m.is_file()) && lock.try_lock().is_ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            holds_only(dir, &OURS).is_some() && older_than(dir, grace)
        }
        Err(_) => false,
    }
}

/// The lock file at `path`, opened without following a link or waiting (on a FIFO, say).
fn open_lock(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NOFOLLOW | libc::O_NONBLOCK);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::custom_flags(&mut options, windows::OPEN_REPARSE_POINT);
    options.open(path)
}

/// The files earlier versions decrypted `master.db` and `exportLibrary.db` into, each in a
/// temporary folder of its own (`tempfile`'s `.tmp` and six characters), and what SQLite or an
/// export left beside them.
const LEGACY: [&str; 9] = [
    "master.db",
    "master.db.enc",
    "master.db-wal",
    "master.db-shm",
    "master.db-journal",
    "export.db",
    "export.db-journal",
    "exportLibrary.db",
    "hash.db",
];

/// Which of [`LEGACY`] are a decrypted copy, not only what was left beside one.
const LEGACY_COPIES: [&str; 4] = ["master.db", "master.db.enc", "exportLibrary.db", "hash.db"];

/// Whether `dir` is a folder an earlier version left a decrypted copy in: it holds files of
/// those names ([`LEGACY`]) and nothing else, a copy among them ([`LEGACY_COPIES`]), and it
/// hasn't changed for `grace`. Any other program's temporary folders hold something else, or
/// are newer.
fn legacy(dir: &Path, grace: Duration) -> bool {
    let copy = holds_only(dir, &LEGACY).is_some_and(|names| names.iter().any(|n| LEGACY_COPIES.contains(&n.as_str())));
    copy && older_than(dir, grace)
}

/// The names of what's in `dir`, when each is a file (never a link or a folder) named one of
/// `names`.
fn holds_only(dir: &Path, names: &[&str]) -> Option<Vec<String>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().into_string().ok().filter(|n| names.contains(&n.as_str()))?;
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            return None;
        }
        found.push(name);
    }
    Some(found)
}

/// Whether `path` was last changed more than `grace` ago.
fn older_than(path: &Path, grace: Duration) -> bool {
    let changed = std::fs::symlink_metadata(path).and_then(|m| m.modified());
    changed.is_ok_and(|t| SystemTime::now().duration_since(t).is_ok_and(|age| age >= grace))
}

#[cfg(test)]
mod tests;
