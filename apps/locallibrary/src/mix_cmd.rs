//! `locallibrary mix`: find a drive's tracks in recorded mixes, print the tracklist, and write
//! the interactive HTML report, the report as data, or a mix's tracklist in a format a plugin
//! adds.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args;
use locallibrary::mix::index::{Cache, IndexReport, LONGEST_TRACK_SECS};
use locallibrary::mix::report::{self, Drive, playable_audio};
use locallibrary::mix::{Identifier, MixReport, Options, Play, export};
use locallibrary::profile::ProfilePrefs;
use locallibrary::progress::Ctx;
use locallibrary::{Store, api, fsutil};
use loclib_plugin::ExportSource;

use crate::output::{self, Output, clock, dim};

#[derive(Args)]
#[command(
    after_help = "Without a mix, fingerprints the drive's tracks ahead of time (identifying does it on demand).\n\
Fingerprints are cached in the store, so each track is only decoded once."
)]
pub struct MixArgs {
    /// Recordings: WAV, AIFF, MP3, M4A or FLAC files, or Audacity projects (.aup3, .aup4)
    mixes: Vec<PathBuf>,
    /// Library or drive to look in [default: the mounted rekordbox drive]
    #[arg(long, short)]
    drive: Option<String>,
    /// Write the interactive report here (a folder when there are several mixes)
    #[arg(long, value_name = "PATH")]
    html: Option<PathBuf>,
    /// Open the report in a browser (written to the store's reports folder without --html)
    #[arg(long)]
    open: bool,
    /// Write the report as data for other software (Local Network renders it on a DJ profile):
    /// a .json file, or a folder when there are several mixes; or, with --format, in a plugin's
    /// format
    #[arg(long, value_name = "PATH")]
    export: Option<PathBuf>,
    /// What --export writes of a single mix: a format a plugin adds for mixes (the report is
    /// kept in the store for it) [default: the report as data]
    #[arg(long, value_parser = formats(), requires = "export")]
    format: Option<String>,
    /// A plugin's format: an option for it, like key=value (repeatable; `locallibrary plugin
    /// info` shows them)
    #[arg(long = "option", value_name = "KEY=VALUE", requires = "format")]
    options: Vec<String>,
    /// Replace files --html and --export would write that are there already
    #[arg(long)]
    replace: bool,
    /// Audio for the report's player [default: the mix itself, or an export next to a project]
    #[arg(long, value_name = "FILE")]
    audio: Option<PathBuf>,
    /// Playback speeds to search, in percent either way
    #[arg(long, value_name = "PERCENT", default_value_t = 16.0)]
    tempo_range: f64,
}

/// `--format`'s values: the formats plugins add for mixes.
fn formats() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(crate::export_cmd::plugin_formats(loclib_plugin::ExportFrom::Mix))
}

