//! A track's rows in OneLibrary's `cue` table, which players that read only OneLibrary use
//! instead of the analysis files' cue lists.
//!
//! What the columns hold, as worked out from rekordbox 7's schema, the rows rekordbox writes,
//! pyrekordbox/rbox's notes and the analysis files' own fields (not yet checked on a player):
//!
//! * `kind`: 0 for a memory cue, otherwise the hot cue's number as rekordbox's own `djmdCue.Kind`
//!   numbers them (1-3 = A-C, 5-9 = D-H; 4 is a memory loop there). Assumed, since no rekordbox
//!   export checked so far had rows in this table.
//! * `inUsec` / `outUsec`: the position and the loop's end in microseconds; `outUsec` is -1 for a
//!   cue that isn't a loop. `in150FramePerSec` / `out150FramePerSec`: the same in rekordbox's
//!   1/150 s frames (-1 for no end).
//! * `inMpegFrameNumber`, `inMpegAbs`, `inDecodingStartFramePosition`, `inFileOffsetInBlock`,
//!   `inNumberOfSampleInBlock` and their `out` twins: where decoding starts to land exactly on the
//!   position in a VBR/ABR file, 0 when not needed. They can't be worked out without decoding the
//!   audio, so a position that moves gets 0s (the time is still right).
//! * `colorTableIndex`: the colour the analysis files give the cue: a row of the colour table
//!   (1 pink ... 8 purple) for a memory cue, a palette code (1-62) for a hot cue; 0 for none.
//! * `cueComment`: the name. `isActiveLoop`: 1 for a loop that starts active.
//! * `beatLoopNumerator` / `beatLoopDenominator`: a quantised loop's length in beats.
//!
//! Rows keep their `cue_id` while they survive an edit, since hot cue bank lists
//! (`hotCueBankList_cue`) point at them, and a column keeps its stored value unless what it means
//! changed.

use rusqlite::params;
use rusqlite::types::Value;
use schemars::JsonSchema;
use serde::Serialize;

use super::{OneLibrary, ident, int, text};
use crate::{Error, Result, masterdb};

/// Seek columns for a cue's position and its loop end, reset when the position moves.
const IN_SEEK: [&str; 5] = [
    "inMpegFrameNumber",
    "inMpegAbs",
    "inDecodingStartFramePosition",
    "inFileOffsetInBlock",
    "inNumberOfSampleInBlock",
];
const OUT_SEEK: [&str; 5] = [
    "outMpegFrameNumber",
    "outMpegAbs",
    "outDecodingStartFramePosition",
    "OutFileOffsetInBlock",
    "outNumberOfSampleInBlock",
];

/// One row of the `cue` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CueRow {
    /// `None` for a row to add.
    pub cue_id: Option<i64>,
    /// `None` for memory cues; `Some(0)` = hot cue A, `Some(1)` = B, ...
    pub hot_cue: Option<u8>,
    pub in_usec: i64,
    /// End of the loop, for loops.
    pub out_usec: Option<i64>,
    pub comment: String,
    /// `colorTableIndex` (see the module docs); 0 for none.
    pub color: u8,
    pub active_loop: bool,
    /// A quantised loop's length in beats, as numerator and denominator.
    pub loop_beats: Option<(u16, u16)>,
    /// Every column as stored (but `cue_id` and `content_id`); empty for a new row. Columns are
    /// written back as they were unless the fields above say otherwise.
    pub stored: Vec<(String, Value)>,
}

/// What [`OneLibrary::replace_cues`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct CueRowChanges {
    pub updated: usize,
    pub added: usize,
    pub removed: usize,
    /// Hot cue bank list entries that pointed at removed cues.
    pub bank_list_entries_removed: usize,
}

impl std::ops::AddAssign for CueRowChanges {
    fn add_assign(&mut self, o: CueRowChanges) {
        self.updated += o.updated;
        self.added += o.added;
        self.removed += o.removed;
        self.bank_list_entries_removed += o.bank_list_entries_removed;
    }
}

/// rekordbox's 1/150 s frames. (In 128 bits: `usec * 150` overflows past about 6e16 µs.)
fn frames(usec: i64) -> i64 {
    (i128::from(usec) * 150 / 1_000_000) as i64
}

