//! Writing rekordbox's collection (`master.db`), as ADR 0007 decides: only while rekordbox and
//! its agent are closed, and only over a fresh backup that reads back.
//!
//! [`write()`] does the whole of one write. In order:
//!
//! 1. It refuses ([`Refusal`]) while rekordbox or its agent (`rekordboxAgent`) runs (or which
//!    programs run can't be told), or while a WAL, shared memory file or journal beside
//!    `master.db` holds anything: rekordbox checkpoints and deletes its WAL when it quits, so a
//!    closed collection is one file. It refuses a `master.db` with another name (a hard link), on
//!    a filesystem that may not replace a file in one rename (FAT, exFAT, a network share), or of
//!    a rekordbox version whose layout hasn't been checked ([`VERSIONS`]). It takes the lock on
//!    the collection's writes (`master.db.locallibrary-lock` beside it), and removes what a write
//!    that didn't finish left: its new file beside `master.db`, a partial backup.
//! 2. It copies `master.db` as it is into the backups folder, checks the copy is byte for byte the
//!    file, then reads the backup back: decrypted, opened, `PRAGMA integrity_check`, a
//!    `djmdContent` table, and the tables it writes laid out as the versions checked lay them out.
//!    No backup, no write.
//! 3. It refuses while Cloud Library Sync is on, or might be ([`sync_on`]): what a synced write
//!    has to move hasn't been worked out.
//! 4. The edit runs on that decrypted backup, in one transaction, through [`Edit`], which stamps
//!    the rows it changes as rekordbox does: `rb_local_usn` from the one counter
//!    (`agentRegistry`'s `localUpdateCount`), which ends at least at every stamp and only rises,
//!    and `updated_at` in rekordbox's format.
//! 5. The edited copy is checked (integrity, no trigger or view, the counter, no row left pointing
//!    nowhere, and each track whose cues were written: its rows and list pointing at it and its
//!    list the one its rows make), encrypted again with the original's salt, written beside
//!    `master.db`, read back page for page, and swapped in with one rename, only if `master.db`
//!    is still byte for byte what was backed up and rekordbox and its agent are still closed.
//!    Should either have started as it was swapped in, the write says so
//!    ([`Error::OpenedDuringSwap`]).
//!
//! Decrypted copies live in a temporary folder removed on every path out, errors included. Until
//! the swap is tried, the new file beside `master.db` is removed on the way out, and so is the
//! backup, since `master.db` is as it was. From then on the backup stays whatever happens, and a
//! swap that fails keeps the new file too ([`Error::SwapFailed`]). A write that returned can
//! still come back as the old file after a power cut on Windows, which has no way to flush a
//! folder; the next write removes the new file it left.
//!
//! What rekordbox reads is laid out as rekordbox lays it out (see [`Edit::set_cues`]). Where it's
//! looser than it could be, a write here is stricter: the whole edit is one transaction and checked
//! before the swap, and each track whose cues it wrote has its `djmdCue` rows and its `contentCue`
//! list agreeing after it. What an edit does through [`Edit::conn`] is checked only that far.
//!
//! Undo moves forward: each edit answers what it replaced ([`CuesBefore`], [`TrackBefore`]), which
//! a later write puts back as a new change, with new stamps. Restoring a backup whole would take
//! `localUpdateCount` back, so it's only for a damaged file.

use std::fs::File;
use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

use super::files::{Running, rekordbox_version, running_now, write_pages};
use super::{Salt, agent_log_path, default_path, open_copy, resolve_key};
use crate::error::IoContext;
use crate::onelibrary::cipher::{self, Codec};
use crate::{Error, Refusal, Result};

mod cues;
mod sync;

pub use cues::{CueEntry, CuesBefore, cue_list_mismatches, cue_rows_at};
pub use sync::sync_on;

/// The rekordbox versions a write lays the collection out for: 7.2.18, watched setting a hot cue
/// (LOC-1150), and 7.2.14, whose cue rows and lists were read and lay out the same.
pub const VERSIONS: &[&str] = &["7.2.14", "7.2.18"];

