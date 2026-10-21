//! `plugins.run`: a plugin's command.

use locallibrary::plugin::CommandOutput;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::PluginContext;
use crate::plugins::host;
use crate::rpc::methods::Registry;

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
}