pub fn run(args: MixArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    for m in &args.mixes {
        if !m.is_file() {
            bail!("{} doesn't exist", m.display());
        }
        if !locallibrary::mix::is_mix_file(m) {
            bail!(
                "{} isn't a format mix reads (WAV, AIFF, MP3, M4A, FLAC, or an Audacity .aup3/.aup4 project)",
                m.display()
            );
        }
    }
    if args.audio.is_some() && args.mixes.len() != 1 {
        bail!("--audio goes with a single mix");
    }
    if args.format.is_some() && args.mixes.len() != 1 {
        bail!("--format goes with a single mix");
    }
    let device = api::mix_drive(args.drive.as_deref())?;
    // Fingerprints are cached in the store, made here if there's none yet.
    let store = Store::open_or_init(store.clone().unwrap_or_else(Store::default_root))?;
    let id = Identifier::new(&device, &Cache::in_store(&store), ctx)?;
    if args.mixes.is_empty() {
        out.emit(&id.report, |r| index_summary(&id.drive, r));
        return Ok(ExitCode::SUCCESS);
    }

    let options = Options { tempo_range: args.tempo_range / 100.0 };
    let mut reports = Vec::new();
    for m in &args.mixes {
        reports.push(id.identify(m, &options, ctx).with_context(|| format!("identifying {}", m.display()))?);
    }

    let pages: Vec<PathBuf> = match args.html.is_some() || args.open {
        true => reports.iter().map(|r| report_path(args.html.as_deref(), reports.len(), r, &store)).collect(),
        false => Vec::new(),
    };
    // A plugin's format is written once the rest is (see below).
    let data: Vec<PathBuf> = match (&args.export, &args.format) {
        (Some(target), None) => reports.iter().map(|r| export_path(target, reports.len(), r)).collect(),
        _ => Vec::new(),
    };
    // Every file named with --html or --export is checked before any is written. The store's own
    // copy of a page (--open alone) is the recording's, replaced each time it's identified.
    let named: Vec<&Path> = pages.iter().filter(|_| args.html.is_some()).chain(&data).map(PathBuf::as_path).collect();
    fsutil::refuse_to_replace(&named, args.replace)?;
    let prefs = ProfilePrefs::load(&store)?;
    let shown = crate::plugins::shown_fields(Some(store.clone()));
    let drive = Some(Drive { fields: Some(&shown), ..Drive::new(&device, &id.library) });
    // A page written with --html is made to be shared: it names the recording by its file alone.
    let shared = args.html.is_some();
    for (r, path) in reports.iter().zip(&pages) {
        let audio = args.audio.clone().or_else(|| playable_audio(&r.mix.path));
        fsutil::write_atomic(path, report::page(r, drive, audio.as_deref(), path, &prefs, shared).as_bytes())?;
    }
    for (r, path) in reports.iter().zip(&data) {
        fsutil::write_atomic(path, &serde_json::to_vec(&export::build(r, drive, &prefs))?)?;
    }
    let written: Vec<&PathBuf> = pages.iter().chain(&data).collect();

    // With a plugin's format, what it writes is what's answered as JSON.
    if !(args.format.is_some() && out.json) {
        out.emit(&reports, |reports| {
            for (i, r) in reports.iter().enumerate() {
                if i > 0 {
                    say!();
                }
                tracklist(r);
            }
            if reports.first().is_some_and(|r| r.library.fingerprinted > 0) {
                let fingerprinted = reports[0].library.fingerprinted;
                say!("{}", dim(format!("fingerprinted {fingerprinted} new track(s) on {}", id.drive)));
            }
            for p in &written {
                let what = if p.extension().is_some_and(|e| e == "json") { "data" } else { "report" };
                output::ok(format!("{what}: {}", p.display()));
            }
        });
    }
    if args.open {
        for p in written.iter().filter(|p| p.extension().is_some_and(|e| e == "html")) {
            if let Err(e) = locallibrary::opener::open_file(p) {
                output::warn(format!("couldn't open {}: {e}", p.display()));
            }
        }
    }
    if let (Some(format), Some(target)) = (&args.format, &args.export) {
        let mix = report::save(&store, &reports[0])?;
        let source =
            ExportSource { drive: device.root().display().to_string(), mix: Some(mix), ..ExportSource::default() };
        let written = crate::export_cmd::Written {
            output: target,
            options: &args.options,
            dry_run: false,
            replace: args.replace,
        };
        return crate::export_cmd::plugin_export(format, source, written, Some(&store), out, ctx);
    }
    Ok(ExitCode::SUCCESS)
}

fn index_summary(drive: &str, r: &IndexReport) {
    output::ok(format!("{drive}: {} of {} tracks fingerprinted ({} new)", r.indexed, r.tracks, r.fingerprinted));
    if !r.missing.is_empty() {
        output::warn(format!("{} track(s) have no audio file on the drive", r.missing.len()));
    }
    if !r.long.is_empty() {
        let minutes = LONGEST_TRACK_SECS / 60;
        say!(
            "{}",
            dim(format!("{} track(s) over {minutes} minutes left out: recorded sets, not tracks", r.long.len()))
        );
    }
    for f in r.failed.iter().take(5) {
        output::warn(format!("couldn't read {}: {}", f.track.name, f.error));
    }
}

fn truncate(s: &str, n: usize) -> String {
    crate::output::fit(s, n).trim_end().to_string()
}

