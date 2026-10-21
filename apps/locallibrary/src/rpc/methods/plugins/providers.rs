//! `plugins.provide*`: what providers find for tracks, and the edits of what's picked, and
//! `plugins.providers*`: the providers, and when a candidate is picked for the user.

use locallibrary::edit::EditPlan;
use locallibrary::op::Planned;
use locallibrary::plugin::{PlanProvided, Provide, Provided, plan_provided, set_auto_pick};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::plugins::host;
use crate::plugins::providers::Providers;
use crate::rpc::RpcError;
use crate::rpc::methods::{Call, PlanStore, Registry};

#[derive(Deserialize, JsonSchema)]
struct SetAutoPick {
    /// A candidate alone scoring this or more (0 to 1; more than 1: never) is picked for the user;
    /// `null` (or left out) puts it back to its default.
    #[serde(default)]
    auto_pick: Option<f64>,
}

pub(super) fn register(r: &mut Registry) {
    r.read("plugins.provide", |c, req: Provide| -> Result<Provided, _> {
        Ok(host()?.provide(&req, c.existing()?.as_ref(), c.ctx)?)
    })
    .long();
    // Over what tracks have, with `replace`.
    r.plan("plugins.provide.plan", provide_plan).long().destructive();
    r.apply("plugins.provide.apply", PlanStore::None, |c, req: PlanProvided| {
        Ok(provide_plan(c, req)?.apply(None, c.ctx)?)
    })
    .long()
    .destructive();
    r.read("plugins.providers", |c, _: locallibrary::api::NoParams| Ok(Providers::of(host()?, c.existing()?.as_ref())))
        .quick();
    r.immediate("plugins.providers.set", |c, req: SetAutoPick| {
        let store = c.store()?;
        set_auto_pick(&store, req.auto_pick)?;
        Ok(Providers::of(host()?, Some(&store)))
    })
    .keeps_cache();
}

fn provide_plan(c: &Call<'_>, req: PlanProvided) -> Result<Planned<EditPlan>, RpcError> {
    Ok(plan_provided(&req, c.ctx)?)
}
