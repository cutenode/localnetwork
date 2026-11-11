//! `plugins.run`: a plugin's command; and `plugins.command.*`, one planned first (`plan = true`),
//! planned (or the question it asks first) and applied.

use locallibrary::plugin::{CommandOutput, CommandPlanned};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::PluginContext;
use crate::plugins::host;
use crate::rpc::RpcError;
use crate::rpc::methods::{Call, PlanStore, Registry};

#[derive(Deserialize, JsonSchema)]
struct RunPluginCommand {
    plugin: String,
    command: String,
    /// As the command's `params` schema says (`plugins.describe`).
    #[serde(default)]
    params: Value,
    #[serde(default)]
    context: PluginContext,
}

pub(super) fn register(r: &mut Registry) {
    r.read("plugins.run", |c, req: RunPluginCommand| -> Result<CommandOutput, _> {
        let store = c.existing()?;
        let context = req.context.into();
        // Its rows sent in parts are in the view it answers.
        Ok(host()?.run_command(&req.plugin, &req.command, req.params, context, store.as_ref(), c.ctx, &|_| {})?)
    })
    .long();
    // What it writes, the plugin writes: the store only holds its folder for what it keeps.
    r.plan("plugins.command.plan", command_plan).long().destructive();
    r.apply("plugins.command.apply", PlanStore::Existing, |c, req: RunPluginCommand| {
        let store = c.existing()?;
        Ok(command_plan(c, req)?.plan()?.apply(store.as_ref(), c.ctx)?)
    })
    .long()
    .destructive();
}

/// The command's plan, or the question it asks first.
fn command_plan(c: &Call<'_>, req: RunPluginCommand) -> Result<CommandPlanned, RpcError> {
    let store = c.existing()?;
    Ok(host()?.plan_command(&req.plugin, &req.command, req.params, req.context.into(), store.as_ref(), c.ctx)?)
}