/// The columns those versions give the tables a write adds whole rows to; a collection whose
/// tables have others, or lack some, isn't written.
const MEASURED: &[(&str, &[&str])] = &[
    (
        "djmdCue",
        &[
            "ID",
            "ContentID",
            "InMsec",
            "InFrame",
            "InMpegFrame",
            "InMpegAbs",
            "OutMsec",
            "OutFrame",
            "OutMpegFrame",
            "OutMpegAbs",
            "Kind",
            "Color",
            "ColorTableIndex",
            "ActiveLoop",
            "Comment",
            "BeatLoopSize",
            "CueMicrosec",
            "InPointSeekInfo",
            "OutPointSeekInfo",
            "ContentUUID",
            "UUID",
            "rb_data_status",
            "rb_local_data_status",
            "rb_local_deleted",
            "rb_local_synced",
            "usn",
            "rb_local_usn",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "contentCue",
        &[
            "ID",
            "ContentID",
            "Cues",
            "rb_cue_count",
            "UUID",
            "rb_data_status",
            "rb_local_data_status",
            "rb_local_deleted",
            "rb_local_synced",
            "usn",
            "rb_local_usn",
            "created_at",
            "updated_at",
        ],
    ),
];

/// The columns a write sets in the tables it only changes rows of.
const NEEDED: &[(&str, &[&str])] = &[
    ("djmdContent", &["ID", "UUID", "BPM", "KeyID", "CueUpdated", "rb_local_deleted", "rb_local_usn", "updated_at"]),
    ("agentRegistry", &["registry_id", "int_1", "str_1", "updated_at"]),
];

/// The `master.db` a write goes to, and what it needs besides.
#[derive(Debug, Clone)]
pub struct Target {
    /// The `master.db`.
    pub master_db: PathBuf,
    /// Its SQLCipher passphrase (see [`super::resolve_key`]); a plain SQLite file stays plain.
    pub key: String,
    /// The folder each write's backup of `master.db` goes in, made (readable by its owner only)
    /// when missing. Keeping only the latest several is up to whoever gives it. A backup holds
    /// everything `master.db` does, the cloud agent's sign-in too, and it's on this computer's
    /// disk: it's for a write that went wrong, not a disk that did.
    pub backups: PathBuf,
    /// rekordbox's agent's log (`log.log`), which says whether Cloud Library Sync is on. `None`, or
    /// one that says nothing, counts as it being on.
    pub agent_log: Option<PathBuf>,
    /// Where decrypted working copies go, in a folder of their own removed afterwards: the system's
    /// temporary folder when `None`.
    pub work: Option<PathBuf>,
    /// Whether rekordbox and its agent are running ([`running_now`], unless a test says
    /// otherwise); `None` when that can't be told.
    pub running: fn() -> Option<Running>,
    /// rekordbox's version ([`rekordbox_version`], unless a test says otherwise).
    pub version: fn() -> Option<String>,
}

impl Target {
    /// rekordbox's collection on this computer ([`super::default_path`], [`super::PATH_ENV`] too)
    /// with its key ([`super::resolve_key`]) and its agent's log, backed up into `backups`. `None`
    /// where rekordbox doesn't run.
    pub fn this_computer(backups: PathBuf) -> Option<Target> {
        Some(Target {
            master_db: default_path()?,
            key: resolve_key(None),
            backups,
            agent_log: agent_log_path(),
            work: None,
            running: running_now,
            version: rekordbox_version,
        })
    }
}

/// A write that was made.
#[derive(Debug, Clone)]
pub struct Written<T> {
    /// What the edit answered.
    pub value: T,
    /// The backup of `master.db` as it was right before; `None` when the edit changed nothing, so
    /// nothing was written (and no backup kept).
    pub backup: Option<PathBuf>,
    /// `localUpdateCount` before the write, and after.
    pub count: (i64, i64),
}

/// Make the edit `edit` to the collection at `target`, as the module docs say: over a fresh backup,
/// in one transaction, swapped in with one rename. When it fails or is refused before the swap,
/// `master.db` is as it was and nothing decrypted is left behind.
pub fn write<T>(target: &Target, edit: impl FnOnce(&mut Edit<'_>) -> Result<T>) -> Result<Written<T>> {
    #[cfg(test)]
    tests::not_rekordboxs_own(&target.master_db);
    let master = target.master_db.as_path();
    closed(target)?;
    alone(master)?;
    replaced_in_one_step(master, filesystem(master).at(master)?)?;
    let version = (target.version)();
    if !measured(version.as_deref()) {
        let which = version.map_or("rekordbox's version can't be told".into(), |v| format!("rekordbox {v}"));
        let why = format!("{which}: only how {} lay out the collection has been checked", VERSIONS.join(" and "));
        return Err(Error::Refused(Refusal::Odd(why)));
    }
    let _lock = lock(master, &target.backups)?;
    let before = Fingerprint::of(master)?;
    let work = match &target.work {
        Some(dir) => tempfile::tempdir_in(dir).at(dir)?,
        None => tempfile::tempdir().at(std::env::temp_dir())?,
    };
    let backup = Removed(backup(master, &target.backups, &before)?);

    // The backup read back is the working copy: what's edited is what was backed up.
    let (db, salt) = open_copy(&backup.0, &target.key, work.path(), true)?;
    let conn = &db.conn;
    let at = |e: rusqlite::Error| Error::parse(&backup.0, e);
    integrity(conn).map_err(at)?.map_or(Ok(()), |why| Err(Error::parse(&backup.0, why)))?;
    if let Some(why) = unmeasured(conn).map_err(at)? {
        return Err(Error::Refused(Refusal::Odd(why)));
    }
    let header = journal_mode(&backup.0, salt, &target.key)?;
    if let Some(why) = loclib_sqlcipher::untrusted::distrust(conn, &["trigger", "view"], "rekordbox").map_err(at)? {
        return Err(Error::Refused(Refusal::Odd(why)));
    }
    if let Some(why) = sync_on(conn, target.agent_log.as_deref(), master)? {
        return Err(Error::Refused(Refusal::CloudSync(why)));
    }
    unchanged(master, &before)?;

    // Rows already pointing nowhere (rekordbox leaves some) aren't this write's to answer for.
    let pointing_nowhere = dangling(conn).map_err(at)?;

    let changes = |c: &Connection| c.query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0));
    let changed_before = changes(conn).map_err(at)?;
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(at)?;
    let mut e = Edit::new(&tx, &backup.0)?;
    let value = edit(&mut e)?;
    let count = e.finish()?;
    let (touched, cue_tracks) = (std::mem::take(&mut e.touched), std::mem::take(&mut e.cue_tracks));
    drop(e);
    tx.commit().map_err(at)?;
    // An edit that changed nothing is no write: the backup goes with the working copy.
    if changes(conn).map_err(at)? == changed_before {
        return Ok(Written { value, backup: None, count });
    }
    check(conn, count, &pointing_nowhere, &touched, &cue_tracks)
        .map_err(|why| Error::Refused(Refusal::Odd(format!("the edited copy {why}"))))?;
    // A plain collection's working copy has a WAL: everything goes into the file that's copied.
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).map_err(at)?;
    drop(db);

    // Back in WAL mode, as rekordbox keeps it, and encrypted again with the original's salt.
    let plain = work.path().join("master.db");
    set_journal_mode(&plain, header)?;
    let new_path = beside(master, "locallibrary-new");
    let mut out = File::create_new(&new_path).at(&new_path)?;
    let new = Removed(new_path);
    let codec = salt.map(|salt| Codec::with_key(target.key.as_bytes(), salt));
    write_pages(&plain, codec.as_ref(), &mut out, &new.0)?;
    out.sync_all().at(&new.0)?;
    drop(out);
    if let Ok(meta) = std::fs::metadata(master) {
        let _ = std::fs::set_permissions(&new.0, meta.permissions());
    }
    #[cfg(test)]
    if let Some(hook) = tests::BEFORE_READ_BACK.get() {
        hook(&new.0);
    }
    read_back(&new.0, &target.key, work.path(), count.1, &plain, codec.is_some())?;

    // The hash takes longest, so it goes before the checks that rekordbox is still closed, which
    // go right before the rename.
    unchanged(master, &before)?;
    closed(target)?;
    alone(master)?;
    // From here the backup stays, whatever happens to the rename.
    let backup = backup.keep();
    if let Err(source) = rename(&new.0, master) {
        let new = new.keep();
        return Err(Error::SwapFailed { master_db: master.to_path_buf(), new, backup, source });
    }
    new.keep();
    sync_dir(master);
    if closed(target).and_then(|()| alone(master)).is_err() {
        return Err(Error::OpenedDuringSwap(backup));
    }
    Ok(Written { value, backup: Some(backup), count })
}

