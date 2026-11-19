//! Plugins' commands (`command/run`): headless ones answer any JSON, view ones a [`View`] each
//! host draws its own way (read as panels' are, see `panels`), its rows (or items) shown as they
//! come when the plugin sends them in parts, or first a question for the user ([`View::Choose`],
//! the `choose` feature), after which the host runs the command again with what was chosen. A
//! command planned first (`plan = true`) is asked for a dry run, whose plan (the files it would
//! write, as an exporter says them) is confirmed and then applied by running the same request
//! without `dry_run`.

use std::path::PathBuf;
use std::sync::{Arc, Weak};

use loclib_plugin::{
    Choice, Command, CommandPlan, CommandPlanAnswer, Context, Mode, Partial, RunCommand, View, method,
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::blobs::Scratch;
use super::exports::{ExportFile, size_said};
use super::operations::unasked;
use super::process::{Live, Slot};
use super::{Cli, Plugins};
use crate::op::{Operation, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

/// What a command answers.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CommandOutput {
    /// A headless command's: shown as it is (`--json`), or not at all.
    Headless(Value),
    View(View),
}

/// What planning a plugin's command comes to: its plan, or a question for the user first, after
/// which it's planned again with each of its params set ([`Choice::answered`]).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
// Made once a run, and the plan taken out at once: boxing it would only make each caller unbox it.
#[allow(clippy::large_enum_variant)]
pub enum CommandPlanned {
    Choose { choose: Choice },
    Plan(Planned<PluginCommand>),
}

impl CommandPlanned {
    /// The plan, for a caller that can't ask: a question is refused, saying how to answer it.
    pub fn plan(self) -> Result<Planned<PluginCommand>> {
        match self {
            CommandPlanned::Plan(plan) => Ok(plan),
            CommandPlanned::Choose { choose } => Err(unasked(&choose)),
        }
    }
}

/// A plugin's command, planned: what it said it would do, to run once confirmed.
#[derive(Serialize, JsonSchema)]
pub struct PluginCommand {
    pub plugin: String,
    pub command: String,
    /// The files it would write, as the plugin says, each with whether it's there already.
    pub files: Vec<ExportFile>,
    /// The request its dry run was, but for `dry_run`.
    #[serde(skip)]
    #[schemars(skip)]
    run: RunCommand,
    /// The plugin, while it's hosted: a plan kept doesn't keep it.
    #[serde(skip)]
    #[schemars(skip)]
    slot: Weak<Slot>,
    /// The drive it's given again while it runs (see [`Slot::give`]).
    #[serde(skip)]
    #[schemars(skip)]
    given: Option<(String, PathBuf)>,
}

impl std::fmt::Debug for PluginCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let PluginCommand { plugin, command, files, run, .. } = self;
        f.debug_struct("PluginCommand")
            .field("plugin", plugin)
            .field("command", command)
            .field("files", files)
            .field("run", run)
            .finish_non_exhaustive()
    }
}

/// `context` as the command `declared` is run with: one about the collection alone runs on no
/// drive, whichever the caller was on (the command line gives none, the player's palette drops
/// the drive open, and a sidecar client may send one).
fn placed(declared: &Command, context: Context) -> Context {
    match Cli::Command(declared).collection_only() {
        true => Context { drive: None, ..context },
        false => context,
    }
}

impl Plugins {
    /// Runs one of a plugin's commands with `params` (checked against its schema; `null` is no
    /// params; one from the selection, `x-from`, that isn't given is filled in from `context`),
    /// where the user is (`context`, its drive by id, name or path as the CLI takes it). A view
    /// command's rows (or items) sent in parts go to `shown` as they come; the view it answers
    /// has them too, first. A question it answers ([`View::Choose`]) is one the user can answer
    /// with a param it takes: answered, run it again with that param ([`loclib_plugin::Choice::answered`]).
    /// One about the collection alone (`where = ["collection"]`) runs on no drive. A command
    /// planned first is refused: [`Plugins::plan_command`] plans it.
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
        if declared.plan {
            return Err(Error::invalid(format!("{command} is planned first: plan it, then apply the plan")));
        }
        let (context, _, _given) = slot.context(placed(declared, context), store)?;
        let params = super::fitting(declared.params.as_ref(), params, &context)?;
        let run = RunCommand { command: command.into(), params, dry_run: false, context };
        slot.command(declared, &run, store, ctx, shown)
    }

    /// Plans one of a plugin's commands planned first (`plan = true`), with `params` and `context`
    /// as [`Plugins::run_command`] takes them: the plugin's dry run says what it would do and which
    /// files it would write, doing nothing. The plan (operation `<plugin>.command.<name>`) leads
    /// with its summary, is destructive when it would overwrite a file, and is stale once one of
    /// its files changes. Applying it runs the same request without `dry_run`, answering what the
    /// command answers. Or the question it asks first, one the user can answer with a param it
    /// takes.
    pub fn plan_command(
        &self,
        plugin: &str,
        command: &str,
        params: Value,
        context: Context,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<CommandPlanned> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared("command", command, &slot.manifest.contributes.commands, |c| &c.name)?;
        if !declared.plan {
            return Err(Error::invalid(format!("{command} isn't planned first: run it")));
        }
        let (context, device, _given) = slot.context(placed(declared, context), store)?;
        let given = context.drive.clone().zip(device.map(|d| d.root().to_path_buf()));
        let params = super::fitting(declared.params.as_ref(), params, &context)?;
        let run = RunCommand { command: command.into(), params, dry_run: true, context };
        let planned = |answer: Value, _: &Scratch, live: &Live| {
            let answer: CommandPlanAnswer = serde_json::from_value(answer)
                .map_err(|e| slot.misbehaved(format!("its plan for {command} isn't one: {e}")))?;
            match answer {
                CommandPlanAnswer::Plan(plan) => {
                    Ok(CommandPlanned::Plan(slot.command_plan(&run, plan, given.clone())?))
                }
                CommandPlanAnswer::Choose { choose } => {
                    slot.askable(&choose, declared.params.as_ref(), &run.params, live)?;
                    Ok(CommandPlanned::Choose { choose })
                }
            }
        };
        slot.call_with(method::RUN_COMMAND, &run, store, ctx, &|_| {}, planned)
    }
}

