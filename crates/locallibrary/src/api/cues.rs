//! Cue points: list a track's hot cues, memory cues and loops, and edit them
//! (see [`crate::cue_edit`]).

use schemars::JsonSchema;
use serde::Deserialize;

use super::{LibraryCache, LibraryRef, TrackSelector, resolve_drive};
use crate::collection_write;
use crate::cue_edit::{self, AutoCues, CueEdit, CuePlan, CueRef, CueReport, SetCue, TrackCues, UpdateCue};
use crate::device::Device;
use crate::op::{Operation, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};
use loclib_rekordbox_db::masterdb;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListCues {
    #[serde(flatten)]
    pub library: LibraryRef,
    #[serde(flatten)]
    pub tracks: TrackSelector,
}

pub fn cues_list(req: &ListCues) -> Result<Vec<TrackCues>> {
    super::cues_list_cached(LibraryCache::none(), req)
}

/// What to do to the selected tracks' cues, tagged by `op`:
/// `{"op": "set", "hot_cue": "B", "at": {"ms": 62500}, "loop_beats": 8, "name": "Drop", "color": "red"}`,
/// `{"op": "update", "cue": {"hot": "B"}, "name": "Drop"}`, `{"op": "delete", "cue": "all"}`,
/// `{"op": "copy", "from": {"ids": [12]}}`, `{"op": "auto", "style": "rekordbox"}`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CueOp {
    Set(SetCue),
    Update(UpdateCue),
    Delete {
        cue: CueRef,
    },
    /// Replace the selected tracks' cues with a copy of one other track's.
    Copy {
        from: TrackSelector,
    },
    /// Place the cues a style puts on each track, from its own beat grid and phrases.
    Auto(AutoCues),
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditCues {
    /// The drive or library whose cues to edit; left out with `collection`.
    #[serde(default)]
    pub drive: String,
    /// Edit rekordbox's collection on this computer instead of a drive (`{}` for rekordbox's own
    /// `master.db`). It's written only while rekordbox and its agent are closed, over a fresh
    /// backup in the store; an edit made while either runs waits in the store until both have quit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<super::CollectionRef>,
    /// The tracks to edit.
    #[serde(flatten)]
    pub tracks: TrackSelector,
    pub edit: CueOp,
}

fn edit_of(req: &EditCues) -> Result<(Device, Vec<u32>, CueEdit)> {
    let d = resolve_drive(&req.drive)?;
    let lib = d.library()?;
    let ids = req.tracks.select_on(&d, &lib)?;
    let edit = match &req.edit {
        CueOp::Set(s) => CueEdit::Set(s.clone()),
        CueOp::Update(u) => CueEdit::Update(u.clone()),
        CueOp::Delete { cue } => CueEdit::Delete(cue.clone()),
        CueOp::Copy { from } => match from.select_on(&d, &lib)?[..] {
            [one] => CueEdit::Copy { from: one },
            ref many => return Err(Error::invalid(format!("copy from one track ({} selected)", many.len()))),
        },
        CueOp::Auto(a) => CueEdit::Auto(*a),
    };
    Ok((d, ids, edit))
}