/// Whether `version` (`7.2.14`, `7.2.14.0323`) is one of [`VERSIONS`].
fn measured(version: Option<&str>) -> bool {
    let Some(version) = version else { return false };
    let first_three = || version.trim().split('.').take(3);
    VERSIONS.iter().any(|v| v.split('.').eq(first_three()))
}

/// Why the tables of the database `conn` has open aren't as [`MEASURED`] and [`NEEDED`] say;
/// `None` when they are.
fn unmeasured(conn: &Connection) -> rusqlite::Result<Option<String>> {
    for (table, want) in MEASURED {
        let want: FxHashSet<String> = want.iter().map(|c| c.to_lowercase()).collect();
        if cues::columns(conn, table)? != want {
            return Ok(Some(format!(
                "its {table} table isn't laid out as rekordbox {} lay it out",
                VERSIONS.join(" and ")
            )));
        }
    }
    for (table, want) in NEEDED {
        let have = cues::columns(conn, table)?;
        if let Some(c) = want.iter().find(|c| !have.contains(&c.to_lowercase())) {
            return Ok(Some(format!("its {table} table has no {c} column")));
        }
    }
    Ok(None)
}

/// Refuses while rekordbox or its agent runs, or when that can't be told.
fn closed(target: &Target) -> Result<()> {
    let Some(running) = (target.running)() else {
        let why = "which programs are running can't be told, so neither can whether rekordbox is";
        return Err(Error::Refused(Refusal::Odd(why.into())));
    };
    match () {
        _ if running.rekordbox => Err(Error::Refused(Refusal::RekordboxRunning)),
        _ if running.agent => Err(Error::Refused(Refusal::AgentRunning)),
        _ => Ok(()),
    }
}

