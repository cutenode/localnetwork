//! The plugins locallibrary ships, and the host the CLI, the player and the sidecar run them in
//! (see [`locallibrary::plugin`]). Only plugins linked into this binary run: each is started as
//! `locallibrary plugin serve <id>`, a process of its own speaking the plugin protocol over its
//! stdin and stdout. A plugin whose id or contributions locallibrary couldn't tell from its own
//! commands, the sidecar's methods or another plugin's is left out (see [`check`]): the command
//! line says why as it starts, the sidecar's `plugins.list` has it with why, and every other
//! plugin works.

pub mod params;
pub mod providers;
pub mod roles;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use clap::{CommandFactory, ValueEnum};
use indexmap::IndexMap;
use locallibrary::api::RpcFailure;
use locallibrary::plugin::{Launch, Log, Options, PluginSpec, Plugins, Reads};
use locallibrary::progress::Ctx;
use locallibrary::{Error, Result};
use loclib_plugin::{Capability, ExportFrom, Manifest, SettingType};
use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use params::properties;

/// Serves a plugin over stdin and stdout until its input closes (the SDK's `serve`).
type Serve = fn(std::io::Stdin, std::io::Stdout);

/// The plugins linked into locallibrary: each one's manifest (its crate's
/// `locallibrary-plugin.toml`) and how it serves. Adding one is a line here and a dependency on
/// its crate.
const FIRST_PARTY: &[(&str, Serve)] = &[
    (locallibrary_plugin_coverart::MANIFEST, locallibrary_plugin_coverart::serve),
    (locallibrary_plugin_interop::MANIFEST, locallibrary_plugin_interop::serve),
    (locallibrary_plugin_sound::MANIFEST, locallibrary_plugin_sound::serve),
    (locallibrary_plugin_tracklist::MANIFEST, locallibrary_plugin_tracklist::serve),
];

/// Plugins the CLI's and the sidecar's tests add: manifest files (a list, as `PATH` is), each
/// served by the plugin host's test plugin. Only in builds with debug assertions (the tests'):
/// a release has no test plugin.
#[cfg(debug_assertions)]
const TEST_PLUGINS: &str = "LOCALLIBRARY_TEST_PLUGINS";

#[cfg(debug_assertions)]
#[path = "../../../crates/locallibrary/src/bin/test-plugin/plugin.rs"]
mod test_plugin;

/// The flag a contributed command or operation takes its params as JSON with.
pub const PARAMS: &str = "params";
/// The flag an operation is only planned with.
pub const DRY_RUN: &str = "dry-run";
/// The flag a question a command or an operation asks is answered with (`PARAM=VALUE`).
pub const CHOOSE: &str = "choose";

/// How `plugin serve` serves a plugin.
#[derive(Clone)]
enum Serves {
    Linked(Serve),
    /// The test plugin, serving this manifest.
    #[cfg(debug_assertions)]
    Test(String),
}

struct Loaded {
    plugins: Plugins,
    serves: FxHashMap<String, Serves>,
    /// The plugins [`check`] left out, and why.
    left_out: Vec<(Manifest, String)>,
}

static LOADED: OnceLock<std::result::Result<Loaded, String>> = OnceLock::new();

/// The plugins, once checked; why they were refused, otherwise.
fn loaded() -> Result<&'static Loaded> {
    LOADED.get_or_init(|| load().map_err(|e| e.to_string())).as_ref().map_err(|e| Error::Invalid(e.clone()))
}

/// The plugins, if they've been checked and weren't refused: none while they're being checked,
/// which reads the command line (whose `export --format` takes their formats).
pub fn checked() -> Option<&'static Plugins> {
    LOADED.get()?.as_ref().ok().map(|l| &l.plugins)
}

/// The host the CLI, the player and the sidecar run plugins in. Plugins start when they're first
/// used.
pub fn host() -> Result<&'static Plugins> {
    Ok(&loaded()?.plugins)
}

/// The plugins left out for clashing, and why: none if they couldn't be loaded at all.
pub fn left_out() -> &'static [(Manifest, String)] {
    loaded().map_or(&[], |l| &l.left_out)
}

