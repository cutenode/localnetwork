//! Plugins' commands (`command/run`): headless ones answer any JSON, view ones a [`View`] each
//! host draws its own way (read as panels' are, see `panels`), its rows (or items) shown as they
//! come when the plugin sends them in parts, or first a question for the user ([`View::Choose`],
//! the `choose` feature), after which the host runs the command again with what was chosen.

use loclib_plugin::{Context, Mode, Partial, RunCommand, View, method};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::Plugins;
use super::blobs::Scratch;
use super::process::Live;
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
    /// params; one from the selection, `x-from`, that isn't given is filled in from `context`),
    /// where the user is (`context`, its drive by id, name or path as the CLI takes it). A view
    /// command's rows (or items) sent in parts go to `shown` as they come; the view it answers
    /// has them too, first. A question it answers ([`View::Choose`]) is one the user can answer
    /// with a param it takes: answered, run it again with that param ([`loclib_plugin::Choice::answered`]).
    #[allow(clippy::too_many_arguments)]
    pub fn run_command(
        &self,
        plugin: &str,
        command: &str,
        params: Value,
        context: Context,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
        shown: &dyn Fn(Partial),
    ) -> Result<CommandOutput> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared("command", command, &slot.manifest.contributes.commands, |c| &c.name)?;
        let (context, _, _given) = slot.context(context, store)?;
        let params = super::fitting(declared.params.as_ref(), params, &context)?;
        let run = RunCommand { command: command.into(), params, context };
        let shown = |partial| {
            if declared.mode == Mode::View {
                shown(partial);
            }
        };
        let read = |answer: Value, scratch: &Scratch, live: &Live| {
            if declared.mode == Mode::Headless {
                return Ok(CommandOutput::Headless(answer));
            }
            let view = slot.view(command, answer, scratch, live)?;
            if let View::Choose(choice) = &view {
                slot.askable(choice, declared.params.as_ref(), &run.params, live)?;
            }
            Ok(CommandOutput::View(view))
        };
        slot.call_with(method::RUN_COMMAND, &run, store, ctx, &shown, read)
    }
}
