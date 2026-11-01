//! `plugin`: the plugins locallibrary has, their settings and accounts, which analyzer each field
//! role is read from, which checks run and when a provider's candidate is chosen for the user;
//! `analyze`, running their analyzers on a drive's tracks; `art fetch` and `track fetch`, what
//! their providers find; `import`, what their importers read; their panels under `track show`'s
//! details; and the commands and operations they add to the command line (`locallibrary <plugin>
//! <name>`, or the place a manifest gives one, `playlist import`), whose flags come from their
//! params' JSON Schemas.

mod account;
mod analyze;
mod checks;
mod choose;
mod contributed;
mod fetch;
mod import;
mod listing;
mod panels;
pub(crate) mod view;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Subcommand};
use locallibrary::plugin::CheckChange;
use locallibrary::progress::Ctx;

use crate::output::Output;
use crate::plugins::{self, Described, Listed};

pub use analyze::{AnalyzeArgs, analyze, fields, show_fields};
pub use checks::fix_line;
pub use contributed::{Contributed, add_to, options, run_contributed, take};
pub use fetch::{FetchArgs, fetch};
pub use import::{ImportArgs, import};
pub use panels::{panels, show_panels};

#[derive(Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    command: Option<PluginCommand>,
}

#[derive(Subcommand)]
enum PluginCommand {
    /// The plugins locallibrary has: what each may do, and what it adds (the default)
    #[command(visible_alias = "ls")]
    List,
    /// One plugin in full: what it may do, its commands, operations, analyzers, export and import
    /// formats and settings
    Info {
        /// The plugin's id
        plugin: String,
    },
    /// A plugin's settings: show them, set one (KEY VALUE, or a secret: KEY --secret), or put
    /// one back to its default (KEY --reset)
    Settings {
        /// The plugin's id
        plugin: String,
        /// The setting [default: every one]
        key: Option<String>,
        /// Its new value
        #[arg(conflicts_with = "reset")]
        value: Option<String>,
        /// Put it back to its default (a secret: forget it)
        #[arg(long, requires = "key")]
        reset: bool,
        /// Set a secret (a password, a key), read from stdin without showing it: kept in the
        /// system's keychain, and never shown
        #[arg(long, requires = "key", conflicts_with_all = ["value", "reset"])]
        secret: bool,
    },
    /// One of a plugin's accounts on another service: whether it's signed in (the default), signing
    /// in in the browser, or out
    Account(account::AccountArgs),
    /// Which analyzer each role is read from (a track's key, tempo and level, for doctor, the
    /// check before a set, ordering a set and `analyze --apply`; a published set's MusicBrainz
    /// IDs): show them, choose one (ROLE PLUGIN.ANALYZER), read one from none (ROLE --none), or
    /// put one back to its default (ROLE --reset)
    Roles {
        /// The role (`key`, `bpm`, `loudness`...) [default: every one]
        role: Option<String>,
        /// The analyzer to read it from, as PLUGIN.ANALYZER (`sound.tempo-key-loudness`)
        #[arg(conflicts_with = "reset")]
        analyzer: Option<String>,
        /// Put it back to its default: the only analyzer with it, else the first plugin's
        #[arg(long, requires = "role")]
        reset: bool,
        /// Read it from no analyzer (`recording --none`: `sets publish` looks up no MusicBrainz
        /// IDs)
        #[arg(long, requires = "role", conflicts_with_all = ["analyzer", "reset"])]
        none: bool,
    },
    /// The plugins' checks (run with `check` and `doctor`) and guards (run before changes are
    /// made): show them, turn one on or off (CHECK --on, --off), have a guard stop changes it finds
    /// something about (--blocking, --no-blocking), put one back as it is by default (CHECK
    /// --reset), or give a check some seconds to answer (--timeout SECS, or --reset without CHECK)
    Checks {
        /// The check, as PLUGIN.CHECK (`echo.untitled`) [default: every one]
        check: Option<String>,
        /// Run it
        #[arg(long, requires = "check", conflicts_with = "off")]
        on: bool,
        /// Don't run it
        #[arg(long, requires = "check")]
        off: bool,
        /// A guard: stop changes it finds a warning or a problem about (it only warns otherwise)
        #[arg(long, requires = "check", conflicts_with = "no_blocking")]
        blocking: bool,
        /// A guard: only warn about changes
        #[arg(long, requires = "check")]
        no_blocking: bool,
        /// How long a check has to answer, in seconds (at most 3600), before it's stopped [default: 10]
        #[arg(long, value_name = "SECS", conflicts_with_all = ["check", "reset"])]
        timeout: Option<f64>,
        /// Put the check back as it is by default (without CHECK: the timeout)
        #[arg(long, conflicts_with_all = ["on", "off", "blocking", "no_blocking"])]
        reset: bool,
    },
    /// The providers (what `art fetch` and `track fetch` ask), and the score a candidate alone
    /// scoring it or more is chosen for you with: show them, set it (--auto-pick SCORE), or put it
    /// back to its default (--reset)
    Providers {
        /// 0 to 1 (more than 1: never choose for you)
        #[arg(long, value_name = "SCORE", conflicts_with = "reset")]
        auto_pick: Option<f64>,
        /// Put it back to its default
        #[arg(long)]
        reset: bool,
    },
    /// Run a plugin, speaking its protocol over stdin and stdout: how locallibrary starts one
    #[command(hide = true)]
    Serve {
        /// The plugin's id
        plugin: String,
    },
}

impl PluginArgs {
    /// `plugin serve`: the plugin's protocol has stdout, and its host decides when it stops.
    pub fn serves(&self) -> bool {
        matches!(self.command, Some(PluginCommand::Serve { .. }))
    }
}

pub fn run(args: PluginArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let plugins = plugins::host()?;
    match args.command.unwrap_or(PluginCommand::List) {
        PluginCommand::List => {
            let listed: Vec<Listed> = plugins.manifests().map(|m| Listed::of(m, plugins)).collect();
            out.emit(&listed, |l| listing::list(l));
        }
        PluginCommand::Info { plugin } => {
            let store = crate::diff_cmd::existing_store(store_path)?;
            out.emit(&Described::of(&plugin, plugins, store.as_ref())?, listing::info);
        }
        PluginCommand::Settings { plugin, key, value, reset, secret } => {
            let change = listing::Change { value, reset, secret };
            return listing::settings(plugins, plugin, key, change, store_path, out);
        }
        PluginCommand::Account(args) => return account::account(plugins, args, store_path, out, ctx),
        PluginCommand::Roles { role, analyzer, reset, none } => {
            return listing::roles(role, analyzer, reset, none, store_path, out);
        }
        PluginCommand::Checks { check, on, off, blocking, no_blocking, timeout, reset } => {
            let either = |yes: bool, no: bool| (yes || no).then_some(yes);
            let check_change = CheckChange { on: either(on, off), blocking: either(blocking, no_blocking), reset };
            return checks::checks(plugins, checks::Change { check, check_change, timeout }, store_path, out);
        }
        PluginCommand::Providers { auto_pick, reset } => {
            return listing::providers(plugins, auto_pick, reset, store_path, out);
        }
        PluginCommand::Serve { plugin } => {
            // Ctrl-C in the terminal reaches it too: it stops when its host says.
            let _ = ctrlc::set_handler(|| {});
            plugins::serve(&plugin)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