fn load() -> Result<Loaded> {
    let mut found: Vec<(String, Serves)> =
        FIRST_PARTY.iter().map(|&(m, serve)| (m.into(), Serves::Linked(serve))).collect();
    #[cfg(debug_assertions)]
    for file in std::env::var_os(TEST_PLUGINS).iter().flat_map(std::env::split_paths) {
        let manifest = std::fs::read_to_string(&file)
            .map_err(|e| Error::Invalid(format!("the test plugin {}: {e}", file.display())))?;
        found.push((manifest.clone(), Serves::Test(manifest)));
    }
    let (mut specs, mut serves, mut left_out) = (Vec::new(), FxHashMap::default(), Vec::new());
    let program = exe().map_err(|e| Error::Invalid(format!("where locallibrary is: {e}")))?;
    let mut all = Vec::new();
    for (manifest, how) in found {
        // The shell's completion request would have it answer that instead of serving.
        let env_remove = vec![crate::completion::VAR.into()];
        let mut spec =
            PluginSpec::first_party(&manifest, Launch { program: program.clone(), args: Vec::new(), env_remove })?;
        spec.launch.args = vec!["plugin".into(), "serve".into(), spec.manifest.id.clone().into()];
        all.push((spec, how));
    }
    let clashes = check(&all.iter().map(|(s, _)| &s.manifest).collect::<Vec<_>>());
    for ((spec, how), clash) in all.into_iter().zip(clashes) {
        match clash {
            Some(why) => left_out.push((spec.manifest, why)),
            None => {
                serves.insert(spec.manifest.id.clone(), how);
                specs.push(spec);
            }
        }
    }
    let stderr = Options::default().log;
    let log: Log = Arc::new(move |id, line| match LOG.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        Some(log) => log(id, line),
        None => stderr(id, line),
    });
    let options = Options { log, ..Options::default() };
    let plugins = Plugins::new(specs, Arc::new(SidecarReads), options)?;
    // What core reads tracks' keys, tempos and levels from.
    locallibrary::fields::roles::set_offered(plugins.roles());
    Ok(Loaded { plugins, serves, left_out })
}

/// Where plugins' logs go instead of stderr, while the player draws on the terminal.
static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// Sends plugins' logs to `log` (the player's messages), or with `None` back to stderr.
#[cfg(feature = "tui")]
pub fn log_to(log: Option<Log>) {
    *LOG.lock().unwrap_or_else(PoisonError::into_inner) = log;
}

/// The locallibrary binary, which serves the plugins.
#[cfg(not(test))]
fn exe() -> std::io::Result<PathBuf> {
    std::env::current_exe()
}

/// A unit test's binary is the test harness, which can't serve a plugin: the locallibrary binary
/// cargo builds beside it for the integration tests does.
#[cfg(test)]
fn exe() -> std::io::Result<PathBuf> {
    let deps = std::env::current_exe()?;
    let dir = deps.parent().and_then(|d| d.parent()).ok_or_else(|| std::io::Error::other("no target folder"))?;
    let exe = dir.join(format!("locallibrary{}", std::env::consts::EXE_SUFFIX));
    match exe.is_file() {
        true => Ok(exe),
        false => Err(std::io::Error::other(format!("{} isn't built (cargo build)", exe.display()))),
    }
}

/// A name a plugin adds where it must be its alone (the command line, `export --format`, `import
/// --format`): whose it is ("the echo plugin's command") and what it is ("the echo plugin's
/// command \"say\"").
type Claim = (String, String);

