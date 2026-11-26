//! Writing rekordbox's collection (`master.db`), as ADR 0007 decides: only while rekordbox and
//! its agent are closed, and only over a fresh backup that reads back.
//!
//! [`write()`] does the whole of one write. In order:
//!
//! 1. It refuses ([`Refusal`]) while rekordbox or its agent (`rekordboxAgent`) runs, or while a
//!    WAL, shared memory file or journal beside `master.db` holds anything: rekordbox checkpoints
//!    and deletes its WAL when it quits, so a closed collection is one file.
//! 2. It copies `master.db` as it is into the backups folder, checks the copy is byte for byte the
//!    file, then reads the backup back: decrypted, opened, `PRAGMA integrity_check`, a
//!    `djmdContent` table. No backup, no write.
//! 3. It refuses while Cloud Library Sync is on, or might be ([`sync_on`]): what a synced write
//!    has to move hasn't been worked out.
//! 4. The edit runs on that decrypted backup, in one transaction, through [`Edit`], which stamps
//!    the rows it changes as rekordbox does: `rb_local_usn` from the one counter
//!    (`agentRegistry`'s `localUpdateCount`), which ends at least at every stamp and only rises,
//!    and `updated_at` in rekordbox's format.
//! 5. The edited copy is checked (integrity, the counter, each edited track's cue lists agreeing),
//!    encrypted again with the original's salt, written beside `master.db`, read back, and swapped
//!    in with one rename, only if rekordbox and its agent are still closed and `master.db` is still
//!    byte for byte what was backed up.
//!
//! Decrypted copies live in a temporary folder removed on every path out, errors included; the new
//! file beside `master.db` is removed unless it was swapped in, and so is the backup, since
//! `master.db` is then as it was.
//!
//! What rekordbox reads is laid out as rekordbox lays it out (see [`Edit::set_cues`]). Where it's
//! looser than it could be, a write here is stricter: the whole edit is one transaction and checked
//! before the swap, and a track's `djmdCue` rows and its `contentCue` list always agree after it.
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

use super::files::{Running, running_now, write_pages};
use super::{Salt, agent_log_path, default_path, open_copy, resolve_key};
use crate::error::IoContext;
use crate::onelibrary::cipher::{self, Codec};
use crate::{Error, Refusal, Result};

mod cues;
mod sync;

pub use cues::{CueEntry, CuesBefore, cue_list_mismatches};
pub use sync::sync_on;

/// The `master.db` a write goes to, and what it needs besides.
#[derive(Debug, Clone)]
pub struct Target {
    /// The `master.db`.
    pub master_db: PathBuf,
    /// Its SQLCipher passphrase (see [`super::resolve_key`]); a plain SQLite file stays plain.
    pub key: String,
    /// The folder each write's backup of `master.db` goes in, made when missing. Keeping only the
    /// latest several is up to whoever gives it. A backup holds everything `master.db` does,
    /// the cloud agent's sign-in too.
    pub backups: PathBuf,
    /// rekordbox's agent's log (`log.log`), which says whether Cloud Library Sync is on. `None`, or
    /// one that says nothing, counts as it being on.
    pub agent_log: Option<PathBuf>,
    /// Where decrypted working copies go, in a folder of their own removed afterwards: the system's
    /// temporary folder when `None`.
    pub work: Option<PathBuf>,
    /// Whether rekordbox and its agent are running: [`running_now`], unless a test says otherwise.
    pub running: fn() -> Running,
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
        })
    }
}

/// A write that was made.
#[derive(Debug, Clone)]
pub struct Written<T> {
    /// What the edit answered.
    pub value: T,
    /// The backup of `master.db` as it was right before.
    pub backup: PathBuf,
    /// `localUpdateCount` before the write, and after.
    pub count: (i64, i64),
}

