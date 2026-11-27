//! Saving locallibrary's analysis of a track (beat grid, phrases, waveform, key, auto cues) to
//! rekordbox's collection on this computer, as [`crate::analysis_edit`] saves it to a drive.
//!
//! It goes through the collection's one write path ([`loclib_rekordbox_db::masterdb::write::write`]): only while
//! rekordbox and its agent are closed and Cloud Library Sync is off, over a fresh backup of
//! `master.db`, in one transaction, swapped in with one rename. In that write:
//!
//! * The grid, phrases and waveform go into the track's analysis files in rekordbox's `share`
//!   folder, as a drive's take them ([`crate::analysis_edit`]'s writers, the same checks and
//!   warnings): each file changed is backed up, its `contentFile` row given its new MD5 and size,
//!   and renamed in after `master.db` ([`loclib_rekordbox_db::masterdb::write::Edit::replace_analysis_file`]). A track
//!   rekordbox hasn't analysed has no files, and is refused.
//! * `djmdContent` gets the grid's tempo as `BPM` and the key as `KeyID`: one of `djmdKey`'s
//!   rows, the key in the spelling the track's key has (Camelot, a note name, `min`/`maj`), else in
//!   the one most of the collection has, and its `AnalysisUpdated` goes up.
//! * Auto cues ([`crate::cue_edit::auto`]) are placed on the new grid and phrases among the cues
//!   the collection has (`djmdCue`), and written as the cue writer writes a cue list
//!   ([`loclib_rekordbox_db::masterdb::write::Edit::set_cues`]): rekordbox 7 keeps a collection's cues there, not in
//!   the analysis files.
//!
//! Each save answers what it replaced ([`AnalysisBefore`]), which [`undo`] puts back as a new
//! write with new stamps. Errors are the write path's: [`Error::Refused`] says why nothing was
//! written, and [`loclib_rekordbox_db::Refusal::waits`] which refusals pass once rekordbox quits;
//! [`Error::PartlyWritten`] that `master.db` was, but analysis files it lists are still beside
//! theirs, which the next write puts in.

use rusqlite::OptionalExtension;
use rusqlite::types::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use loclib_rekordbox_db::masterdb::write::{
    self as masterdb_write, CueEntry, CuesBefore, Edit, FileBefore, Target, TrackBefore,
};
use loclib_rekordbox_db::onelibrary::hot_cue_from_kind;
use loclib_rekordbox_db::{Error, Result};

use crate::analysis_edit::{self, NewAnalysis};
use crate::anlz::{self, CuePoint, DEFAULT_HOT_CUE_RGB};
use crate::cue_edit::{self, AutoCues};
use crate::harmony::Camelot;
use crate::model::Track;

/// What [`save`] writes to a track of the collection; what it leaves out stays as it is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CollectionAnalysis {
    /// The beat grid, phrases and waveform, as a drive's analysis edit gives them. The waveform is
    /// given as bands, in base64.
    #[serde(flatten)]
    pub analysis: NewAnalysis,
    /// The key, in any notation (`8A`, `Am`, `A minor`): saved as one of the collection's keys that
    /// spells it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Cues placed on the track's grid and phrases (the new ones, when this gives them).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cues: Option<AutoCues>,
}

/// What a [`save`] replaced on a track: what [`undo`] puts back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnalysisBefore {
    /// The track's `djmdContent.ID`.
    pub track_id: String,
    /// Its analysis files replaced, each with where its backup is.
    pub files: Vec<FileBefore>,
    /// Its `BPM` and `KeyID`, when they were set.
    pub track: Option<TrackBefore>,
    /// Its cues, when they were set.
    pub cues: Option<CuesBefore>,
}

impl AnalysisBefore {
    /// Whether the save changed nothing.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.track.is_none() && self.cues.is_none()
    }
}

/// A save that was made.
#[derive(Debug, Clone)]
pub struct Saved {
    /// What it replaced.
    pub before: AnalysisBefore,
    /// What to warn of (cues left off a moved grid, an extended grid dropped...).
    pub warnings: Vec<String>,
    /// The backup of `master.db` as it was right before; the analysis files' backups are in
    /// `<backup>.share`.
    pub backup: PathBuf,
    /// `localUpdateCount` before and after.
    pub count: (i64, i64),
}