/// Why each plugin is left out, if it is (by its place in `manifests`): its contributions couldn't
/// be told from locallibrary's own, or from another plugin's. That's an id or a place on the
/// command line (`cli`) named like a command there (one of locallibrary's, or another plugin's id
/// or command), a place in something that isn't one of locallibrary's groups of commands, an id
/// that's a namespace of the sidecar's methods (its plans would be named like theirs,
/// `sync.group`), a command and an operation of one name, a param named like a flag its command
/// has anyway, and an export format (or an import format) named like another, or like one of
/// locallibrary's own for what it writes out (`export`'s, `sets export`'s). A place in a group
/// is under its name, not an alias. Plugins that clash with each other are all left out, since
/// none is more right than the others; one left out already (clashing with locallibrary) clashes
/// with none.
fn check(manifests: &[&Manifest]) -> Vec<Option<String>> {
    let cli = crate::Cli::command();
    let names = |c: &clap::Command| -> Vec<String> {
        let subcommands = c.get_subcommands().flat_map(|c| std::iter::once(c.get_name()).chain(c.get_all_aliases()));
        subcommands.chain(["help"]).map(String::from).collect()
    };
    let mut top: FxHashMap<String, String> =
        names(&cli).into_iter().map(|name| (name, "a locallibrary command".to_string())).collect();
    // The commands in each group a plugin may add to.
    for group in cli.get_subcommands().filter(|c| c.has_subcommands()) {
        for name in names(group) {
            top.insert(format!("{} {name}", group.get_name()), "a locallibrary command".into());
        }
    }
    let namespaces = crate::rpc::namespaces();
    let mut flags: Vec<&str> = cli.get_arguments().filter(|a| a.is_global_set()).filter_map(|a| a.get_long()).collect();
    flags.extend(["help", "version", PARAMS, DRY_RUN, CHOOSE]);
    fn format_names<F: ValueEnum>() -> FxHashMap<String, String> {
        let values = F::value_variants().iter().filter_map(ValueEnum::to_possible_value);
        let names = values.flat_map(|v| v.get_name_and_aliases().map(String::from).collect::<Vec<_>>());
        names.map(|f| (f, "a locallibrary format".to_string())).collect()
    }
    // locallibrary's own formats for what each kind of exporter writes out: a mix has none.
    let formats = [
        (ExportFrom::Drive, format_names::<crate::export_cmd::FormatArg>()),
        (ExportFrom::Set, format_names::<crate::sets_cmd::FormatArg>()),
        (ExportFrom::Mix, FxHashMap::default()),
    ];
    let claim = |claims: &mut IndexMap<String, Claim>, ours: &FxHashMap<String, String>, name: &str, whose, what| {
        if let Some(other) = ours.get(name).or_else(|| claims.get(name).map(|c| &c.0)) {
            return Err(format!("{what} is {other} already"));
        }
        claims.insert(name.into(), (whose, what));
        Ok(())
    };
    // A plugin against locallibrary and itself: what it claims, unless it clashes.
    let alone = |m: &Manifest| -> std::result::Result<[IndexMap<String, Claim>; 3], String> {
        let id = &m.id;
        if namespaces.contains(&id.as_str()) {
            return Err(format!(
                "the {id} plugin's id is the sidecar's {id}.* methods' too: its plans would be named like them"
            ));
        }
        let (mut places, mut exports, mut imports) = (IndexMap::new(), IndexMap::new(), IndexMap::new());
        claim(&mut places, &top, id, format!("the {id} plugin"), format!("the {id} plugin's id"))?;
        let commands = m.contributes.commands.iter().filter_map(|c| c.cli.as_deref());
        for place in commands.chain(m.contributes.operations.iter().filter_map(|o| o.cli.as_deref())) {
            if let Some((group, _)) = place.split_once(' ')
                && !cli.get_subcommands().any(|c| c.get_name() == group && c.has_subcommands())
            {
                return Err(format!(
                    "the {id} plugin's command {place:?} is in {group:?}, which isn't a group of commands"
                ));
            }
            let what = format!("the {id} plugin's command {place:?}");
            claim(&mut places, &top, place, format!("the {id} plugin's command"), what)?;
        }
        let commands = m.contributes.commands.iter().map(|c| (c.name.as_str(), c.params.as_ref()));
        let operations = m.contributes.operations.iter().map(|o| (o.name.as_str(), o.params.as_ref()));
        let mut subcommands = vec!["help"];
        for (name, params) in commands.chain(operations) {
            if subcommands.contains(&name) {
                return Err(format!("the {id} plugin has two commands called {name:?} (a command and an operation)"));
            }
            subcommands.push(name);
            let mut given: Vec<String> = Vec::new();
            for (param, schema) in properties(params).filter(|(_, s)| !params::positional(s)) {
                let flag = params::flag(param, schema);
                if flags.contains(&flag.as_str()) || given.contains(&flag) {
                    return Err(format!(
                        "the {id} plugin's {name} has a param {param:?} for --{flag}, a flag it has already"
                    ));
                }
                given.push(flag);
            }
        }
        for e in &m.contributes.exporters {
            let what = format!("the {id} plugin's export format {:?}", e.format);
            let ours = &formats.iter().find(|(of, _)| *of == e.source).expect("every kind has its formats").1;
            claim(&mut exports, ours, &e.format, format!("the {id} plugin's"), what)?;
        }
        for format in m.contributes.importers.iter().map(|i| i.format.as_str()) {
            let what = format!("the {id} plugin's import format {format:?}");
            claim(&mut imports, &FxHashMap::default(), format, format!("the {id} plugin's"), what)?;
        }
        Ok([places, exports, imports])
    };
    let mut left_out = Vec::new();
    // Each name the plugins kept so far claim, and which of them claim it.
    let mut claimed: [IndexMap<String, Vec<(usize, Claim)>>; 3] = Default::default();
    for (i, m) in manifests.iter().enumerate() {
        match alone(m) {
            Err(why) => left_out.push(Some(why)),
            Ok(claims) => {
                left_out.push(None);
                for (all, claims) in claimed.iter_mut().zip(claims) {
                    for (name, claim) in claims {
                        all.entry(name).or_default().push((i, claim));
                    }
                }
            }
        }
    }
    for claims in claimed.iter().flat_map(|c| c.values()) {
        let Some(((first, (whose, _)), others)) = claims.split_first() else { continue };
        for (other, (_, what)) in others {
            let why = format!("{what} is {whose} already");
            for i in [*first, *other] {
                left_out[i].get_or_insert_with(|| why.clone());
            }
        }
    }
    left_out
}

