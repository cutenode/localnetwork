//! Editing cues in rekordbox's collection on this computer (`master.db`), not a drive's: the same
//! edits ([`CueEdit`]), worked out on the cues `master.db` holds and written through
//! [`loclib_rekordbox_db::masterdb::write`] (only while rekordbox and its agent are closed, over a fresh backup). Where a
//! drive's cues live in analysis files, the collection's live in `djmdCue` and `contentCue`;
//! rekordbox's analysis files in its `share` folder give only the beat grid and phrases.
//!
//! An edit is planned on the cues as they're read, and made again on the cues `master.db` holds
//! when it's written (`apply_in`): a staged edit written after rekordbox changed the track's
//! other cues keeps those changes, as a drive's journal replay does.

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use super::{CueChange, CueEdit, CuePlan, Draft, Loaded, TrackCueChange, TrackState, edited, info, sibling_key};
use crate::anlz::{self, CuePoint, DEFAULT_HOT_CUE_RGB, HOT_CUE_PALETTE};
use crate::model::{Library, Track};
use crate::{Error, Result};
use loclib_model::analysis::{Cue, hot_cue_palette};
use loclib_rekordbox_db::masterdb::Collection;
use loclib_rekordbox_db::masterdb::write::{CueEntry, CuesBefore, Edit};

/// A cue edit of the collection's, as it's kept until it's written (see
/// [`crate::collection_write`]): its tracks by `djmdContent.ID`, which stays put, and the edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionCueEdit {
    pub tracks: Vec<CollectionTrack>,
    pub change: CueChange,
    /// The track a copy or carry is from, by `djmdContent.ID`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// A track a collection cue edit changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionTrack {
    /// Its `djmdContent.ID`.
    pub id: String,
    /// Its name, to show.
    pub name: String,
    /// Its cues after the edit, as planned: what the player shows until the edit is written.
    pub after: Vec<CueEntry>,
}

impl CollectionCueEdit {
    /// `set hot cue A on 1 track(s)`.
    pub fn describe(&self) -> String {
        self.change.describe(self.tracks.len())
    }
}

/// A cue as the collection's reader would read it back, for showing a cue that isn't written yet.
pub fn entry_cue(e: &CueEntry) -> Cue {
    Cue {
        hot_cue: e.hot_cue,
        start_ms: e.start_ms,
        end_ms: e.end_ms,
        comment: e.comment.clone(),
        color: e.hot_cue.and(e.color).and_then(hot_cue_palette),
        color_id: 0,
        loop_beats: None,
        active_loop: e.active_loop,
    }
}

fn point(e: &CueEntry) -> CuePoint {
    let code = e.color.unwrap_or(0);
    let rgb = match e.hot_cue {
        Some(_) => anlz::hot_cue_palette(code).unwrap_or(DEFAULT_HOT_CUE_RGB),
        None => [0; 3],
    };
    CuePoint {
        hot_cue: e.hot_cue,
        start_ms: e.start_ms,
        end_ms: e.end_ms,
        comment: e.comment.clone(),
        color_code: code,
        color_rgb: rgb,
        active_loop: e.active_loop,
        ..Default::default()
    }
}

fn entry(c: &CuePoint) -> CueEntry {
    let code = Some(c.color_code).filter(|n| c.hot_cue.is_some() && (1..=HOT_CUE_PALETTE.len() as u8).contains(n));
    CueEntry {
        hot_cue: c.hot_cue,
        start_ms: c.start_ms,
        end_ms: c.end_ms.filter(|e| *e > c.start_ms),
        comment: c.comment.clone(),
        color: code,
        active_loop: c.active_loop && c.end_ms.is_some(),
    }
}

/// Whether two cue lists hold the same cues, in any order.
fn same(a: &[CueEntry], b: &[CueEntry]) -> bool {
    let key = |l: &[CueEntry]| {
        let mut k: Vec<_> =
            l.iter().map(|c| (c.hot_cue, c.start_ms, c.end_ms, c.comment.clone(), c.color, c.active_loop)).collect();
        k.sort();
        k
    };
    key(a) == key(b)
}

/// `track`'s beat grid and phrases, from its analysis files in rekordbox's `share` folder, with
/// `cues` as its cues. A track rekordbox hasn't analysed has no grid: times still work, beats don't.
fn loaded(collection: &Collection, track: &Track, cues: &[CueEntry]) -> Loaded {
    let dat = (!track.analyze_path.is_empty()).then(|| collection.share_file(&track.analyze_path));
    let read = |p: std::path::PathBuf| std::fs::read(p).ok();
    let dat_bytes = dat.clone().and_then(read).unwrap_or_default();
    let ext_bytes = match track.analyze_path.is_empty() {
        true => None,
        false => sibling_key(&track.analyze_path, "EXT").map(|k| collection.share_file(&k)).and_then(read),
    };
    Loaded {
        dat: String::new(),
        ext: None,
        beats: anlz::beat_grid(&dat_bytes).unwrap_or_default(),
        dat_bytes: Vec::new(),
        ext_bytes,
        cues: cues.iter().map(point).collect(),
    }
}

