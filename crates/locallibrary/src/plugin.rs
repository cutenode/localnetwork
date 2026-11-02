//! The plugin host: plugins are programs locallibrary runs and talks to over their stdin and
//! stdout ([`loclib_plugin`]'s protocol), adding commands, operations, panels, track analyzers,
//! checks, exporters, importers, providers, accounts, destinations, services, library sources,
//! themes and settings, and listening for events. The CLI, the TUI and the RPC sidecar host them
//! through [`Plugins`].
//!
//! Only plugins locallibrary ships run for now ([`PluginSpec::built_in`]): the app hands the
//! host each one's manifest, compiled in, and how to start it (the locallibrary binary itself,
//! `plugin serve <id>`). A plugin starts when it's first used, in a folder of its own in the store,
//! and must speak the protocol, describe itself exactly as its manifest does and have the features
//! (kinds of hooks) what it contributes needs; it's stopped once it has been idle a while,
//! started again after it crashed, and killed when it doesn't stop as asked. None starts from an
//! elevated process.
//!
//! Plugins never write to a drive. They read the library by calling back for the sidecar's read
//! methods their capabilities grant, within limits (see [`Reads`]), and reach the servers their
//! `net:` capabilities grant through the host (`net/fetch`), which fills in their secret settings
//! (kept in the keychain) and the tokens of the accounts they're given access to (signed in to by
//! the host, see [`Plugins::begin_account_sign_in`]); an operation's edits are planned and applied
//! by the host as any edit plan is, and an exporter's overwrites are confirmed first. Answers too
//! big for a message are files in a folder the host makes for each request (`scratch`), which it
//! reads itself.
//! Analyzers' values go to the [`crate::fields`] store, their beat grids, phrases and waveforms
//! to its overlay store. What providers find for tracks, the host offers the user and makes the
//! edits of itself (see [`Plugins::provide`]), as it does of what importers read (see
//! [`Plugins::import_plan`]). A library source's tracks are copied to a drive only from the
//! folders the user grants it, each file checked by the host (see [`Plugins::plan_source_export`]).
//! What checks find joins the check before a set and doctor, and guards say what they make of an
//! edit plan before it's confirmed (see [`Plugins::run_checks`], [`Plugins::guard`]). A store
//! destination keeps a copy of the store, which the host alone decides the content of and checks
//! what it fetches back from ([`Plugins::store_destination`]); a set destination publishes a set
//! once the user confirms, and the host keeps its link ([`Plugins::publish_plan`]). Plugins are
//! told of what happens as the front ends notice it ([`Plugins::deliver`]), and services run while
//! a front end that stays open does ([`Plugins::run_services`]); what either tells the user, the
//! front end shows ([`Options::notices`]), and nothing they suggest is planned unless the user
//! chooses to.

mod accounts;
mod analysis;
mod analyzers;
mod apply;
mod blobs;
mod callbacks;
mod checks;
mod commands;
mod destinations;
mod events;
mod exports;
mod imports;
mod net;
mod operations;
mod panels;
mod process;
mod providers;
mod secrets;
mod services;
mod settings;
mod sources;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use indexmap::IndexMap;
use loclib_plugin::{Command, Context, Manifest, Operation, Selection, check_params};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{Map, Value};

use crate::fields::Analyzer;
use crate::fields::roles::{Offered, RoleField};
use crate::keychain::Storage;
use crate::model::{Library, Track};
use crate::{Error, Result};

pub use accounts::REFRESH_BEFORE;
pub use accounts::access::{Access, AccountId};
pub use accounts::ready::Unready;
pub use accounts::shared::{Account, AccountSignIn, AccountSignedIn, AccountSignedOut, Accounts};
pub use analysis::{AnalysisPart, ApplyAnalysis, same_grid};
pub use analyzers::Analyzed;
pub use apply::ApplyFields;
pub(crate) use blobs::abandoned as abandoned_scratch;
pub use callbacks::Reads;
pub(crate) use checks::hosted;
pub use checks::{
    CheckChange, CheckSettings, Fix, MAX_TIMEOUT, MIN_TIMEOUT, PluginFix, TIMEOUT, check_timeout, set_check_timeout,
    set_hosted,
};
pub use commands::CommandOutput;
pub use destinations::{SetPublish, SetPublished, StoreDestination, destination_spec};
pub use events::{
    Event, MAX_SUGGESTED, PluginNotice, Suggestions, forget_suggested, keep_suggested, suggested, tracks_on,
};
pub use exports::{ExportFile, PluginExport};
pub use imports::{PluginImport, PluginImported};
pub use operations::OpPlanned;
pub use providers::{
    AUTO_PICK, Found, Offer, Pick, PlanProvided, Provide, Provided, auto_pick, plan_provided, set_auto_pick,
};
pub(crate) use settings::{all_saved_settings, host_settings, save_host_settings, save_settings, set_host_setting};
pub use sources::SourceExport;