/// Refuses when `master.db` isn't one file on its own: its WAL, shared memory or rollback journal
/// holds anything (an empty WAL or shared memory file is what a clean close can leave). Refuses a
/// `master.db` that isn't a file too, or that has another name (a hard link, which the rename
/// would leave on the old collection).
fn alone(master: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(master).at(master)?;
    if !meta.is_file() {
        return Err(Error::Refused(Refusal::Odd(format!("{} isn't a plain file", master.display()))));
    }
    #[cfg(unix)]
    if std::os::unix::fs::MetadataExt::nlink(&meta) > 1 {
        return Err(Error::Refused(Refusal::Odd(format!("{} has other names (hard links)", master.display()))));
    }
    for end in ["-wal", "-shm", "-journal"] {
        let path = PathBuf::from(format!("{}{end}", master.display()));
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.len() == 0 && m.is_file() => {}
            Ok(_) => return Err(Error::Refused(Refusal::Unsaved(path))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Io { path, source: e }),
        }
    }
    Ok(())
}

/// Refuses `master` on a filesystem that may not replace a file in one rename (`fs`, as
/// [`filesystem`] names it): on a FAT or exFAT drive, or a network share, a rename can remove
/// `master.db` and then fail.
fn replaced_in_one_step(master: &Path, fs: Option<String>) -> Result<()> {
    if !swaps_in_one_step(fs.as_deref()) {
        let why = format!(
            "{} is on a {} drive, which may not replace a file in one step",
            master.display(),
            fs.unwrap_or_default()
        );
        return Err(Error::Refused(Refusal::Odd(why)));
    }
    Ok(())
}

/// Whether a filesystem [`filesystem`] names replaces a file in one rename: APFS or HFS+ (macOS),
/// NTFS or ReFS (Windows). `None` is a system rekordbox doesn't run on.
fn swaps_in_one_step(fs: Option<&str>) -> bool {
    fs.is_none_or(|fs| ["apfs", "hfs", "ntfs", "refs"].contains(&fs.to_lowercase().as_str()))
}

/// The filesystem `path` is on, as macOS names it (`apfs`, `msdos`, `smbfs`).
#[cfg(target_os = "macos")]
fn filesystem(path: &Path) -> std::io::Result<Option<String>> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: statfs fills the struct it's given, which is plain data, for a NUL-terminated path.
    let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(path.as_ptr(), &mut fs) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: f_fstypename is NUL-terminated.
    let name = unsafe { std::ffi::CStr::from_ptr(fs.f_fstypename.as_ptr()) };
    Ok(Some(name.to_string_lossy().into_owned()))
}

/// The filesystem `path` is on, as Windows names it (`NTFS`, `exFAT`).
#[cfg(windows)]
fn filesystem(path: &Path) -> std::io::Result<Option<String>> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationByHandleW;
    let file = File::open(path)?;
    let mut name = [0u16; 261];
    // SAFETY: the handle is open for the call, and `name` holds as many characters as it says.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let len = name.iter().position(|c| *c == 0).unwrap_or(name.len());
    Ok(Some(String::from_utf16_lossy(&name[..len])))
}

/// rekordbox runs only on macOS and Windows.
#[cfg(not(any(target_os = "macos", windows)))]
fn filesystem(_: &Path) -> std::io::Result<Option<String>> {
    Ok(None)
}

/// Takes the lock on writes of `master` (`master.db.locallibrary-lock` beside it, which stays:
/// removing a lock file is what lets two writers both think they hold it), refusing while another
/// write holds it. A write that didn't finish (killed, or the power went) can leave its new file
/// beside `master.db` and a partial backup in `backups`: with the lock held, they're no other
/// write's, so they're removed.
fn lock(master: &Path, backups: &Path) -> Result<File> {
    let path = beside(master, "locallibrary-lock");
    let lock = File::options().create(true).truncate(false).write(true).open(&path).at(&path)?;
    match lock.try_lock() {
        Err(std::fs::TryLockError::WouldBlock) => return Err(Error::Refused(Refusal::Busy)),
        locked => locked.map_err(std::io::Error::from).at(&path)?,
    }
    let partial = |e: &std::fs::DirEntry| {
        let name = e.file_name().to_string_lossy().into_owned();
        name.starts_with("master.") && name.ends_with(".db.partial")
    };
    let partials = std::fs::read_dir(backups).into_iter().flatten().flatten().filter(partial).map(|e| e.path());
    for left in std::iter::once(beside(master, "locallibrary-new")).chain(partials) {
        if left.exists() {
            std::fs::remove_file(&left).at(&left)?;
            tracing::warn!(path = %left.display(), "removed what a write of rekordbox's collection that didn't finish left");
        }
    }
    Ok(lock)
}

/// What `master.db` is at one moment: its size, modification time and contents' hash.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    modified: Option<std::time::SystemTime>,
    hash: blake3::Hash,
}

impl Fingerprint {
    fn of(path: &Path) -> Result<Fingerprint> {
        let meta = std::fs::metadata(path).at(path)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update_reader(File::open(path).at(path)?).at(path)?;
        Ok(Fingerprint { len: meta.len(), modified: meta.modified().ok(), hash: hasher.finalize() })
    }
}