/// The cues `edit` leaves `track` with, from `cues`; `source`: a copy's track and its cues.
fn after(
    collection: &Collection,
    track: &Track,
    cues: &[CueEntry],
    edit: &CueEdit,
    source: Option<(&Track, &[CueEntry])>,
) -> Result<(Loaded, Vec<CueEntry>)> {
    let l = loaded(collection, track, cues);
    let source = source.map(|(t, c)| loaded(collection, t, c));
    let drafts: Vec<Draft> = edited(track, &l, edit, source.as_ref())?;
    Ok((l, drafts.iter().map(|d| entry(&d.cue)).collect()))
}

fn content_id(collection: &Collection, id: u32) -> Result<String> {
    collection.content_ids.get(&id).cloned().ok_or_else(|| Error::no_track(id))
}

/// Plan `edit` on the collection's tracks `ids` (ids of `collection.library`), as a [`CuePlan`]
/// whose `drive` is the collection and whose `root` its `master.db`. Tracks the edit leaves as
/// they are aren't in it.
pub(crate) fn plan(collection: &Collection, ids: &[u32], edit: &CueEdit) -> Result<CuePlan> {
    if ids.is_empty() {
        return Err(Error::invalid("no tracks selected"));
    }
    let one_track = !matches!(edit, CueEdit::Copy { .. } | CueEdit::Carry { .. } | CueEdit::Delete(super::CueRef::All))
        && !matches!(edit, CueEdit::Auto(_));
    if one_track && ids.len() > 1 {
        return Err(Error::invalid(format!("{} tracks selected; pick one", ids.len())));
    }
    let library: &Library = &collection.library;
    let track = |id: u32| library.track(id).ok_or_else(|| Error::no_track(id));
    let cues_of = |id: u32| -> Vec<CueEntry> {
        collection.cues.get(&id).map(|l| l.iter().map(CueEntry::from).collect()).unwrap_or_default()
    };
    let source = match edit {
        CueEdit::Copy { from } | CueEdit::Carry { from, .. } => Some((track(*from)?, cues_of(*from))),
        _ => None,
    };
    let mut warnings = Vec::new();
    let mut tracks = Vec::new();
    let mut kept = Vec::new();
    for &id in ids {
        let t = track(id)?;
        if source.as_ref().is_some_and(|(s, _)| s.id == id) {
            continue;
        }
        let before = cues_of(id);
        let (l, now) = match after(collection, t, &before, edit, source.as_ref().map(|(s, c)| (*s, c.as_slice()))) {
            Ok(r) => r,
            Err(e) if matches!(edit, CueEdit::Auto(_)) => {
                warnings.push(e.to_string());
                continue;
            }
            Err(e) => return Err(e),
        };
        if matches!(edit, CueEdit::Auto(_)) && l.beats.is_empty() {
            warnings.push(format!("{} has no beat grid, so no cues were placed on it", t.display_name()));
            continue;
        }
        if same(&before, &now) {
            continue;
        }
        let points: Vec<CuePoint> = now.iter().map(point).collect();
        if points.iter().any(|c| c.hot_cue.is_none() && c.color_id != 0) || memory_colour(edit) {
            warnings.push("a memory cue's colour isn't written to rekordbox's collection yet".into());
        }
        tracks.push(TrackCueChange {
            track: t.into(),
            before: l.cues.iter().map(|c| info(c, &l.beats)).collect(),
            after: points.iter().map(|c| info(c, &l.beats)).collect(),
            files: Vec::new(),
            onelibrary: false,
            state: TrackState { dat: String::new(), ext: None, before: l.cues, after: points, plus: None },
        });
        kept.push(CollectionTrack { id: content_id(collection, id)?, name: t.display_name(), after: now });
    }
    warnings.dedup();
    let from = match edit {
        CueEdit::Copy { from } | CueEdit::Carry { from, .. } => Some(content_id(collection, *from)?),
        _ => None,
    };
    let change = CueChange::of(edit, library);
    let needed_bytes = std::fs::metadata(&collection.path).map_or(0, |m| m.len());
    Ok(CuePlan {
        drive: "rekordbox's collection".into(),
        root: collection.path.clone(),
        tracks,
        needed_bytes,
        free_space: None,
        fits: true,
        warnings,
        recorded: None,
        collection: Some(CollectionCueEdit { tracks: kept, change, from }),
        collection_key: String::new(),
    })
}