impl Slot {
    /// Runs `run`, the command `declared`, reading what it answers: a view's rows sent in parts
    /// go to `shown`, and a question it asks must be one the user can answer, never once its plan
    /// was confirmed.
    fn command(
        self: &Arc<Self>,
        declared: &Command,
        run: &RunCommand,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
        shown: &dyn Fn(Partial),
    ) -> Result<CommandOutput> {
        let shown = |partial| {
            if declared.mode == Mode::View {
                shown(partial);
            }
        };
        let read = |answer: Value, scratch: &Scratch, live: &Live| {
            if declared.mode == Mode::Headless {
                return Ok(CommandOutput::Headless(answer));
            }
            let view = self.view(&run.command, answer, scratch, live)?;
            if let View::Choose(choice) = &view {
                if declared.plan {
                    return Err(self.misbehaved("it asks a question once its plan was confirmed: ask in the dry run"));
                }
                self.askable(choice, declared.params.as_ref(), &run.params, live)?;
            }
            Ok(CommandOutput::View(view))
        };
        self.call_with(method::RUN_COMMAND, run, store, ctx, &shown, read)
    }

    /// The plan of `run` (a dry run), as the plugin answered it: its files each absolute, the
    /// drive it was on (`given`) to give it again when it runs.
    fn command_plan(
        self: &Arc<Self>,
        run: &RunCommand,
        plan: CommandPlan,
        given: Option<(String, PathBuf)>,
    ) -> Result<Planned<PluginCommand>> {
        let CommandPlan { summary, warnings, files } = plan;
        if let Some(f) = files.iter().find(|f| !f.path.is_absolute()) {
            return Err(self.misbehaved(format!("it would write {}, which isn't an absolute path", f.path.display())));
        }
        let files: Vec<ExportFile> = files.into_iter().map(ExportFile::of).collect();
        let overwrites = files.iter().filter(|f| f.exists).count();
        let mut lines = vec![summary];
        if !files.is_empty() {
            lines.push(format!("write {} file(s){}", files.len(), size_said(self, &files)?));
        }
        if overwrites > 0 {
            lines.push(format!("overwrite {overwrites} of them"));
        }
        // Each by its size and time: there or not as it was when planned, or the plan is stale.
        let touched: Vec<Touched> =
            files.iter().map(|f| Touched::path(&f.file.path, true, vec![Probe::Stamp(vec![String::new()])])).collect();
        let (plugin, command) = (self.manifest.id.clone(), run.command.clone());
        let operation = format!("{plugin}.command.{command}");
        let run = RunCommand { dry_run: false, ..run.clone() };
        let planned = PluginCommand { plugin, command, files, run, slot: Arc::downgrade(self), given };
        let planned =
            Planned::new(operation, planned, ()).summary(lines).warnings(warnings).destructive(overwrites > 0);
        Ok(touched.into_iter().fold(planned, Planned::touches))
    }
}

impl Operation for PluginCommand {
    type Request = ();
    type Report = CommandOutput;

    /// Runs the command as its dry run was asked, on the drive it was on.
    fn apply(planned: &Planned<Self>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<CommandOutput> {
        let c = &planned.plan;
        let slot = c.slot.upgrade().ok_or_else(|| Error::stale(format!("the {} plugin", c.plugin), "stopped"))?;
        let declared = slot.declared("command", &c.command, &slot.manifest.contributes.commands, |x| &x.name)?;
        let _given = slot.give(c.given.clone());
        slot.command(declared, &c.run, store, ctx, &|_| {})
    }
}
