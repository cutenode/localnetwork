//! The plugins locallibrary ships, and the host the CLI, the player and the sidecar run them in
//! (see [`locallibrary::plugin`]). Each plugin linked into this binary is started as
//! `locallibrary plugin serve <id>`, a process of its own speaking the plugin protocol over its
//! stdin and stdout. An account plugin is its manifest alone, which locallibrary signs in from and
//! never starts. A plugin whose id or contributions locallibrary couldn't tell from its own
//! commands, the sidecar's methods or another plugin's is left out (see [`check`]): the command
//! line says why as it starts, the sidecar's `plugins.list` has it with why, and every other
//! plugin works. A developer may give plugins locallibrary doesn't ship for one run
//! (`--dev-plugin`, [`DEV_PLUGINS`]): those run as any other, held to a third party's rules, and
//! one that can't run fails the run instead of being left out.

pub mod checks;
pub mod events;
pub mod params;
pub mod providers;
pub mod roles;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use clap::{CommandFactory, ValueEnum};
use indexmap::IndexMap;
use locallibrary::api::RpcFailure;
use locallibrary::mix::report::ShownField;
use locallibrary::plugin::{
    Access, AccessQuestion, AccountSignIn, AskAccess, EnteredKeys, Launch, Log, Options, PluginSpec, Plugins, Reads,
};
use locallibrary::progress::Ctx;
use locallibrary::{Error, Result};
use loclib_plugin::{AccountType, Capability, ExportFrom, Manifest, SettingType};
use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use params::properties;

/// Serves a plugin over stdin and stdout until its input closes (the SDK's `serve`).
type Serve = fn(std::io::Stdin, std::io::Stdout);

/// The plugins built into locallibrary: each one's manifest (its `locallibrary-plugin.toml`) and
/// how it serves, which an account plugin doesn't: locallibrary signs in from its manifest and
/// never starts it. build.rs writes it from the folders in `plugins/`, so adding one is its folder
/// and a dependency on its crate, or an account plugin's folder in `plugins/accounts/`.
const BUILT_IN: &[(&str, &str, Option<Serve>)] = include!(concat!(env!("OUT_DIR"), "/built_in.rs"));

/// Plugins the CLI's and the sidecar's tests add: manifest files (a list, as `PATH` is), each
/// served by the plugin host's test plugin. Only in builds with debug assertions (the tests'):
/// a release has no test plugin.
#[cfg(debug_assertions)]
const TEST_PLUGINS: &str = "LOCALLIBRARY_TEST_PLUGINS";

/// Set by the CLI's tests: plugins reach no server but this computer's
/// ([`locallibrary::plugin::Options::test_offline`]). Only in builds with debug assertions.
#[cfg(debug_assertions)]
const TEST_OFFLINE: &str = "LOCALLIBRARY_TEST_OFFLINE";

/// Set by the CLI's tests that sign in at a mock service on this computer: plugins and account
/// plugins may reach it ([`locallibrary::plugin::Options::test_loopback`]). Only in builds with
/// debug assertions.
#[cfg(debug_assertions)]
const TEST_LOOPBACK: &str = "LOCALLIBRARY_TEST_LOOPBACK";

/// Set by the CLI's tests: the binary, run with no arguments, serves the test plugin with the
/// manifest in this file, as a program `--dev-plugin` names. Only in builds with debug assertions.
#[cfg(debug_assertions)]
const TEST_SERVE: &str = "LOCALLIBRARY_TEST_SERVE";

#[cfg(debug_assertions)]
#[path = "../../../crates/locallibrary/src/bin/test-plugin/plugin.rs"]
mod test_plugin;

/// Plugins locallibrary doesn't ship, for this run alone, when `--dev-plugin` gives none: each
/// one's manifest file and the program to run it with, one plugin after another, joined as `PATH`
/// joins paths ([`SEPARATOR`]). Nothing is installed: a plugin keeps only what any keeps under its
/// id (its settings, and its access to your accounts).
pub const DEV_PLUGINS: &str = "LOCALLIBRARY_DEV_PLUGINS";

/// The environment variables a plugin is started without: the shell's completion request, which
/// would have locallibrary answer that instead of serving, and the plugins given for this run.
pub const ENV_REMOVE: [&str; 2] = [crate::completion::VAR, DEV_PLUGINS];