impl CueRow {
    /// A row from its stored columns.
    fn decode(cue_id: Option<i64>, stored: Vec<(String, Value)>) -> CueRow {
        let get = |name: &str| stored.iter().find(|(n, _)| n == name).map(|(_, v)| v);
        let num = |name: &str| get(name).map_or(0, |v| int(v.into()));
        let in_usec = num("inUsec");
        let out_usec = num("outUsec");
        let (numerator, denominator) = (num("beatLoopNumerator"), num("beatLoopDenominator"));
        CueRow {
            cue_id,
            hot_cue: masterdb::hot_cue_from_kind(num("kind")),
            in_usec,
            out_usec: (out_usec >= 0 && out_usec > in_usec).then_some(out_usec),
            comment: get("cueComment").map(|v| text(v.into())).unwrap_or_default(),
            color: num("colorTableIndex").clamp(0, 255) as u8,
            active_loop: num("isActiveLoop") != 0,
            loop_beats: match (u16::try_from(numerator), u16::try_from(denominator)) {
                (Ok(n), Ok(d)) if n > 0 && d > 0 => Some((n, d)),
                _ => None,
            },
            stored,
        }
    }

    /// Whether two rows mean the same cue (ignoring how they're stored).
    pub fn same_as(&self, o: &CueRow) -> bool {
        (self.hot_cue, self.in_usec, self.out_usec, &self.comment, self.color, self.active_loop, self.loop_beats)
            == (o.hot_cue, o.in_usec, o.out_usec, &o.comment, o.color, o.active_loop, o.loop_beats)
    }

    /// The columns to write: every modelled one for a new row, else those whose meaning changed
    /// from what's stored.
    fn changed_columns(&self) -> Vec<(&'static str, Value)> {
        let was = (!self.stored.is_empty()).then(|| CueRow::decode(None, self.stored.clone()));
        let changed = |f: &dyn Fn(&CueRow) -> bool| was.as_ref().is_none_or(|w| !f(w));
        let i = Value::Integer;
        let mut out = Vec::new();
        if changed(&|w| w.hot_cue == self.hot_cue) {
            out.push(("kind", i(self.hot_cue.map_or(0, masterdb::kind_for_hot_cue))));
        }
        if changed(&|w| w.in_usec == self.in_usec) {
            out.push(("inUsec", i(self.in_usec)));
            out.push(("in150FramePerSec", i(frames(self.in_usec))));
            out.extend(IN_SEEK.iter().map(|c| (*c, i(0))));
        }
        if changed(&|w| w.out_usec == self.out_usec) {
            out.push(("outUsec", i(self.out_usec.unwrap_or(-1))));
            out.push(("out150FramePerSec", i(self.out_usec.map_or(-1, frames))));
            out.extend(OUT_SEEK.iter().map(|c| (*c, i(0))));
        }
        if changed(&|w| w.comment == self.comment) {
            out.push(("cueComment", Value::Text(self.comment.clone())));
        }
        if changed(&|w| w.color == self.color) {
            out.push(("colorTableIndex", i(self.color.into())));
        }
        if changed(&|w| w.active_loop == self.active_loop) {
            out.push(("isActiveLoop", i(self.active_loop.into())));
        }
        if changed(&|w| w.loop_beats == self.loop_beats) {
            let (n, d) = self.loop_beats.unwrap_or((0, 0));
            out.push(("beatLoopNumerator", i(n.into())));
            out.push(("beatLoopDenominator", i(d.into())));
        }
        out
    }
}

