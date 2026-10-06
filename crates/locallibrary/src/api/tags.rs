//! My Tags (see [`crate::tags`]): `tags.list`, `tags.plan`, `tags.apply`.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{DriveRef, TrackSelector, resolve_drive};
use crate::Result;
use crate::device::Device;
use crate::op::{Operation, Planned, Touched};
use crate::progress::Ctx;
use crate::store::Store;
use crate::tags::{self, Edit, MyTags, TagPlan, TagReport};

/// A My Tag edit: `{"drive": "Main", "action": "add", "tag": "Genre/Deep House", "query": "..."}`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TagsEdit {
    pub drive: String,
    #[serde(flatten)]
    pub action: TagAction,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TagAction {
    /// Give the selected tracks a tag (`Category/Tag`), creating it where it's missing.
    Add {
        tag: String,
        #[serde(flatten)]
        tracks: TrackSelector,
    },
    /// Take a tag off the selected tracks.
    Remove {
        tag: String,
        #[serde(flatten)]
        tracks: TrackSelector,
    },
    /// Rename a tag (`Category/Tag`) or a category (`Category`).
    Rename { tag: String, to: String },
    /// Delete a tag, taking it off every track.
    Delete { tag: String },
}

impl TagAction {
    fn edit(&self, device: &Device) -> Result<Edit> {
        Ok(match self {
            TagAction::Add { tag, tracks } => {
                Edit::Add { tag: tag.clone(), tracks: tracks.select_on(device, &device.library()?)? }
            }
            TagAction::Remove { tag, tracks } => {
                Edit::Remove { tag: tag.clone(), tracks: tracks.select_on(device, &device.library()?)? }
            }
            TagAction::Rename { tag, to } => Edit::Rename { tag: tag.clone(), to: to.clone() },
            TagAction::Delete { tag } => Edit::Delete { tag: tag.clone() },
        })
    }
}

/// A drive's categories and tags, the tracks carrying each, and where `exportExt.pdb` and
/// OneLibrary disagree.
pub fn tags_list(req: &DriveRef) -> Result<MyTags> {
    tags::read(&resolve_drive(&req.drive)?)
}

/// What a My Tag edit would change.
pub fn tags_plan(req: &TagsEdit) -> Result<Planned<TagPlan>> {
    let d = resolve_drive(&req.drive)?;
    let plan = tags::plan(&d, &req.action.edit(&d)?)?;
    let line = match &plan.to {
        Some(to) => format!("rename {} to {to}", plan.tag),
        None => format!("{} {} ({} track(s))", plan.action, plan.tag, plan.tracks.len()),
    };
    let destructive = matches!(plan.action.as_str(), "remove" | "delete");
    let (noop, warnings) = (plan.is_noop(), plan.warnings.clone());
    Planned::new("tags", plan, ())
        .line(line)
        .warnings(warnings)
        .destructive(destructive)
        .noop(noop)
        .touches(Touched::library(&d, true))
        .guard(None)
}

impl Operation for TagPlan {
    type Request = ();
    type Report = TagReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<TagReport> {
        tags::apply(&planned.plan, &mut planned.drive(0)?, ctx)
    }
}

/// Plan and edit My Tags in one go, for callers that don't confirm (see [`tags_plan`]).
pub fn tags_apply(req: &TagsEdit, ctx: &Ctx<'_>) -> Result<TagReport> {
    tags_plan(req)?.apply(None, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_flat_json() {
        let req: TagsEdit = serde_json::from_value(serde_json::json!({
            "drive": "Main", "action": "add", "tag": "Genre/Deep House", "query": "Loopmasters", "ids": [3]
        }))
        .unwrap();
        match req.action {
            TagAction::Add { tag, tracks } => {
                assert_eq!(tag, "Genre/Deep House");
                assert_eq!(tracks.query.as_deref(), Some("Loopmasters"));
                assert_eq!(tracks.ids, [3]);
            }
            other => panic!("{other:?}"),
        }
        let req: TagsEdit = serde_json::from_value(
            serde_json::json!({ "drive": "Main", "action": "rename", "tag": "Genre", "to": "Style" }),
        )
        .unwrap();
        assert!(matches!(req.action, TagAction::Rename { .. }));
    }
}