/// What joins a plugin's manifest and program in `--dev-plugin` and [`DEV_PLUGINS`], as it joins
/// paths in `PATH`: Windows' paths have a colon after the drive letter.
const SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

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
    /// The ids of the plugins given for this run, which locallibrary doesn't ship.
    dev: Vec<String>,
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

/// The plugins given for this run, which locallibrary doesn't ship: none if they couldn't be
/// loaded at all.
pub fn dev() -> Vec<&'static Manifest> {
    let Ok(l) = loaded() else { return Vec::new() };
    l.dev.iter().filter_map(|id| l.plugins.manifest(id).ok()).collect()
}

/// Fails, saying why, when plugins were given for this run and can't run: every command does
/// then, since theirs would be missing from the command line.
pub fn dev_loaded() -> Result<()> {
    if !dev_values().0.is_empty() {
        host()?;
    }
    Ok(())
}

/// What gives the plugins for this run, and its name: `--dev-plugin`'s values, read from the
/// arguments before the command line is parsed (it's built from the plugins), else
/// [`DEV_PLUGINS`]'s value.
fn dev_values() -> (Vec<OsString>, &'static str) {
    let mut args = std::env::args_os().skip(1).take_while(|a| a != "--");
    let mut given = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--dev-plugin" {
            given.extend(args.next());
        } else if let Some(value) = arg.to_str().and_then(|a| a.strip_prefix("--dev-plugin=")) {
            given.push(value.into());
        }
    }
    match given.is_empty() {
        true => (std::env::var_os(DEV_PLUGINS).filter(|v| !v.is_empty()).into_iter().collect(), DEV_PLUGINS),
        false => (given, "--dev-plugin"),
    }
}

/// Each plugin given for this run: its manifest file and program.
fn dev_given() -> Result<Vec<(PathBuf, PathBuf)>> {
    let (values, from) = dev_values();
    let mut given = Vec::new();
    for value in values {
        let paths: Vec<PathBuf> = std::env::split_paths(&value).collect();
        if !paths.len().is_multiple_of(2) || paths.iter().any(|p| p.as_os_str().is_empty()) {
            return Err(Error::Invalid(format!(
                "{from} {}: give each plugin's manifest and the program to run it with, as MANIFEST{SEPARATOR}PROGRAM",
                value.to_string_lossy()
            )));
        }
        given.extend(paths.chunks(2).map(|pair| (pair[0].clone(), pair[1].clone())));
    }
    Ok(given)
}

/// The manifest in `file`, of a plugin locallibrary doesn't ship.
pub fn dev_manifest(file: &Path) -> Result<Manifest> {
    let text = std::fs::read_to_string(file).map_err(|source| Error::Io { path: file.to_path_buf(), source })?;
    Manifest::parse(&text).map_err(|e| Error::Parse { path: file.to_path_buf(), message: e.to_string() })
}

/// Why `m`, a plugin locallibrary doesn't ship, can't run beside the others, if it can't: its id
/// is one of a plugin locallibrary ships, whose settings and access to your accounts it would
/// have, or `clash`, what [`check`] says of it beside them.
fn unfit(m: &Manifest, clash: Option<String>) -> Option<String> {
    if BUILT_IN.iter().any(|row| row.0 == m.id) {
        return Some(format!(
            "the {} plugin is one locallibrary ships: another can't take its id, or its settings and access to your accounts",
            m.id
        ));
    }
    clash
}

/// Why `m`, a plugin locallibrary doesn't ship, couldn't run beside those it does, if it
/// couldn't (see [`unfit`]).
pub fn unfit_beside_shipped(m: &Manifest) -> Option<String> {
    let shipped: Vec<Manifest> =
        BUILT_IN.iter().map(|&(_, json, _)| Manifest::from_json(json).expect("build.rs wrote it")).collect();
    let all: Vec<&Manifest> = shipped.iter().chain([m]).collect();
    unfit(m, check(&all, true).pop().flatten())
}

