//! `plugins.analyze`, a plugin's analyzer on a drive's tracks, and `plugins.apply_fields.*`, writing
//! the key and BPM it worked out to the tracks.

use locallibrary::edit::EditPlan;
use locallibrary::op::Planned;
use locallibrary::plugin::{Analyzed, ApplyFields};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::plugins::host;
use crate::rpc::RpcError;
use crate::rpc::methods::{Call, PlanStore, Registry};

#[derive(Deserialize, JsonSchema)]
struct AnalyzeWithPlugin {
    plugin: String,
    analyzer: String,
    /// The library or drive whose tracks to analyse.
    drive: String,
    /// Only these tracks, by id [default: every one].
    #[serde(default)]
    tracks: Option<Vec<u32>>,
}

pub(super) fn register(r: &mut Registry) {
    r.immediate("plugins.analyze", |c, req: AnalyzeWithPlugin| -> Result<Analyzed, _> {
        let tracks = req.tracks.as_deref();
        Ok(host()?.analyze(&req.plugin, &req.analyzer, &req.drive, tracks, &c.store()?, c.ctx)?)
    })
    .long()
    .keeps_cache();
    // Overwriting what tracks have, when asked to.
    r.plan("plugins.apply_fields.plan", apply_fields_plan).long().destructive();
    r.apply("plugins.apply_fields.apply", PlanStore::None, |c, req: ApplyFields| {
        Ok(apply_fields_plan(c, req)?.apply(None, c.ctx)?)
    })
    .long()
    .destructive();
}

fn apply_fields_plan(c: &Call<'_>, req: ApplyFields) -> Result<Planned<EditPlan>, RpcError> {
    Ok(host()?.plan_apply_fields(&req, &c.store()?, c.ctx)?)
}