/// Make the edit `edit` to the collection at `target`, as the module docs say: over a fresh backup,
/// in one transaction, swapped in with one rename. When it fails or is refused, `master.db` is as
/// it was and nothing decrypted is left behind.
pub fn write<T>(target: &Target, edit: impl FnOnce(&mut Edit<'_>) -> Result<T>) -> Result<Written<T>> {
    #[cfg(test)]
    tests::not_rekordboxs_own(&target.master_db);
    let master = target.master_db.as_path();
    closed(target)?;
    alone(master)?;
    let before = Fingerprint::of(master)?;
    let work = match &target.work {
        Some(dir) => tempfile::tempdir_in(dir).at(dir)?,
        None => tempfile::tempdir().at(std::env::temp_dir())?,
    };
    let backup = Removed::new(backup(master, &target.backups, &before)?);

    // The backup read back is the working copy: what's edited is what was backed up.
    let (db, salt) = open_copy(&backup.0, &target.key, work.path(), true)?;
    let conn = &db.conn;
    let at = |e: rusqlite::Error| Error::parse(&backup.0, e);
    integrity(conn).map_err(at)?.map_or(Ok(()), |why| Err(Error::parse(&backup.0, why)))?;
    let header = journal_mode(&backup.0, salt, &target.key)?;
    if let Some(why) = loclib_sqlcipher::untrusted::distrust(conn, &["trigger", "view"], "rekordbox").map_err(at)? {
        return Err(Error::Refused(Refusal::Odd(why)));
    }
    if let Some(why) = sync_on(conn, target.agent_log.as_deref())? {
        return Err(Error::Refused(Refusal::CloudSync(why)));
    }
    unchanged(master, &before)?;

    // Rows already pointing nowhere (rekordbox leaves some) aren't this write's to answer for.
    let pointing_nowhere = dangling(conn).map_err(at)?;

    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate).map_err(at)?;
    let mut e = Edit::new(&tx, &backup.0)?;
    let value = edit(&mut e)?;
    let count = e.finish()?;
    let (touched, cue_tracks) = (std::mem::take(&mut e.touched), std::mem::take(&mut e.cue_tracks));
    drop(e);
    tx.commit().map_err(at)?;
    check(conn, count, &pointing_nowhere, &touched, &cue_tracks)
        .map_err(|why| Error::Refused(Refusal::Odd(format!("the edited copy {why}"))))?;
    drop(db);

    // Back in WAL mode, as rekordbox keeps it, and encrypted again with the original's salt.
    let plain = work.path().join("master.db");
    set_journal_mode(&plain, header)?;
    let new = Removed::new(beside(master, "locallibrary-new"));
    let mut out = File::create_new(&new.0).at(&new.0)?;
    let codec = salt.map(|salt| Codec::with_key(target.key.as_bytes(), salt));
    write_pages(&plain, codec.as_ref(), &mut out, &new.0)?;
    out.sync_all().at(&new.0)?;
    drop(out);
    if let Ok(meta) = std::fs::metadata(master) {
        let _ = std::fs::set_permissions(&new.0, meta.permissions());
    }
    read_back(&new.0, &target.key, work.path(), count.1)?;

    closed(target)?;
    alone(master)?;
    if Fingerprint::of(master)? != before {
        return Err(Error::Refused(Refusal::Changed));
    }
    std::fs::rename(&new.0, master).at(master)?;
    new.keep();
    sync_dir(master);
    Ok(Written { value, backup: backup.keep(), count })
}

/// Refuses while rekordbox or its agent runs.
fn closed(target: &Target) -> Result<()> {
    let running = (target.running)();
    match () {
        _ if running.rekordbox => Err(Error::Refused(Refusal::RekordboxRunning)),
        _ if running.agent => Err(Error::Refused(Refusal::AgentRunning)),
        _ => Ok(()),
    }
}

