//! A track's cues in `master.db`, written as rekordbox 7.2.18 lays them out when a hot cue is set
//! (watched on a decrypted copy before and after, LOC-1150). rekordbox deleted and reinserted the
//! track's rows then, but the rows it leaves from earlier edits and analysis passes keep their IDs
//! and every column (on 92 of the 118 tracks of one library with cues added after import), so a
//! write here changes only the rows it has to:
//!
//! * `djmdCue`: a row that reads as one of the cues given (as [`super::super::open`] reads it)
//!   stays as it is. A cue given in a row's place (the same hot cue, or a memory cue at the same
//!   position) is written into that row: only the columns whose meaning changed, each in the row's
//!   own shape (an empty value stays NULL, or 0 or empty text, as the row has it). A moved position
//!   resets the columns that seek to it (the MPEG ones, its seek info, `CueMicrosec`), and a loop of
//!   another length its `BeatLoopSize`. The track's other rows are deleted outright (not marked
//!   `rb_local_deleted`), and each cue left gets a row as rekordbox's newest are: a new random `ID`
//!   (a 32-bit number, as text) and `UUID`, the track's `ContentID` and `ContentUUID`, `InMsec`,
//!   `InFrame` = `InMsec` × 150 / 1000 (rekordbox's frames are 1/150 s, truncated as in all 7,305
//!   rows of one library), `OutMsec` -1 and `OutFrame` 0 for a cue that isn't a loop, `Color` -1,
//!   `ColorTableIndex` the hot cue's palette code (NULL for none), `Kind` 0 for a memory cue, 4 for
//!   a memory loop, 1-3 for hot cues A-C and 5 on for D on, the MPEG seek columns 0, and
//!   `ActiveLoop`, `Comment`, `BeatLoopSize` and `CueMicrosec` NULL. rekordbox doesn't stamp these
//!   rows: `rb_local_usn` and `usn` stay NULL.
//! * `contentCue`: one row a track (`ID` the track's `UUID`, a `UUID` of its own), whose `Cues` is
//!   the track's whole cue list as JSON (each row's columns that aren't NULL or empty text, but the
//!   `rb_` ones and `usn`, in [`JSON_KEYS`]' order, the times as `2026-11-26T07:23:46.343+00:00`)
//!   and `rb_cue_count` how many. Created when missing; stamped. rekordbox shows the rows, and its
//!   own analysis can leave the list behind them, so the list is made again from the rows.
//! * `djmdContent`: `CueUpdated` goes up by one, and the row is stamped.
//!
//! rekordbox's count rose by 15 for five stamped rows when it set a hot cue; this stamps two (the
//! list, then the track). The other three are most likely the `djmdSampler` rows rekordbox
//! restamps each time it starts (inferred).
//!
//! Older scripts wrote `djmdCue` rows without the `contentCue` list, and tracks whose cues were set
//! before rekordbox 7 can have no `contentCue` row at all. [`Edit::repair_cue_lists`] makes the
//! lists agree with the rows, when asked.

use loclib_model::analysis::{Cue, HOT_CUE_PALETTE};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::Edit;
use crate::masterdb::{CUE_COLUMNS, cue};
use crate::onelibrary::kind_for_hot_cue;
use crate::{Error, Refusal, Result};

/// The `djmdCue` columns a `contentCue` list gives each cue, in its order (as rekordbox 7.2.14's
/// lists hold them, `ActiveLoop`, `BeatLoopSize` and `CueMicrosec` included).
const JSON_KEYS: &[&str] = &[
    "ID",
    "ContentID",
    "ContentUUID",
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
    "UUID",
    "created_at",
    "updated_at",
];

/// The highest hot cue a write gives a cue: P. A row a write leaves as it is keeps any `Kind`
/// (rekordbox's collections have been seen with up to I, `Kind` 10).
const LAST_HOT_CUE: u8 = 15;

/// A cue to write: what [`Edit::set_cues`] takes for each of a track's cues.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CueEntry {
    /// `None` for a memory cue or loop; `Some(0)` = hot cue A ... `Some(15)` = P.
    pub hot_cue: Option<u8>,
    /// Where it is (where a loop starts), in milliseconds.
    pub start_ms: u32,
    /// The loop's end, for a loop; after `start_ms`.
    pub end_ms: Option<u32>,
    /// Its name (`Comment`), empty for none.
    pub comment: String,
    /// A hot cue's colour as a code of rekordbox's hot cue palette (1-62, `ColorTableIndex`); `None`
    /// for none, and for a memory cue (whose colours aren't written yet).
    pub color: Option<u8>,
    /// A loop that's active (`ActiveLoop`).
    pub active_loop: bool,
}

