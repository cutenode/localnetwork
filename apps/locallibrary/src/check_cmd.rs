//! `locallibrary check`: are these drives ready for a set?

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use console::style;
use locallibrary::api;
use locallibrary::check::{CheckOptions, CheckReport, Level, PlayersFrom};
use locallibrary::progress::Ctx;

use crate::output::{Outcome, Output, dim, heading};

#[derive(Args)]
#[command(after_long_help = "Examples:
  locallibrary check --players CDJ-3000X,CDJ-2000NXS2
  locallibrary check SD --verbose            the tracks a player won't load
  locallibrary check Home --audio            also decode every file's end (slow)")]
pub struct CheckArgs {
    /// The drives you're taking (a library's name: its mounted drives); the first is the one the
    /// others must match [default: the mounted drives of one library]
    drives: Vec<String>,
    /// Player models to check the drives against, e.g. CDJ-2000NXS2,CDJ-3000X (repeatable or comma-separated)
    /// [default: the ones `locallibrary players set` remembered]
    #[arg(long, value_delimiter = ',')]
    players: Vec<String>,
    /// List the tracks each player won't load (and, with --audio, the files with problems)
    #[arg(long, short)]
    verbose: bool,
    /// Also open every audio file and decode its end, to find files cut short by an interrupted
    /// copy (reads a little of every file: slow on a big drive)
    #[arg(long)]
    audio: bool,
}

/// Exits with 4 when there are problems, so scripts can gate on it.
pub fn run(args: CheckArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let drives = crate::jobs_cmd::drives_or_group(args.drives, store, None)?;
    let store = crate::diff_cmd::existing_store(store);
    let options = CheckOptions { players: args.players, audio: args.audio };
    let req = api::CheckRequest { drives, options };
    let report = api::check(store.as_ref(), &req, ctx)?;
    let outcome = if report.ok() { Outcome::Applied } else { Outcome::Problems };
    out.page();
    Ok(out.report(&report, outcome, |r| render(r, args.verbose)))
}

pub(crate) fn render(r: &CheckReport, verbose: bool) {
    let mut drive = None;
    let mut unlisted = 0;
    for i in &r.items {
        if drive != Some(&i.drive) {
            heading(&i.drive);
            drive = Some(&i.drive);
            if let Some(p) = r.players.iter().find(|p| p.drive == i.drive).filter(|p| p.from != PlayersFrom::Given) {
                let from = match (p.from, &p.library) {
                    (PlayersFrom::Library, Some(l)) => format!("remembered for {l}"),
                    (PlayersFrom::Library, None) => "remembered for its library".into(),
                    _ => "remembered".into(),
                };
                if !p.models.is_empty() {
                    say!("  {}", dim(format!("players: {} ({from})", p.models.join(", "))));
                }
            }
        }
        let mark = match i.level {
            Level::Ok => style("✓").green().bold(),
            Level::Warning => style("!").yellow().bold(),
            Level::Problem => style("✗").red().bold(),
        };
        say!("  {mark} {:<13} {}", dim(i.check), i.message);
        let same = |drive: &str, message: &str| drive == i.drive && message == i.message;
        let loudness = r.loudness.iter().find(|l| i.check == "loudness" && l.drive == i.drive);
        let lines: Vec<(String, String)> = match (
            r.unloadable.iter().find(|u| same(&u.drive, &u.message)),
            r.audio.iter().find(|a| same(&a.drive, &a.message)),
            loudness,
        ) {
            (Some(u), ..) => u.tracks.iter().map(|t| (t.name.clone(), t.file_path.clone())).collect(),
            (None, Some(a), _) => a
                .files
                .iter()
                .map(|f| (f.track.name.clone(), format!("{}  {}", f.problem, f.track.file_path)))
                .collect(),
            (None, None, Some(l)) => l
                .tracks
                .iter()
                .map(|f| {
                    let lufs = f.integrated_lufs.map_or("silent".into(), |l| format!("{l:.1} LUFS"));
                    let detail = format!("{lufs}, {:+.1} dBTP, {} clipped run(s)", f.true_peak_dbtp, f.clipped_runs);
                    (f.track.name.clone(), detail)
                })
                .collect(),
            (None, None, None) => continue,
        };
        if !verbose {
            unlisted += 1;
            continue;
        }
        for (name, detail) in lines {
            say!("{:>18}{name}  {}", "", dim(detail));
        }
    }
    say!();
    if unlisted > 0 {
        crate::output::hint("--verbose lists the tracks.");
    }
    match (r.problems, r.warnings) {
        (0, 0) => say!("{}", style("Ready.").green().bold()),
        (0, w) => say!("{} {}", style("Ready,").green().bold(), format_args!("with {w} warning(s).")),
        (p, w) => say!("{} {}", style(format!("{p} problem(s)")).red().bold(), dim(format!("and {w} warning(s)"))),
    }
}