/// Writes `edit` to the track `track_id` (its `djmdContent.ID`) of the collection at `target`,
/// as the module docs say.
pub fn save(target: &Target, track_id: &str, edit: &CollectionAnalysis) -> Result<Saved> {
    let bands = edit.analysis.waveform.as_ref().map(analysis_edit::given_bands).transpose().map_err(invalid)?;
    let written = masterdb_write::write(target, |e| {
        let t = track(e, track_id)?;
        let mut before = AnalysisBefore { track_id: track_id.to_string(), ..AnalysisBefore::default() };
        let exts = ["DAT", "EXT", "2EX"];
        let mut files = [None, None, None];
        for (file, ext) in files.iter_mut().zip(exts) {
            *file = e.read_analysis_file(track_id, ext)?;
        }
        let mut warnings = Vec::new();
        // The collection's cues are in djmdCue, not in its analysis files: a moved grid is
        // weighed against those.
        let have = cues(e, track_id)?;
        if edit.analysis.beats.is_some() || edit.analysis.phrases.is_some() || bands.is_some() {
            let (changed, warned) =
                analysis_edit::rewrite_files(&t, files.clone(), have.clone(), &edit.analysis, bands.as_ref())
                    .map_err(invalid)?;
            warnings = warned;
            for (ext, bytes) in changed {
                let i = exts.iter().position(|x| *x == ext).expect("a file rewrite_files names");
                before.files.extend(e.replace_analysis_file(track_id, &ext, &bytes)?);
                files[i] = Some(bytes);
            }
        }

        let mut values: Vec<(&str, Value)> = Vec::new();
        if let Some(bpm) = edit.analysis.tempo() {
            values.push(("BPM", Value::Integer((bpm * 100.0).round() as i64)));
        }
        if let Some(name) = &edit.key {
            values.push(("KeyID", Value::Text(key_id(e, track_id, name)?)));
        }
        if !values.is_empty() {
            before.track = Some(e.update_track(track_id, &values)?);
        }

        if let Some(auto) = edit.cues {
            // Cues are placed on the grid, but the grid and key don't depend on them: cues that
            // can't be placed are left out with a warning, unless they're all the save is. Both
            // refuse before writing anything.
            match place_cues(e, track_id, &t, &have, &files, auto) {
                Ok((cues, said)) => {
                    before.cues = cues;
                    warnings.extend(said);
                }
                Err(Error::Invalid(why)) if !(edit.analysis.is_empty() && edit.key.is_none()) => {
                    warnings.push(format!("{}'s cues weren't placed ({why}): the rest is saved", t.display_name()));
                }
                Err(e) => return Err(e),
            }
        }
        Ok((before, warnings))
    })?;
    let (before, warnings) = written.value;
    Ok(Saved { before, warnings, backup: written.backup, count: written.count })
}

/// Puts back what a [`save`] (or an undo) replaced, as a new write with new stamps: the files from
/// their backups (which have to be kept), the track's `BPM` and `KeyID`, and its cues. Answers
/// what that replaced in turn, which undoes the undo.
pub fn undo(target: &Target, before: &AnalysisBefore) -> Result<Saved> {
    let written = masterdb_write::write(target, |e| {
        let mut now = AnalysisBefore { track_id: before.track_id.clone(), ..AnalysisBefore::default() };
        for file in &before.files {
            now.files.extend(e.restore_analysis_file(file)?);
        }
        if let Some(track) = &before.track {
            now.track = Some(e.restore_track(track)?);
        }
        if let Some(cues) = &before.cues {
            now.cues = Some(e.restore_cues(cues)?);
        }
        Ok(now)
    })?;
    Ok(Saved { before: written.value, warnings: Vec::new(), backup: written.backup, count: written.count })
}

/// Places `auto`'s cues on `t` among the cues it `has`, on the grid and phrases of its analysis
/// `files` (as the save left them), and writes them if they changed: what that replaced, and what
/// to warn of.
fn place_cues(
    e: &mut Edit<'_>,
    track_id: &str,
    t: &Track,
    has: &[CuePoint],
    files: &[Option<Vec<u8>>; 3],
    auto: AutoCues,
) -> Result<(Option<CuesBefore>, Vec<String>)> {
    let Some(dat) = files[0].as_deref() else {
        return Err(invalid(crate::Error::invalid(format!(
            "{} has no analysis files (rekordbox hasn't analysed it), so cues can't be placed on its beat grid",
            t.display_name()
        ))));
    };
    let placed = cue_edit::auto_placed(t, has.to_vec(), dat, files[1].as_deref(), auto).map_err(invalid)?;
    let warnings = Vec::new();
    let entries = |c: &[CuePoint]| {
        let mut out: Vec<CueEntry> = c.iter().map(entry).collect();
        out.sort_by_key(|c| (c.hot_cue.map_or(0, |h| u32::from(h) + 1), c.start_ms));
        out
    };
    if entries(&placed) == entries(has) {
        return Ok((None, warnings));
    }
    Ok((Some(e.set_cues(track_id, &entries(&placed))?), warnings))
}

fn invalid(e: crate::Error) -> Error {
    Error::Invalid(e.to_string())
}

/// The track `id` as the analysis writers take it: its title, length, tempo and files.
fn track(e: &Edit<'_>, id: &str) -> Result<Track> {
    let sql = "SELECT IFNULL(Title, ''), IFNULL(Length, 0), IFNULL(BPM, 0), IFNULL(FolderPath, ''), \
               IFNULL(AnalysisDataPath, '') FROM djmdContent WHERE ID = ?1 AND IFNULL(rb_local_deleted, 0) = 0";
    let row = e
        .conn()
        .query_row(sql, [id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get(3)?, r.get(4)?))
        })
        .optional()
        .map_err(|e| Error::OneLibrary(e.to_string()))?;
    let Some((title, length, bpm, file_path, analyze_path)) = row else {
        return Err(Error::NotFound(format!("track {id} in master.db")));
    };
    Ok(Track {
        title: if title.is_empty() { format!("track {id}") } else { title },
        duration_secs: u32::try_from(length).unwrap_or(0),
        bpm: bpm as f64 / 100.0,
        file_path,
        analyze_path,
        ..Track::default()
    })
}

