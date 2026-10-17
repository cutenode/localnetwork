//! `locallibrary export`: playlists, folders, history lists or a whole library as M3U8, CSV or
//! text, or the library for other DJ software (`--format traktor|serato|engine`), or in a format
//! a plugin adds.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::builder::{PossibleValue, PossibleValuesParser};
use clap::{Args, ValueEnum};
use locallibrary::api;
use locallibrary::export::{Format, PathStyle};
use locallibrary::interop::{InteropOptions, InteropPlan, Software};
use locallibrary::progress::Ctx;
use loclib_plugin::ExportSource;

use crate::output::{Outcome, Output, bytes, dim, heading, ok, warn};

#[derive(Args)]
#[command(after_long_help = "Examples:
  locallibrary export Home \"Sets/2026/Friday\" -o friday.m3u8
  locallibrary export Home Sets --split -o ~/Desktop/sets          a file per playlist
  locallibrary export Home --format traktor -o ~/Desktop/for-traktor --dry-run")]
pub struct ExportArgs {
    #[arg(help = crate::LIBRARY_HELP)]
    drive: String,
    /// Playlist or folder (Folder/Name, or a unique name) [default: the whole library]
    playlist: Option<String>,
    /// A history list (name or path) instead of a playlist
    #[arg(long, conflicts_with = "playlist")]
    history: Option<String>,
    /// File format, the DJ software to write for, or a format a plugin adds [default: from the
    /// file name, else m3u8]
    #[arg(long, value_parser = formats())]
    format: Option<String>,
    /// File to write (with --split, the folder to write into; for other DJ software, the folder to write into)
    #[arg(long, short)]
    output: PathBuf,
    /// How paths to the audio are written (Engine DJ: relative to its Engine Library folder, or drive for one copied onto the drive)
    #[arg(long, value_enum, default_value_t = PathsArg::Relative)]
    paths: PathsArg,
    /// For a folder: one file per playlist (in folders like the playlist tree's) instead of one file
    #[arg(long)]
    split: bool,
    /// Other DJ software: where the drive's Contents/ will be when it reads the files [default: the drive's mount point]
    #[arg(long, value_name = "DIR")]
    location_root: Option<PathBuf>,
    /// Other DJ software: leave out beat grids and cues
    #[arg(long)]
    no_analysis: bool,
    /// Replace files already there
    #[arg(long)]
    replace: bool,
    /// Other DJ software and plugins' formats: list what would be written, and write nothing
    #[arg(long)]
    dry_run: bool,
    /// A plugin's format: an option for it, like key=value (repeatable; `locallibrary plugin
    /// info` shows them)
    #[arg(long = "option", value_name = "KEY=VALUE")]
    options: Vec<String>,
    /// Serato: copy the audio into the output folder and write cue points into the copies' tags (asks first)
    #[arg(long)]
    write_tags: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum FormatArg {
    /// Extended M3U in UTF-8 (#EXTM3U / #EXTINF), for media players and DJ software
    #[value(alias = "m3u")]
    M3u8,
    /// Spreadsheet: title, artist, album, genre, BPM, key, rating, duration, comment, path...
    Csv,
    /// "01. Artist - Title", one per line
    #[value(alias = "text")]
    Txt,
}

impl From<FormatArg> for Format {
    fn from(f: FormatArg) -> Self {
        match f {
            FormatArg::M3u8 => Format::M3u8,
            FormatArg::Csv => Format::Csv,
            FormatArg::Txt => Format::Txt,
        }
    }
}

/// `export --format`: the list formats, and other DJ software's.
#[derive(Clone, Copy, ValueEnum)]
pub enum ExportFormatArg {
    /// Extended M3U in UTF-8 (#EXTM3U / #EXTINF), for media players and DJ software
    #[value(alias = "m3u")]
    M3u8,
    /// Spreadsheet: title, artist, album, genre, BPM, key, rating, duration, comment, path...
    Csv,
    /// "01. Artist - Title", one per line
    #[value(alias = "text")]
    Txt,
    /// Traktor Pro: collection.nml with tracks, beat grids, cues and playlists (-o is a folder)
    Traktor,
    /// Serato DJ: a crate per playlist (-o is a folder; --write-tags for cue points)
    Serato,
    /// Engine DJ: a new Engine Library with tracks and playlists (-o is a folder; --paths drive to put it on the drive)
    Engine,
}

impl ExportFormatArg {
    /// The list format, or the DJ software the format is for.
    fn kind(self) -> Result<Format, Software> {
        match self {
            ExportFormatArg::M3u8 => Ok(Format::M3u8),
            ExportFormatArg::Csv => Ok(Format::Csv),
            ExportFormatArg::Txt => Ok(Format::Txt),
            ExportFormatArg::Traktor => Err(Software::Traktor),
            ExportFormatArg::Serato => Err(Software::Serato),
            ExportFormatArg::Engine => Err(Software::Engine),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PathsArg {
    /// Relative to the file written (keeps working when the file sits on the drive)
    Relative,
    /// The drive's own path: /Contents/...
    Drive,
    /// Absolute on this computer, through the drive's mount point
    Absolute,
}

impl From<PathsArg> for PathStyle {
    fn from(p: PathsArg) -> Self {
        match p {
            PathsArg::Relative => PathStyle::Relative,
            PathsArg::Drive => PathStyle::Drive,
            PathsArg::Absolute => PathStyle::Absolute,
        }
    }
}

/// `--format`'s values: the list formats, other DJ software's, and those plugins add.
fn formats() -> PossibleValuesParser {
    let own = ExportFormatArg::value_variants().iter().filter_map(ValueEnum::to_possible_value);
    let plugins = crate::plugins::checked().into_iter().flat_map(|p| p.manifests());
    let added = plugins.flat_map(|m| &m.contributes.exporters).map(|e| PossibleValue::new(&e.format).help(&e.title));
    PossibleValuesParser::new(own.chain(added).collect::<Vec<_>>())
}

pub fn run(args: ExportArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let format = match args.format.as_deref().map(|f| (f, ExportFormatArg::from_str(f, true))) {
        Some((f, Err(_))) => return run_plugin(f.to_string(), args, store, out, ctx),
        Some((_, Ok(f))) => Some(f),
        None => None,
    };
    if !args.options.is_empty() {
        bail!("--option is for plugins' formats");
    }
    let format = match format.map(ExportFormatArg::kind) {
        Some(Err(software)) => return run_interop(args, software, out, ctx),
        Some(Ok(f)) => Some(f),
        None => None,
    };
    if args.location_root.is_some() || args.no_analysis || args.dry_run || args.write_tags {
        bail!("--location-root, --no-analysis, --dry-run and --write-tags are for other DJ software's formats");
    }
    let req = api::ExportPlaylist {
        drive: args.drive,
        playlist: args.playlist,
        history: args.history,
        archived: true,
        format,
        output: args.output,
        paths: args.paths.into(),
        split: args.split,
        replace: args.replace,
    };
    let store = crate::diff_cmd::existing_store(store)?;
    let report = api::export_playlist(store.as_ref(), &req, ctx)?;
    out.done(&report, |r| {
        if r.files.is_empty() {
            ok("no playlists there; nothing written");
        }
        for f in &r.files {
            ok(format!("wrote {} ({}, {} tracks)", f.path.display(), f.name, f.tracks));
        }
    });
    Ok(ExitCode::SUCCESS)
}

/// `--format traktor|serato|engine`: the library for other DJ software, in a folder on this
/// computer.
fn run_interop(args: ExportArgs, software: Software, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    if args.history.is_some() || args.split {
        bail!("--history and --split don't apply to {}", software.name());
    }
    let req = api::InteropRequest {
        drive: args.drive,
        software,
        output: args.output,
        options: InteropOptions {
            location_root: args.location_root,
            only: args.playlist.into_iter().collect(),
            skip_analysis: args.no_analysis,
            replace: args.replace,
            write_tags: args.write_tags,
            paths: args.paths.into(),
        },
    };
    let plan = api::interop_plan(&req, ctx)?;
    if !out.json {
        render_plan(&plan, args.dry_run);
    }
    // Tags mean copying the audio: ask first.
    let question = format!(
        "Copy {} of audio into {} and write Serato's cue tags into the copies?",
        bytes(plan.copy_bytes),
        plan.output.display()
    );
    let stop = match args.dry_run || !args.write_tags {
        true => args.dry_run.then_some(Outcome::DryRun),
        false => out.decide(false, &question),
    };
    if let Some(o) = stop {
        if !out.json {
            say!("{}", dim("nothing was written"));
        }
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(None, ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || {
        ok(format!("wrote {} file(s) for {} to {}", report.files.len(), software.name(), report.output.display()))
    }))
}

/// A plugin's format: the plugin says which files it would write, overwriting any is asked about
/// first (unless `--replace`), then it writes them.
fn run_plugin(
    format: String,
    args: ExportArgs,
    store: &Option<PathBuf>,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    if args.split
        || args.location_root.is_some()
        || args.no_analysis
        || args.write_tags
        || args.paths != PathsArg::Relative
    {
        bail!("--split, --paths, --location-root, --no-analysis and --write-tags are for locallibrary's own formats");
    }
    let plugins = crate::plugins::host()?;
    let (plugin, exporter) = plugins
        .manifests()
        .find_map(|m| Some((m.id.as_str(), m.contributes.exporters.iter().find(|e| e.format == format)?)))
        .expect("--format takes the formats there are");
    let options = crate::plugin_cmd::options(exporter.options.as_ref(), &args.options)?;
    let source = ExportSource {
        drive: args.drive,
        playlists: args.playlist.into_iter().collect(),
        history: args.history.into_iter().collect(),
    };
    let output = std::path::absolute(&args.output)?;
    let store = crate::diff_cmd::existing_store(store)?;
    let plan = plugins.export_plan(plugin, &format, source, &output, options, store.as_ref(), ctx)?;
    if !out.json {
        crate::output::summary(&plan.summary, &plan.warnings);
        for f in &plan.overwrites {
            say!("    {} {}", f.display(), dim("(already there)"));
        }
    }
    if plan.noop {
        return Ok(out.nothing(&plan, "nothing to write"));
    }
    let stop = match args.dry_run {
        true => Some(Outcome::DryRun),
        false if plan.destructive && !args.replace => out.decide(false, "Overwrite them?"),
        false => None,
    };
    if let Some(o) = stop {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(store.as_ref(), ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || {
        for w in &report.warnings {
            warn(w);
        }
        ok(format!("wrote {} file(s) to {}", report.files.len(), output.display()))
    }))
}

fn render_plan(p: &InteropPlan, dry_run: bool) {
    heading(&format!(
        "{} for {}: {} tracks, {} playlists in {} folders, {} beat grids, {} cues",
        if dry_run { "Would write" } else { "Writing" },
        p.software.name(),
        p.tracks,
        p.playlists,
        p.folders,
        p.beat_grids,
        p.cues
    ));
    if p.copy_bytes > 0 {
        say!("  {}", dim(format!("{} of audio to copy", bytes(p.copy_bytes))));
    }
    for f in p.files.iter().take(20) {
        let replaces = if f.exists { " (already there)" } else { "" };
        say!("  {}  {}{}", f.path.display(), dim(format!("{}, {}", f.what, bytes(f.bytes))), replaces);
    }
    if p.files.len() > 20 {
        say!("  {}", dim(format!("... and {} more", p.files.len() - 20)));
    }
    for n in &p.notes {
        say!("  {}", dim(n));
    }
    for w in &p.warnings {
        warn(w);
    }
}