/// Whether `edit` gives a memory cue a colour, which the collection's cues don't keep yet.
fn memory_colour(edit: &CueEdit) -> bool {
    let set = |s: &super::SetCue| s.hot_cue.is_none() && s.color.as_deref().is_some_and(|c| c != "none");
    match edit {
        CueEdit::Set(s) => set(s),
        CueEdit::SetMany(l) => l.iter().any(set),
        CueEdit::Update(u) => {
            !matches!(u.cue, super::CueRef::Hot(_)) && u.color.as_deref().is_some_and(|c| c != "none")
        }
        _ => false,
    }
}

/// The edit `change` stands for on `collection`, a copy's source (`from`) by its id there.
fn edit_of(collection: &Collection, change: &CueChange, from: Option<&str>) -> Result<CueEdit> {
    let source = || -> Result<u32> {
        let from = from.ok_or_else(|| Error::invalid("a copy of cues names no track to copy from"))?;
        collection
            .content_ids
            .iter()
            .find(|(_, c)| c.as_str() == from)
            .map(|(id, _)| *id)
            .ok_or_else(|| Error::NotFound(format!("track {from} in rekordbox's collection")))
    };
    Ok(match change {
        CueChange::Set(s) => CueEdit::Set(s.clone()),
        CueChange::SetMany(l) => CueEdit::SetMany(l.clone()),
        CueChange::Update(u) => CueEdit::Update(u.clone()),
        CueChange::Delete(r) => CueEdit::Delete(r.clone()),
        CueChange::Copy { .. } => CueEdit::Copy { from: source()? },
        CueChange::Carry { shift_ms, .. } => CueEdit::Carry { from: source()?, shift_ms: *shift_ms },
        CueChange::Auto(a) => CueEdit::Auto(*a),
    })
}

/// Make `edit` on the cues `master.db` holds now, inside a write (`e`); `collection` is the
/// collection read just before, for its tracks' grids and lengths. A track the edit no longer
/// applies to (its hot cue was deleted in rekordbox since, the track is gone) is passed over with a
/// warning. Answers what each track's cues were.
pub(crate) fn apply_in(
    e: &mut Edit<'_>,
    collection: &Collection,
    edit: &CollectionCueEdit,
    warnings: &mut Vec<String>,
) -> Result<Vec<CuesBefore>> {
    let ids: FxHashMap<&str, u32> = collection.content_ids.iter().map(|(n, c)| (c.as_str(), *n)).collect();
    let track = |id: &str| ids.get(id).and_then(|n| collection.library.track(*n));
    let made = edit_of(collection, &edit.change, edit.from.as_deref());
    let source = match &edit.from {
        Some(from) => match track(from) {
            Some(t) => Some((t, e.cues(from)?)),
            None => None,
        },
        None => None,
    };
    let mut out = Vec::new();
    for t in &edit.tracks {
        let attempt = (|| -> Result<Option<CuesBefore>> {
            let edit = made.as_ref().map_err(|e| Error::invalid(e.to_string()))?;
            let track = track(&t.id).ok_or_else(|| Error::NotFound(format!("{} in rekordbox's collection", t.name)))?;
            let now = e.cues(&t.id)?;
            let (_, cues) = after(collection, track, &now, edit, source.as_ref().map(|(s, c)| (*s, c.as_slice())))?;
            if same(&now, &cues) {
                return Ok(None);
            }
            Ok(Some(e.set_cues(&t.id, &cues)?))
        })();
        match attempt {
            Ok(Some(before)) => out.push(before),
            Ok(None) => {}
            Err(err) => warnings.push(format!("{} on {}: {err}", edit.describe(), t.name)),
        }
    }
    Ok(out)
}

/// Each staged edit's tracks' cues as they'll be once written, over `cues` (the collection's, by
/// track id): what the player shows meanwhile. `edits` in the order they were made.
pub fn overlay<'a>(
    cues: &mut indexmap::IndexMap<u32, Vec<Cue>>,
    content_ids: &FxHashMap<u32, String>,
    edits: impl IntoIterator<Item = &'a CollectionCueEdit>,
) {
    let ids: FxHashMap<&str, u32> = content_ids.iter().map(|(n, c)| (c.as_str(), *n)).collect();
    for edit in edits {
        for t in &edit.tracks {
            if let Some(id) = ids.get(t.id.as_str()) {
                let mut list: Vec<Cue> = t.after.iter().map(entry_cue).collect();
                list.sort_by_key(|c| (c.hot_cue.map_or(0, |h| u32::from(h) + 1), c.start_ms));
                cues.insert(*id, list);
            }
        }
    }
}