/// Refuses when `master` is no longer what `before` found.
fn unchanged(master: &Path, before: &Fingerprint) -> Result<()> {
    if Fingerprint::of(master)? != *before {
        return Err(Error::Refused(Refusal::Changed));
    }
    Ok(())
}

/// Copies `master` (as `before` found it) into `folder` (made, readable by its owner only, when
/// missing) as `master.<time>.db`, answering where. The copy is made under another name and
/// renamed once it's whole and hashes as `before` does, and `master` still does; otherwise
/// nothing is left.
fn backup(master: &Path, folder: &Path, before: &Fingerprint) -> Result<PathBuf> {
    let mut dir = std::fs::DirBuilder::new();
    dir.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut dir, 0o700);
    dir.create(folder).at(folder)?;
    let name = format!("master.{}.db", chrono::Utc::now().format("%Y-%m-%dT%H%M%S%.3fZ"));
    backup_as(master, &folder.join(name), before)
}

/// [`backup`] to the file `to`, which mustn't be there yet, readable by its owner only.
fn backup_as(master: &Path, to: &Path, before: &Fingerprint) -> Result<PathBuf> {
    let partial_path = beside(to, "partial");
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(&partial_path).at(&partial_path)?;
    let partial = Removed(partial_path);
    std::io::copy(&mut File::open(master).at(master)?, &mut out).at(&partial.0)?;
    out.sync_all().at(&partial.0)?;
    drop(out);
    let copy = Fingerprint::of(&partial.0)?;
    if (copy.len, copy.hash) != (before.len, before.hash) {
        return Err(Error::Refused(Refusal::Changed));
    }
    unchanged(master, before)?;
    if to.exists() {
        return Err(Error::Exists(format!("{} is there already", to.display())));
    }
    std::fs::rename(&partial.0, to).at(to)?;
    partial.keep();
    sync_dir(to);
    Ok(to.to_path_buf())
}

/// A file this write made, removed when this is dropped unless [`Removed::keep`] said to keep it.
/// Made only once the file is there, so a file some other write made is never removed.
struct Removed(PathBuf);

impl Removed {
    fn keep(mut self) -> PathBuf {
        std::mem::take(&mut self.0)
    }
}

impl Drop for Removed {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

/// `<file>.<end>`, beside `file`.
fn beside(file: &Path, end: &str) -> PathBuf {
    PathBuf::from(format!("{}.{end}", file.display()))
}

/// `std::fs::rename`, which a test can make fail.
fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if tests::SWAP_FAILS.get() {
        return Err(std::io::Error::other("the test made the rename fail"));
    }
    std::fs::rename(from, to)
}

/// `PRAGMA integrity_check`: `Some` says what's wrong.
fn integrity(conn: &Connection) -> rusqlite::Result<Option<String>> {
    let check: String = conn.query_row("PRAGMA integrity_check(1)", [], |r| r.get(0))?;
    Ok((check != "ok").then(|| format!("SQLite finds it damaged: {check}")))
}

/// Header bytes 18-19 of the database at `path` (encrypted with `salt` under `key`, else plain):
/// 2, 2 for WAL mode, as rekordbox keeps `master.db`. The decrypted copy says 1, 1 so it reads
/// without a WAL.
fn journal_mode(path: &Path, salt: Option<Salt>, key: &str) -> Result<[u8; 2]> {
    let mut page = vec![0u8; cipher::PAGE_SIZE];
    std::io::Read::read_exact(&mut File::open(path).at(path)?, &mut page).at(path)?;
    if let Some(salt) = salt {
        page = Codec::with_key(key.as_bytes(), salt).decrypt_page(1, &page).map_err(|e| Error::parse(path, e))?;
    }
    Ok([page[18], page[19]])
}

/// Puts `header` back as bytes 18-19 of the plain database at `path`.
fn set_journal_mode(path: &Path, header: [u8; 2]) -> Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new().write(true).open(path).at(path)?;
    f.seek(SeekFrom::Start(18)).at(path)?;
    f.write_all(&header).at(path)?;
    f.sync_all().at(path)
}

