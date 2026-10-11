//! `locallibrary analysis bench`: rekordbox's stored keys and tempos, and loclib-analysis's
//! estimates, scored against the GiantSteps datasets.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Subcommand};
use console::style;
use indexmap::IndexMap;
use locallibrary::api;
use locallibrary::progress::Ctx;
use locallibrary::sound::bench::{BenchOptions, BenchReport};
use locallibrary::sound::{KeyOutcome, Outcome as Scored, Report, TempoOutcome};

use crate::output::{Outcome, Output, dim, heading, warn};

#[derive(Args)]
pub struct AnalysisArgs {
    #[command(subcommand)]
    command: AnalysisCommand,
}

#[derive(Subcommand)]
enum AnalysisCommand {
    /// Score rekordbox's stored keys and tempos, and the analysis `doctor --analyze` runs, against
    /// the GiantSteps datasets' annotations (tracks match annotations by file name)
    #[command(after_long_help = "Examples:
  locallibrary analysis bench GIANTSTEPS --keys giantsteps-key-dataset/annotations/key
  locallibrary analysis bench GIANTSTEPS --tempos giantsteps-tempo-dataset/annotations_v2/tempo
  locallibrary analysis bench --keys giantsteps-key-dataset/annotations/key --audio giantsteps-key-dataset/audio

Get the annotations from github.com/GiantSteps/giantsteps-key-dataset,
github.com/GiantSteps/giantsteps-mtg-key-dataset and github.com/GiantSteps/giantsteps-tempo-dataset,
whose scripts download the audio. To score rekordbox, import the audio into rekordbox, export it to
a drive and name that drive here.")]
    Bench {
        /// The drive (or library) whose stored keys and tempos to score; its tracks are analysed
        /// too unless --audio is given
        drive: Option<String>,
        /// GiantSteps Key's annotations/key folder, or GiantSteps MTG Key's annotations.txt
        #[arg(long, value_name = "PATH", required_unless_present = "tempos")]
        keys: Option<PathBuf>,
        /// GiantSteps Tempo's annotations/tempo or annotations_v2/tempo folder
        #[arg(long, value_name = "DIR")]
        tempos: Option<PathBuf>,
        /// A folder of the datasets' audio to analyse (instead of the drive's tracks)
        #[arg(long, value_name = "DIR")]
        audio: Option<PathBuf>,
    },
}

pub fn run(args: AnalysisArgs, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let AnalysisCommand::Bench { drive, keys, tempos, audio } = args.command;
    let req = api::AnalysisBenchRequest { drive, options: BenchOptions { keys, tempos, audio } };
    let report = api::analysis_bench(&req, ctx)?;
    Ok(out.report(&report, Outcome::Applied, render))
}

/// Each outcome's share, after the first (the hits): `fifth 5.0%, relative 8.1%`.
fn breakdown<O: Scored>(r: &Report<O>, name: impl Fn(O) -> &'static str) -> String {
    let parts: Vec<String> = O::ALL[1..]
        .iter()
        .filter(|o| r.count(**o) > 0)
        .map(|o| format!("{} {:.1}%", name(*o), r.percent(*o)))
        .collect();
    parts.join(", ")
}

fn analyzers<O: Scored>(reports: &IndexMap<String, Report<O>>, line: impl Fn(&str, &Report<O>) -> String) {
    for (analyzer, r) in reports {
        say!("  {:<12} {}", style(analyzer).bold(), line(analyzer, r));
    }
}

pub fn render(r: &BenchReport) {
    if !r.keys.is_empty() {
        heading("Keys");
        analyzers(&r.keys, |_, k| {
            let name = |o| match o {
                KeyOutcome::Fifth => "fifth",
                KeyOutcome::Relative => "relative",
                KeyOutcome::Parallel => "parallel",
                KeyOutcome::Missing => "no key",
                _ => "other",
            };
            format!(
                "weighted {:.1}%, correct {:.1}%  {}",
                k.score * 100.0,
                k.percent(KeyOutcome::Correct),
                dim(format!("{} ({} tracks)", breakdown(k, name), k.tracks))
            )
        });
    }
    if !r.tempos.is_empty() {
        heading("Tempos");
        analyzers(&r.tempos, |analyzer, t| {
            let name = |o| match o {
                TempoOutcome::Double => "double",
                TempoOutcome::Half => "half",
                TempoOutcome::Triple => "triple",
                TempoOutcome::Third => "third",
                TempoOutcome::Missing => "no tempo",
                _ => "other",
            };
            format!(
                "Accuracy1 {:.1}%, Accuracy2 {:.1}%  {}",
                t.score * 100.0,
                r.accuracy2.get(analyzer).copied().unwrap_or_default(),
                dim(format!("{} ({} tracks)", breakdown(t, name), t.tracks))
            )
        });
    }
    if r.unmatched > 0 {
        say!("  {}", dim(format!("{} annotated track(s) had no audio to score", r.unmatched)));
    }
    if let Some((name, error)) = r.failed.first() {
        warn(format!("{} file(s) couldn't be analysed (e.g. {name}: {error})", r.failed.len()));
    }
}
