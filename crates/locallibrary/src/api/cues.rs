//! Cue points: list a track's hot cues, memory cues and loops, and edit them
//! (see [`crate::cue_edit`]).

use schemars::JsonSchema;
use serde::Deserialize;

use super::{LibraryCache, LibraryRef, TrackSelector, resolve_drive};
use crate::cue_edit::{self, CueEdit, CuePlan, CueRef, CueReport, SetCue, TrackCues, UpdateCue};
use crate::device::Device;
use crate::op::{Operation, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

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
/// `{"op": "copy", "from": {"ids": [12]}}`.
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
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditCues {
    pub drive: String,
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
    };
    Ok((d, ids, edit))
}

/// What a cue edit would change on the selected tracks.
pub fn cues_plan(req: &EditCues, ctx: &Ctx<'_>) -> Result<Planned<CuePlan>> {
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
        .guard(None)
}

impl Operation for CuePlan {
    type Request = ();
    type Report = CueReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<CueReport> {
        cue_edit::apply(&planned.plan, &mut planned.drive(0)?, ctx)
    }
}

/// Plan and edit cues in one go, for callers that don't confirm (see [`cues_plan`]).
pub fn cues_apply(req: &EditCues, ctx: &Ctx<'_>) -> Result<CueReport> {
    cues_plan(req, ctx)?.apply(None, ctx)
}