impl From<&Cue> for CueEntry {
    /// A hot cue's colour becomes the palette code nearest it, as the analysis files' writer
    /// picks one; a memory cue's isn't written (the collection's reader doesn't read it back).
    fn from(c: &Cue) -> CueEntry {
        let code = |rgb: [u8; 3]| {
            let distance = |p: &[u8; 3]| (0..3).map(|i| (i32::from(p[i]) - i32::from(rgb[i])).pow(2)).sum::<i32>();
            let nearest = HOT_CUE_PALETTE.iter().enumerate().min_by_key(|(_, p)| distance(p));
            nearest.map(|(i, _)| i as u8 + 1)
        };
        CueEntry {
            hot_cue: c.hot_cue,
            start_ms: c.start_ms,
            end_ms: c.end_ms.filter(|e| *e > c.start_ms),
            comment: c.comment.clone(),
            color: c.hot_cue.and(c.color).and_then(code),
            active_loop: c.active_loop,
        }
    }
}

/// What a track's cues were before an edit replaced them: its `djmdCue` rows (every column, as
/// stored) and its `contentCue` row, when it had one. [`Edit::restore_cues`] puts the rows back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuesBefore {
    /// The track's `djmdContent.ID`.
    pub track_id: String,
    /// Its `djmdCue` rows, in the order stored, each as (column, value) pairs.
    pub rows: Vec<Vec<(String, serde_json::Value)>>,
    /// Its `contentCue` row, as (column, value) pairs, when it had one. Kept for the record:
    /// restoring rewrites the list from the rows, so the two agree.
    pub content_cue: Option<Vec<(String, serde_json::Value)>>,
}

/// One of a track's `djmdCue` rows as stored: its `rowid`, every column, and the cue it reads as.
struct Row {
    rowid: i64,
    columns: Vec<(String, Value)>,
    reads: CueEntry,
}

impl Row {
    fn get(&self, name: &str) -> Option<&Value> {
        self.columns.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }
}

/// How a cue given is written: it's a row already (`Same`), it goes into a row in its place
/// (`Into`), or it's a new row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placed {
    Same(usize),
    Into(usize),
    New,
}

