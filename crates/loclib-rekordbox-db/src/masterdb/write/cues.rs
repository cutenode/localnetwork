//! A track's cues in `master.db`, written as rekordbox 7.2.18 writes them when a hot cue is set
//! (watched on a decrypted copy before and after, LOC-1150):
//!
//! * `djmdCue`: the rows of the cues edited are deleted outright (not marked `rb_local_deleted`),
//!   and a row is inserted for each new cue with a new random `ID` (a 32-bit number, as text) and `UUID`, the
//!   track's `ContentID` and `ContentUUID`, `InMsec`, `InFrame` = `InMsec` × 150 / 1000 (rekordbox's
//!   frames are 1/150 s), `OutMsec` -1 and `OutFrame` 0 for a cue that isn't a loop, `Color` -1,
//!   `ColorTableIndex` the hot cue's palette code (NULL for none), `Kind` 0 for a memory cue, 4 for
//!   a memory loop, 1-3 for hot cues A-C and 5 on for D on, and the MPEG seek columns 0. rekordbox
//!   doesn't stamp these rows: `rb_local_usn` and `usn` stay NULL.
//! * `contentCue`: one row a track (`ID` the track's `UUID`, a `UUID` of its own), whose `Cues` is
//!   the track's whole cue list as JSON (each row's columns that aren't NULL or empty text, but the
//!   `rb_` ones and
//!   `usn`, in [`JSON_KEYS`]' order, the times as `2026-11-26T07:23:46.343+00:00`) and
//!   `rb_cue_count` how many. Created when missing; stamped.
//! * `djmdContent`: `CueUpdated` goes up by one, and the row is stamped.
//!
//! Older scripts wrote `djmdCue` rows without the `contentCue` list (193 tracks in one library), and
//! tracks whose cues were set before rekordbox 7 have no `contentCue` row at all.
//! [`Edit::repair_cue_lists`] makes the lists agree with the rows.

use loclib_model::analysis::{Cue, HOT_CUE_PALETTE};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::Edit;
use crate::onelibrary::{hot_cue_from_kind, kind_for_hot_cue};
use crate::{Error, Refusal, Result};

/// The `djmdCue` columns a `contentCue` list gives each cue, in its order.
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

/// The highest hot cue rekordbox's collection has been seen with: H.
const LAST_HOT_CUE: u8 = 7;

/// A cue to write: what [`Edit::set_cues`] takes for each of a track's cues.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CueEntry {
    /// `None` for a memory cue or loop; `Some(0)` = hot cue A ... `Some(7)` = H.
    pub hot_cue: Option<u8>,
    /// Where it is (where a loop starts), in milliseconds.
    pub start_ms: u32,
    /// The loop's end, for a loop; after `start_ms`.
    pub end_ms: Option<u32>,
    /// Its name (`Comment`), empty for none.
    pub comment: String,
    /// Its colour as a code of rekordbox's hot cue palette (1-62, `ColorTableIndex`); `None` for
    /// none.
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