/// Reads the new file at `path` back as a read would (decrypted in a folder of its own in `work`),
/// checks it whole, that its `localUpdateCount` is `count`, and that it holds what the edited copy
/// at `plain` does, page for page (`encrypted`: each page's last bytes are its IV and tag).
fn read_back(path: &Path, key: &str, work: &Path, count: i64, plain: &Path, encrypted: bool) -> Result<()> {
    let dir = tempfile::tempdir_in(work).at(work)?;
    let (db, _) = open_copy(path, key, dir.path(), false)?;
    let odd = |why: String| Error::Refused(Refusal::Odd(format!("the file written {why}")));
    let at = |e: rusqlite::Error| odd(e.to_string());
    if let Some(why) = integrity(&db.conn).map_err(at)? {
        return Err(odd(why));
    }
    let read = Stamp::count(&db.conn).map_err(at)?;
    if read != Some(count) {
        return Err(odd(format!("reads localUpdateCount {read:?}, not {count}")));
    }
    let copy = dir.path().join("master.db");
    let (a, b) = (std::fs::read(&copy).at(&copy)?, std::fs::read(plain).at(plain)?);
    let reserved = if encrypted { cipher::RESERVE } else { 0 };
    // Bytes 18-19 say how it's to be read (WAL mode or not), which a copy that's read changes.
    let same = |(i, (x, y)): (usize, (&[u8], &[u8]))| {
        let end = x.len().saturating_sub(reserved);
        match i {
            0 => x[..18] == y[..18] && x[20..end] == y[20..end],
            _ => x[..end] == y[..end],
        }
    };
    let mut pages = a.chunks(cipher::PAGE_SIZE).zip(b.chunks(cipher::PAGE_SIZE)).enumerate();
    if a.len() != b.len() || !pages.all(same) {
        return Err(odd("doesn't hold what the edited copy does".into()));
    }
    Ok(())
}

/// Flushes the folder `file` is in, so a rename into it is on disk (Unix; Windows can't flush a
/// folder).
fn sync_dir(file: &Path) {
    #[cfg(unix)]
    let _ = File::open(file.parent().unwrap_or(Path::new("."))).and_then(|d| d.sync_all());
    #[cfg(not(unix))]
    let _ = file;
}

/// The rows pointing at a row that isn't there: (table, rowid, the table pointed at). SQLite's
/// `foreign_key_check` finds those of declared foreign keys; rekordbox declares none, so its hot
/// cue bank lists' entries (`djmdSongHotCueBanklist.CueID`) are looked at too.
fn dangling(conn: &Connection) -> rusqlite::Result<FxHashSet<(String, Option<i64>, String)>> {
    let mut stmt = conn.prepare("SELECT \"table\", rowid, parent FROM pragma_foreign_key_check")?;
    let mut out: FxHashSet<_> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    if !cues::columns(conn, "djmdSongHotCueBanklist")?.is_empty() {
        let sql = "SELECT rowid FROM djmdSongHotCueBanklist WHERE CueID NOT IN (SELECT ID FROM djmdCue)";
        for rowid in conn.prepare(sql)?.query_map([], |r| r.get(0))? {
            out.insert(("djmdSongHotCueBanklist".into(), Some(rowid?), "djmdCue".into()));
        }
    }
    Ok(out)
}

/// Checks the edited copy before it's written: SQLite finds it whole, it has no trigger or view,
/// no row points nowhere that didn't before (`dangling`), `localUpdateCount` is at least every
/// stamp, each track `touched` is there, and each one whose `cues` were written has its live
/// `djmdCue` rows and `contentCue` list pointing at its `UUID` and the list the one its rows make.
/// `Err` says what's wrong.
fn check(
    conn: &Connection,
    count: (i64, i64),
    dangling_before: &FxHashSet<(String, Option<i64>, String)>,
    touched: &FxHashSet<String>,
    cue_tracks: &FxHashSet<String>,
) -> std::result::Result<(), String> {
    let sql = |e: rusqlite::Error| e.to_string();
    if let Some(why) = integrity(conn).map_err(sql)? {
        return Err(why);
    }
    if let Some(why) = loclib_sqlcipher::untrusted::distrust(conn, &["trigger", "view"], "rekordbox").map_err(sql)? {
        return Err(format!("has what the edit made: {why}"));
    }
    if let Some((table, _, parent)) = dangling(conn).map_err(sql)?.difference(dangling_before).next() {
        return Err(format!("has a row in {table} pointing at one in {parent} that isn't there"));
    }
    let stored = Stamp::count(conn).map_err(sql)?;
    let highest = Stamp::highest(conn).map_err(sql)?;
    if stored != Some(count.1) || count.1 < count.0 || count.1 < highest {
        return Err(format!(
            "has localUpdateCount {stored:?} (was {}, highest stamp {highest}): it must be at least every stamp",
            count.0
        ));
    }
    for track in touched {
        let there: bool = conn
            .query_row("SELECT count(*) FROM djmdContent WHERE ID = ?1", [track], |r| r.get::<_, i64>(0))
            .map_err(sql)?
            == 1;
        if !there {
            return Err(format!("has no track {track}"));
        }
    }
    const ASTRAY: &str = "SELECT (SELECT count(*) FROM djmdCue c WHERE c.ContentID = t.ID \
                              AND IFNULL(c.rb_local_deleted, 0) = 0 AND c.ContentUUID IS NOT t.UUID) \
                          + (SELECT count(*) FROM contentCue l WHERE l.ContentID = t.ID \
                              AND IFNULL(l.rb_local_deleted, 0) = 0 AND l.ID IS NOT t.UUID) \
                          FROM djmdContent t WHERE t.ID = ?1";
    for track in cue_tracks {
        if conn.query_row(ASTRAY, [track], |r| r.get::<_, i64>(0)).map_err(sql)? > 0 {
            return Err(format!("has track {track}'s cues or cue list pointing at another track's UUID"));
        }
    }
    let mismatched = cues::mismatches(conn, false, Some(cue_tracks)).map_err(|e| e.to_string())?;
    if let Some(track) = mismatched.first() {
        return Err(format!("has track {track}'s contentCue disagreeing with its djmdCue rows"));
    }
    Ok(())
}

