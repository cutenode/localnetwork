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
//!   takes a file whose size disagrees with its row to be still syncing.
//! * `djmdContent.AnalysisUpdated` counts up from 1 (1-9 in that library), as `CueUpdated` does
//!   for cues: taken to be bumped when the analysis changes, as `CueUpdated` is (inferred, not seen).
//! * Setting a hot cue doesn't touch the files: rekordbox 7 keeps a collection's cues in
//!   `djmdCue` and `contentCue`, and puts them in analysis files when it exports to a drive.
//!
//! So a file replaced here gets its `contentFile` row's `Hash` and `Size` and a stamp, and the
//! track its `AnalysisUpdated` bumped and a stamp, in the write's transaction. The file itself is
//! backed up into a folder named after `master.db`'s backup (`master.<time>.db.share/PIONEER/...`),
//! its new bytes are written beside it (`ANLZ0000.EXT.locallibrary-new`) and, once `master.db` is
//! swapped in, renamed over it, each file with one rename. Only files that are there are replaced:
//! a track rekordbox hasn't analysed has none, and its `contentFile` rows would have to be made.

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
}

/// A file replaced in the edit, waiting to be renamed in once `master.db` has been.
pub(super) struct Replaced {
    track_id: String,
    path: String,
    /// Where it is, and its new bytes beside it.
    file: PathBuf,
    new: Removed,
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

    pub(super) fn keep(mut self) {
        self.1 = false;
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
            Ok(_) => Ok(Some(std::fs::read(&file).at(&file)?)),
        }
    }

    /// Replaces what track `track_id`'s analysis file with extension `ext` (`DAT`, `EXT` or `2EX`)
    /// holds with `bytes`, as the module docs say: backed up, its `contentFile` row given the new
    /// hash and size and stamped, the track's `AnalysisUpdated` bumped and stamped once in the
    /// write, and the file renamed in after `master.db`. Answers what it replaced, which
    /// [`Edit::restore_analysis_file`] puts back; `None` when it holds `bytes` already. Refused for
    /// a file that isn't there, one replaced already in this write, and one whose `contentFile`
    /// rows aren't the one rekordbox keeps.
    pub fn replace_analysis_file(&mut self, track_id: &str, ext: &str, bytes: &[u8]) -> Result<Option<FileBefore>> {
        let (path, key, file) = self.analysis_file(track_id, ext)?;
        if self.replaced.iter().any(|r| r.file == file) {
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
        let share = self.share.canonicalize().at(self.share)?;
        if !file.canonicalize().at(&file)?.starts_with(&share) {
            return Err(not_plain(&file));
        }
        let old = std::fs::read(&file).at(&file)?;
        if old == bytes {
            return Ok(None);
        }
        let backup = paths::to_fs(self.share_backup, &key);
        if let Some(dir) = backup.parent() {
            std::fs::create_dir_all(dir).at(dir)?;
        }
        write_new(&backup, &old)?;
        // Only a file this write made is removed should it fail: one there already (another
        // write's, or one a crash left) is refused, and left as it is.
        let new = PathBuf::from(format!("{}.locallibrary-new", file.display()));
        write_new(&new, bytes)?;
        let new = Removed::new(new);
        let _ = std::fs::set_permissions(&new.0, meta.permissions());

        let (old_md5, md5) = (md5_hex(&old), md5_hex(bytes));
        if has_content_file(self.conn).map_err(|e| self.sql(e))? {
            let ids: Vec<String> = self
                .conn
                .prepare(
                    "SELECT ID FROM contentFile WHERE ContentID = ?1 AND lower(Path) = lower(?2) \
                     AND IFNULL(rb_local_deleted, 0) = 0",
                )
                .and_then(|mut s| s.query_map([track_id, path.as_str()], |r| r.get(0))?.collect())
                .map_err(|e| self.sql(e))?;
            let [id] = &ids[..] else {
                return Err(Error::Refused(Refusal::Odd(format!(
                    "track {track_id}'s {path} has {} contentFile rows, where rekordbox keeps one for each analysis file",
                    ids.len()
                ))));
            };
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
            old_md5: old_md5.clone(),
            md5,
            size: bytes.len() as u64,
        });
        Ok(Some(FileBefore { track_id: track_id.to_string(), path, backup, md5: old_md5 }))
    }

    /// Puts back what [`Edit::replace_analysis_file`] replaced, from its backup, as a new change
    /// with new stamps, answering what that replaced in turn. Refused when the backup is gone, isn't
    /// one of this collection's write backups, or no longer holds what was backed up, and when the
    /// track's analysis files have moved since (rekordbox analysed it again).
    pub fn restore_analysis_file(&mut self, before: &FileBefore) -> Result<Option<FileBefore>> {
        let ext = before.path.rsplit_once('.').map_or("", |(_, ext)| ext);
        let (path, _, _) = self.analysis_file(&before.track_id, ext)?;
        if !path.eq_ignore_ascii_case(&before.path) {
            return Err(Error::Invalid(format!(
                "track {}'s analysis files are {path} now, not {}: rekordbox analysed it again since",
                before.track_id, before.path
            )));
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

/// Refuses when a file to replace no longer holds what was backed up, or its new bytes don't
/// read back.
pub(super) fn unchanged(replaced: &[Replaced]) -> Result<()> {
    for r in replaced {
        if md5_hex(&std::fs::read(&r.file).at(&r.file)?) != r.old_md5 {
            return Err(Error::Refused(Refusal::Changed));
        }
        if md5_hex(&std::fs::read(&r.new.0).at(&r.new.0)?) != r.md5 {
            return Err(Error::Refused(Refusal::Odd(format!("the new {} doesn't read back", r.path))));
        }
    }
    Ok(())
}

/// Renames each file's new bytes over it, once `master.db` has been swapped in. Should one fail,
/// `master.db` already gives it its new hash: the error says which file, and it and those after it
/// keep what they held.
pub(super) fn swap_in(replaced: Vec<Replaced>) -> Result<()> {
    for r in replaced {
        std::fs::rename(&r.new.0, &r.file).at(&r.file)?;
        r.new.keep();
        sync_dir(&r.file);
    }
    Ok(())
}
