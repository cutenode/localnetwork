//! `plugins.op.*`: a plugin's operation, planned (or the question it asks first) and applied.

use locallibrary::plugin::OpPlanned;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::PluginContext;
use crate::plugins::host;
use crate::rpc::RpcError;
use crate::rpc::methods::{Call, PlanStore, Registry};

#[derive(Deserialize, JsonSchema)]
struct PlanPluginOp {
    plugin: String,
    operation: String,
    /// As the operation's `params` schema says (`plugins.describe`).
    #[serde(default)]
    params: Value,
    context: PluginContext,
}

pub(super) fn register(r: &mut Registry) {
    r.plan("plugins.op.plan", op_plan).long().destructive();
    r.apply("plugins.op.apply", PlanStore::None, |c, req: PlanPluginOp| {
        Ok(op_plan(c, req)?.plan()?.apply(None, c.ctx)?)
    })
    .long()
    .destructive();
}

/// The operation's plan, or the question it asks first.
fn op_plan(c: &Call<'_>, req: PlanPluginOp) -> Result<OpPlanned, RpcError> {
    let store = c.existing()?;
    Ok(host()?.plan_op(&req.plugin, &req.operation, req.params, req.context.into(), store.as_ref(), c.ctx)?)
}