/// Refuses when `master.db` isn't one file on its own: its WAL, shared memory or rollback journal
/// holds anything (an empty WAL or shared memory file is what a clean close can leave). Refuses a
/// `master.db` that isn't a file too.
fn alone(master: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(master).at(master)?;
    if !meta.is_file() {
        return Err(Error::Refused(Refusal::Odd(format!("{} isn't a plain file", master.display()))));
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

/// Copies `master` (as `before` found it) into `folder` as `master.<time>.db`, answering where. The
/// copy is made under another name and renamed once it's whole and hashes as `before` does, and
/// `master` still does; otherwise nothing is left.
fn backup(master: &Path, folder: &Path, before: &Fingerprint) -> Result<PathBuf> {
    std::fs::create_dir_all(folder).at(folder)?;
    let name = format!("master.{}.db", chrono::Utc::now().format("%Y-%m-%dT%H%M%S%.3fZ"));
    backup_as(master, &folder.join(name), before)
}

/// [`backup`] to the file `to`, which mustn't be there yet.
fn backup_as(master: &Path, to: &Path, before: &Fingerprint) -> Result<PathBuf> {
    let partial = Removed::new(beside(to, "partial"));
    let mut out = File::create_new(&partial.0).at(&partial.0)?;
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
    Ok(to.to_path_buf())
}

/// A file removed when this is dropped, unless [`Removed::keep`] said to keep it.
struct Removed(PathBuf, bool);

impl Removed {
    fn new(path: PathBuf) -> Removed {
        Removed(path, true)
    }

    fn keep(mut self) -> PathBuf {
        self.1 = false;
        std::mem::take(&mut self.0)
    }
}

impl Drop for Removed {
    fn drop(&mut self) {
        if self.1 {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

/// `<file>.<end>`, beside `file`.
fn beside(file: &Path, end: &str) -> PathBuf {
    PathBuf::from(format!("{}.{end}", file.display()))
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
/// checks it whole, and that its `localUpdateCount` is `count`.
fn read_back(path: &Path, key: &str, work: &Path, count: i64) -> Result<()> {
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
    Ok(())
}

/// Flushes the folder `file` is in, so the rename is on disk (Unix; elsewhere the rename is).
fn sync_dir(file: &Path) {
    #[cfg(unix)]
    let _ = File::open(file.parent().unwrap_or(Path::new("."))).and_then(|d| d.sync_all());
    #[cfg(not(unix))]
    let _ = file;
}

/// The rows `PRAGMA foreign_key_check` finds pointing at a row that isn't there: (table, rowid,
/// the table pointed at).
fn dangling(conn: &Connection) -> rusqlite::Result<FxHashSet<(String, Option<i64>, String)>> {
    let mut stmt = conn.prepare("SELECT \"table\", rowid, parent FROM pragma_foreign_key_check")?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()
}

/// Checks the edited copy before it's written: SQLite finds it whole, no row points nowhere that
/// didn't before (`dangling`), `localUpdateCount` is at least every stamp, each track `touched` is
/// there, and each one whose `cues` were written has its `djmdCue` rows and `contentCue` list
/// agreeing. `Err` says what's wrong.
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

/// One edit of the collection, inside [`write()`]'s transaction: the changes made through it are
/// stamped as rekordbox stamps them. [`Edit::conn`] runs anything else; a row changed that way is
/// stamped with [`Edit::stamp`] and [`Edit::now`].
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
    /// what the columns held before, which [`Edit::restore_track`] puts back. Columns rekordbox
    /// keeps itself (`ID`, `UUID`, `created_at`, `updated_at`, `usn` and the `rb_` ones) can't be
    /// set, nor columns the table lacks.
    pub fn update_track(&mut self, id: &str, values: &[(&str, Value)]) -> Result<TrackBefore> {
        self.track_uuid(id)?;
        let columns = cues::columns(self.conn, "djmdContent").map_err(|e| self.sql(e))?;
        let mut seen = FxHashSet::default();
        for (name, _) in values {
            let lower = name.to_lowercase();
            let kept = ["id", "uuid", "created_at", "updated_at", "usn"].contains(&lower.as_str());
            if kept || lower.starts_with("rb_") || !columns.contains(&lower) || !seen.insert(lower) {
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

    /// Puts back what [`Edit::update_track`] replaced, as a new change with a new stamp, answering
    /// what that replaced in turn.
    pub fn restore_track(&mut self, before: &TrackBefore) -> Result<TrackBefore> {
        let values: Vec<(&str, Value)> =
            before.values.iter().map(|(n, v)| Ok((n.as_str(), cues::from_json(v)?))).collect::<Result<_>>()?;
        self.update_track(&before.track_id, &values)
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