impl Edit<'_> {
    /// Sets track `track_id`'s cue list to `cues`, changing only what differs (see the module
    /// docs), and answers what it replaced. Refused, with nothing changed, for a cue it would
    /// write that's a hot cue past P or given twice, a loop that ends before it starts, a colour
    /// that isn't a palette code, or a memory cue with a colour.
    pub fn set_cues(&mut self, track_id: &str, cues: &[CueEntry]) -> Result<CuesBefore> {
        let uuid = self.track_uuid(track_id)?;
        let rows = self.cue_rows(track_id)?;
        let placed = place(&rows, cues);
        for (c, _) in cues.iter().zip(&placed).filter(|(_, p)| !matches!(p, Placed::Same(_))) {
            if let Some(h) = c.hot_cue {
                if h > LAST_HOT_CUE {
                    return Err(Error::Invalid(format!("hot cue {h} is past P")));
                }
                if cues.iter().filter(|o| o.hot_cue == Some(h)).count() > 1 {
                    return Err(Error::Invalid(format!("hot cue {h} is given twice")));
                }
            }
            if c.end_ms.is_some_and(|e| e <= c.start_ms) {
                return Err(Error::Invalid(format!("a loop at {} ms ends before it starts", c.start_ms)));
            }
            match (c.hot_cue, c.color) {
                (Some(_), Some(code)) if !(1..=HOT_CUE_PALETTE.len() as u8).contains(&code) => {
                    return Err(Error::Invalid(format!("{code} isn't a hot cue palette code")));
                }
                (None, Some(_)) => {
                    let why = format!("the memory cue at {} ms has a colour, which isn't written yet", c.start_ms);
                    return Err(Error::Invalid(why));
                }
                _ => {}
            }
        }
        let before = self.cues_before(track_id)?;
        // New rows first, so none takes the id of a row deleted after.
        for (c, p) in cues.iter().zip(&placed) {
            match *p {
                Placed::Same(_) => {}
                Placed::Into(r) => self.update_cue(&rows[r], c)?,
                Placed::New => self.insert_cue(track_id, &uuid, c)?,
            }
        }
        let used: FxHashSet<usize> = placed
            .iter()
            .filter_map(|p| match *p {
                Placed::Same(r) | Placed::Into(r) => Some(r),
                Placed::New => None,
            })
            .collect();
        for row in rows.iter().enumerate().filter(|(i, _)| !used.contains(i)).map(|(_, r)| r) {
            self.conn.execute("DELETE FROM djmdCue WHERE rowid = ?1", [row.rowid]).map_err(|e| self.sql(e))?;
        }
        self.write_cue_list(track_id, &uuid)?;
        Ok(before)
    }

    /// Puts back the cue rows `before` holds (as [`Edit::set_cues`] or this answered them), as a
    /// new change: the track's `contentCue` list is rewritten from them and it and the track are
    /// stamped anew. A row keeps its `ID` unless another track's row has taken it since. Answers
    /// what it replaced in turn.
    pub fn restore_cues(&mut self, before: &CuesBefore) -> Result<CuesBefore> {
        let track_id = before.track_id.as_str();
        let uuid = self.track_uuid(track_id)?;
        let now = self.cues_before(track_id)?;
        self.conn.execute("DELETE FROM djmdCue WHERE ContentID = ?1", [track_id]).map_err(|e| self.sql(e))?;
        for stored in &before.rows {
            let mut row: Vec<(&str, Value)> =
                stored.iter().map(|(n, v)| Ok((n.as_str(), from_json(v)?))).collect::<Result<_>>()?;
            for (name, value) in &mut row {
                if name.eq_ignore_ascii_case("ContentID") {
                    *value = Value::Text(track_id.to_string());
                }
                if name.eq_ignore_ascii_case("ID") && self.cue_id_taken(value)? {
                    *value = Value::Text(self.new_cue_id()?);
                }
            }
            self.insert("djmdCue", &row)?;
        }
        self.write_cue_list(track_id, &uuid)?;
        Ok(now)
    }

    /// Makes each track's `contentCue` list the one its `djmdCue` rows make where it isn't (see
    /// [`cue_list_mismatches`]), through the same path as [`Edit::set_cues`]: the list rewritten,
    /// it and the track stamped. With `create_missing`, tracks with cues and no list get one too.
    /// Answers what each repair replaced; the rows themselves aren't changed.
    pub fn repair_cue_lists(&mut self, create_missing: bool) -> Result<Vec<CuesBefore>> {
        let tracks = mismatches(self.conn, create_missing, None)?;
        self.repair_cue_lists_of(&tracks.iter().map(String::as_str).collect::<Vec<_>>())
    }

    /// [`Edit::repair_cue_lists`] of the tracks `track_ids`, whether their lists disagree or not.
    pub fn repair_cue_lists_of(&mut self, track_ids: &[&str]) -> Result<Vec<CuesBefore>> {
        let mut out = Vec::with_capacity(track_ids.len());
        for track in track_ids {
            let uuid = self.track_uuid(track)?;
            out.push(self.cues_before(track)?);
            self.write_cue_list(track, &uuid)?;
        }
        Ok(out)
    }

    /// Track `track_id`'s cue list as its `djmdCue` rows make it: what a repair writes.
    pub fn cue_list_of(&self, track_id: &str) -> Result<String> {
        Ok(regenerated(self.conn, track_id)?.0)
    }

    /// Puts back the `contentCue` list `before` holds (as a repair answered it), as a new change:
    /// its `Cues` and `rb_cue_count` (or no list, where it had none), the list and the track
    /// stamped anew and `CueUpdated` raised. The rows aren't changed, so the list put back can
    /// disagree with them again, as it did. Answers what it replaced in turn.
    pub fn restore_cue_list(&mut self, before: &CuesBefore) -> Result<CuesBefore> {
        let track_id = before.track_id.as_str();
        self.track_uuid(track_id)?;
        let now = self.cues_before(track_id)?;
        let live = "ContentID = ?1 AND IFNULL(rb_local_deleted, 0) = 0";
        let stamp = self.stamp();
        match &before.content_cue {
            None => {
                let sql = format!("DELETE FROM contentCue WHERE {live}");
                self.conn.execute(&sql, [track_id]).map_err(|e| self.sql(e))?;
            }
            Some(list) => {
                let get =
                    |name: &str| list.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| from_json(v));
                let cues = get("Cues").transpose()?.unwrap_or(Value::Null);
                let count = get("rb_cue_count").transpose()?.unwrap_or(Value::Null);
                let sql = format!(
                    "UPDATE contentCue SET Cues = ?2, rb_cue_count = ?3, rb_local_usn = ?4, updated_at = ?5 WHERE {live}"
                );
                let params = rusqlite::params![track_id, cues, count, stamp, self.now];
                if self.conn.execute(&sql, params).map_err(|e| self.sql(e))? == 0 {
                    let mut row: Vec<(&str, Value)> =
                        list.iter().map(|(n, v)| Ok((n.as_str(), from_json(v)?))).collect::<Result<_>>()?;
                    for (name, value) in &mut row {
                        if name.eq_ignore_ascii_case("rb_local_usn") {
                            *value = stamp.into();
                        }
                    }
                    self.insert("contentCue", &row)?;
                }
            }
        }
        self.cue_updated(track_id)?;
        Ok(now)
    }

    /// The tracks whose `contentCue` list isn't the one their `djmdCue` rows make (see
    /// [`cue_list_mismatches`]).
    pub fn cue_list_mismatches(&self, create_missing: bool) -> Result<Vec<String>> {
        mismatches(self.conn, create_missing, None)
    }

    /// Track `track_id`'s `djmdCue` rows and `contentCue` row as they are, inside the write.
    pub fn cues_before(&self, track_id: &str) -> Result<CuesBefore> {
        cues_of(self.conn, track_id)
    }

    /// Track `track_id`'s `djmdCue` rows rekordbox hasn't deleted, in the order stored.
    fn cue_rows(&self, track_id: &str) -> Result<Vec<Row>> {
        let sql =
            "SELECT rowid, * FROM djmdCue WHERE ContentID = ?1 AND IFNULL(rb_local_deleted, 0) = 0 ORDER BY rowid";
        let mut stmt = self.conn.prepare(sql).map_err(|e| self.sql(e))?;
        let names: Vec<String> = stmt.column_names().skip(1).map(|n| n.to_string()).collect();
        let found = stmt.query_map([track_id], |r| {
            let columns: Vec<(String, Value)> = names
                .iter()
                .enumerate()
                .map(|(i, n)| Ok((n.clone(), r.get(i + 1)?)))
                .collect::<rusqlite::Result<_>>()?;
            Ok((r.get::<_, i64>(0)?, columns))
        });
        let mut rows = Vec::new();
        for row in found.map_err(|e| self.sql(e))? {
            let (rowid, columns) = row.map_err(|e| self.sql(e))?;
            let mut row = Row { rowid, columns, reads: CueEntry::default() };
            let read: Vec<Value> = CUE_COLUMNS.iter().map(|c| row.get(c).cloned().unwrap_or(Value::Null)).collect();
            row.reads = CueEntry::from(&cue(&read));
            rows.push(row);
        }
        Ok(rows)
    }

    /// Writes `to` into `row`, which is in its place: the columns whose meaning changed, and
    /// `updated_at`.
    fn update_cue(&self, row: &Row, to: &CueEntry) -> Result<()> {
        let mut set = changed(row, to);
        set.push(("updated_at", Value::Text(self.now.clone())));
        let names: Vec<String> = set.iter().map(|(n, _)| format!("\"{n}\" = ?")).collect();
        let sql = format!("UPDATE djmdCue SET {} WHERE rowid = ?", names.join(", "));
        let params = set.iter().map(|(_, v)| v.clone()).chain([Value::Integer(row.rowid)]);
        self.conn.execute(&sql, rusqlite::params_from_iter(params)).map_err(|e| self.sql(e))?;
        Ok(())
    }

    /// Adds `c` to track `track_id` (whose `UUID` is `uuid`) as a new row.
    fn insert_cue(&self, track_id: &str, uuid: &str, c: &CueEntry) -> Result<()> {
        let frame = |ms: u32| i64::from(ms) * 150 / 1000;
        let text = |s: &str| Value::Text(s.to_string());
        let row: Vec<(&str, Value)> = vec![
            ("ID", Value::Text(self.new_cue_id()?)),
            ("ContentID", text(track_id)),
            ("InMsec", c.start_ms.into()),
            ("InFrame", frame(c.start_ms).into()),
            ("InMpegFrame", 0.into()),
            ("InMpegAbs", 0.into()),
            ("OutMsec", c.end_ms.map_or(-1, i64::from).into()),
            ("OutFrame", c.end_ms.map_or(0, frame).into()),
            ("OutMpegFrame", 0.into()),
            ("OutMpegAbs", 0.into()),
            ("Kind", kind(c).into()),
            ("Color", (-1).into()),
            ("ColorTableIndex", c.color.map_or(Value::Null, |code| i64::from(code).into())),
            ("ActiveLoop", if c.active_loop { 1.into() } else { Value::Null }),
            ("Comment", if c.comment.is_empty() { Value::Null } else { text(&c.comment) }),
            ("ContentUUID", text(uuid)),
            ("UUID", Value::Text(new_uuid())),
            ("rb_data_status", 0.into()),
            ("rb_local_data_status", 0.into()),
            ("rb_local_deleted", 0.into()),
            ("rb_local_synced", 0.into()),
            ("created_at", text(&self.now)),
            ("updated_at", text(&self.now)),
        ];
        self.insert("djmdCue", &row)
    }

    /// Rewrites track `track_id`'s `contentCue` list from its `djmdCue` rows (creating it when
    /// missing), and bumps its `CueUpdated`, stamping both as rekordbox does: the list first. A
    /// list marked deleted isn't brought back.
    fn write_cue_list(&mut self, track_id: &str, uuid: &str) -> Result<()> {
        let (json, n) = regenerated(self.conn, track_id)?;
        let lists: Vec<(String, bool)> = self
            .conn
            .prepare("SELECT ID, IFNULL(rb_local_deleted, 0) != 0 FROM contentCue WHERE ContentID = ?1")
            .and_then(|mut s| s.query_map([track_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
            .map_err(|e| self.sql(e))?;
        let live: Vec<&String> = lists.iter().filter(|(_, deleted)| !deleted).map(|(id, _)| id).collect();
        let stamp = self.stamp();
        match (&live[..], lists.len()) {
            ([], 0) => {
                let row: Vec<(&str, Value)> = vec![
                    ("ID", Value::Text(uuid.to_string())),
                    ("ContentID", Value::Text(track_id.to_string())),
                    ("Cues", Value::Text(json)),
                    ("rb_cue_count", (n as i64).into()),
                    ("UUID", Value::Text(new_uuid())),
                    ("rb_data_status", 0.into()),
                    ("rb_local_data_status", 0.into()),
                    ("rb_local_deleted", 0.into()),
                    ("rb_local_synced", 0.into()),
                    ("rb_local_usn", stamp.into()),
                    ("created_at", Value::Text(self.now.clone())),
                    ("updated_at", Value::Text(self.now.clone())),
                ];
                self.insert("contentCue", &row)?;
            }
            ([id], _) => {
                let sql = "UPDATE contentCue SET Cues = ?1, rb_cue_count = ?2, rb_local_usn = ?3, updated_at = ?4 \
                           WHERE ID = ?5";
                let params = rusqlite::params![json, n as i64, stamp, self.now, id];
                self.conn.execute(sql, params).map_err(|e| self.sql(e))?;
            }
            ([], _) => {
                let why = format!("track {track_id}'s contentCue list is marked deleted (rb_local_deleted)");
                return Err(Error::Refused(Refusal::Odd(why)));
            }
            _ => {
                let why = format!("track {track_id} has {} contentCue rows, where rekordbox keeps one", live.len());
                return Err(Error::Refused(Refusal::Odd(why)));
            }
        }
        self.cue_updated(track_id)?;
        self.cue_tracks.insert(track_id.to_string());
        Ok(())
    }

    /// Raises track `track_id`'s `CueUpdated` and stamps it.
    fn cue_updated(&mut self, track_id: &str) -> Result<()> {
        let stamp = self.stamp();
        let sql = "UPDATE djmdContent SET CueUpdated = CAST(CAST(IFNULL(CueUpdated, 0) AS INTEGER) + 1 AS TEXT), \
                   rb_local_usn = ?1, updated_at = ?2 WHERE ID = ?3";
        self.conn.execute(sql, rusqlite::params![stamp, self.now, track_id]).map_err(|e| self.sql(e))?;
        self.touched.insert(track_id.to_string());
        Ok(())
    }

    /// Inserts `row` into `table`; a column the table lacks is refused (by SQLite).
    fn insert(&self, table: &str, row: &[(&str, Value)]) -> Result<()> {
        let names: Vec<String> = row.iter().map(|(n, _)| format!("\"{n}\"")).collect();
        let marks = vec!["?"; row.len()].join(", ");
        let sql = format!("INSERT INTO \"{table}\" ({}) VALUES ({marks})", names.join(", "));
        let params = rusqlite::params_from_iter(row.iter().map(|(_, v)| v));
        self.conn.execute(&sql, params).map_err(|e| self.sql(e))?;
        Ok(())
    }

    /// A random `djmdCue` id no row has: a 32-bit number, as rekordbox's are.
    fn new_cue_id(&self) -> Result<String> {
        let random = || {
            let mut bytes = [0u8; 4];
            getrandom::fill(&mut bytes).map_err(|e| Error::Invalid(e.to_string()))?;
            Ok(u32::from_le_bytes(bytes).max(1).to_string())
        };
        first_free(random, |id| self.cue_id_taken(&Value::Text(id.to_string())))
    }

    fn cue_id_taken(&self, id: &Value) -> Result<bool> {
        let sql = "SELECT 1 FROM djmdCue WHERE ID = ?1";
        Ok(self.conn.query_row(sql, [id], |_| Ok(())).optional().map_err(|e| self.sql(e))?.is_some())
    }
}

/// Where each of `cues` goes among `rows`: a row that reads as it, else a row in its place (the
/// same hot cue, or a memory cue at the same position), else a new one. Each row takes one cue.
fn place(rows: &[Row], cues: &[CueEntry]) -> Vec<Placed> {
    let mut used = vec![false; rows.len()];
    let mut take = |f: &dyn Fn(&Row) -> bool| {
        let r = (0..rows.len()).find(|r| !used[*r] && f(&rows[*r]))?;
        used[r] = true;
        Some(r)
    };
    let mut placed: Vec<Placed> =
        cues.iter().map(|c| take(&|r| r.reads == *c).map_or(Placed::New, Placed::Same)).collect();
    for (c, p) in cues.iter().zip(&mut placed) {
        let in_its_place =
            |r: &Row| r.reads.hot_cue == c.hot_cue && (c.hot_cue.is_some() || r.reads.start_ms == c.start_ms);
        if *p == Placed::New
            && let Some(r) = take(&in_its_place)
        {
            *p = Placed::Into(r);
        }
    }
    placed
}

/// The `Kind` a cue is written with.
fn kind(c: &CueEntry) -> i64 {
    match (c.hot_cue, c.end_ms) {
        (Some(h), _) => kind_for_hot_cue(h),
        (None, Some(_)) => 4,
        (None, None) => 0,
    }
}

/// The columns of `row` that change for it to read as `to`, in the row's shape.
fn changed(row: &Row, to: &CueEntry) -> Vec<(&'static str, Value)> {
    let was = &row.reads;
    // Nothing, as the row keeps it: NULL where it has NULL, else `zero`.
    let empty = |name: &str, zero: Value| match row.get(name) {
        None | Some(Value::Null) => Value::Null,
        Some(_) => zero,
    };
    let frame = |ms: u32| i64::from(ms) * 150 / 1000;
    let mut out = Vec::new();
    if kind(was) != kind(to) {
        out.push(("Kind", kind(to).into()));
    }
    if was.start_ms != to.start_ms {
        out.extend([
            ("InMsec", to.start_ms.into()),
            ("InFrame", frame(to.start_ms).into()),
            ("InMpegFrame", empty("InMpegFrame", 0.into())),
            ("InMpegAbs", empty("InMpegAbs", 0.into())),
            ("InPointSeekInfo", Value::Null),
            ("CueMicrosec", empty("CueMicrosec", 0.into())),
        ]);
    }
    if was.end_ms != to.end_ms {
        out.extend([
            ("OutMsec", to.end_ms.map_or(-1, i64::from).into()),
            ("OutFrame", to.end_ms.map_or(0, frame).into()),
            ("OutMpegFrame", empty("OutMpegFrame", 0.into())),
            ("OutMpegAbs", empty("OutMpegAbs", 0.into())),
            ("OutPointSeekInfo", Value::Null),
        ]);
    }
    let length = |c: &CueEntry| c.end_ms.map(|e| e - c.start_ms);
    if length(was) != length(to) {
        out.push(("BeatLoopSize", empty("BeatLoopSize", 0.into())));
    }
    if was.comment != to.comment {
        let comment = if to.comment.is_empty() {
            empty("Comment", Value::Text(String::new()))
        } else {
            to.comment.clone().into()
        };
        out.push(("Comment", comment));
    }
    if was.color != to.color {
        out.push(("ColorTableIndex", to.color.map_or_else(|| empty("ColorTableIndex", 0.into()), |c| c.into())));
    }
    if was.active_loop != to.active_loop {
        out.push(("ActiveLoop", if to.active_loop { 1.into() } else { empty("ActiveLoop", 0.into()) }));
    }
    out
}

/// The first id `next` draws that `taken` says no row has.
fn first_free(mut next: impl FnMut() -> Result<String>, taken: impl Fn(&str) -> Result<bool>) -> Result<String> {
    loop {
        let id = next()?;
        if !taken(&id)? {
            return Ok(id);
        }
    }
}

/// Track `track_id`'s cue list as its `djmdCue` rows make it (rows rekordbox deleted left out, in
/// the order stored), and how many cues it holds.
fn regenerated(conn: &Connection, track_id: &str) -> Result<(String, usize)> {
    let columns = columns(conn, "djmdCue")?;
    let keys: Vec<&str> = JSON_KEYS.iter().copied().filter(|k| columns.contains(&k.to_lowercase())).collect();
    let deleted = if columns.contains("rb_local_deleted") { " AND IFNULL(rb_local_deleted, 0) = 0" } else { "" };
    let list: Vec<String> = keys.iter().map(|k| format!("\"{k}\"")).collect();
    let sql = format!("SELECT {} FROM djmdCue WHERE ContentID = ?1{deleted} ORDER BY rowid", list.join(", "));
    let rows: Vec<Vec<Value>> = conn
        .prepare(&sql)?
        .query_map([track_id], |r| (0..keys.len()).map(|i| r.get(i)).collect())?
        .collect::<rusqlite::Result<_>>()?;
    Ok((cue_list_json(&keys, &rows), rows.len()))
}

/// A track's cue list as `contentCue.Cues` holds it: for each row (its `keys` columns' values),
/// the values that aren't NULL or empty, the times as `2026-11-26T07:23:46.343+00:00`.
fn cue_list_json(keys: &[&str], rows: &[Vec<Value>]) -> String {
    let mut out = String::from("[");
    for (i, row) in rows.iter().enumerate() {
        out.push_str(if i == 0 { "{" } else { ",{" });
        let mut first = true;
        for (key, value) in keys.iter().zip(row) {
            let json = match value {
                // rekordbox leaves an empty comment out as it does a NULL one.
                Value::Null | Value::Blob(_) => continue,
                Value::Text(t) if t.is_empty() => continue,
                Value::Integer(n) => n.to_string(),
                Value::Real(f) => serde_json::Number::from_f64(*f).map_or_else(|| "null".into(), |n| n.to_string()),
                Value::Text(t) if key.ends_with("_at") => json_string(&iso_time(t)),
                Value::Text(t) => json_string(t),
            };
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str(&json_string(key));
            out.push(':');
            out.push_str(&json);
        }
        out.push('}');
    }
    out.push(']');
    out
}

fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// A time as rekordbox stores it in a column (`2026-11-26 07:23:46.343 +00:00`), as its cue lists
/// write it (`2026-11-26T07:23:46.343+00:00`).
fn iso_time(t: &str) -> String {
    match chrono::DateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S%.f %:z") {
        Ok(d) => d.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
        Err(_) => t.to_string(),
    }
}

/// A random (version 4) UUID, as rekordbox gives its rows.
fn new_uuid() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("the system's random numbers");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// A table's columns, lower-cased; empty when there's no such table.
pub(super) fn columns(conn: &Connection, table: &str) -> rusqlite::Result<FxHashSet<String>> {
    let mut stmt = conn.prepare("SELECT lower(name) FROM pragma_table_info(?1)")?;
    stmt.query_map([table], |r| r.get(0))?.collect()
}

/// Every column of `table`'s rows whose `column` is `value`, in the order stored.
fn all_columns(
    conn: &Connection,
    table: &str,
    column: &str,
    value: &str,
) -> Result<Vec<Vec<(String, serde_json::Value)>>> {
    let sql = |e: rusqlite::Error| Error::OneLibrary(e.to_string());
    let mut stmt =
        conn.prepare(&format!("SELECT * FROM \"{table}\" WHERE \"{column}\" = ?1 ORDER BY rowid")).map_err(sql)?;
    let names: Vec<String> = stmt.column_names().map(str::to_string).collect();
    let rows: Vec<Vec<Value>> = stmt
        .query_map([value], |r| (0..names.len()).map(|i| r.get(i)).collect())
        .map_err(sql)?
        .collect::<rusqlite::Result<_>>()
        .map_err(sql)?;
    rows.into_iter()
        .map(|row| names.iter().cloned().zip(row.iter()).map(|(n, v)| Ok((n, to_json(v)?))).collect())
        .collect()
}

/// A stored value as JSON. A blob (which no column written here holds) is refused.
pub(super) fn to_json(v: &Value) -> Result<serde_json::Value> {
    Ok(match v {
        Value::Null => serde_json::Value::Null,
        Value::Integer(n) => (*n).into(),
        Value::Real(f) => serde_json::Number::from_f64(*f).map_or(serde_json::Value::Null, Into::into),
        Value::Text(t) => t.clone().into(),
        Value::Blob(_) => return Err(Error::Invalid("a column holds a blob, which isn't kept to put back".into())),
    })
}

/// [`to_json`]'s value stored again.
pub(super) fn from_json(v: &serde_json::Value) -> Result<Value> {
    Ok(match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::String(s) => Value::Text(s.clone()),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Real(n.as_f64().unwrap_or(0.0)),
        },
        other => return Err(Error::Invalid(format!("{other} isn't a value a column held"))),
    })
}

