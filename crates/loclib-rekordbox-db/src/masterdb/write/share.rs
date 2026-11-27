//! A track's analysis files in rekordbox's `share` folder (beside `master.db`), replaced as part of
//! a write. What a decrypted copy of one library (rekordbox 7.2.18, 699 analysed tracks) shows:
//!
//! * `djmdContent.AnalysisDataPath` names the track's `.DAT` (`/PIONEER/USBANLZ/709/857c4-.../
//!   ANLZ0000.DAT`); its `.EXT`, `.2EX` and, for some, `.3EX` sit beside it.
//! * `contentFile` has one row for each of those files (and the track's artwork): `ContentID` the
//!   track, `Path` as `AnalysisDataPath` spells it, `Hash` the file's MD5 in hex, `Size` its bytes,
//!   `rb_local_path` where it is on this computer, stamped (`rb_local_usn`, `updated_at`). Every
//!   analysed track has them. Where rekordbox rewrote a track's files, their rows have the new hash
//!   and size and a later stamp. Cloud Library Sync syncs these files by these rows, and the reader
//!   takes a file whose size disagrees with its row to be still syncing. A second library
//!   (rekordbox 7.2.14, 2,050 tracks) has the same rows: all 9,322 live rows whose file is there
//!   match it in `Hash` and `Size`, and re-analysis updates a row in place (same `ID`, a new hash
//!   and stamp).
//! * `djmdContent.AnalysisUpdated` counts up from 1, as `CueUpdated` does for cues: 1 to 34 in the
//!   second library, at 1 on every track whose files were never rewritten and higher on those
//!   rekordbox analysed again. That it goes up by one for each write of the files is inferred.
//! * The `.3EX` some tracks have isn't an analysis file: it's a MessagePack map holding an audio
//!   embedding (all 1,278 in the second library), which a beat grid, phrases or waveform don't
//!   change, so it's left as it is.
//! * Setting a hot cue doesn't touch the files: rekordbox 7 keeps a collection's cues in
//!   `djmdCue` and `contentCue`, and puts them in analysis files when it exports to a drive.
//!
//! So a file replaced here gets its `contentFile` row's `Hash` and `Size` and a stamp, and the
//! track its `AnalysisUpdated` bumped and a stamp, in the write's transaction. The file itself is
//! backed up into a folder named after `master.db`'s backup (`master.<time>.db.share/PIONEER/...`),
//! its new bytes are written beside it (`ANLZ0000.EXT.locallibrary-new`) and, once `master.db` is
//! swapped in, renamed over it, each file with one rename. Only files that are there are replaced:
//! a track rekordbox hasn't analysed has none, and its `contentFile` rows would have to be made.
//! A new file keeps the old one's permissions, but not its owner, ACLs or extended attributes, as
//! with `master.db`.
//!
//! Between `master.db` going in and the last file, the two disagree. So the files a write replaces
//! are listed beside `master.db` ([`Pending`]) from the first, each file is checked to be one that
//! can be renamed over before `master.db` goes in, and a rename that fails keeps the new bytes and
//! says `master.db` was written ([`Error::PartlyWritten`]). The next write, before its edit,
//! renames in what the list has left, or clears it where `master.db` never went in ([`resume`]).

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use loclib_model::paths;
use md5::{Digest, Md5};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{Edit, Removed, cues, sync_dir};
use crate::error::IoContext;
use crate::{Error, Refusal, Result};

/// The analysis files a track can have replaced, by extension.
const EXTENSIONS: [&str; 3] = ["DAT", "EXT", "2EX"];

/// What [`Edit::replace_analysis_file`] replaced: which file, and where a copy of what it held is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileBefore {
    /// The track's `djmdContent.ID`.
    pub track_id: String,
    /// The file as `master.db` names it (`/PIONEER/USBANLZ/.../ANLZ0000.EXT`).
    pub path: String,
    /// The copy of what it held, in the write's backups folder. Undo needs it kept.
    pub backup: PathBuf,
    /// Its MD5, in hex, as `contentFile.Hash` gives one.
    pub md5: String,
    /// The MD5 of what the write put there instead: undo takes the backup back only while the file
    /// still holds that.
    #[serde(default)]
    pub after_md5: String,
}

/// A file replaced in the edit, waiting to be renamed in once `master.db` has been.
pub(super) struct Replaced {
    track_id: String,
    path: String,
    /// Where it is, and its new bytes beside it.
    file: PathBuf,
    new: Removed,
    /// Its backup.
    backup: PathBuf,
    /// The MD5 of what it held (and was backed up), and of its new bytes, with their size.
    old_md5: String,
    md5: String,
    size: u64,
}