/// How a key's name is spelled: Camelot or Open Key (`8A`, `1m`), `min`/`maj` (`Amin`), or a note
/// name (`Am`, `Db`), as one collection can hold all three (from tags written by other tools).
fn spelling(name: &str) -> u8 {
    let name = name.trim().to_ascii_lowercase();
    match () {
        _ if name.starts_with(|c: char| c.is_ascii_digit()) => 0,
        _ if name.ends_with("min") || name.ends_with("maj") => 1,
        _ => 2,
    }
}

/// The `djmdKey` row for the key `name` (in any notation [`Camelot::parse`] reads) of track
/// `track`: of the rows spelling that key, the one spelled as the track's key is, else as most of
/// the collection's tracks' keys are, else the first. Refused when no row spells it.
fn key_id(e: &Edit<'_>, track: &str, name: &str) -> Result<String> {
    let sql_err = |e: rusqlite::Error| Error::OneLibrary(e.to_string());
    let Some(key) = Camelot::parse(name) else {
        return Err(Error::Invalid(format!("{name:?} isn't a key (8A, Am, A minor...)")));
    };
    // Each live row with how many live tracks have it, this track's first.
    let sql = "SELECT k.ID, IFNULL(k.ScaleName, ''), \
               (SELECT count(*) FROM djmdContent c WHERE c.KeyID = k.ID AND IFNULL(c.rb_local_deleted, 0) = 0), \
               EXISTS (SELECT 1 FROM djmdContent c WHERE c.ID = ?1 AND c.KeyID = k.ID) \
               FROM djmdKey k WHERE IFNULL(k.rb_local_deleted, 0) = 0 ORDER BY k.rowid";
    let mut stmt = e.conn().prepare(sql).map_err(sql_err)?;
    let rows: Vec<(String, String, i64, bool)> = stmt
        .query_map([track], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(sql_err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(sql_err)?;
    let current = rows.iter().find(|r| r.3).map(|r| spelling(&r.1));
    let mut used = [0i64; 3];
    for (_, name, tracks, _) in &rows {
        used[usize::from(spelling(name))] += tracks;
    }
    rows.iter()
        .filter(|(_, name, _, _)| Camelot::parse(name) == Some(key))
        .min_by_key(|(_, name, _, _)| {
            let s = spelling(name);
            (Some(s) != current, std::cmp::Reverse(used[usize::from(s)]))
        })
        .map(|(id, _, _, _)| id.clone())
        .ok_or_else(|| {
            Error::Invalid(format!(
                "the collection has no key {name:?} ({}, {}) in any spelling, so it isn't set",
                key.code(),
                key.note()
            ))
        })
}

/// The track's cues as the collection has them (`djmdCue`).
fn cues(e: &Edit<'_>, id: &str) -> Result<Vec<CuePoint>> {
    let sql = "SELECT InMsec, OutMsec, Kind, ColorTableIndex, IFNULL(Comment, ''), IFNULL(ActiveLoop, 0) \
               FROM djmdCue WHERE ContentID = ?1 AND IFNULL(rb_local_deleted, 0) = 0 ORDER BY rowid";
    let sql_err = |e: rusqlite::Error| Error::OneLibrary(e.to_string());
    let mut stmt = e.conn().prepare(sql).map_err(sql_err)?;
    let rows = stmt
        .query_map([id], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                r.get::<_, Option<i64>>(1)?.unwrap_or(-1),
                r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })
        .map_err(sql_err)?;
    let mut out = Vec::new();
    for row in rows {
        let (start, end, kind, color, comment, active) = row.map_err(sql_err)?;
        let hot_cue = hot_cue_from_kind(kind);
        let start = u32::try_from(start.max(0)).unwrap_or(u32::MAX);
        let code = hot_cue.and(color).and_then(|c| u8::try_from(c).ok()).filter(|c| (1..=62).contains(c));
        out.push(CuePoint {
            hot_cue,
            start_ms: start,
            end_ms: u32::try_from(end).ok().filter(|e| *e > start),
            comment,
            color_code: code.unwrap_or(0),
            color_rgb: match hot_cue {
                Some(_) => code.and_then(anlz::hot_cue_palette).unwrap_or(DEFAULT_HOT_CUE_RGB),
                None => [0; 3],
            },
            active_loop: active != 0,
            ..CuePoint::default()
        });
    }
    Ok(out)
}

/// A cue as the cue writer takes it.
fn entry(c: &CuePoint) -> CueEntry {
    CueEntry {
        hot_cue: c.hot_cue,
        start_ms: c.start_ms,
        end_ms: c.end_ms.filter(|e| *e > c.start_ms),
        comment: c.comment.clone(),
        color: c.hot_cue.and(Some(c.color_code)).filter(|c| *c != 0),
        active_loop: c.active_loop && c.end_ms.is_some(),
    }
}

#[cfg(test)]
mod tests;