pub(crate) use process::THREAD_ELEVATED;
use process::{Plugin, Slot};

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
    pub built_in: bool,
}

impl PluginSpec {
    /// A plugin locallibrary ships, from its manifest (`locallibrary-plugin.toml`, compiled in),
    /// which is checked only in builds with debug assertions (the tests'): it's the same on every
    /// start, and checking it compiles its JSON Schemas against the 2020-12 meta-schema, which
    /// added about a millisecond to every run of the CLI, `--version` included.
    pub fn built_in(manifest: &str, launch: Launch) -> Result<PluginSpec> {
        let manifest = Manifest::parse(manifest).map_err(|e| Error::invalid(format!("a plugin's manifest: {e}")))?;
        check_built_in(&manifest)?;
        Ok(PluginSpec { manifest, launch, built_in: true })
    }
}

/// [`Manifest::validate`] for a plugin locallibrary ships, in builds with debug assertions (see
/// [`PluginSpec::built_in`]). The app's tests check each manifest it ships.
fn check_built_in(m: &Manifest) -> Result<()> {
    if !cfg!(debug_assertions) {
        return Ok(());
    }
    m.validate(true).map_err(|e| Error::invalid(format!("the {} plugin's manifest: {e}", m.id)))
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
    /// The most a request's scratch folder may hold, in bytes: its answer's files, and what
    /// `net/fetch` wrote there. And the most the images providers find for a [`Plugins::provide`]
    /// may come to in base64, all of them: past it the rest are left out.
    pub max_blobs: u64,
    /// The most candidates a [`Plugins::provide`] keeps for a track, from all its providers: past
    /// it the rest are left out.
    pub track_candidates: usize,
    /// The most candidates a [`Plugins::provide`] keeps for all the tracks: past it the rest are
    /// left out.
    pub candidates: usize,
    /// The most of a `net/fetch` answer given in a message, in bytes: a bigger one goes to a file
    /// in the scratch folder (`into_blob`).
    pub max_body: u64,
    /// Where plugins' secret settings are kept.
    pub secrets: Storage,
    /// `net/fetch` may reach servers on this computer (over http too), for tests: never in a
    /// build without debug assertions, whatever this says.
    pub test_loopback: bool,
    /// Plugins reach no server but this computer's, for tests (`LOCALLIBRARY_TEST_OFFLINE` in the
    /// CLI's), so none reaches MusicBrainz: only in builds with debug assertions.
    pub test_offline: bool,
    /// Where plugins' logs go: what they write to stderr (without terminal escapes), and what
    /// the host has to say about them. A front end drawing on the terminal gives its own.
    pub log: Log,
    /// The most events waiting to be sent to one plugin: past it the oldest is dropped.
    pub events: usize,
    /// What plugins tell the user (`host/notify`), and that a service was left off: each front
    /// end shows them its own way. Called on the thread reading the plugin, so it mustn't wait.
    pub notices: Notices,
    /// How many times in a row a service may stop or fail before it's left off.
    pub restarts: u32,
    /// How long a service that stopped waits to be started again, the first time: twice that the
    /// next, and so on.
    pub backoff: Duration,
}

/// Takes a line about a plugin, with the plugin's id.
pub type Log = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Takes what a plugin tells the user.
pub type Notices = Arc<dyn Fn(PluginNotice) + Send + Sync>;

/// What the switch turning one of a plugin's services on is named with, among its settings:
/// `services.<name>`.
pub const SERVICES: &str = "services.";

/// What the player's key for one of a plugin's commands or operations is named with, among its
/// settings: `keys.<name>`.
pub const KEYS: &str = "keys.";

impl Default for Options {
    fn default() -> Options {
        Options {
            idle: Duration::from_secs(60),
            handshake: Duration::from_secs(10),
            grace: Duration::from_secs(5),
            callbacks: 8,
            analyses: 4,
            max_blobs: 512 << 20,
            track_candidates: 25,
            candidates: 50_000,
            max_body: 8 << 20,
            secrets: Storage::from_env(),
            test_loopback: false,
            test_offline: false,
            log: Arc::new(|id, line| eprintln!("{id}: {line}")),
            events: 64,
            notices: Arc::new(|n| eprintln!("{}: {}", n.plugin, n.message)),
            restarts: 5,
            backoff: Duration::from_secs(1),
        }
    }
}