/// A folder removed (with what's in it) when this is dropped, unless [`RemovedDir::keep`] said to
/// keep it.
pub(super) struct RemovedDir(pub(super) PathBuf, bool);

impl RemovedDir {
    pub(super) fn new(path: PathBuf) -> RemovedDir {
        RemovedDir(path, true)
    }

    pub(super) fn keep(mut self) -> PathBuf {
        self.1 = false;
        std::mem::take(&mut self.0)
    }
}

impl Drop for RemovedDir {
    fn drop(&mut self) {
        if self.1 {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

impl Edit<'_> {
    /// Track `track_id`'s analysis file with extension `ext` (`DAT`, `EXT` or `2EX`): as
    /// `master.db` names it, its key and where it is. Refused for a track with no analysis path,
    /// or one outside the share folder's `PIONEER/USBANLZ`.
    fn analysis_file(&self, track_id: &str, ext: &str) -> Result<(String, String, PathBuf)> {
        self.track_uuid(track_id)?;
        let ext = ext.to_ascii_uppercase();
        if !EXTENSIONS.contains(&ext.as_str()) {
            return Err(Error::Invalid(format!("{ext} isn't an analysis file (DAT, EXT or 2EX)")));
        }
        let dat: Option<String> = self
            .conn
            .query_row("SELECT AnalysisDataPath FROM djmdContent WHERE ID = ?1", [track_id], |r| r.get(0))
            .map_err(|e| self.sql(e))?;
        let dat = dat.unwrap_or_default();
        if dat.is_empty() {
            return Err(Error::Invalid(format!(
                "track {track_id} has no analysis files (rekordbox hasn't analysed it), so it can't hold a beat grid, \
                 phrases or waveforms"
            )));
        }
        let stem =
            dat.len().checked_sub(4).filter(|&i| dat.is_char_boundary(i) && dat[i..].eq_ignore_ascii_case(".DAT"));
        let path = stem.map(|i| format!("{}.{ext}", &dat[..i]));
        let key = path.as_deref().map(paths::key).unwrap_or_default();
        let parts: Vec<&str> = key.split('/').collect();
        let inside = parts.len() > 2
            && parts.iter().all(|c| paths::is_safe_component(c))
            // Windows drops a trailing dot or space, so such a name would be another file there.
            && parts.iter().all(|c| !c.ends_with(['.', ' ']))
            && parts[0].eq_ignore_ascii_case("PIONEER")
            && parts[1].eq_ignore_ascii_case("USBANLZ");
        match path {
            Some(path) if inside => {
                let file = paths::to_fs(self.share, &key);
                Ok((path, key, file))
            }
            _ => Err(Error::Refused(Refusal::Odd(format!(
                "track {track_id}'s analysis path {dat:?} isn't a .DAT in the share folder's PIONEER/USBANLZ"
            )))),
        }
    }

    /// What track `track_id`'s analysis file with extension `ext` (`DAT`, `EXT` or `2EX`) holds:
    /// `None` when it has none.
    pub fn read_analysis_file(&self, track_id: &str, ext: &str) -> Result<Option<Vec<u8>>> {
        let (_, _, file) = self.analysis_file(track_id, ext)?;
        match std::fs::symlink_metadata(&file) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Io { path: file, source: e }),
            Ok(m) if !m.is_file() => Err(not_plain(&file)),
            Ok(_) => {
                self.in_share(&file)?;
                Ok(Some(std::fs::read(&file).at(&file)?))
            }
        }
    }

    /// Refuses `file` unless it's in the share folder, links followed.
    fn in_share(&self, file: &Path) -> Result<()> {
        let share = self.share.canonicalize().at(self.share)?;
        match file.canonicalize().at(file)?.starts_with(&share) {
            true => Ok(()),
            false => Err(not_plain(file)),
        }
    }

