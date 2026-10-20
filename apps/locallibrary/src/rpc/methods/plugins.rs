//! `plugins.*`: the plugins locallibrary has, their settings, running what they add (a command
//! or an operation may ask a question first), which analyzer each field role is read from,
//! writing an analyzer's key and BPM to the tracks, and what providers find for tracks. Their
//! plans are kept apart from locallibrary's own (see [`super::super::Plans::plugins`]).

use std::path::PathBuf;

use locallibrary::edit::EditPlan;
use locallibrary::fields::{Role, roles};
use locallibrary::op::Planned;
use locallibrary::plugin::{
    Analyzed, ApplyFields, CommandOutput, OpPlanned, PlanProvided, PluginExport, Provide, Provided, plan_provided,
    set_auto_pick,
};
use loclib_plugin::{Context, ExportSource, Exported};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{PlanStore, Registry};
use crate::plugins::{Described, Listed, Providers, Secrets, Settings, host, left_out};

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
    /// Of the setting's type (a secret's, text: kept in the system's keychain and never
    /// answered); `null` (or left out) puts it back to its default, or forgets a secret.
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

#[derive(Deserialize, JsonSchema)]
struct SetAutoPick {
    /// A candidate alone scoring this or more (0 to 1; more than 1: never) is picked for the user;
    /// `null` (or left out) puts it back to its default.
    #[serde(default)]
    auto_pick: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
struct ChooseRole {
    role: Role,
    /// The analyzer to read it from, `<plugin>.<analyzer>` (one with the role); `null` (or left
    /// out) puts it back to its default.
    #[serde(default)]
    analyzer: Option<String>,
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
        let store = c.existing()?;
        let settings = host()?.settings(&req.plugin, store.as_ref())?;
        let secrets = Secrets::of(host()?, &req.plugin, store.as_ref())?;
        Ok(Settings { plugin: req.plugin, settings, secrets })
    })
    .quick();
    r.immediate("plugins.settings.set", |c, req: SetPluginSetting| {
        let store = c.store()?;
        let settings = host()?.set_setting(&req.plugin, &store, &req.key, req.value.filter(|v| !v.is_null()))?;
        let secrets = Secrets::of(host()?, &req.plugin, Some(&store))?;
        Ok(Settings { plugin: req.plugin, settings, secrets })
    })
    .keeps_cache();
    r.read("plugins.run", |c, req: RunPluginCommand| -> Result<CommandOutput, _> {
        let store = c.existing()?;
        let context = req.context.into();
        // Its rows sent in parts are in the view it answers.
        Ok(host()?.run_command(&req.plugin, &req.command, req.params, context, store.as_ref(), c.ctx, &|_| {})?)
    })
    .long();
    r.plan("plugins.op.plan", op_plan).long().destructive();
    r.apply("plugins.op.apply", PlanStore::None, |c, req: PlanPluginOp| {
        Ok(op_plan(c, req)?.plan()?.apply(None, c.ctx)?)
    })
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
    // Overwriting what tracks have, when asked to.
    r.plan("plugins.apply_fields.plan", apply_fields_plan).long().destructive();
    r.apply("plugins.apply_fields.apply", PlanStore::None, |c, req: ApplyFields| {
        Ok(apply_fields_plan(c, req)?.apply(None, c.ctx)?)
    })
    .long()
    .destructive();
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
    r.read("plugins.roles", |c, _: locallibrary::api::NoParams| Ok(roles::choices(c.existing()?.as_ref()))).quick();
    r.immediate("plugins.roles.set", |c, req: ChooseRole| {
        Ok(roles::choose(&c.store()?, req.role, req.analyzer.as_deref())?)
    })
    .keeps_cache();
}

fn apply_fields_plan(c: &super::Call<'_>, req: ApplyFields) -> Result<Planned<EditPlan>, super::RpcError> {
    Ok(host()?.plan_apply_fields(&req, &c.store()?, c.ctx)?)
}

/// The operation's plan, or the question it asks first.
fn op_plan(c: &super::Call<'_>, req: PlanPluginOp) -> Result<OpPlanned, super::RpcError> {
    let store = c.existing()?;
    Ok(host()?.plan_op(&req.plugin, &req.operation, req.params, req.context.into(), store.as_ref(), c.ctx)?)
}

fn provide_plan(c: &super::Call<'_>, req: PlanProvided) -> Result<Planned<EditPlan>, super::RpcError> {
    Ok(plan_provided(&req, c.ctx)?)
}

fn export_plan(c: &super::Call<'_>, req: PlanPluginExport) -> Result<Planned<PluginExport>, super::RpcError> {
    let source =
        ExportSource { drive: req.drive, playlists: req.playlists, history: req.history, ..ExportSource::default() };
    let store = c.existing()?;
    Ok(host()?.export_plan(&req.plugin, &req.exporter, source, &req.output, req.options, store.as_ref(), c.ctx)?)
}