/// The update counter: `agentRegistry`'s `localUpdateCount`, which rekordbox keeps at least at
/// every row's `rb_local_usn`. Each stamp is one more than the last; the count written back is the
/// last one handed out (or what it was, should none be).
struct Stamp {
    was: i64,
    next: i64,
}

impl Stamp {
    /// `localUpdateCount` (`int_1`, else a number in `str_1`), when there's one row of it. Text that
    /// isn't a number is no count (SQLite's `CAST` would make it 0).
    fn count(conn: &Connection) -> rusqlite::Result<Option<i64>> {
        let rows: Vec<(Option<i64>, Option<String>)> = conn
            .prepare("SELECT int_1, CAST(str_1 AS TEXT) FROM agentRegistry WHERE registry_id = 'localUpdateCount'")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(match &rows[..] {
            [(Some(n), _)] => Some(*n),
            [(None, s)] => s.as_deref().and_then(|s| s.trim().parse().ok()),
            _ => None,
        })
    }

    /// The highest `rb_local_usn` in any table.
    fn highest(conn: &Connection) -> rusqlite::Result<i64> {
        let mut highest = 0;
        for table in tables_with(conn, "rb_local_usn")? {
            let max: Option<i64> =
                conn.query_row(&format!("SELECT max(rb_local_usn) FROM \"{table}\""), [], |r| r.get(0))?;
            highest = highest.max(max.unwrap_or(0));
        }
        Ok(highest)
    }
}

/// The tables with a column `column`.
pub(crate) fn tables_with(conn: &Connection, column: &str) -> rusqlite::Result<Vec<String>> {
    const SQL: &str = "SELECT m.name FROM sqlite_master m, pragma_table_info(m.name) c \
                       WHERE m.type = 'table' AND lower(c.name) = lower(?1) ORDER BY m.name";
    conn.prepare(SQL)?.query_map([column], |r| r.get(0))?.collect()
}

/// The `djmdContent` columns [`Edit::update_track`] sets: what saving an analysis writes. The
/// others are rekordbox's own, or follow from the audio and analysis files.
const SETTABLE: [&str; 2] = ["bpm", "keyid"];

/// One edit of the collection, inside [`write()`]'s transaction: the changes made through it are
/// stamped as rekordbox stamps them. [`Edit::conn`] runs anything else (checked only as far as the
/// module docs say); a row changed that way is stamped with [`Edit::stamp`] and [`Edit::now`].
pub struct Edit<'c> {
    conn: &'c Connection,
    path: &'c Path,
    stamp: Stamp,
    now: String,
    /// Tracks whose rows were changed, and those whose cue lists were.
    touched: FxHashSet<String>,
    cue_tracks: FxHashSet<String>,
}