    /// Replaces what track `track_id`'s analysis file with extension `ext` (`DAT`, `EXT` or `2EX`)
    /// holds with `bytes`, as the module docs say: backed up, its `contentFile` row given the new
    /// hash and size and stamped, the track's `AnalysisUpdated` bumped and stamped once in the
    /// write, and the file renamed in after `master.db`. Answers what it replaced, which
    /// [`Edit::restore_analysis_file`] puts back; `None` when it holds `bytes` already. Refused for
    /// a file that isn't there, one replaced already in this write, and one whose `contentFile`
    /// rows aren't the one rekordbox keeps, listing what it holds where it is.
    pub fn replace_analysis_file(&mut self, track_id: &str, ext: &str, bytes: &[u8]) -> Result<Option<FileBefore>> {
        let (path, key, file) = self.analysis_file(track_id, ext)?;
        // Two tracks can spell one file in other cases, on a folder that ignores case.
        if self.replaced.iter().any(|r| paths::file_key(&r.path) == paths::file_key(&path)) {
            return Err(Error::Invalid(format!("{path} is replaced twice in one write")));
        }
        let meta = match std::fs::symlink_metadata(&file) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Invalid(format!(
                    "track {track_id} has no {path} in rekordbox's share folder to replace (only files there are \
                     replaced)"
                )));
            }
            Err(e) => return Err(Error::Io { path: file, source: e }),
            Ok(m) if !m.is_file() => return Err(not_plain(&file)),
            Ok(m) => m,
        };
        self.in_share(&file)?;
        let old = std::fs::read(&file).at(&file)?;
        if old == bytes {
            return Ok(None);
        }
        let (old_md5, md5) = (md5_hex(&old), md5_hex(bytes));
        let row = match has_content_file(self.conn).map_err(|e| self.sql(e))? {
            true => Some(self.listed(track_id, &path, &file, &old, &old_md5)?),
            false => None,
        };
        let backup = paths::to_fs(self.share_backup, &key);
        if let Some(dir) = backup.parent() {
            std::fs::create_dir_all(dir).at(dir)?;
        }
        write_new(&backup, &old)?;
        // Each folder made for it, up to the backups folder, is on disk before master.db is swapped.
        let backups = self.share_backup.parent().unwrap_or(self.share_backup);
        for made in backup.ancestors().take_while(|p| *p != backups) {
            sync_dir(made);
        }
        // Only a file this write made is removed should it fail: one there already (another
        // write's, or one a crash left) is refused, and left as it is.
        let new = PathBuf::from(format!("{}.locallibrary-new", file.display()));
        write_new(&new, bytes)?;
        let new = Removed::new(new);
        let _ = std::fs::set_permissions(&new.0, meta.permissions());

        if let Some(id) = &row {
            let stamp = self.stamp();
            let sql = "UPDATE contentFile SET Hash = ?1, Size = ?2, rb_local_usn = ?3, updated_at = ?4 WHERE ID = ?5";
            let params = rusqlite::params![md5, bytes.len() as i64, stamp, self.now, id];
            self.conn.execute(sql, params).map_err(|e| self.sql(e))?;
        }
        if !self.analysed.iter().any(|t| t == track_id) {
            self.analysed.push(track_id.to_string());
        }
        self.touched.insert(track_id.to_string());
        self.replaced.push(Replaced {
            track_id: track_id.to_string(),
            path: path.clone(),
            file,
            new,
            backup: backup.clone(),
            old_md5: old_md5.clone(),
            md5: md5.clone(),
            size: bytes.len() as u64,
        });
        self.list_pending()?;
        Ok(Some(FileBefore { track_id: track_id.to_string(), path, backup, md5: old_md5, after_md5: md5 }))
    }

    /// Lists the files replaced so far beside `master.db` ([`Pending`]), on disk before it
    /// answers. The list goes should the write fail before `master.db` is swapped in.
    fn list_pending(&mut self) -> Result<()> {
        let files = self.replaced.iter().map(|r| PendingFile {
            track_id: r.track_id.clone(),
            path: r.path.clone(),
            md5: r.md5.clone(),
        });
        let list = serde_json::to_vec(&Pending { files: files.collect() }).expect("a list serialises");
        let partial = PathBuf::from(format!("{}.partial", self.pending.display()));
        let _ = std::fs::remove_file(&partial);
        write_new(&partial, &list)?;
        std::fs::rename(&partial, self.pending).at(self.pending)?;
        sync_dir(self.pending);
        if self.listed.is_none() {
            self.listed = Some(Removed::new(self.pending.to_path_buf()));
        }
        Ok(())
    }

    /// The `contentFile` row of track `track_id`'s `path` (at `file`, holding `old`, whose MD5 is
    /// `old_md5`): refused unless there's one, listing what the file holds, and, where it says
    /// where the file is on this computer, saying it's `file`. A file its row doesn't list is half
    /// synced or was changed outside rekordbox, and replacing it would hide that.
    fn listed(&self, track_id: &str, path: &str, file: &Path, old: &[u8], old_md5: &str) -> Result<String> {
        type Row = (String, Option<String>, Option<i64>, Option<String>);
        let rows: Vec<Row> = self
            .conn
            .prepare(
                "SELECT ID, Hash, Size, rb_local_path FROM contentFile WHERE ContentID = ?1 \
                 AND lower(Path) = lower(?2) AND IFNULL(rb_local_deleted, 0) = 0",
            )
            .and_then(|mut s| {
                s.query_map([track_id, path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect()
            })
            .map_err(|e| self.sql(e))?;
        let odd = |why: String| Error::Refused(Refusal::Odd(format!("track {track_id}'s {path} {why}")));
        let [(id, hash, size, local)] = &rows[..] else {
            return Err(odd(format!(
                "has {} contentFile rows, where rekordbox keeps one for each analysis file",
                rows.len()
            )));
        };
        let lists = hash.as_deref() == Some(old_md5) && *size == i64::try_from(old.len()).ok();
        if !lists {
            return Err(odd(format!(
                "holds {} bytes (MD5 {old_md5}) where its contentFile row lists {} (MD5 {}): it's still syncing, \
                 or changed outside rekordbox",
                old.len(),
                size.map_or("no size".into(), |s| format!("{s} bytes")),
                hash.as_deref().unwrap_or("none")
            )));
        }
        let elsewhere = |local: &str| {
            let there = Path::new(local).canonicalize().ok();
            there.is_none() || there != file.canonicalize().ok()
        };
        if let Some(local) = local.as_deref().filter(|l| !l.is_empty() && elsewhere(l)) {
            return Err(odd(format!(
                "is at {local} on this computer by its contentFile row, not in this share folder"
            )));
        }
        Ok(id.clone())
    }

    /// Puts back what [`Edit::replace_analysis_file`] replaced, from its backup, as a new change
    /// with new stamps, answering what that replaced in turn. Refused when the backup is gone, isn't
    /// one of this collection's write backups, or no longer holds what was backed up, and when
    /// rekordbox has analysed the track again since: its analysis files moved, or the file no longer
    /// holds what the write put there.
    pub fn restore_analysis_file(&mut self, before: &FileBefore) -> Result<Option<FileBefore>> {
        let ext = before.path.rsplit_once('.').map_or("", |(_, ext)| ext);
        let (path, _, file) = self.analysis_file(&before.track_id, ext)?;
        if !path.eq_ignore_ascii_case(&before.path) {
            return Err(Error::Invalid(format!(
                "track {}'s analysis files are {path} now, not {}: rekordbox analysed it again since",
                before.track_id, before.path
            )));
        }
        // Re-analysis keeps a track's paths and rows, and changes what its files hold.
        let now = std::fs::read(&file).at(&file)?;
        if md5_hex(&now) != before.after_md5 {
            return Err(Error::Refused(Refusal::Odd(format!(
                "{} no longer holds what was saved there: rekordbox analysed it again since, and undo would throw \
                 that away",
                before.path
            ))));
        }
        // Backups are kept beside master.db's, in the folder this write backs up into.
        let backups = self.share_backup.parent().unwrap_or(self.share_backup);
        let backups = backups.canonicalize().at(backups)?;
        if !before.backup.canonicalize().at(&before.backup)?.starts_with(&backups) {
            return Err(Error::Invalid(format!(
                "{} isn't in the collection's backups folder ({})",
                before.backup.display(),
                backups.display()
            )));
        }
        let bytes = std::fs::read(&before.backup).at(&before.backup)?;
        if md5_hex(&bytes) != before.md5 {
            return Err(Error::Invalid(format!(
                "{} no longer holds what {} held before",
                before.backup.display(),
                before.path
            )));
        }
        self.replace_analysis_file(&before.track_id, ext, &bytes)
    }

    /// Bumps `AnalysisUpdated` on each track whose analysis files were replaced, and stamps it:
    /// once a write, after its files' rows.
    pub(super) fn bump_analysed(&mut self) -> Result<()> {
        let bump = match cues::columns(self.conn, "djmdContent").map_err(|e| self.sql(e))?.contains("analysisupdated") {
            true => "AnalysisUpdated = CAST(CAST(IFNULL(AnalysisUpdated, 0) AS INTEGER) + 1 AS TEXT), ",
            false => "",
        };
        for track in std::mem::take(&mut self.analysed) {
            let stamp = self.stamp();
            let sql = format!("UPDATE djmdContent SET {bump}rb_local_usn = ?1, updated_at = ?2 WHERE ID = ?3");
            self.conn.execute(&sql, rusqlite::params![stamp, self.now, track]).map_err(|e| self.sql(e))?;
        }
        Ok(())
    }
}

fn not_plain(file: &Path) -> Error {
    Error::Refused(Refusal::Odd(format!("{} isn't a plain file in rekordbox's share folder", file.display())))
}

/// Writes `bytes` to a new file at `path`, on disk before it answers. Refused when there's a file
/// there already, which is left as it is; one it made and couldn't finish is removed.
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut out = File::create_new(path).at(path)?;
    let written = out.write_all(bytes).and_then(|()| out.sync_all()).at(path);
    if written.is_err() {
        drop(out);
        let _ = std::fs::remove_file(path);
    }
    written
}