impl OneLibrary {
    /// A track's cue rows, oldest first.
    pub fn cue_rows(&self, content_id: u32) -> Result<Vec<CueRow>> {
        let mut stmt = self.conn.prepare("SELECT * FROM cue WHERE content_id = ?1 ORDER BY cue_id")?;
        let names: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();
        let mut rows = stmt.query([content_id])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let mut cue_id = 0;
            let mut stored = Vec::new();
            for (i, n) in names.iter().enumerate() {
                match n.as_str() {
                    "cue_id" => cue_id = int(r.get_ref(i)?),
                    "content_id" => {}
                    _ => stored.push((n.clone(), r.get::<_, Value>(i)?)),
                }
            }
            out.push(CueRow::decode(Some(cue_id), stored));
        }
        Ok(out)
    }

    /// Make `rows` a track's cues. Rows with a `cue_id` (one of the track's) are updated in place,
    /// rows without one are added, and the track's other rows go, with the hot cue bank list
    /// entries that point at them (those lists are renumbered).
    pub fn replace_cues(&mut self, content_id: u32, rows: &[CueRow]) -> Result<CueRowChanges> {
        let existing: Vec<i64> = self
            .conn
            .prepare("SELECT cue_id FROM cue WHERE content_id = ?1")?
            .query_map([content_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut kept: Vec<i64> = Vec::new();
        for id in rows.iter().filter_map(|r| r.cue_id) {
            if !existing.contains(&id) || kept.contains(&id) {
                return Err(Error::OneLibrary(format!("cue {id} isn't one of track {content_id}'s cues")));
            }
            kept.push(id);
        }
        if existing.len() > kept.len() {
            self.check_no_unknown_references()?;
        }
        let columns = self.columns("cue")?;
        let mut changes = CueRowChanges::default();
        let tx = self.conn.transaction()?;
        // Taken before removing anything, so a removed cue's id never comes back as another cue.
        let mut next_id: i64 = tx.query_row("SELECT coalesce(max(cue_id), 0) + 1 FROM cue", [], |r| r.get(0))?;
        let mut lists: Vec<i64> = Vec::new();
        for id in existing.iter().filter(|id| !kept.contains(id)) {
            let mut stmt = tx.prepare("SELECT DISTINCT hotCueBankList_id FROM hotCueBankList_cue WHERE cue_id = ?1")?;
            for l in stmt.query_map([id], |r| r.get::<_, i64>(0))? {
                lists.push(l?);
            }
            changes.bank_list_entries_removed +=
                tx.execute("DELETE FROM hotCueBankList_cue WHERE cue_id = ?1", [id])?;
            changes.removed += tx.execute("DELETE FROM cue WHERE cue_id = ?1", [id])?;
        }
        lists.sort_unstable();
        lists.dedup();
        for list in lists {
            let rowids: Vec<i64> = tx
                .prepare(
                    "SELECT rowid FROM hotCueBankList_cue WHERE hotCueBankList_id = ?1 ORDER BY sequenceNo, rowid",
                )?
                .query_map([list], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let mut update = tx.prepare("UPDATE hotCueBankList_cue SET sequenceNo = ?1 WHERE rowid = ?2")?;
            for (n, rowid) in rowids.iter().enumerate() {
                update.execute(params![n as i64 + 1, rowid])?;
            }
        }
        for row in rows {
            let values: Vec<(String, Value)> = row
                .changed_columns()
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .filter(|(k, _)| columns.contains(k))
                .collect();
            match row.cue_id {
                Some(_) if values.is_empty() => {}
                Some(id) => {
                    let set: Vec<String> =
                        values.iter().enumerate().map(|(i, (k, _))| format!("{} = ?{}", ident(k), i + 1)).collect();
                    let sql = format!("UPDATE cue SET {} WHERE cue_id = ?{}", set.join(", "), values.len() + 1);
                    let mut args: Vec<&Value> = values.iter().map(|(_, v)| v).collect();
                    let id = Value::Integer(id);
                    args.push(&id);
                    tx.execute(&sql, rusqlite::params_from_iter(args))?;
                    changes.updated += 1;
                }
                None => {
                    let id = next_id;
                    next_id += 1;
                    let mut all: Vec<(String, Value)> = vec![
                        ("cue_id".into(), Value::Integer(id)),
                        ("content_id".into(), Value::Integer(content_id.into())),
                    ];
                    // What was stored (a row copied from another track), then what changed.
                    all.extend(
                        row.stored
                            .iter()
                            .filter(|(k, _)| columns.contains(k) && !values.iter().any(|(v, _)| v == k))
                            .cloned(),
                    );
                    all.extend(values);
                    let sql = format!(
                        "INSERT INTO cue ({}) VALUES ({})",
                        all.iter().map(|(k, _)| ident(k)).collect::<Vec<_>>().join(", "),
                        vec!["?"; all.len()].join(", ")
                    );
                    tx.execute(&sql, rusqlite::params_from_iter(all.iter().map(|(_, v)| v)))?;
                    changes.added += 1;
                }
            }
        }
        tx.commit()?;
        Ok(changes)
    }

    /// Hot cue bank list entries whose cue doesn't exist.
    pub fn dangling_bank_list_entries(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM hotCueBankList_cue WHERE cue_id NOT IN (SELECT cue_id FROM cue)",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library_with_cues() -> (OneLibrary, u32) {
        let mut one = OneLibrary::create().unwrap();
        let defaults = one.content_defaults().unwrap();
        let rec = super::super::ContentRecord::from_track(
            &crate::model::tests::track(1, "A", "B", "/Contents/A/B.mp3"),
            true,
            &defaults,
        );
        let t = one.add_track(&rec, "/Contents/A/B.mp3", "", None).unwrap();
        one.conn
            .execute_batch(&format!(
                "INSERT INTO cue (cue_id, content_id, kind, colorTableIndex, cueComment, isActiveLoop, beatLoopNumerator,
                     beatLoopDenominator, inUsec, outUsec, in150FramePerSec, out150FramePerSec, inMpegFrameNumber,
                     outMpegFrameNumber, inMpegAbs, outMpegAbs, inDecodingStartFramePosition,
                     outDecodingStartFramePosition, inFileOffsetInBlock, OutFileOffsetInBlock,
                     inNumberOfSampleInBlock, outNumberOfSampleInBlock)
                 VALUES (7, {t}, 0, 0, '', 0, 0, 0, 1500250, -1, 225, -1, 57, 0, 120, 0, 3, 0, 418, 0, 1152, 0),
                        (8, {t}, 2, 42, 'Drop', 0, 0, 0, 30000000, -1, 4500, -1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
                 INSERT INTO hotCueBankList VALUES (1, 0, 'Bank', NULL, 0, 0);
                 INSERT INTO hotCueBankList_cue VALUES (1, 7, 1), (1, 8, 2);"
            ))
            .unwrap();
        (one, t)
    }

    #[test]
    fn reads_rows() {
        let (one, t) = library_with_cues();
        let rows = one.cue_rows(t).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0].cue_id, rows[0].hot_cue, rows[0].in_usec, rows[0].out_usec),
            (Some(7), None, 1_500_250, None)
        );
        assert_eq!((rows[1].hot_cue, rows[1].comment.as_str(), rows[1].color), (Some(1), "Drop", 42));
    }

    #[test]
    fn updates_adds_and_removes_keeping_ids() {
        let (mut one, t) = library_with_cues();
        let mut rows = one.cue_rows(t).unwrap();
        // Rename the memory cue (its seek columns stay), drop hot cue B, add a loop on hot cue C.
        rows[0].comment = "Intro".into();
        rows.remove(1);
        rows.push(CueRow {
            hot_cue: Some(2),
            in_usec: 60_000_000,
            out_usec: Some(64_000_000),
            loop_beats: Some((8, 1)),
            ..Default::default()
        });
        let changes = one.replace_cues(t, &rows).unwrap();
        assert_eq!(changes, CueRowChanges { updated: 1, added: 1, removed: 1, bank_list_entries_removed: 1 });
        let after = one.cue_rows(t).unwrap();
        assert_eq!(after.len(), 2);
        assert_eq!((after[0].cue_id, after[0].comment.as_str()), (Some(7), "Intro"));
        let seek: i64 =
            one.conn.query_row("SELECT inFileOffsetInBlock FROM cue WHERE cue_id = 7", [], |r| r.get(0)).unwrap();
        assert_eq!(seek, 418, "untouched columns keep their values");
        assert_eq!(
            (after[1].cue_id, after[1].out_usec, after[1].loop_beats),
            (Some(9), Some(64_000_000), Some((8, 1)))
        );
        let frames: (i64, i64) = one
            .conn
            .query_row("SELECT in150FramePerSec, out150FramePerSec FROM cue WHERE cue_id = 9", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(frames, (9000, 9600));
        // The bank list lost its entry for B and was renumbered.
        let bank: Vec<(i64, i64)> = one
            .conn
            .prepare("SELECT cue_id, sequenceNo FROM hotCueBankList_cue")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(bank, [(7, 1)]);
        assert_eq!(one.dangling_bank_list_entries().unwrap(), 0);

        // Moving a cue resets its seek columns.
        let mut rows = one.cue_rows(t).unwrap();
        rows[0].in_usec = 2_000_000;
        one.replace_cues(t, &rows).unwrap();
        let seek: (i64, i64) = one
            .conn
            .query_row("SELECT inFileOffsetInBlock, in150FramePerSec FROM cue WHERE cue_id = 7", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(seek, (0, 300));
        // Another track's cue can't be claimed.
        let foreign = CueRow { cue_id: Some(7), ..Default::default() };
        assert!(one.replace_cues(t + 1, &[foreign]).is_err());
    }
}
