//! `analyze`: plugins' analyzers on a library's tracks, and what they worked out under a track's
//! details.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Args;
use indexmap::IndexMap;
use locallibrary::api::{self, LibraryCache, PluginFieldsRequest};
use locallibrary::fields::roles;
use locallibrary::fields::{PluginsFields, Role};
use locallibrary::plugin::{AnalysisPart, Analyzed, ApplyAnalysis, ApplyFields};
use locallibrary::progress::Ctx;
use serde::Serialize;

use crate::jobs_cmd::usage;
use crate::output::{self, Outcome, Output, heading, ok, warn};
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
    /// Then write what the analyzer worked out to the tracks (`key` and `bpm`, or `grid`,
    /// `phrases` and `waveform` into their analysis files; comma-separated), planned and asked
    /// about as any edit is: only where a track has none, unless --overwrite
    #[arg(
        long,
        value_name = "FIELDS",
        value_delimiter = ',',
        value_parser = ["key", "bpm", "grid", "phrases", "waveform"]
    )]
    apply: Vec<String>,
    /// With --apply: also over what a track has
    #[arg(long, requires = "apply")]
    overwrite: bool,
    /// With --apply: leave out values the analyzer is less sure of than this (0 to 1)
    #[arg(long, requires = "apply", value_name = "CONFIDENCE")]
    min_confidence: Option<f64>,
    /// With --apply: show the plan without changing anything
    #[arg(long, requires = "apply")]
    dry_run: bool,
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
/// status 5), every one failing an error. With `--apply`, only the analyzer for those fields runs
/// (the one chosen for them, when several could), and what it worked out is then planned onto the
/// tracks: a key and BPM into the databases, or a beat grid, phrases and a waveform into their
/// analysis files (`plugins.apply_analysis`), one or the other in a run.
pub fn analyze(args: AnalyzeArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let plugins = plugins::host()?;
    if let Some(p) = &args.plugin {
        plugins.manifest(p)?;
    }
    let (parts, roles): (Vec<&String>, Vec<&String>) =
        args.apply.iter().partition(|f| AnalysisPart::ALL.iter().any(|p| p.name() == f.as_str()));
    if !parts.is_empty() && !roles.is_empty() {
        return Err(usage("write a key and BPM in a run of their own, apart from a grid, phrases and a waveform"));
    }
    if !parts.is_empty() && args.min_confidence.is_some() {
        return Err(usage("--min-confidence is for a key and BPM"));
    }
    let parts: Vec<AnalysisPart> =
        AnalysisPart::ALL.into_iter().filter(|p| parts.iter().any(|f| f.as_str() == p.name())).collect();
    let apply: Vec<Role> = roles.iter().map(|r| r.parse().map_err(usage)).collect::<Result<_>>()?;
    let mut selected: Vec<(&str, &str)> = plugins
        .manifests()
        .filter(|m| args.plugin.as_ref().is_none_or(|p| *p == m.id))
        .flat_map(|m| m.contributes.analyzers.iter().map(move |a| (m, a)))
        .filter(|(_, a)| args.analyzer.as_deref().is_none_or(|n| n == a.name))
        .filter(|(_, a)| apply.iter().all(|r| a.roles.contains_key(r)))
        .filter(|(_, a)| parts.iter().all(|p| a.fields.values().any(|t| *t == p.kind())))
        .map(|(m, a)| (m.id.as_str(), a.name.as_str()))
        .collect();
    let fields: Vec<&str> = apply.iter().map(|r| r.name()).chain(parts.iter().map(|p| p.name())).collect();
    if selected.is_empty() {
        return Err(usage(match (&args.analyzer, &args.plugin) {
            _ if !fields.is_empty() => format!("no plugin's analyzer here works out {}", fields.join(" and ")),
            (Some(a), _) => format!("no plugin has an analyzer {a:?} (`locallibrary plugin list` lists them)"),
            (None, Some(p)) => format!("the {p} plugin has no analyzer"),
            (None, None) => "no plugin locallibrary has analyses tracks".into(),
        }));
    }
    let drive = crate::jobs_cmd::drive_or_default(args.drive, store_path, "analyze")?;
    let store = crate::open_store(store_path)?;
    if let Some(&role) = apply.first().filter(|_| selected.len() > 1) {
        let chosen = roles::chosen(Some(&store), role);
        let named = |(p, a): &(&str, &str)| {
            chosen.as_ref().is_some_and(|f| f.analyzer.plugin == *p && f.analyzer.analyzer == *a)
        };
        selected.retain(named);
    }
    if (!fields.is_empty() && selected.len() > 1) || selected.is_empty() {
        return Err(usage(format!(
            "several analyzers work out {}: say which with --plugin and --analyzer",
            fields.join(" and ")
        )));
    }
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
    let ran = Analyzing { drive, analyzers };
    if fields.is_empty() {
        let outcome = if failed > 0 { Outcome::Partial } else { Outcome::Applied };
        return Ok(out.done_with(&ran, outcome, said));
    }
    if !out.json {
        said(&ran);
    }
    // Narrowed to one above.
    let Ran { plugin, analyzer, .. } = &ran.analyzers[0];
    let (plugin, analyzer, drive, tracks) =
        (plugin.clone(), analyzer.clone(), ran.drive.clone(), tracks.map(<[u32]>::to_vec));
    let planned = if parts.is_empty() {
        let req = ApplyFields {
            plugin,
            analyzer,
            drive,
            fields: apply,
            tracks,
            overwrite: args.overwrite,
            min_confidence: args.min_confidence,
        };
        plugins.plan_apply_fields(&req, &store, ctx)?
    } else {
        let req = ApplyAnalysis { plugin, analyzer, drive, parts, tracks, overwrite: args.overwrite };
        plugins.plan_apply_analysis(&req, &store, ctx)?
    };
    if !out.json {
        heading(&planned.summary[0]);
    }
    crate::edit_cmd::confirm_and_apply(&ran.drive, planned, args.dry_run, out, ctx)
}

/// What each analyzer did, a line each.
fn said(a: &Analyzing) {
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

/// A track's fields under its details (`track show`), each named by its plugin's id and its own
/// (`sound.bpm`): a plugin's analyzers never share a field name, so which one filled it is left
/// out. Aligned with the details above, or with the longest name; each shown as its analyzer
/// says, else as it is.
pub fn show_fields(fields: &PluginsFields) {
    let host = plugins::host().ok();
    let named: Vec<(String, String)> = fields
        .iter()
        .flat_map(|(plugin, analyzers)| {
            analyzers.values().flatten().map(move |(n, v)| {
                let shown = match host.and_then(|p| p.display(plugin, n)) {
                    Some(hint) => hint.show(v),
                    None => cell(&serde_json::to_value(v).unwrap_or_default()),
                };
                (format!("{plugin}.{n}"), shown)
            })
        })
        .collect();
    let width = named.iter().map(|(name, _)| name.len()).max().unwrap_or(0).max(11);
    for (name, shown) in named {
        say!("  {name:<width$} {shown}");
    }
}