/// A `contentCue` row's `Cues` and `rb_cue_count`.
type Listed = (Option<String>, Option<i64>);

/// The tracks (not deleted ones; only those in `only`, when given) whose `contentCue` list isn't
/// the one their `djmdCue` rows make ([`regenerated`]): its text or its count differ, or there's
/// more than one. Lists marked deleted don't count. With `create_missing`, a track with cues and
/// no list too. None when the table isn't there.
pub(super) fn mismatches(
    conn: &Connection,
    create_missing: bool,
    only: Option<&FxHashSet<String>>,
) -> Result<Vec<String>> {
    if columns(conn, "contentCue")?.is_empty() {
        return Ok(Vec::new());
    }
    let wanted = |id: &String| only.is_none_or(|o| o.contains(id));
    let live = |table: &str| -> Result<&'static str> {
        Ok(match columns(conn, table)?.contains("rb_local_deleted") {
            true => " WHERE IFNULL(rb_local_deleted, 0) = 0",
            false => "",
        })
    };
    let mut lists: FxHashMap<String, Vec<Listed>> = FxHashMap::default();
    let mut stmt =
        conn.prepare(&format!("SELECT ContentID, Cues, rb_cue_count FROM contentCue{}", live("contentCue")?))?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?, r.get(1)?, r.get(2)?)))? {
        let (track, cues, count) = row?;
        lists.entry(track.unwrap_or_default()).or_default().push((cues, count));
    }
    let mut stmt = conn.prepare(&format!("SELECT ID FROM djmdContent{} ORDER BY rowid", live("djmdContent")?))?;
    let tracks: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::new();
    for track in tracks.into_iter().filter(wanted) {
        let (json, n) = regenerated(conn, &track)?;
        let disagrees = match lists.get(&track).map(Vec::as_slice) {
            None | Some([]) => create_missing && n > 0,
            Some([(cues, count)]) => cues.as_deref() != Some(json.as_str()) || *count != Some(n as i64),
            Some(_) => true,
        };
        if disagrees {
            out.push(track);
        }
    }
    Ok(out)
}

