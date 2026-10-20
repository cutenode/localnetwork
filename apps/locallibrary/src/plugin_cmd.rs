//! `plugin`: the plugins locallibrary has, their settings and which analyzer each field role is
//! read from; `analyze`, running their analyzers
//! on a drive's tracks; and the commands and operations they add to the command line
//! (`locallibrary <plugin> <name>`, or the place a manifest gives one, `playlist import`), whose
//! flags come from their params' JSON Schemas.

mod analyze;
mod contributed;
mod listing;
pub(crate) mod view;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::output::Output;
use crate::plugins::{self, Described, Listed};

pub use analyze::{AnalyzeArgs, analyze, fields, show_fields};
pub use contributed::{Contributed, add_to, options, run_contributed, take};

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
    /// One plugin in full: what it may do, its commands, operations, analyzers, export formats
    /// and settings
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
    /// Which analyzer each role is read from (a track's key, tempo and level, for doctor, the
    /// check before a set, ordering a set and `analyze --apply`): show them, choose one (ROLE
    /// PLUGIN.ANALYZER), or put one back to its default (ROLE --reset)
    Roles {
        /// The role (`key`, `bpm`, `loudness`...) [default: every one]
        role: Option<String>,
        /// The analyzer to read it from, as PLUGIN.ANALYZER (`sound.tempo-key-loudness`)
        #[arg(conflicts_with = "reset")]
        analyzer: Option<String>,
        /// Put it back to its default: the only analyzer with it, else the first plugin's
        #[arg(long, requires = "role")]
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

pub fn run(args: PluginArgs, store_path: &Option<PathBuf>, out: Output) -> Result<ExitCode> {
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
        PluginCommand::Roles { role, analyzer, reset } => {
            return listing::roles(role, analyzer, reset, store_path, out);
        }
        PluginCommand::Serve { plugin } => {
            // Ctrl-C in the terminal reaches it too: it stops when its host says.
            let _ = ctrlc::set_handler(|| {});
            plugins::serve(&plugin)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
