//! The plugin host: plugins are programs locallibrary runs and talks to over their stdin and
//! stdout ([`loclib_plugin`]'s protocol), adding commands, operations, track analyzers, exporters
//! and settings. The CLI, the TUI and the RPC sidecar host them through [`Plugins`].
//!
//! Only plugins locallibrary ships run for now ([`PluginSpec::first_party`]): the app hands the
//! host each one's manifest, compiled in, and how to start it (the locallibrary binary itself,
//! `plugin serve <id>`). A plugin starts when it's first used, and must speak the protocol and
//! describe itself exactly as its manifest does; it's stopped once it has been idle a while,
//! started again after it crashed, and killed when it doesn't stop as asked. None starts from an
//! elevated process.
//!
//! Plugins never write to a drive. They read the library by calling back for the sidecar's read
//! methods their capabilities grant, within limits (see [`Reads`]); an operation's edits are
//! planned and applied by the host as any edit plan is, and an exporter's overwrites are
//! confirmed first. Analyzers' values go to the [`crate::fields`] store.

mod analyzers;
mod callbacks;
mod commands;
mod exports;
mod operations;
mod process;
mod settings;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use loclib_plugin::{Command, Manifest, Operation, SettingType, check_params};
use rustc_hash::FxHashSet;
use serde_json::{Map, Value};

use crate::fields::Analyzer;
use crate::{Error, Result};

pub use analyzers::Analyzed;
pub use callbacks::Reads;
pub use commands::CommandOutput;
pub use exports::{ExportFile, PluginExport};

use process::Slot;
pub(crate) use process::THREAD_ELEVATED;

/// A plugin's command or operation, as the command line runs it.
#[derive(Debug, Clone, Copy)]
pub enum Cli<'a> {
    Command(&'a Command),
    Operation(&'a Operation),
}

/// How the app starts a plugin: a program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Environment variables it's started without: those that would have it do something else
    /// first (answer a shell's completion request, say).
    pub env_remove: Vec<OsString>,
}

/// A plugin the app hands the host.
#[derive(Debug, Clone)]
pub struct PluginSpec {
    pub manifest: Manifest,
    pub launch: Launch,
    /// locallibrary ships it: the only plugins that run for now.
    pub first_party: bool,
}

impl PluginSpec {
    /// A plugin locallibrary ships, from its manifest (`locallibrary-plugin.toml`, compiled in).
    pub fn first_party(manifest: &str, launch: Launch) -> Result<PluginSpec> {
        let manifest = Manifest::parse(manifest).map_err(|e| Error::invalid(format!("a plugin's manifest: {e}")))?;
        manifest.validate(true).map_err(|e| Error::invalid(format!("the {} plugin's manifest: {e}", manifest.id)))?;
        Ok(PluginSpec { manifest, launch, first_party: true })
    }
}

/// How the host runs plugins.
#[derive(Clone)]
pub struct Options {
    /// How long a plugin with nothing to do keeps running.
    pub idle: Duration,
    /// How long a plugin has to answer `hello` and `describe` once started.
    pub handshake: Duration,
    /// How long a plugin has to answer a request it was asked to cancel, and to exit once it's
    /// stopped (idle, or at [`Plugins::shutdown`]), before it's killed.
    pub grace: Duration,
    /// The most read callbacks one plugin may have under way at once: more are refused.
    pub callbacks: usize,
    /// The most tracks analysed at once.
    pub analyses: usize,
    /// Where plugins' logs go: what they write to stderr (without terminal escapes), and what
    /// the host has to say about them. A front end drawing on the terminal gives its own.
    pub log: Log,
}

/// Takes a line about a plugin, with the plugin's id.
pub type Log = Arc<dyn Fn(&str, &str) + Send + Sync>;

impl Default for Options {
    fn default() -> Options {
        Options {
            idle: Duration::from_secs(60),
            handshake: Duration::from_secs(10),
            grace: Duration::from_secs(5),
            callbacks: 8,
            analyses: 4,
            log: Arc::new(|id, line| eprintln!("{id}: {line}")),
        }
    }
}

/// The plugins the app has, by id.
pub struct Plugins {
    plugins: IndexMap<String, Arc<Slot>>,
}

