//! Plugins' commands (`command/run`): headless ones answer any JSON, view ones a [`View`] each
//! host draws its own way.

use loclib_plugin::{Context, Mode, RunCommand, View, method};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::Plugins;
use crate::Result;
use crate::progress::Ctx;
use crate::store::Store;

/// What a command answers.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CommandOutput {
    /// A headless command's: shown as it is (`--json`), or not at all.
    Headless(Value),
    View(View),
}

impl Plugins {
    /// Runs one of a plugin's commands with `params` (checked against its schema; `null` is no
    /// params), where the user is (`context`, its drive by id, name or path as the CLI takes it).
    pub fn run_command(
        &self,
        plugin: &str,
        command: &str,
        params: Value,
        context: Context,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<CommandOutput> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared("command", command, &slot.manifest.contributes.commands, |c| &c.name)?;
        let params = super::fitting(declared.params.as_ref(), params)?;
        let (context, _) = slot.context(context, store)?;
        let run = RunCommand { command: command.into(), params, context };
        let answer: Value = slot.call(method::RUN_COMMAND, &run, ctx)?;
        match declared.mode {
            Mode::Headless => Ok(CommandOutput::Headless(answer)),
            Mode::View => serde_json::from_value(answer)
                .map(CommandOutput::View)
                .map_err(|e| slot.misbehaved(format!("its view of {command} isn't one: {e}"))),
        }
    }
}