fn load() -> Result<Loaded> {
    // Only the test plugins are added to it, and a release has none.
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut found: Vec<(Manifest, Option<Serves>)> = BUILT_IN
        .iter()
        .map(|&(_, m, serve)| (Manifest::from_json(m).expect("build.rs wrote it"), serve.map(Serves::Linked)))
        .collect();
    #[cfg(debug_assertions)]
    for file in std::env::var_os(TEST_PLUGINS).iter().flat_map(std::env::split_paths) {
        let manifest = std::fs::read_to_string(&file)
            .map_err(|e| Error::Invalid(format!("the test plugin {}: {e}", file.display())))?;
        let parsed = Manifest::parse(&manifest).map_err(|e| Error::Invalid(format!("a plugin's manifest: {e}")))?;
        found.push((parsed, Some(Serves::Test(manifest))));
    }
    let (mut specs, mut serves, mut left_out) = (Vec::new(), FxHashMap::default(), Vec::new());
    let program = exe().map_err(|e| Error::Invalid(format!("where locallibrary is: {e}")))?;
    let env_remove: Vec<OsString> = ENV_REMOVE.map(OsString::from).to_vec();
    let mut all = Vec::new();
    for (manifest, how) in found {
        let launch = Launch { program: program.clone(), args: Vec::new(), env_remove: env_remove.clone() };
        let mut spec = PluginSpec::shipped(manifest, launch)?;
        spec.launch.args = vec!["plugin".into(), "serve".into(), spec.manifest.id.clone().into()];
        all.push((spec, how));
    }
    let given = dev_given()?;
    let mut dev_specs = Vec::new();
    for (file, program) in &given {
        let launch = Launch { program: program.clone(), args: Vec::new(), env_remove: env_remove.clone() };
        dev_specs.push(PluginSpec::third_party(dev_manifest(file)?, launch)?);
    }
    let manifests = all.iter().map(|(s, _)| &s.manifest).chain(dev_specs.iter().map(|s| &s.manifest));
    let mut clashes = check(&manifests.collect::<Vec<_>>(), !dev_specs.is_empty());
    for ((spec, (file, _)), clash) in dev_specs.iter().zip(&given).zip(clashes.split_off(all.len())) {
        if let Some(why) = unfit(&spec.manifest, clash) {
            return Err(Error::Invalid(format!("the plugin in {} can't run: {why}", file.display())));
        }
    }
    for ((spec, how), clash) in all.into_iter().zip(clashes) {
        match clash {
            Some(why) => left_out.push((spec.manifest, why)),
            None => {
                if let Some(how) = how {
                    serves.insert(spec.manifest.id.clone(), how);
                }
                specs.push(spec);
            }
        }
    }
    let dev = dev_specs.iter().map(|s| s.manifest.id.clone()).collect();
    specs.extend(dev_specs);
    let stderr = Options::default().log;
    let log: Log = Arc::new(move |id, line| match LOG.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        Some(log) => log(id, line),
        None => stderr(id, line),
    });
    let ask: Arc<dyn AskAccess> = Arc::new(Asking);
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut options = Options { notices: events::notices(log.clone()), log, ask: Some(ask), ..Options::default() };
    // The CLI's tests: no plugin they run goes online (to MusicBrainz, say).
    #[cfg(debug_assertions)]
    {
        options.test_offline = std::env::var_os(TEST_OFFLINE).is_some();
        options.test_loopback = std::env::var_os(TEST_LOOPBACK).is_some();
    }
    let plugins = Plugins::new(specs, Arc::new(SidecarReads), options)?;
    events::tell_applied();
    // What core reads tracks' keys, tempos and levels from.
    locallibrary::fields::roles::set_offered(plugins.roles());
    Ok(Loaded { plugins, serves, left_out, dev })
}

/// Where plugins' logs go instead of stderr, while the player draws on the terminal.
static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// Sends plugins' logs to `log` (the player's messages), or with `None` back to stderr.
#[cfg(feature = "tui")]
pub fn log_to(log: Option<Log>) {
    *LOG.lock().unwrap_or_else(PoisonError::into_inner) = log;
}

/// Who asks the user whether a plugin may use more of an account it has access to: nobody until
/// a front end that can ask says so (see [`ask_to`]).
static ASK: Mutex<Option<Arc<dyn AskAccess>>> = Mutex::new(None);

/// Has `ask` ask the user whether a plugin may use more of an account it has access to (the
/// terminal a command runs in, the player), or with `None` nobody: the plugin is told how to give
/// it instead.
pub fn ask_to(ask: Option<Arc<dyn AskAccess>>) {
    *ASK.lock().unwrap_or_else(PoisonError::into_inner) = ask;
}

/// Asks through whoever [`ask_to`] said, when anyone.
struct Asking;

