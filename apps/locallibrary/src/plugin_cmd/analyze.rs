//! `analyze`: plugins' analyzers on a library's tracks, and what they worked out under a track's
//! details.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Args;
use indexmap::IndexMap;
use locallibrary::api::{self, LibraryCache, PluginFieldsRequest};
use locallibrary::fields::PluginsFields;
use locallibrary::plugin::Analyzed;
use locallibrary::progress::Ctx;
use serde::Serialize;

use crate::jobs_cmd::usage;
use crate::output::{self, Outcome, Output, dim, ok, warn};
use crate::plugins::{self, params::cell};

/// `analyze`.
#[derive(Args)]
pub struct AnalyzeArgs {
    #[arg(help = crate::ONE_DRIVE_HELP)]
    drive: Option<String>,
    /// Only this plugin's analyzers
    #[arg(long)]
    plugin: Option<String>,
    /// Only this analyzer
    #[arg(long)]
    analyzer: Option<String>,
    /// Only these tracks, by id (repeatable)
    #[arg(long = "track", value_name = "ID")]
    tracks: Vec<u32>,
}

/// What `analyze` did.
#[derive(Serialize)]
struct Analyzing {
    drive: String,
    analyzers: Vec<Ran>,
}

#[derive(Serialize)]
struct Ran {
    plugin: String,
    analyzer: String,
    #[serde(flatten)]
    analyzed: Analyzed,
}

/// `analyze`: each analyzer asked for on the drive's tracks (those it hasn't worked out already),
/// keeping what they work out for `track show`. Some tracks failing is a partial outcome (exit
/// status 5), every one failing an error.
pub fn analyze(args: AnalyzeArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let plugins = plugins::host()?;
    if let Some(p) = &args.plugin {
        plugins.manifest(p)?;
    }
    let selected: Vec<(&str, &str)> = plugins
        .manifests()
        .filter(|m| args.plugin.as_ref().is_none_or(|p| *p == m.id))
        .flat_map(|m| m.contributes.analyzers.iter().map(move |a| (m.id.as_str(), a.name.as_str())))
        .filter(|(_, a)| args.analyzer.as_deref().is_none_or(|n| n == *a))
        .collect();
    if selected.is_empty() {
        return Err(usage(match &args.analyzer {
            Some(a) => format!("no plugin has an analyzer {a:?} (`locallibrary plugin list` lists them)"),
            None => "no plugin locallibrary has analyses tracks".into(),
        }));
    }
    let drive = crate::jobs_cmd::drive_or_default(args.drive, store_path, "analyze")?;
    let store = crate::open_store(store_path)?;
    let tracks = (!args.tracks.is_empty()).then_some(args.tracks.as_slice());
    let mut analyzers = Vec::new();
    for (plugin, analyzer) in selected {
        let analyzed = plugins.analyze(plugin, analyzer, &drive, tracks, &store, ctx)?;
        analyzers.push(Ran { plugin: plugin.into(), analyzer: analyzer.into(), analyzed });
    }
    let count = |of: fn(&Analyzed) -> usize| analyzers.iter().map(|r| of(&r.analyzed)).sum::<usize>();
    let failed = count(|a| a.failed);
    if failed > 0 && count(|a| a.analyzed + a.kept) == 0 {
        bail!("every track failed ({failed}): the warnings above say why");
    }
    let outcome = if failed > 0 { Outcome::Partial } else { Outcome::Applied };
    Ok(out.done_with(&Analyzing { drive, analyzers }, outcome, |a| {
        for r in &a.analyzers {
            let n = &r.analyzed;
            let line = format!(
                "{} ({}): {} analyzed, {} already",
                r.analyzer,
                r.plugin,
                output::plural(n.analyzed, "track"),
                n.kept
            );
            match n.failed {
                0 => ok(line),
                failed => warn(format!("{line}, {failed} failed")),
            }
        }
    }))
}

/// What the installed analyzers worked out for these tracks of the drive, by track id.
pub fn fields(drive: &str, tracks: Vec<u32>, store_path: &Option<PathBuf>) -> Result<IndexMap<u32, PluginsFields>> {
    // Plugins that were refused have nothing to show either.
    let installed = plugins::host().map(|p| p.analyzers()).unwrap_or_default();
    // Nothing to read, and no library to read again.
    if installed.is_empty() {
        return Ok(IndexMap::new());
    }
    let store = crate::diff_cmd::existing_store(store_path)?;
    let req = PluginFieldsRequest { library: drive.to_string().into(), tracks: Some(tracks), plugin: None };
    let read = api::track_fields_of(LibraryCache::none(), store.as_ref(), &req, &installed)?;
    Ok(read.tracks.into_iter().map(|t| (t.track, t.plugins)).collect())
}

/// A track's fields under its details (`track show`).
pub fn show_fields(fields: &PluginsFields) {
    for (plugin, analyzers) in fields {
        for (analyzer, values) in analyzers {
            for (name, value) in values {
                let value = serde_json::to_value(value).unwrap_or_default();
                say!("  {name:<11} {}  {}", cell(&value), dim(format!("{plugin} {analyzer}")));
            }
        }
    }
}