/// A cue edit of rekordbox's collection: the selected tracks among its own (selected by their
/// paths on this computer, as the player has them).
fn collection_plan(req: &EditCues, c: &super::CollectionRef) -> Result<Planned<CuePlan>> {
    let path = super::collection_path(c.db.as_deref())?;
    let mut collection = masterdb::open(&path, &masterdb::resolve_key(c.key.as_deref()))?;
    // Planned on the cues as they'll be once what's waiting is written.
    let store = super::store_root().and_then(|r| Store::open_existing(r).ok().flatten());
    let waiting = match &store {
        Some(s) => collection_write::staged(s, &path)?,
        None => Vec::new(),
    };
    let edits = waiting.iter().filter_map(|s| match &s.change {
        collection_write::Change::Cues(e) => Some(e),
        collection_write::Change::Undo { .. } => None,
    });
    cue_edit::collection::overlay(&mut collection.cues, &collection.content_ids, edits);
    let library = collection.library_from_root(&masterdb::CloudFolders::this_computer());
    let query = crate::query::QueryContext::new(&library);
    let ids = req.tracks.select_with(&query)?;
    let edit = match &req.edit {
        CueOp::Set(s) => CueEdit::Set(s.clone()),
        CueOp::Update(u) => CueEdit::Update(u.clone()),
        CueOp::Delete { cue } => CueEdit::Delete(cue.clone()),
        CueOp::Copy { from } => match from.select_with(&query)?[..] {
            [one] => CueEdit::Copy { from: one },
            ref many => return Err(Error::invalid(format!("copy from one track ({} selected)", many.len()))),
        },
        CueOp::Auto(a) => CueEdit::Auto(*a),
    };
    let mut plan = cue_edit::collection::plan(&collection, &ids, &edit)?;
    plan.collection_key = masterdb::resolve_key(c.key.as_deref());
    let summary = plan
        .tracks
        .iter()
        .map(|t| format!("{}: {} cue(s) -> {}", t.track.name, t.before.len(), t.after.len()))
        .collect::<Vec<_>>();
    let destructive = plan.tracks.iter().any(|t| t.after.len() < t.before.len());
    let (noop, warnings) = (plan.is_noop(), plan.warnings.clone());
    Ok(Planned::new("cues", plan, ())
        .summary(summary)
        .warnings(warnings)
        .destructive(destructive)
        .noop(noop)
        .touches(Touched::path(&path, true, Vec::new())))
}

/// What a cue edit would change on the selected tracks, with what plugins' guards make of it.
pub fn cues_plan(req: &EditCues, ctx: &Ctx<'_>) -> Result<Planned<CuePlan>> {
    if let Some(c) = &req.collection {
        return collection_plan(req, c);
    }
    let (d, ids, edit) = edit_of(req)?;
    let plan = cue_edit::plan(&d, &ids, &edit, ctx)?;
    let summary = plan
        .tracks
        .iter()
        .map(|t| format!("{}: {} cue(s) -> {}", t.track.name, t.before.len(), t.after.len()))
        .collect::<Vec<_>>();
    let destructive = plan.tracks.iter().any(|t| t.after.len() < t.before.len());
    let files = plan.tracks.iter().flat_map(|t| t.files.iter().cloned()).collect();
    let (noop, warnings) = (plan.is_noop(), plan.warnings.clone());
    Planned::new("cues", plan, ())
        .summary(summary)
        .warnings(warnings)
        .destructive(destructive)
        .noop(noop)
        .touches(Touched::drive(&d, true, vec![Probe::Databases, Probe::Identity, Probe::Content(files)]))
        .reaching_clones(ctx)?
        .guard(None)?
        .guarded(|p| p.recorded.iter().cloned().collect(), ctx)
}

impl Operation for CuePlan {
    type Request = ();
    type Report = CueReport;

    fn apply(planned: &Planned<Self>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<CueReport> {
        let plan = &planned.plan;
        if let Some(edit) = &plan.collection {
            if plan.is_noop() {
                return Ok(CueReport::default());
            }
            let store = collection_write::store_for(store)?;
            let change = collection_write::Change::Cues(edit.clone());
            let write = collection_write::make_now(&store, &plan.root, &plan.collection_key, change, ctx)?;
            let tracks_changed = if write.written > 0 { plan.tracks.len() } else { 0 };
            return Ok(CueReport {
                tracks_changed,
                backup: write.backup.as_ref().map(|b| b.display().to_string()),
                collection: Some(write),
                ..Default::default()
            });
        }
        cue_edit::apply(plan, &mut planned.drive(0)?, ctx)
    }
}

/// Plan and edit cues in one go, for callers that don't confirm (see [`cues_plan`]).
pub fn cues_apply(req: &EditCues, ctx: &Ctx<'_>) -> Result<CueReport> {
    cues_plan(req, ctx)?.apply(None, ctx)
}