impl Edit<'_> {
    /// Sets track `track_id`'s whole cue list to `cues`, as rekordbox does (see the module docs),
    /// answering what it replaced. A cue the track has already, exactly so (as [`Edit::cues`] reads
    /// it), keeps its row and all it holds. Refused, with nothing changed, for a hot cue past H or given
    /// twice, a loop that ends before it starts, or a colour that isn't a palette code.
    pub fn set_cues(&mut self, track_id: &str, cues: &[CueEntry]) -> Result<CuesBefore> {
        let uuid = self.track_uuid(track_id)?;
        let mut hot = FxHashSet::default();
        for c in cues {
            if let Some(h) = c.hot_cue
                && (h > LAST_HOT_CUE || !hot.insert(h))
            {
                return Err(Error::Invalid(format!("hot cue {h} is past H or given twice")));
            }
            if c.end_ms.is_some_and(|e| e <= c.start_ms) {
                return Err(Error::Invalid(format!("a loop at {} ms ends before it starts", c.start_ms)));
            }
            if c.color.is_some_and(|code| !(1..=HOT_CUE_PALETTE.len() as u8).contains(&code)) {
                return Err(Error::Invalid(format!("{:?} isn't a hot cue palette code", c.color)));
            }
        }
        let before = self.cues_before(track_id)?;
        // A cue the track has already, as it is, keeps its row: its ID and UUID, and what isn't
        // modelled here (a memory cue's colour, a loop's stored length). rekordbox replaces only the
        // cue edited too.
        let mut rows = self.cue_rows(track_id)?;
        let mut keep = Vec::new();
        let mut new = Vec::new();
        for c in cues {
            match rows.iter().position(|(_, have)| have == c) {
                Some(i) => keep.push(rows.swap_remove(i).0),
                None => new.push(c),
            }
        }
        for (id, _) in &rows {
            self.conn
                .execute("DELETE FROM djmdCue WHERE ContentID = ?1 AND ID IS ?2", rusqlite::params![track_id, id])
                .map_err(|e| self.sql(e))?;
        }
        if columns(self.conn, "djmdCue").map_err(|e| self.sql(e))?.contains("rb_local_deleted") {
            let sql = "DELETE FROM djmdCue WHERE ContentID = ?1 AND IFNULL(rb_local_deleted, 0) != 0";
            self.conn.execute(sql, [track_id]).map_err(|e| self.sql(e))?;
        }
        for c in new {
            let frame = |ms: u32| i64::from(ms) * 150 / 1000;
            let kind = match (c.hot_cue, c.end_ms) {
                (Some(h), _) => kind_for_hot_cue(h),
                (None, Some(_)) => 4,
                (None, None) => 0,
            };
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
                ("Kind", kind.into()),
                ("Color", (-1).into()),
                ("ColorTableIndex", c.color.map_or(Value::Null, |code| i64::from(code).into())),
                ("ActiveLoop", if c.active_loop { 1.into() } else { Value::Null }),
                ("Comment", if c.comment.is_empty() { Value::Null } else { text(&c.comment) }),
                ("ContentUUID", text(&uuid)),
                ("UUID", Value::Text(new_uuid())),
                ("rb_data_status", 0.into()),
                ("rb_local_data_status", 0.into()),
                ("rb_local_deleted", 0.into()),
                ("rb_local_synced", 0.into()),
                ("created_at", text(&self.now)),
                ("updated_at", text(&self.now)),
            ];
            self.insert("djmdCue", &row)?;
        }
        self.write_cue_list(track_id, &uuid)?;
        Ok(before)
    }

    /// Track `track_id`'s cues as they are (rows marked deleted left out), in the order stored.
    pub fn cues(&self, track_id: &str) -> Result<Vec<CueEntry>> {
        Ok(self.cue_rows(track_id)?.into_iter().map(|(_, c)| c).collect())
    }

    /// Track `track_id`'s `djmdCue` rows that aren't marked deleted: each one's `ID` and cue.
    fn cue_rows(&self, track_id: &str) -> Result<Vec<(Option<String>, CueEntry)>> {
        let deleted = match columns(self.conn, "djmdCue").map_err(|e| self.sql(e))?.contains("rb_local_deleted") {
            true => " AND IFNULL(rb_local_deleted, 0) = 0",
            false => "",
        };
        let sql = format!(
            "SELECT ID, Kind, InMsec, OutMsec, ColorTableIndex, Comment, ActiveLoop FROM djmdCue \
             WHERE ContentID = ?1{deleted} ORDER BY rowid"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| self.sql(e))?;
        let rows = stmt
            .query_map([track_id], |r| {
                let v = |i| r.get::<_, Value>(i);
                Ok((v(0)?, v(1)?, v(2)?, v(3)?, v(4)?, v(5)?, v(6)?))
            })
            .map_err(|e| self.sql(e))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| self.sql(e))?;
        let int = super::super::int;
        Ok(rows
            .into_iter()
            .map(|(id, kind, start, end, color, comment, active)| {
                let hot_cue = hot_cue_from_kind(int(&kind));
                let start = int(&start).clamp(0, i64::from(u32::MAX));
                let end = int(&end);
                let code = int(&color);
                let id = (id != Value::Null).then(|| super::super::text(&id));
                let entry = CueEntry {
                    hot_cue,
                    start_ms: start as u32,
                    end_ms: (end > start).then(|| end.min(i64::from(u32::MAX)) as u32),
                    comment: super::super::text(&comment),
                    color: hot_cue
                        .and_then(|_| u8::try_from(code).ok())
                        .filter(|c| (1..=HOT_CUE_PALETTE.len() as u8).contains(c)),
                    active_loop: int(&active) == 1,
                };
                (id, entry)
            })
            .collect())
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

    /// Makes each track's `contentCue` list agree with its `djmdCue` rows where they don't (see
    /// [`cue_list_mismatches`]), through the same path as [`Edit::set_cues`]: the list rewritten, it and
    /// the track stamped. With `create_missing`, tracks with cues and no list get one too. Answers
    /// what each repair replaced; the rows themselves aren't changed.
    pub fn repair_cue_lists(&mut self, create_missing: bool) -> Result<Vec<CuesBefore>> {
        let tracks = mismatches(self.conn, create_missing, None)?;
        let mut out = Vec::with_capacity(tracks.len());
        for track in tracks {
            let uuid = self.track_uuid(&track)?;
            out.push(self.cues_before(&track)?);
            self.write_cue_list(&track, &uuid)?;
        }
        Ok(out)
    }

    /// The tracks whose `contentCue` list disagrees with their `djmdCue` rows (see [`cue_list_mismatches`]).
    pub fn cue_list_mismatches(&self, create_missing: bool) -> Result<Vec<String>> {
        mismatches(self.conn, create_missing, None)
    }

    /// Track `track_id`'s `djmdCue` rows and `contentCue` row as they are.
    fn cues_before(&self, track_id: &str) -> Result<CuesBefore> {
        let rows = all_columns(self.conn, "djmdCue", "ContentID", track_id)?;
        let mut lists = if has_table(self.conn, "contentCue")? {
            all_columns(self.conn, "contentCue", "ContentID", track_id)?
        } else {
            Vec::new()
        };
        Ok(CuesBefore { track_id: track_id.to_string(), rows, content_cue: lists.pop() })
    }

    /// Rewrites track `track_id`'s `contentCue` list from its `djmdCue` rows (creating it when
    /// missing), and bumps its `CueUpdated`, stamping both as rekordbox does: the list first.
    fn write_cue_list(&mut self, track_id: &str, uuid: &str) -> Result<()> {
        let columns = columns(self.conn, "djmdCue").map_err(|e| self.sql(e))?;
        let keys: Vec<&str> = JSON_KEYS.iter().copied().filter(|k| columns.contains(&k.to_lowercase())).collect();
        let deleted = if columns.contains("rb_local_deleted") { " AND IFNULL(rb_local_deleted, 0) = 0" } else { "" };
        let list: Vec<String> = keys.iter().map(|k| format!("\"{k}\"")).collect();
        let sql = format!("SELECT {} FROM djmdCue WHERE ContentID = ?1{deleted} ORDER BY rowid", list.join(", "));
        let mut stmt = self.conn.prepare(&sql).map_err(|e| self.sql(e))?;
        let rows: Vec<Vec<Value>> = stmt
            .query_map([track_id], |r| (0..keys.len()).map(|i| r.get(i)).collect())
            .map_err(|e| self.sql(e))?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| self.sql(e))?;
        let json = cue_list_json(&keys, &rows);
        if has_table(self.conn, "contentCue")? {
            let ids: Vec<String> = self
                .conn
                .prepare("SELECT ID FROM contentCue WHERE ContentID = ?1")
                .and_then(|mut s| s.query_map([track_id], |r| r.get(0))?.collect())
                .map_err(|e| self.sql(e))?;
            let stamp = self.stamp();
            let n = rows.len() as i64;
            match &ids[..] {
                [] => {
                    let row: Vec<(&str, Value)> = vec![
                        ("ID", Value::Text(uuid.to_string())),
                        ("ContentID", Value::Text(track_id.to_string())),
                        ("Cues", Value::Text(json)),
                        ("rb_cue_count", n.into()),
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
                [id] => {
                    let sql = "UPDATE contentCue SET Cues = ?1, rb_cue_count = ?2, rb_local_deleted = 0, \
                               rb_local_usn = ?3, updated_at = ?4 WHERE ID = ?5";
                    let params = rusqlite::params![json, n, stamp, self.now, id];
                    self.conn.execute(sql, params).map_err(|e| self.sql(e))?;
                }
                _ => {
                    let why = format!("track {track_id} has {} contentCue rows, where rekordbox keeps one", ids.len());
                    return Err(Error::Refused(Refusal::Odd(why)));
                }
            }
        }
        let stamp = self.stamp();
        let cue_updated = match columns_of(self.conn, "djmdContent")?.contains("cueupdated") {
            true => "CueUpdated = CAST(CAST(IFNULL(CueUpdated, 0) AS INTEGER) + 1 AS TEXT), ",
            false => "",
        };
        let sql = format!("UPDATE djmdContent SET {cue_updated}rb_local_usn = ?1, updated_at = ?2 WHERE ID = ?3");
        self.conn.execute(&sql, rusqlite::params![stamp, self.now, track_id]).map_err(|e| self.sql(e))?;
        self.touched.insert(track_id.to_string());
        self.cue_tracks.insert(track_id.to_string());
        Ok(())
    }

    /// Inserts `row` into `table`, leaving out the columns this version of the table lacks.
    fn insert(&self, table: &str, row: &[(&str, Value)]) -> Result<()> {
        let have = columns(self.conn, table).map_err(|e| self.sql(e))?;
        let row: Vec<&(&str, Value)> = row.iter().filter(|(n, _)| have.contains(&n.to_lowercase())).collect();
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

/// The first id `next` draws that `taken` says no row has.
fn first_free(mut next: impl FnMut() -> Result<String>, taken: impl Fn(&str) -> Result<bool>) -> Result<String> {
    loop {
        let id = next()?;
        if !taken(&id)? {
            return Ok(id);
        }
    }
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

fn columns_of(conn: &Connection, table: &str) -> Result<FxHashSet<String>> {
    columns(conn, table).map_err(|e| Error::OneLibrary(e.to_string()))
}

fn has_table(conn: &Connection, table: &str) -> Result<bool> {
    Ok(!columns_of(conn, table)?.is_empty())
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

/// What tells two cue lists apart: each cue's id, position, loop end and kind.
type CueKey = (String, i64, i64, i64);

/// The tracks (not deleted ones; only those in `only`, when given) whose `contentCue` list
/// disagrees with their `djmdCue` rows: its count or its cues (by id, position, loop end and kind)
/// differ, it can't be read, or there's more than one. With `create_missing`, a track with cues and
/// no list too. None when the table isn't there (rekordbox 6).
pub(super) fn mismatches(
    conn: &Connection,
    create_missing: bool,
    only: Option<&FxHashSet<String>>,
) -> Result<Vec<String>> {
    let sql = |e: rusqlite::Error| Error::OneLibrary(e.to_string());
    if !has_table(conn, "contentCue")? {
        return Ok(Vec::new());
    }
    let wanted = |id: &String| only.is_none_or(|o| o.contains(id));
    let deleted = |table: &str| -> Result<&'static str> {
        Ok(match columns_of(conn, table)?.contains("rb_local_deleted") {
            true => " WHERE IFNULL(rb_local_deleted, 0) = 0",
            false => "",
        })
    };
    let mut rows: FxHashMap<String, Vec<CueKey>> = FxHashMap::default();
    let cue_sql = format!("SELECT ContentID, ID, InMsec, OutMsec, Kind FROM djmdCue{}", deleted("djmdCue")?);
    let mut stmt = conn.prepare(&cue_sql).map_err(sql)?;
    let found = stmt.query_map([], |r| {
        let text = |i| r.get::<_, Value>(i).map(|v| super::super::text(&v));
        let int = |i| r.get::<_, Value>(i).map(|v| super::super::int(&v));
        Ok((text(0)?, (text(1)?, int(2)?, int(3)?, int(4)?)))
    });
    for row in found.map_err(sql)? {
        let (track, key) = row.map_err(sql)?;
        rows.entry(track).or_default().push(key);
    }
    let mut lists: FxHashMap<String, Vec<Listed>> = FxHashMap::default();
    let mut stmt = conn.prepare("SELECT ContentID, Cues, rb_cue_count FROM contentCue").map_err(sql)?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?, r.get(1)?, r.get(2)?))).map_err(sql)? {
        let (track, cues, count) = row.map_err(sql)?;
        lists.entry(track.unwrap_or_default()).or_default().push((cues, count));
    }
    let tracks_sql = format!("SELECT ID FROM djmdContent{} ORDER BY rowid", deleted("djmdContent")?);
    let mut stmt = conn.prepare(&tracks_sql).map_err(sql)?;
    let tracks: Vec<String> =
        stmt.query_map([], |r| r.get(0)).map_err(sql)?.collect::<rusqlite::Result<_>>().map_err(sql)?;
    let mut out = Vec::new();
    for track in tracks.into_iter().filter(wanted) {
        let mut have = rows.remove(&track).unwrap_or_default();
        have.sort();
        let disagrees = match lists.get(&track).map(Vec::as_slice) {
            None | Some([]) => create_missing && !have.is_empty(),
            Some([(cues, count)]) => {
                let listed = cues.as_deref().and_then(listed_cues);
                *count != Some(have.len() as i64)
                    || listed.is_none_or(|mut l| {
                        l.sort();
                        l != have
                    })
            }
            Some(_) => true,
        };
        if disagrees {
            out.push(track);
        }
    }
    Ok(out)
}

/// The cues a `contentCue.Cues` list holds; `None` when it can't be read.
fn listed_cues(json: &str) -> Option<Vec<CueKey>> {
    let list: Vec<serde_json::Map<String, serde_json::Value>> = serde_json::from_str(json).ok()?;
    let int = |c: &serde_json::Map<String, serde_json::Value>, k: &str| match c.get(k) {
        Some(serde_json::Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        _ => Some(0),
    };
    list.iter()
        .map(|c| {
            let id = match c.get("ID")? {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            Some((id, int(c, "InMsec")?, int(c, "OutMsec")?, int(c, "Kind")?))
        })
        .collect()
}

/// The tracks of the `master.db` at `path` (read as [`super::super::open`] reads it) whose
/// `contentCue` list disagrees with their `djmdCue` rows, as [`Edit::repair_cue_lists`] would
/// repair them.
pub fn cue_list_mismatches(path: &Path, key: &str, create_missing: bool) -> Result<Vec<String>> {
    let dir = tempfile::tempdir().map_err(|e| Error::Io { path: std::env::temp_dir(), source: e })?;
    let (db, _) = super::super::open_copy(path, key, dir.path(), false)?;
    mismatches(&db.conn, create_missing, None)
}

#[cfg(test)]
mod tests;