/// `plugin serve <id>`: serves the plugin over stdin and stdout until its input closes, writing
/// nothing else to stdout.
pub fn serve(id: &str) -> Result<()> {
    let loaded = loaded()?;
    let serves = loaded.serves.get(id).ok_or_else(|| Error::NotFound(format!("plugin {id:?}")))?;
    match serves {
        Serves::Linked(serve) => serve(std::io::stdin(), std::io::stdout()),
        #[cfg(debug_assertions)]
        Serves::Test(manifest) => {
            let hooks = test_plugin::Test { wrong_fields: false }.hooks(&[]);
            loclib_plugin::serve(manifest, hooks, std::io::stdin(), std::io::stdout());
        }
    }
    Ok(())
}

/// What plugins read: the sidecar's read methods (`tracks.query`...), run on the thread the
/// callback came in on. Never queued for one of the sidecar's workers: the hook calling back is
/// running on one, waiting for the answer.
struct SidecarReads;

impl Reads for SidecarReads {
    fn read(&self, method: &str, params: Value, ctx: &Ctx<'_>) -> std::result::Result<Value, RpcFailure> {
        crate::rpc::read(method, params, ctx)
    }
}

/// The analyzers installed and what they worked out, for filters and sorts naming their fields
/// (`sound.bpm:>120`): kept in `store`. Plugins that were refused have none.
pub struct Installed {
    store: Option<locallibrary::Store>,
    analyzers: Vec<locallibrary::fields::Analyzer>,
    declared: locallibrary::fields::Declared,
}

impl Installed {
    pub fn with(store: Option<locallibrary::Store>) -> Installed {
        let host = host().ok();
        Installed {
            store,
            analyzers: host.map(Plugins::analyzers).unwrap_or_default(),
            declared: host.map(Plugins::fields).unwrap_or_default(),
        }
    }

    pub fn source(&self) -> locallibrary::fields::Source<'_> {
        locallibrary::fields::Source {
            store: self.store.as_ref(),
            analyzers: &self.analyzers,
            declared: &self.declared,
        }
    }
}

/// A capability a plugin has, and what it lets the plugin do.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Granted {
    pub capability: Capability,
    /// In plain words.
    pub means: String,
}

impl Granted {
    pub fn all(m: &Manifest) -> Vec<Granted> {
        m.capabilities.iter().map(|c| Granted { capability: c.clone(), means: means(c) }).collect()
    }
}

fn means(c: &Capability) -> String {
    match c {
        Capability::LibraryRead => "read your libraries' tracks and playlists",
        Capability::AnalysisRead => "read tracks' analysis (beat grids, waveforms) and what analyzers worked out",
        Capability::ArtworkRead => "read cover art",
        Capability::DriveInfo => "see your drives and their details",
        Capability::HealthRead => "check your drives' health",
        Capability::StoreRead => "read the store's libraries and snapshots",
        Capability::SettingsRead => "read My Settings and your player models",
        Capability::PlaysRead => "read what players recorded",
        Capability::SetsRead => "read your sets",
        Capability::AccountStatus => "see whether you're signed in to your Atmosphere account",
        Capability::CollectionRead => "read rekordbox's collection on this computer",
        Capability::LibraryPlan => "plan changes to playlists and tracks, which you confirm",
        Capability::LibraryDelete => "plan removing tracks, playlists and their entries, which you confirm",
        Capability::TagsPlan => "plan My Tag changes, which you confirm",
        Capability::CuesPlan => "plan cue changes, which you confirm",
        Capability::ArtworkPlan => "plan cover art changes, which you confirm",
        Capability::Net(host) => {
            return match host.strip_prefix("*.") {
                Some(domain) => format!("talk to any server under {domain}"),
                None => format!("talk to {host}"),
            };
        }
    }
    .into()
}

