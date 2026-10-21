//! `plugins.apply_analysis.*`: writing the beat grid, phrases and waveform an analyzer worked out
//! (feature `analysis`) to the tracks' analysis files.

use locallibrary::edit::EditPlan;
use locallibrary::op::Planned;
use locallibrary::plugin::ApplyAnalysis;

use crate::plugins::host;
use crate::rpc::RpcError;
use crate::rpc::methods::{Call, PlanStore, Registry};

pub(super) fn register(r: &mut Registry) {
    // Overwriting what tracks have, when asked to.
    r.plan("plugins.apply_analysis.plan", plan).long().destructive();
    r.apply("plugins.apply_analysis.apply", PlanStore::None, |c, req: ApplyAnalysis| {
        Ok(plan(c, req)?.apply(None, c.ctx)?)
    })
    .long()
    .destructive();
}

fn plan(c: &Call<'_>, req: ApplyAnalysis) -> Result<Planned<EditPlan>, RpcError> {
    Ok(host()?.plan_apply_analysis(&req, &c.store()?, c.ctx)?)
}
