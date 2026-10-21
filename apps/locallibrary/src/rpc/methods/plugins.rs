//! `plugins.*`: the plugins locallibrary has and their settings, and what each family of hooks adds
//! (see loclib-plugin's `hooks`), registered from a module of its own. Their plans are kept apart
//! from locallibrary's own (see [`super::super::Plans::plugins`]).

mod analyzers;
mod commands;
mod exporters;
mod importers;
mod operations;
mod providers;
mod roles;

use loclib_plugin::Context;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::Registry;
use crate::plugins::{Described, Listed, Secrets, Settings, host, left_out};

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
    /// The set they're on, by id (`sets.list`).
    #[serde(default)]
    set: Option<String>,
}

impl From<PluginContext> for Context {
    fn from(c: PluginContext) -> Context {
        Context { drive: c.drive, playlist: c.playlist, tracks: c.tracks, set: c.set, ..Context::default() }
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
    // Each family's, in the order the contract lists them (a family's placed as loclib-plugin's
    // `hooks` says).
    commands::register(r);
    operations::register(r);
    exporters::register(r);
    importers::register(r);
    analyzers::register(r);
    providers::register(r);
    roles::register(r);
}
