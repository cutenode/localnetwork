//! `plugin check`: whether a plugin locallibrary doesn't ship would run, given with
//! `--dev-plugin`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;
use console::style;
use locallibrary::Error;
use locallibrary::api::RpcFailure;
use locallibrary::plugin::{Launch, Options, PluginSpec, Plugins, Reads};
use locallibrary::progress::Ctx;
use serde::Serialize;
use serde_json::Value;

use crate::output::{Outcome, Output, dim, ok};
use crate::plugins;

/// What `plugin check` found.
#[derive(Serialize)]
struct Checked {
    id: String,
    version: String,
    /// The features it and locallibrary both have, once it has shaken hands: none when it hasn't.
    features: Vec<String>,
    /// What keeps it from running: none when it would.
    problems: Vec<String>,
}

/// Checks the plugin `manifest` describes, run as `program` (its arguments after it), as
/// `--dev-plugin` would run it: its manifest by a third party's rules and beside the plugins
/// locallibrary ships, then the program started without a store, given nothing (every callback
/// refused, see [`Options::isolated`]), to shake hands (an account plugin is its manifest alone,
/// which nothing starts). Exits with 4 when it wouldn't run.
pub fn check(manifest: &Path, program: Vec<OsString>, out: Output) -> Result<ExitCode> {
    let m = plugins::dev_manifest(manifest)?;
    let mut program = program.into_iter();
    let launch = Launch {
        program: program.next().map(PathBuf::from).expect("clap requires a program"),
        args: program.collect(),
        env_remove: plugins::ENV_REMOVE.map(OsString::from).to_vec(),
    };
    let mut checked =
        Checked { id: m.id.clone(), version: m.version.clone(), features: Vec::new(), problems: Vec::new() };
    let hosted = PluginSpec::third_party(m.clone(), launch)
        .and_then(|spec| Plugins::new(vec![spec], Arc::new(NoReads), options()));
    match hosted {
        Err(e) => checked.problems.push(e.to_string()),
        Ok(hosted) => {
            checked.problems.extend(plugins::unfit_beside_shipped(&m));
            if !m.is_account_plugin() {
                match hosted.start(&m.id) {
                    Ok(features) => checked.features = features,
                    Err(e) => checked.problems.push(e.to_string()),
                }
            }
            hosted.shutdown();
        }
    }
    let outcome = if checked.problems.is_empty() { Outcome::Applied } else { Outcome::Problems };
    Ok(out.report(&checked, outcome, render))
}

fn render(c: &Checked) {
    if c.problems.is_empty() {
        ok(format!("the {} plugin {} would run", c.id, c.version));
    }
    for problem in &c.problems {
        say!("{} {problem}", style("✗").red().bold());
    }
    if !c.features.is_empty() {
        say!("  {} {}", dim("features"), c.features.join(", "));
    }
}

/// How the plugin being checked is hosted: given nothing, every callback refused.
fn options() -> Options {
    Options { isolated: true, ..Options::default() }
}

/// What a plugin being checked reads: nothing, as the host never asks (see [`Options::isolated`]).
struct NoReads;

impl Reads for NoReads {
    fn read(&self, method: &str, _: Value, _: &Ctx<'_>) -> std::result::Result<Value, RpcFailure> {
        Err(Error::Invalid(format!("`plugin check` gives a plugin nothing to read, {method} included")).into())
    }
}

#[cfg(test)]
mod tests;