fn tracklist(r: &MixReport) {
    let mut head = format!("{}", console::style(&r.mix.title).bold());
    if let Some(a) = &r.mix.artist {
        head.push_str(&format!(" by {a}"));
    }
    say!("{head}");
    let mut facts = vec![clock(r.mix.duration_secs), format!("{} tracks from {}", r.plays.len(), r.drive)];
    facts.push(format!("{:.0}% identified", r.coverage * 100.0));
    if let Some(rec) = &r.mix.recorder {
        facts.push(format!("recorded on {rec}"));
    }
    say!("{}", dim(facts.join(" · ")));
    say!();

    enum Row<'a> {
        Play(usize, &'a Play),
        Gap(f64, f64),
    }
    let mut rows: Vec<(f64, Row)> = r.plays.iter().enumerate().map(|(i, p)| (p.start, Row::Play(i, p))).collect();
    rows.extend(r.unidentified.iter().map(|g| (g.start, Row::Gap(g.start, g.end))));
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, row) in rows {
        match row {
            Row::Play(i, p) => {
                let key = match (&p.key, &p.played_key) {
                    (Some(k), Some(played)) if k != played => format!("{k}→{played}"),
                    (_, Some(played)) => played.clone(),
                    _ => p.track.key.clone(),
                };
                let bpm = p.played_bpm.map(|b| format!("{b:.1}")).unwrap_or_default();
                let speed = format!("{:+.1}%", (p.tempo - 1.0) * 100.0);
                let lock = match p.key_lock {
                    Some(true) => "key lock",
                    Some(false) => "pitched",
                    None => "",
                };
                say!(
                    "{:>3}  {:>8}  {:<48}  {:<7}  {:>6}  {:>6}  {:<8}  {}",
                    i + 1,
                    clock(p.start),
                    truncate(&p.track.display_name(), 48),
                    key,
                    bpm,
                    speed,
                    lock,
                    dim(played(p))
                );
                let mut seen = rustc_hash::FxHashSet::default();
                for a in p.alternates.iter().filter(|a| seen.insert(a.name.clone())) {
                    let note = if a.name == p.track.display_name() {
                        format!("duplicate file: {}", a.file_path)
                    } else if a.same_audio {
                        format!("same audio as {}", a.name)
                    } else {
                        format!("shares material with {}", a.name)
                    };
                    say!("{:>15}{}", "", dim(note));
                }
            }
            Row::Gap(s, e) => {
                say!("{:>3}  {:>8}  {}", dim("?"), clock(s), dim(format!("not in the library ({})", clock(e - s))));
            }
        }
    }
}

/// The dimmed end of a play's row: the stretch of the track played, and that the match is
/// tentative when it is (as the HTML report and the TUI say).
fn played(p: &Play) -> String {
    let stretch = format!("{}–{} of {}", clock(p.track_start), clock(p.track_end), clock(p.track.duration_secs as f64));
    match p.is_tentative() {
        true => format!("{stretch} · tentative ({:.0}%)", p.confidence * 100.0),
        false => stretch,
    }
}

/// Where a report goes: `--html` as given (a directory when there are several mixes), or the
/// store's reports folder for `--open` on its own.
fn report_path(html: Option<&Path>, count: usize, r: &MixReport, store: &Store) -> PathBuf {
    match html {
        Some(p) => in_folder(p, count, r, "html"),
        None => report::report_file(store, r, "html"),
    }
}

/// Where `--export` writes a mix's data.
fn export_path(target: &Path, count: usize, r: &MixReport) -> PathBuf {
    in_folder(target, count, r, "mix.json")
}

/// `target` as given, or `<id>.<ext>` inside it (see [`report::id`]) when it's a folder (one there,
/// or one to make, named with a separator at the end) or there are several mixes.
fn in_folder(target: &Path, count: usize, r: &MixReport, ext: &str) -> PathBuf {
    if target.is_dir() || count > 1 || target.as_os_str().to_string_lossy().ends_with(std::path::is_separator) {
        target.join(format!("{}.{ext}", report::id(r)))
    } else {
        target.to_path_buf()
    }
}

#[cfg(test)]
mod tests;