impl<'c> Edit<'c> {
    fn new(conn: &'c Connection, path: &'c Path) -> Result<Edit<'c>> {
        let sql = |e: rusqlite::Error| Error::parse(path, e);
        let Some(was) = Stamp::count(conn).map_err(sql)? else {
            let why = "its agentRegistry has no single localUpdateCount, which rekordbox counts every change with";
            return Err(Error::Refused(Refusal::Odd(why.into())));
        };
        // rekordbox keeps the count at least at every stamp; should a script have stamped past it,
        // the next stamp is past those too.
        let next = was.max(Stamp::highest(conn).map_err(sql)?) + 1;
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f +00:00").to_string();
        Ok(Edit {
            conn,
            path,
            stamp: Stamp { was, next },
            now,
            touched: FxHashSet::default(),
            cue_tracks: FxHashSet::default(),
        })
    }

    /// The decrypted copy being edited, inside the write's transaction.
    pub fn conn(&self) -> &Connection {
        self.conn
    }

    /// The next `rb_local_usn`: one more than the last stamp, and than every stamp already in the
    /// database. `localUpdateCount` ends at the last one handed out.
    pub fn stamp(&mut self) -> i64 {
        self.stamp.next += 1;
        self.stamp.next - 1
    }

    /// This write's time as rekordbox writes `created_at` and `updated_at`
    /// (`2026-11-26 07:24:06.375 +00:00`).
    pub fn now(&self) -> &str {
        &self.now
    }

    /// Writes the count back: the last stamp handed out, which is never less than it was nor than
    /// any stamp already in the database (with no stamp handed out, a count some script left below
    /// a stamp is raised to it). A count kept as text in `str_1` is raised there too. Answers it
    /// before and after.
    fn finish(&mut self) -> Result<(i64, i64)> {
        let count = self.stamp.next - 1;
        if count != self.stamp.was {
            self.conn
                .execute(
                    "UPDATE agentRegistry SET int_1 = ?1, \
                     str_1 = CASE WHEN str_1 IS NOT NULL AND trim(str_1) GLOB '[0-9]*' THEN CAST(?1 AS TEXT) \
                     ELSE str_1 END, updated_at = ?2 WHERE registry_id = 'localUpdateCount'",
                    rusqlite::params![count, self.now],
                )
                .map_err(|e| self.sql(e))?;
        }
        Ok((self.stamp.was, count))
    }

    fn sql(&self, e: rusqlite::Error) -> Error {
        Error::parse(self.path, e)
    }

    /// The track `id`'s `UUID`, when it's there and not deleted.
    fn track_uuid(&self, id: &str) -> Result<String> {
        let uuid: Option<Option<String>> = self
            .conn
            .query_row("SELECT UUID FROM djmdContent WHERE ID = ?1 AND IFNULL(rb_local_deleted, 0) = 0", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| self.sql(e))?;
        match uuid {
            None => Err(Error::NotFound(format!("track {id} in master.db"))),
            Some(None) => Err(Error::Invalid(format!("track {id} has no UUID, which its cues point at"))),
            Some(Some(u)) => Ok(u),
        }
    }

    /// Sets `values` (column, value) on the track `id`'s `djmdContent` row and stamps it, answering
    /// what the columns held before, which [`Edit::restore_track`] puts back. Only `BPM` (hundredths
    /// of a beat per minute, an integer) and `KeyID` (a `djmdKey` row's `ID`) can be set, each once.
    pub fn update_track(&mut self, id: &str, values: &[(&str, Value)]) -> Result<TrackBefore> {
        for (name, value) in values {
            let fits = match (name.to_lowercase().as_str(), value) {
                ("bpm", Value::Integer(n)) => *n >= 0,
                ("keyid", Value::Text(key)) => {
                    let sql = "SELECT 1 FROM djmdKey WHERE ID = ?1";
                    self.conn.query_row(sql, [key], |_| Ok(())).optional().map_err(|e| self.sql(e))?.is_some()
                }
                // Refused as a column below.
                (column, _) => !SETTABLE.contains(&column),
            };
            if !fits {
                return Err(Error::Invalid(format!("{value:?} isn't a value djmdContent.{name} can take here")));
            }
        }
        self.set_track(id, values)
    }

    /// Puts back what [`Edit::update_track`] replaced, as a new change with a new stamp, answering
    /// what that replaced in turn.
    pub fn restore_track(&mut self, before: &TrackBefore) -> Result<TrackBefore> {
        let values: Vec<(&str, Value)> =
            before.values.iter().map(|(n, v)| Ok((n.as_str(), cues::from_json(v)?))).collect::<Result<_>>()?;
        self.set_track(&before.track_id, &values)
    }

    /// [`Edit::update_track`], whatever the values ([`Edit::restore_track`] puts back what was
    /// stored, a `KeyID` with no row included).
    fn set_track(&mut self, id: &str, values: &[(&str, Value)]) -> Result<TrackBefore> {
        self.track_uuid(id)?;
        let mut seen = FxHashSet::default();
        for (name, _) in values {
            let lower = name.to_lowercase();
            if !SETTABLE.contains(&lower.as_str()) || !seen.insert(lower) {
                return Err(Error::Invalid(format!("djmdContent.{name} can't be set here")));
            }
        }
        let mut before = Vec::with_capacity(values.len());
        for (name, _) in values {
            let v: Value = self
                .conn
                .query_row(&format!("SELECT \"{name}\" FROM djmdContent WHERE ID = ?1"), [id], |r| r.get(0))
                .map_err(|e| self.sql(e))?;
            before.push((name.to_string(), cues::to_json(&v)?));
        }
        let stamp = self.stamp();
        let sets: String = values.iter().map(|(n, _)| format!("\"{n}\" = ?, ")).collect();
        let sql = format!("UPDATE djmdContent SET {sets}rb_local_usn = ?, updated_at = ? WHERE ID = ?");
        let mut params: Vec<Value> = values.iter().map(|(_, v)| v.clone()).collect();
        params.extend([Value::Integer(stamp), Value::Text(self.now.clone()), Value::Text(id.to_string())]);
        self.conn.execute(&sql, rusqlite::params_from_iter(params)).map_err(|e| self.sql(e))?;
        self.touched.insert(id.to_string());
        Ok(TrackBefore { track_id: id.to_string(), values: before })
    }
}

/// What [`Edit::update_track`] replaced on a track: each column it set, with what it held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackBefore {
    /// The track's `djmdContent.ID`.
    pub track_id: String,
    /// Each column set, with its value before (`null` for NULL).
    pub values: Vec<(String, serde_json::Value)>,
}

#[cfg(test)]
mod tests;