impl Asking {
    fn asker() -> Option<Arc<dyn AskAccess>> {
        ASK.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl AskAccess for Asking {
    fn allow(&self, question: &AccessQuestion) -> Option<bool> {
        Asking::asker()?.allow(question)
    }

    fn sign_in(&self, url: &str) {
        Asking::asker().into_iter().for_each(|ask| ask.sign_in(url));
    }

    fn enter_keys(&self, start: &AccountSignIn) -> Option<EnteredKeys> {
        Asking::asker()?.enter_keys(start)
    }
}

/// The locallibrary binary, which serves the plugins.
#[cfg(not(test))]
fn exe() -> std::io::Result<PathBuf> {
    std::env::current_exe()
}

/// A unit test's binary is the test harness, which can't serve a plugin: the locallibrary binary
/// cargo builds for the tests does. Its path is looked up when the test runs, not built in, so it
/// holds when nextest runs the tests from an archive somewhere else.
#[cfg(test)]
pub(crate) fn exe() -> std::io::Result<PathBuf> {
    std::env::var_os("CARGO_BIN_EXE_locallibrary").map(PathBuf::from).ok_or_else(|| {
        std::io::Error::other(
            "CARGO_BIN_EXE_locallibrary isn't set: run the tests with cargo 1.94 or later, or nextest",
        )
    })
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
/// locallibrary's own for what it writes out (`export`'s, `sets export`'s), and an account type
/// named like an `account` command. A place in a group is under its name, not an alias. Plugins
/// that clash with each other are all left out, since none is more right than the others; one
/// left out already (clashing with locallibrary) clashes with none. Then a plugin asking for
/// accounts is left out when an account plugin kept would be no use to it (see [`unmet`]).
fn check(manifests: &[&Manifest], dev: bool) -> Vec<Option<String>> {
    // Only in builds with debug assertions, or with plugins given for this run (`dev`): the
    // plugins a release ships don't clash, which the tests check, and this builds the whole
    // command line a second time on every start.
    if !cfg!(debug_assertions) && !dev {
        return vec![None; manifests.len()];
    }
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
            // Taken on each command line it's given on.
            let ours: FxHashMap<String, String> =
                formats.iter().filter(|(of, _)| e.takes(*of)).flat_map(|(_, names)| names.clone()).collect();
            claim(&mut exports, &ours, &e.format, format!("the {id} plugin's"), what)?;
        }
        for format in m.contributes.importers.iter().map(|i| i.format.as_str()) {
            let what = format!("the {id} plugin's import format {format:?}");
            claim(&mut imports, &FxHashMap::default(), format, format!("the {id} plugin's"), what)?;
        }
        // `account <type>` names one of its accounts on the command line.
        if let Some(t) = m.contributes.account_types.iter().find(|t| top.contains_key(&format!("account {}", t.name))) {
            return Err(format!(
                "the {id} plugin's account type {:?} is named like the account {} command",
                t.name, t.name
            ));
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
    let kept = manifests.iter().zip(&left_out).filter(|(m, why)| why.is_none() && m.is_account_plugin());
    let providers: Vec<&Manifest> = kept.map(|(m, _)| *m).collect();
    for (m, why) in manifests.iter().zip(left_out.iter_mut()).filter(|(_, why)| why.is_none()) {
        *why = unmet(m, &providers);
    }
    left_out
}

/// Why `m` is no use asking for the accounts it asks for, if it is, with the account plugins
/// `providers`: a scope of a type no account plugin providing the type lists, or a host a request
/// of one goes to that none of its `net:` capabilities reaches.
fn unmet(m: &Manifest, providers: &[&Manifest]) -> Option<String> {
    let id = &m.id;
    for need in &m.accounts {
        let kind = &need.name;
        let types: Vec<(&str, &AccountType)> = providers
            .iter()
            .filter_map(|p| Some((p.id.as_str(), p.contributes.account_types.iter().find(|t| t.name == *kind)?)))
            .collect();
        if types.is_empty() {
            return Some(format!("the {id} plugin asks for {kind} accounts, which no account plugin provides"));
        }
        for scope in need.scopes.iter().chain(need.optional.iter().flat_map(|o| &o.scopes)) {
            let listing: Vec<_> = types.iter().filter_map(|(_, t)| t.scopes.get(scope)).collect();
            if listing.is_empty() {
                let ids: Vec<&str> = types.iter().map(|(id, _)| *id).collect();
                let ids = ids.join(", ");
                return Some(format!(
                    "the {id} plugin asks for {kind}'s scope {scope:?}, which no account plugin providing {kind} lists ({ids})"
                ));
            }
            let hosts = listing.iter().flat_map(|s| &s.requests).filter_map(|r| r.host());
            if let Some(host) = hosts.into_iter().find(|h| !m.capabilities.iter().any(|c| c.reaches(h))) {
                return Some(format!(
                    "the {id} plugin asks for {kind}'s scope {scope:?}, whose requests go to {host}, which it has no net:{host} capability for"
                ));
            }
        }
    }
    None
}

/// `plugin serve <id>`: serves the plugin over stdin and stdout until its input closes, writing
/// nothing else to stdout. An account plugin has nothing to serve.
pub fn serve(id: &str) -> Result<()> {
    // Ctrl-C in the terminal reaches it too: it stops when its host says.
    let _ = ctrlc::set_handler(|| {});
    let loaded = loaded()?;
    let Some(serves) = loaded.serves.get(id) else {
        return Err(match loaded.plugins.manifest(id).is_ok_and(Manifest::is_account_plugin) {
            true => Error::Invalid(format!(
                "the {id} plugin is an account plugin, which nothing runs: locallibrary signs in from its manifest"
            )),
            false => Error::NotFound(format!("plugin {id:?}")),
        });
    };
    match serves {
        Serves::Linked(serve) => serve(std::io::stdin(), std::io::stdout()),
        #[cfg(debug_assertions)]
        Serves::Test(manifest) => serve_test_plugin(manifest),
    }
    Ok(())
}

/// Serves the test plugin with `manifest` over stdin and stdout until its input closes.
#[cfg(debug_assertions)]
fn serve_test_plugin(manifest: &str) {
    let hooks = test_plugin::Test { wrong_fields: false }.hooks(&[]);
    loclib_plugin::serve(manifest, hooks, std::io::stdin(), std::io::stdout());
}

/// Serves the test plugin with the manifest in the file [`TEST_SERVE`] names, if it's set, with
/// nothing else set up: the CLI's tests give the locallibrary binary to `--dev-plugin` as a
/// plugin's program, which runs it with no arguments.
#[cfg(debug_assertions)]
pub fn serve_test() -> bool {
    let Some(file) = std::env::var_os(TEST_SERVE) else { return false };
    let _ = ctrlc::set_handler(|| {});
    serve_test_plugin(&std::fs::read_to_string(file).expect("the test plugin's manifest"));
    true
}

/// Serves the plugin `id`, if locallibrary ships one that serves, with nothing else set up: a
/// `check` starts every plugin that checks, and each spent as long as a command starting.
pub fn serve_shipped(id: &str) -> bool {
    let Some(serve) = BUILT_IN.iter().find(|row| row.0 == id).and_then(|row| row.2) else { return false };
    let _ = ctrlc::set_handler(|| {});
    serve(std::io::stdin(), std::io::stdout());
    true
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

/// The analyzers installed and what they worked out, kept in `store`, for filters and sorts
/// naming their fields (`sound.bpm:>120`).
pub fn installed(store: Option<locallibrary::Store>) -> locallibrary::fields::Installed {
    locallibrary::fields::Installed::new(store, host().ok())
}

/// What analyzers worked out for a track on a drive, kept in `store`, that a page shows: each
/// field whose analyzer says how it's shown, so (see [`locallibrary::mix::report::ShownFields`]).
pub fn shown_fields(
    store: Option<locallibrary::Store>,
) -> impl Fn(&locallibrary::Device, &locallibrary::Track) -> Vec<ShownField> {
    let plugins = host().ok();
    let analyzers = plugins.map(Plugins::analyzers).unwrap_or_default();
    move |device, track| {
        let (Some(plugins), Some(store)) = (plugins, &store) else { return Vec::new() };
        let Ok(audio) = device.checked_path(&locallibrary::paths::key(&track.file_path)) else { return Vec::new() };
        let kept = locallibrary::fields::FieldStore::in_store(store).get(&audio, &analyzers);
        let each = kept.iter().flat_map(|(plugin, by)| by.values().flatten().map(move |(field, v)| (plugin, field, v)));
        let shown = each.filter_map(|(plugin, field, v)| {
            let hint = plugins.display(plugin, field)?;
            Some(ShownField { label: format!("{plugin}.{field}"), value: hint.show(v) })
        });
        shown.collect()
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
        Capability::LibraryDeleteUnbacked => {
            "remove tracks rekordbox's next export puts back, without backing up their files"
        }
        Capability::LibraryDelete => "plan removing tracks, playlists and their entries, which you confirm",
        Capability::TagsPlan => "plan My Tag changes, which you confirm",
        Capability::CuesPlan => "plan cue changes, which you confirm",
        Capability::ArtworkPlan => "plan cover art changes, which you confirm",
        Capability::AnalysisPlan => "plan beat grid, phrase and waveform changes, which you confirm",
        Capability::EventsDrives => "hear of drives plugged in, unplugged, changed and saved into the store",
        Capability::EventsJobs => "hear of the sidecar's requests finishing",
        Capability::EventsPlans => "hear of changes applied, and what they were",
        Capability::FsRead => "read audio files in folders you grant it, to build a drive from",
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
    /// The account types it asks for (`[[accounts]]`).
    pub accounts: Vec<String>,
    /// What it adds: its account types by type, its commands, destinations, operations, panels,
    /// analyzers, checks, providers, services, library sources and themes by name, the events it
    /// listens for, its export and import formats and its settings' keys.
    pub account_types: Vec<String>,
    pub commands: Vec<String>,
    pub destinations: Vec<String>,
    pub events: Vec<String>,
    pub operations: Vec<String>,
    pub panels: Vec<String>,
    pub analyzers: Vec<String>,
    pub checks: Vec<String>,
    pub exporters: Vec<String>,
    pub importers: Vec<String>,
    pub providers: Vec<String>,
    pub services: Vec<String>,
    pub sources: Vec<String>,
    pub themes: Vec<String>,
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
            accounts: m.accounts.iter().map(|x| x.name.clone()).collect(),
            account_types: c.account_types.iter().map(|x| x.name.clone()).collect(),
            commands: c.commands.iter().map(|x| x.name.clone()).collect(),
            destinations: c.destinations.iter().map(|x| x.name.clone()).collect(),
            events: c.events.iter().flat_map(|x| &x.on).map(|e| e.name().to_string()).collect(),
            operations: c.operations.iter().map(|x| x.name.clone()).collect(),
            panels: c.panels.iter().map(|x| x.name.clone()).collect(),
            analyzers: c.analyzers.iter().map(|x| x.name.clone()).collect(),
            checks: c.checks.iter().map(|x| x.name.clone()).collect(),
            exporters: c.exporters.iter().map(|x| x.format.clone()).collect(),
            importers: c.importers.iter().map(|x| x.format.clone()).collect(),
            providers: c.providers.iter().map(|x| x.name.clone()).collect(),
            services: c.services.iter().map(|x| x.name.clone()).collect(),
            sources: c.sources.iter().map(|x| x.name.clone()).collect(),
            themes: c.themes.iter().map(|x| x.name.clone()).collect(),
            settings: c.settings.iter().map(|x| x.key.clone()).collect(),
            running: plugins.is_running(&m.id),
            left_out: None,
        }
    }
}

/// A plugin in full, as `plugin info` and the sidecar's `plugins.describe` show it: its manifest
/// (with the JSON Schemas of its params and options, to draw forms from), what its capabilities
/// let it do, its settings now, and the access it has to your accounts.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Described {
    pub manifest: Manifest,
    pub capabilities: Vec<Granted>,
    pub settings: IndexMap<String, Value>,
    #[serde(flatten)]
    pub secrets: Secrets,
    /// Its access to the accounts the store keeps, of the types it asks for (`manifest.accounts`)
    /// or any other.
    pub accesses: Vec<Access>,
    pub running: bool,
}

impl Described {
    pub fn of(plugin: &str, plugins: &Plugins, store: Option<&locallibrary::Store>) -> Result<Described> {
        let manifest = plugins.manifest(plugin)?;
        let secrets = Secrets::of(plugins, plugin, store)?;
        let accesses = store.map(|s| plugins.accesses(s, Some(plugin), None)).transpose()?.unwrap_or_default();
        Ok(Described {
            manifest: manifest.clone(),
            capabilities: Granted::all(manifest),
            settings: plugins.settings(plugin, store)?,
            secrets,
            accesses,
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