/// Track `track_id`'s `djmdCue` rows and `contentCue` row (the last, should there be more), every
/// column as stored, in the database `conn` has open.
fn cues_of(conn: &Connection, track_id: &str) -> Result<CuesBefore> {
    let rows = all_columns(conn, "djmdCue", "ContentID", track_id)?;
    let mut lists = all_columns(conn, "contentCue", "ContentID", track_id)?;
    Ok(CuesBefore { track_id: track_id.to_string(), rows, content_cue: lists.pop() })
}

/// Tracks `track_ids`' cues in the `master.db` at `path` (read as [`super::super::open`] reads it),
/// as [`Edit::cues_before`] answers them, writing nothing.
pub fn cue_rows_at(path: &Path, key: &str, track_ids: &[&str]) -> Result<Vec<CuesBefore>> {
    let dir = tempfile::tempdir().map_err(|e| Error::Io { path: std::env::temp_dir(), source: e })?;
    let (db, _) = super::super::open_copy(path, key, dir.path(), false)?;
    track_ids.iter().map(|t| cues_of(&db.conn, t)).collect()
}

/// The tracks of the `master.db` at `path` (read as [`super::super::open`] reads it) whose
/// `contentCue` list isn't the one their `djmdCue` rows make, as [`Edit::repair_cue_lists`] would
/// repair them.
pub fn cue_list_mismatches(path: &Path, key: &str, create_missing: bool) -> Result<Vec<String>> {
    let dir = tempfile::tempdir().map_err(|e| Error::Io { path: std::env::temp_dir(), source: e })?;
    let (db, _) = super::super::open_copy(path, key, dir.path(), false)?;
    mismatches(&db.conn, create_missing, None)
}

#[cfg(test)]
mod tests;