/// The plugins the app has, by id.
pub struct Plugins {
    plugins: IndexMap<String, Arc<Slot>>,
    /// What every plugin is hosted with.
    host: Arc<process::Host>,
    /// Sign-ins to accounts of a type waiting for the browser.
    signing_in: accounts::SigningIn,
}

impl Plugins {
    /// The host for `specs` (their manifests' text made printable, as all a plugin tells the host
    /// is), reading for them through `reads`. Refused: a plugin locallibrary
    /// doesn't ship, an invalid manifest (in builds with debug assertions: see
    /// [`PluginSpec::built_in`]), two plugins with one id, and two commands (or operations) with
    /// one `cli` place (the app checks them against its own commands).
    pub fn new(specs: Vec<PluginSpec>, reads: Arc<dyn Reads>, options: Options) -> Result<Plugins> {
        let host =
            Arc::new(process::Host { reads, options, turns: Mutex::default(), account_plugins: OnceLock::new() });
        let mut plugins = IndexMap::new();
        let mut cli: FxHashSet<String> = FxHashSet::default();
        for spec in specs {
            let manifest = process::printable_manifest(spec.manifest)
                .map_err(|e| Error::invalid(format!("a plugin's manifest: {e}")))?;
            let spec = PluginSpec { manifest, ..spec };
            let m = &spec.manifest;
            if !spec.built_in {
                return Err(Error::invalid(format!("{}: third-party plugins aren't supported yet", m.id)));
            }
            check_built_in(m)?;
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
        let account_plugins = plugins.iter().filter(|(_, s)| s.manifest.is_account_plugin());
        let account_plugins = account_plugins.map(|(id, s)| (id.clone(), Arc::downgrade(&s.plugin))).collect();
        // Set here only, once every plugin is.
        let _ = host.account_plugins.set(account_plugins);
        Ok(Plugins { plugins, host, signing_in: Default::default() })
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

    /// Every analyzer at the version installed, with what its manifest declares.
    fn installed(&self) -> impl Iterator<Item = (Analyzer, &loclib_plugin::Analyzer)> {
        self.manifests().flat_map(|m| {
            m.contributes
                .analyzers
                .iter()
                .map(|a| (Analyzer { plugin: m.id.clone(), analyzer: a.name.clone(), version: a.version.clone() }, a))
        })
    }

    /// Every analyzer, at the version installed: whose values [`crate::api::track_fields_of`]
    /// reads.
    pub fn analyzers(&self) -> Vec<Analyzer> {
        self.installed().map(|(a, _)| a).collect()
    }

    /// The analyzers' fields with each role, in the order the app gave the plugins: what it tells
    /// the readers ([`crate::fields::roles::set_offered`]).
    pub fn roles(&self) -> Offered {
        let mut offered = Offered::new();
        for (analyzer, declared) in self.installed() {
            for (role, field) in &declared.roles {
                let field = RoleField { analyzer: analyzer.clone(), field: field.clone() };
                offered.entry(*role).or_default().push(field);
            }
        }
        offered
    }

    /// How the plugin's analyzers say their field `field` is shown, if they say.
    pub fn display(&self, plugin: &str, field: &str) -> Option<&loclib_plugin::FieldDisplay> {
        let analyzers = &self.plugins.get(plugin)?.manifest.contributes.analyzers;
        analyzers.iter().find_map(|a| a.display.get(field))
    }

    /// Every analyzer's fields but its beat grids, phrases and waveforms (no columns), by name as
    /// filters and sorts name them (`sound.bpm`), with their types: in the order the plugins, and
    /// their analyzers, declare them.
    pub fn fields(&self) -> crate::fields::Declared {
        let all = self.manifests().flat_map(|m| m.contributes.analyzers.iter().map(move |a| (m, a)));
        let columns =
            all.flat_map(|(m, a)| a.fields.iter().map(move |(name, &kind)| (format!("{}.{name}", m.id), kind)));
        columns.filter(|(_, kind)| !kind.is_structured()).collect()
    }

    /// Whether the plugin is running now. Never waits.
    pub fn is_running(&self, plugin: &str) -> bool {
        self.slot(plugin).is_ok_and(|s| s.is_running())
    }

    /// Stops every plugin running, all at once, once the events waiting for it are sent (for as
    /// long as it may take to start and stop) and its services stopped, each given its grace to
    /// exit before it's killed. They start again when used, but services only once they're run
    /// again. One already stopping (idle, or another caller's shutdown) is waited for: once this
    /// returns, each has exited and its folder in the temporary folder is gone, so the host can
    /// exit. But for one a hook under way holds: it's let go once the hook is answered, and stops
    /// then, on that hook's thread.
    pub fn shutdown(&self) {
        std::thread::scope(|s| {
            for slot in self.plugins.values() {
                s.spawn(|| {
                    slot.sent_events(slot.host.options.handshake + slot.host.options.grace);
                    slot.stop_services();
                    slot.stop();
                });
            }
        });
    }

    /// Stops every service, each given its grace, all at once.
    pub fn stop_services(&self) {
        std::thread::scope(|s| {
            for slot in self.plugins.values() {
                s.spawn(|| slot.stop_services());
            }
        });
    }

    fn slot(&self, plugin: &str) -> Result<&Arc<Slot>> {
        self.plugins.get(plugin).ok_or_else(|| Error::NotFound(format!("plugin {plugin:?}")))
    }
}

/// Services hold their plugin until they're stopped.
impl Drop for Plugins {
    fn drop(&mut self) {
        self.stop_services();
    }
}

impl Plugin {
    /// The plugin's contribution of a kind (`what`) named `name`, among `all` (named by
    /// `named`).
    fn declared<'a, T>(&self, what: &str, name: &str, all: &'a [T], named: fn(&T) -> &str) -> Result<&'a T> {
        let id = &self.manifest.id;
        all.iter()
            .find(|c| named(c) == name)
            .ok_or_else(|| Error::NotFound(format!("the {id} plugin's {what} {name:?}")))
    }
}