/// A plugin, as `plugin list` and the sidecar's `plugins.list` show it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Listed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub capabilities: Vec<Granted>,
    /// What it adds: its commands, operations, analyzers and providers by name, its export and
    /// import formats and its settings' keys.
    pub commands: Vec<String>,
    pub operations: Vec<String>,
    pub analyzers: Vec<String>,
    pub exporters: Vec<String>,
    pub importers: Vec<String>,
    pub providers: Vec<String>,
    pub settings: Vec<String>,
    /// It's running now: plugins start when they're first used, and stop once they're idle.
    pub running: bool,
    /// Why it's left out, if it is: what it clashes with. Then nothing it adds can be used. Only
    /// the sidecar lists these: the command line says why as it starts.
    pub left_out: Option<String>,
}

impl Listed {
    pub fn of(m: &Manifest, plugins: &Plugins) -> Listed {
        let c = &m.contributes;
        Listed {
            id: m.id.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
            capabilities: Granted::all(m),
            commands: c.commands.iter().map(|x| x.name.clone()).collect(),
            operations: c.operations.iter().map(|x| x.name.clone()).collect(),
            analyzers: c.analyzers.iter().map(|x| x.name.clone()).collect(),
            exporters: c.exporters.iter().map(|x| x.format.clone()).collect(),
            importers: c.importers.iter().map(|x| x.format.clone()).collect(),
            providers: c.providers.iter().map(|x| x.name.clone()).collect(),
            settings: c.settings.iter().map(|x| x.key.clone()).collect(),
            running: plugins.is_running(&m.id),
            left_out: None,
        }
    }
}

/// A plugin in full, as `plugin info` and the sidecar's `plugins.describe` show it: its manifest
/// (with the JSON Schemas of its params and options, to draw forms from), what its capabilities
/// let it do, and its settings now.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Described {
    pub manifest: Manifest,
    pub capabilities: Vec<Granted>,
    pub settings: IndexMap<String, Value>,
    #[serde(flatten)]
    pub secrets: Secrets,
    pub running: bool,
}

impl Described {
    pub fn of(plugin: &str, plugins: &Plugins, store: Option<&locallibrary::Store>) -> Result<Described> {
        let manifest = plugins.manifest(plugin)?;
        Ok(Described {
            manifest: manifest.clone(),
            capabilities: Granted::all(manifest),
            settings: plugins.settings(plugin, store)?,
            secrets: Secrets::of(plugins, plugin, store)?,
            running: plugins.is_running(plugin),
        })
    }
}

/// A plugin's settings now: each one set, else its default.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Settings {
    pub plugin: String,
    /// Each one set, else its default; never a secret.
    pub settings: IndexMap<String, Value>,
    #[serde(flatten)]
    pub secrets: Secrets,
}

/// Whether each of a plugin's secret settings is set, never what it is.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Secrets {
    /// Whether each secret setting is set, never what it is; `null` for each when the keychain
    /// can't say (see `secrets_unknown`).
    #[serde(rename = "secrets")]
    pub set: IndexMap<String, Option<bool>>,
    /// Why the keychain can't say, when it can't: the rest is answered all the same.
    #[serde(rename = "secrets_unknown", skip_serializing_if = "Option::is_none")]
    pub unknown: Option<String>,
}

impl Secrets {
    /// A plugin's, with `store`'s (none set without one).
    pub fn of(plugins: &Plugins, plugin: &str, store: Option<&locallibrary::Store>) -> Result<Secrets> {
        let manifest = plugins.manifest(plugin)?;
        Ok(match plugins.secrets(plugin, store) {
            Ok(set) => Secrets { set: set.into_iter().map(|(key, set)| (key, Some(set))).collect(), unknown: None },
            Err(e) => {
                let keys = manifest.contributes.settings.iter().filter(|s| s.kind == SettingType::Secret);
                Secrets { set: keys.map(|s| (s.key.clone(), None)).collect(), unknown: Some(e.to_string()) }
            }
        })
    }
}

#[cfg(test)]
mod tests;