pub(super) fn md5_hex(bytes: &[u8]) -> String {
    Md5::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn has_content_file(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(!cues::columns(conn, "contentFile")?.is_empty())
}

/// Checks the edited copy's `contentFile` rows give each file `replaced` its new hash and size.
/// `Err` says what's wrong.
pub(super) fn check(conn: &Connection, replaced: &[Replaced]) -> std::result::Result<(), String> {
    let sql = |e: rusqlite::Error| e.to_string();
    if !has_content_file(conn).map_err(sql)? {
        return Ok(());
    }
    for r in replaced {
        let row: Option<(Option<String>, Option<i64>)> = conn
            .query_row(
                "SELECT Hash, Size FROM contentFile WHERE ContentID = ?1 AND lower(Path) = lower(?2) \
                 AND IFNULL(rb_local_deleted, 0) = 0",
                [&r.track_id, &r.path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        if row != Some((Some(r.md5.clone()), Some(r.size as i64))) {
            return Err(format!("has {}'s contentFile row disagreeing with the file written", r.path));
        }
    }
    Ok(())
}

/// Refuses when a file to replace no longer holds what was backed up, its backup doesn't read
/// back as that, its new bytes don't read back, or it can't be replaced where it is: what would
/// stop it going in once `master.db` has.
pub(super) fn unchanged(replaced: &[Replaced]) -> Result<()> {
    for r in replaced {
        replaceable(&r.file)?;
        if md5_hex(&std::fs::read(&r.file).at(&r.file)?) != r.old_md5 {
            return Err(Error::Refused(Refusal::Changed));
        }
        if md5_hex(&std::fs::read(&r.backup).at(&r.backup)?) != r.old_md5 {
            return Err(Error::Refused(Refusal::Odd(format!("the backup of {} doesn't read back", r.path))));
        }
        if md5_hex(&std::fs::read(&r.new.0).at(&r.new.0)?) != r.md5 {
            return Err(Error::Refused(Refusal::Odd(format!("the new {} doesn't read back", r.path))));
        }
    }
    Ok(())
}

/// Renames each file's new bytes over it, once `master.db` has been swapped in, then takes the
/// list of them away (`pending`). Should a rename fail, `master.db` already lists the new bytes:
/// they stay beside their files with the list, which the next write finishes ([`resume`]), and
/// the error says so, with both backups.
pub(super) fn swap_in(replaced: Vec<Replaced>, pending: &Path, backup: &Path, share_backup: &Path) -> Result<()> {
    let mut left = replaced.into_iter();
    while let Some(r) = left.next() {
        if let Err(source) = rename_in(&r.new.0, &r.file) {
            let files = std::iter::once(r).chain(left).map(|r| {
                r.new.keep();
                r.file
            });
            return Err(Error::PartlyWritten {
                backup: backup.to_path_buf(),
                share_backup: share_backup.to_path_buf(),
                pending: files.collect(),
                source,
            });
        }
        r.new.keep();
        sync_dir(&r.file);
    }
    match std::fs::remove_file(pending) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(Error::Io { path: pending.into(), source: e });
        }
        _ => sync_dir(pending),
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// Whether [`swap_in`]'s renames fail, for a test on this thread.
    pub(super) static FAIL_RENAMES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Renames `new` over `file`.
fn rename_in(new: &Path, file: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if FAIL_RENAMES.get() {
        return Err(std::io::Error::other("a test's rename"));
    }
    std::fs::rename(new, file)
}

/// The analysis files a write replaces, listed beside `master.db`
/// (`master.db.locallibrary-pending`) from the first one replaced until each is renamed in: what
/// the next write finishes or clears should this one be stopped (killed, a power cut, a rename
/// that failed) between swapping `master.db` in and renaming the last file. Each file by its track
/// and its path as `master.db` names it, with the MD5 of its new bytes; where it is comes from
/// the share folder, not from the list.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Pending {
    files: Vec<PendingFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PendingFile {
    track_id: String,
    path: String,
    md5: String,
}

/// Finishes, or clears, what [`Pending`] at `pending` lists, before a write's edit, by what
/// `master.db` (open as `conn`) and the files in `share` say. For each file:
///
/// * `master.db` lists its new bytes, which are beside it: renamed in (the write got that far);
/// * `master.db` lists what it holds: the write never swapped `master.db` in (or it was put back
///   from its backup), so the new bytes beside it, if any, go;
/// * anything else is refused, with what's there left as it is: someone has to look.
///
/// Then the list goes.
pub(super) fn resume(conn: &Connection, pending: &Path, share: &Path) -> Result<()> {
    let list = match std::fs::read(pending) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        list => list.at(pending)?,
    };
    let odd = |why: String| Error::Refused(Refusal::Odd(format!("{} {why}", pending.display())));
    let list: Pending = serde_json::from_slice(&list).map_err(|e| odd(format!("can't be read ({e})")))?;
    let sql = |e: rusqlite::Error| Error::Refused(Refusal::Odd(e.to_string()));
    let md5_of = |p: &Path| match std::fs::read(p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        bytes => bytes.at(p).map(|b| Some(md5_hex(&b))),
    };
    if !has_content_file(conn).map_err(sql)? {
        return Err(
            odd("lists analysis files being replaced, but master.db has no contentFile rows to tell by".into()),
        );
    }
    // Each file's new bytes, and where they go (`None`: nowhere, they go).
    let mut steps = Vec::new();
    for f in &list.files {
        let key = paths::key(&f.path);
        let disagree = || {
            odd(format!(
                "lists {} as being replaced by an earlier write, but master.db and the files there don't agree with \
                 it: check them, then remove {}",
                f.path,
                pending.display()
            ))
        };
        if !key.split('/').all(paths::is_safe_component) {
            return Err(disagree());
        }
        let file = paths::to_fs(share, &key);
        let new = PathBuf::from(format!("{}.locallibrary-new", file.display()));
        let listed: Option<String> = conn
            .query_row(
                "SELECT Hash FROM contentFile WHERE ContentID = ?1 AND lower(Path) = lower(?2) \
                 AND IFNULL(rb_local_deleted, 0) = 0",
                [&f.track_id, &f.path],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?
            .flatten();
        let (listed, held, beside) = (listed.as_ref(), md5_of(&file)?, md5_of(&new)?);
        let ours = beside.as_ref().is_none_or(|b| *b == f.md5);
        steps.push(match () {
            // master.db is in, the file not yet.
            _ if ours && listed == Some(&f.md5) && beside.is_some() => (new, Some(file)),
            // Both in already, or master.db never was: either way the file is what it lists.
            _ if ours && listed.is_some() && listed == held.as_ref() => (new, None),
            _ => return Err(disagree()),
        });
    }
    for (new, into) in steps {
        match into {
            Some(file) => {
                std::fs::rename(&new, &file).at(&file)?;
                sync_dir(&file);
            }
            None => match std::fs::remove_file(&new) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(Error::Io { path: new, source: e }),
                _ => {}
            },
        }
    }
    std::fs::remove_file(pending).at(pending)?;
    sync_dir(pending);
    Ok(())
}

/// Refuses `file` unless it can be replaced: not read-only, and open to write (and, on Windows,
/// to delete, which renaming over it takes: a scanner or indexer holding it open without sharing
/// that would stop the rename after `master.db` is in).
fn replaceable(file: &Path) -> Result<()> {
    let refused =
        |why: &str| Err(Error::Refused(Refusal::Odd(format!("{} {why}, so it can't be replaced", file.display()))));
    if std::fs::metadata(file).at(file)?.permissions().readonly() {
        return refused("is read-only");
    }
    let mut open = std::fs::OpenOptions::new();
    open.write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // DELETE, with every other opener's access shared.
        open.access_mode(0x0001_0000 | 0x4000_0000).share_mode(0x7);
    }
    match open.open(file) {
        Ok(_) => Ok(()),
        Err(e) => refused(&format!("can't be opened to write ({e})")),
    }
}
