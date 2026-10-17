//! `plugins.*`: the plugins locallibrary has, their settings, and running what they add. Their
//! plans are kept apart from locallibrary's own (see [`super::super::Plans::plugins`]).

use std::path::PathBuf;

use locallibrary::edit::EditPlan;
use locallibrary::op::Planned;
use locallibrary::plugin::{Analyzed, CommandOutput, PluginExport};
use loclib_plugin::{Context, ExportSource, Exported};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{PlanStore, Registry};
use crate::plugins::{Described, Listed, Settings, host, left_out};

#[derive(Deserialize, JsonSchema)]
struct PluginRef {
    /// The plugin's id.
    plugin: String,
}

/// Where the user is, for a plugin's command or operation.
#[derive(Default, Deserialize, JsonSchema)]
struct PluginContext {
    /// A library's name, or a drive's mount path, name or id prefix. An operation needs one.
    #[serde(default)]
    drive: Option<String>,
    /// The playlist they're on, as `playlists.tree` names it.
    #[serde(default)]
    playlist: Option<String>,
    /// The tracks they chose, by id.
    #[serde(default)]
    tracks: Vec<u32>,
}

impl From<PluginContext> for Context {
    fn from(c: PluginContext) -> Context {
        Context { drive: c.drive, playlist: c.playlist, tracks: c.tracks, ..Context::default() }
    }
}

#[derive(Deserialize, JsonSchema)]
struct SetPluginSetting {
    plugin: String,
    key: String,
    /// Of the setting's type; `null` (or left out) puts it back to its default.
    #[serde(default)]
    value: Option<Value>,
}

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

#[derive(Deserialize, JsonSchema)]
struct PlanPluginOp {
    plugin: String,
    operation: String,
    /// As the operation's `params` schema says (`plugins.describe`).
    #[serde(default)]
    params: Value,
    context: PluginContext,
}

#[derive(Deserialize, JsonSchema)]
struct PlanPluginExport {
    plugin: String,
    /// The exporter's format.
    exporter: String,
    /// The library or drive to export.
    drive: String,
    /// Only these playlists (and folders), by path; every one when empty.
    #[serde(default)]
    playlists: Vec<String>,
    /// History lists to put in, by name.
    #[serde(default)]
    history: Vec<String>,
    /// The file, or folder, to write (absolute), on this computer: never on a drive or in the
    /// store.
    output: PathBuf,
    /// As the exporter's `options` schema says (`plugins.describe`).
    #[serde(default)]
    options: Value,
}

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
    r.read("plugins.list", |_, _: locallibrary::api::NoParams| {
        let plugins = host()?;
        let left_out =
            left_out().iter().map(|(m, why)| Listed { left_out: Some(why.clone()), ..Listed::of(m, plugins) });
        Ok(plugins.manifests().map(|m| Listed::of(m, plugins)).chain(left_out).collect::<Vec<_>>())
    })
    .quick();
    r.read("plugins.describe", |c, req: PluginRef| Ok(Described::of(&req.plugin, host()?, c.existing()?.as_ref())?))
        .quick();
    r.read("plugins.settings", |c, req: PluginRef| {
        let settings = host()?.settings(&req.plugin, c.existing()?.as_ref())?;
        Ok(Settings { plugin: req.plugin, settings })
    })
    .quick();
    r.immediate("plugins.settings.set", |c, req: SetPluginSetting| {
        let settings = host()?.set_setting(&req.plugin, &c.store()?, &req.key, req.value.filter(|v| !v.is_null()))?;
        Ok(Settings { plugin: req.plugin, settings })
    })
    .keeps_cache();
    r.read("plugins.run", |c, req: RunPluginCommand| -> Result<CommandOutput, _> {
        let store = c.existing()?;
        Ok(host()?.run_command(&req.plugin, &req.command, req.params, req.context.into(), store.as_ref(), c.ctx)?)
    })
    .long();
    r.plan("plugins.op.plan", op_plan).long().destructive();
    r.apply("plugins.op.apply", PlanStore::None, |c, req: PlanPluginOp| Ok(op_plan(c, req)?.apply(None, c.ctx)?))
        .long()
        .destructive();
    r.plan("plugins.export.plan", export_plan).long().destructive().writes_to("output");
    r.apply("plugins.export.apply", PlanStore::Existing, |c, req: PlanPluginExport| -> Result<Exported, _> {
        let plan = export_plan(c, req)?;
        Ok(plan.apply(c.existing()?.as_ref(), c.ctx)?)
    })
    .long()
    .destructive()
    .writes_to("output");
    r.immediate("plugins.analyze", |c, req: AnalyzeWithPlugin| -> Result<Analyzed, _> {
        let tracks = req.tracks.as_deref();
        Ok(host()?.analyze(&req.plugin, &req.analyzer, &req.drive, tracks, &c.store()?, c.ctx)?)
    })
    .long()
    .keeps_cache();
}

fn op_plan(c: &super::Call<'_>, req: PlanPluginOp) -> Result<Planned<EditPlan>, super::RpcError> {
    let store = c.existing()?;
    Ok(host()?.plan_op(&req.plugin, &req.operation, req.params, req.context.into(), store.as_ref(), c.ctx)?)
}

fn export_plan(c: &super::Call<'_>, req: PlanPluginExport) -> Result<Planned<PluginExport>, super::RpcError> {
    let source = ExportSource { drive: req.drive, playlists: req.playlists, history: req.history };
    let store = c.existing()?;
    Ok(host()?.export_plan(&req.plugin, &req.exporter, source, &req.output, req.options, store.as_ref(), c.ctx)?)
}