impl Plugins {
    /// The host for `specs`, reading for them through `reads`. Refused: a plugin locallibrary
    /// doesn't ship, an invalid manifest, one with a secret setting (none can be kept yet), two
    /// plugins with one id, and two commands (or operations) with one `cli` place (the app checks
    /// them against its own commands).
    pub fn new(specs: Vec<PluginSpec>, reads: Arc<dyn Reads>, options: Options) -> Result<Plugins> {
        let host = Arc::new(process::Host { reads, options });
        let mut plugins = IndexMap::new();
        let mut cli: FxHashSet<String> = FxHashSet::default();
        for spec in specs {
            let m = &spec.manifest;
            if !spec.first_party {
                return Err(Error::invalid(format!("{}: third-party plugins aren't supported yet", m.id)));
            }
            m.validate(true).map_err(|e| Error::invalid(format!("the {} plugin's manifest: {e}", m.id)))?;
            if let Some(s) = m.contributes.settings.iter().find(|s| s.kind == SettingType::Secret) {
                return Err(Error::invalid(format!(
                    "the {} plugin's setting {:?} is a secret, which locallibrary can't keep yet",
                    m.id, s.key
                )));
            }
            let commands = m.contributes.commands.iter().filter_map(|c| c.cli.as_deref());
            let operations = m.contributes.operations.iter().filter_map(|o| o.cli.as_deref());
            if let Some(name) = commands.chain(operations).find(|n| !cli.insert(n.to_string())) {
                return Err(Error::invalid(format!("the {} plugin's command {name:?} is another plugin's too", m.id)));
            }
            if plugins.contains_key(&m.id) {
                return Err(Error::invalid(format!("two plugins are called {:?}", m.id)));
            }
            let id = m.id.clone();
            plugins.insert(id, Arc::new(Slot::new(spec.manifest, spec.launch, host.clone())));
        }
        Ok(Plugins { plugins })
    }

    /// Each plugin's manifest, in the order the app gave them.
    pub fn manifests(&self) -> impl Iterator<Item = &Manifest> {
        self.plugins.values().map(|s| &s.manifest)
    }

    pub fn manifest(&self, plugin: &str) -> Result<&Manifest> {
        Ok(&self.slot(plugin)?.manifest)
    }

    /// The command or operation with a place of its own on the command line (`import`,
    /// `playlist import`), with its plugin's id.
    pub fn cli(&self, place: &str) -> Option<(&str, Cli<'_>)> {
        self.manifests().find_map(|m| {
            let c = &m.contributes;
            let command = c.commands.iter().find(|c| c.cli.as_deref() == Some(place)).map(Cli::Command);
            let operation = || c.operations.iter().find(|o| o.cli.as_deref() == Some(place)).map(Cli::Operation);
            Some((m.id.as_str(), command.or_else(operation)?))
        })
    }

    /// Every analyzer, at the version installed: whose values [`crate::api::track_fields_of`]
    /// reads.
    pub fn analyzers(&self) -> Vec<Analyzer> {
        self.manifests()
            .flat_map(|m| {
                m.contributes.analyzers.iter().map(|a| Analyzer {
                    plugin: m.id.clone(),
                    version: m.version.clone(),
                    analyzer: a.name.clone(),
                })
            })
            .collect()
    }

    /// Every analyzer's fields, by name as filters and sorts name them (`sound.bpm`), with their
    /// types: in the order the plugins, and their analyzers, declare them.
    pub fn fields(&self) -> crate::fields::Declared {
        let all = self.manifests().flat_map(|m| m.contributes.analyzers.iter().map(move |a| (m, a)));
        all.flat_map(|(m, a)| a.fields.iter().map(move |(name, &kind)| (format!("{}.{name}", m.id), kind))).collect()
    }

    /// Whether the plugin is running now. Never waits.
    pub fn is_running(&self, plugin: &str) -> bool {
        self.slot(plugin).is_ok_and(|s| s.is_running())
    }

    /// Stops every plugin running, all at once, each given its grace to exit before it's killed
    /// (a hook under way holds its plugin until it's answered). They start again when used. One
    /// already stopping (idle, or another caller's shutdown) is waited for: once this returns,
    /// none is left stopping, so the host can exit.
    pub fn shutdown(&self) {
        std::thread::scope(|s| {
            for slot in self.plugins.values() {
                s.spawn(|| slot.stop());
            }
        });
    }

    fn slot(&self, plugin: &str) -> Result<&Arc<Slot>> {
        self.plugins.get(plugin).ok_or_else(|| Error::NotFound(format!("plugin {plugin:?}")))
    }
}

impl Slot {
    /// The plugin's contribution of a kind (`what`) named `name`, among `all` (named by
    /// `named`).
    fn declared<'a, T>(&self, what: &str, name: &str, all: &'a [T], named: fn(&T) -> &str) -> Result<&'a T> {
        let id = &self.manifest.id;
        all.iter()
            .find(|c| named(c) == name)
            .ok_or_else(|| Error::NotFound(format!("the {id} plugin's {what} {name:?}")))
    }
}

/// `params` as a hook takes them, once they fit `schema` (a contribution's `params` or
/// `options`): `null` is none. Where they don't is `fields.path`, as the sidecar says it.
fn fitting(schema: Option<&Value>, params: Value) -> Result<Value> {
    let params = if params.is_null() { Value::Object(Map::new()) } else { params };
    check_params(schema, &params).map_err(|e| {
        let path = e.data.and_then(|d| d.fields.get("path").cloned());
        let invalid = Error::Invalid(e.message);
        match path {
            Some(path) => invalid.with("path", path),
            None => invalid,
        }
    })?;
    Ok(params)
}