/// What `schema` (a contribution's `params` or `options`) says the option `key` takes, or why it
/// takes no such option, naming the contribution as `what` (`source`, `format`): the command
/// line's `--option` and the sidecar refuse an undeclared option in the same words.
pub fn declared_option<'a>(what: &str, schema: Option<&'a Value>, key: &str) -> std::result::Result<&'a Value, String> {
    let properties = schema.and_then(|s| s["properties"].as_object());
    if let Some(property) = properties.and_then(|p| p.get(key)) {
        return Ok(property);
    }
    let all: Vec<&str> = properties.into_iter().flatten().map(|(name, _)| name.as_str()).collect();
    Err(match all.is_empty() {
        true => format!("this {what} has no options, not even {key:?}"),
        false => format!("this {what} has no option {key:?}: its options are {}", all.join(", ")),
    })
}

/// A source's, an exporter's or an importer's (`what`) `options`, as [`fitting`] makes them,
/// refusing one `schema` doesn't declare as [`declared_option`] says it. (A command's or an
/// operation's params aren't held to theirs: plugins take ones they don't list.)
fn fitting_options(what: &str, schema: Option<&Value>, options: Value, context: &Context) -> Result<Value> {
    for key in options.as_object().into_iter().flat_map(Map::keys) {
        let pointer = format!("/{}", key.replace('~', "~0").replace('/', "~1"));
        declared_option(what, schema, key).map_err(|e| Error::Invalid(e).with("path", pointer))?;
    }
    fitting(schema, options, context)
}

/// `params` as a hook takes them, once they fit `schema` (a contribution's `params` or
/// `options`): `null` is none, and one filled in from the selection (`x-from`) that isn't given is
/// what `context` (as the plugin gets it) has selected, if anything; given as `null`, it's none
/// (cleared, rather than left to the selection) and left out. Where they don't fit is
/// `fields.path`, as the sidecar says it.
fn fitting(schema: Option<&Value>, params: Value, context: &Context) -> Result<Value> {
    let mut params = if params.is_null() { Value::Object(Map::new()) } else { params };
    if let Value::Object(given) = &mut params {
        for (name, property) in schema.and_then(|s| s["properties"].as_object()).into_iter().flatten() {
            let Some(from) = Selection::of(property).ok().flatten() else { continue };
            match given.get(name) {
                Some(Value::Null) => {
                    given.remove(name);
                }
                Some(_) => {}
                None => given.extend(from.value(context).map(|v| (name.clone(), v))),
            }
        }
    }
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

/// A library's tracks by id, for finding many: [`Library::track`] for each would scan the library
/// each time. Collected from the back, so the first of a repeated id wins, as there.
fn by_id(library: &Library) -> FxHashMap<u32, &Track> {
    library.tracks.iter().rev().map(|t| (t.id, t)).collect()
}

/// Each of `ids`' tracks, or an error naming the first the library lacks.
fn tracks_named<'a>(library: &'a Library, ids: &[u32]) -> Result<Vec<&'a Track>> {
    let by_id = by_id(library);
    ids.iter().map(|&id| by_id.get(&id).copied().ok_or_else(|| Error::no_track(id))).collect()
}

#[cfg(test)]
mod tests;
