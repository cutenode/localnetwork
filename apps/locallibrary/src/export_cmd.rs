//! `locallibrary export`: playlists, folders, history lists or a whole library as M3U8, CSV or
//! text, or in a format a plugin adds (the interop plugin's for other DJ software: `--format
//! traktor|serato|engine|rekordbox`, which `locallibrary xml` writes too). A plugin's formats for
//! sets and mixes are `sets export`'s and `mix`'s, written out the same way ([`plugin_export`]).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::builder::{PossibleValue, PossibleValuesParser};
use clap::{Args, ValueEnum};
use locallibrary::Store;
use locallibrary::api;
use locallibrary::export::{Format, PathStyle};
use locallibrary::op::Planned;
use locallibrary::plugin::{ExportFile, PluginExport};
use locallibrary::progress::Ctx;
use loclib_plugin::{ExportFrom, ExportSource};

use crate::output::{Outcome, Output, bytes, dim, ok, warn};

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
    /// File format, or a format a plugin adds (Traktor, Serato and Engine DJ are the interop
    /// plugin's) [default: from the file name, else m3u8]
    #[arg(long, value_parser = formats())]
    format: Option<String>,
    /// File to write (with --split, or for a plugin's format that writes a folder, the folder to write into)
    #[arg(long, short)]
    output: PathBuf,
    /// How paths to the audio are written
    #[arg(long, value_enum, default_value_t = PathsArg::Relative)]
    paths: PathsArg,
    /// For a folder: one file per playlist (in folders like the playlist tree's) instead of one file
    #[arg(long)]
    split: bool,
    /// Replace files already there
    #[arg(long)]
    replace: bool,
    /// Plugins' formats: list what would be written, and write nothing
    #[arg(long)]
    dry_run: bool,
    /// A plugin's format: an option for it, like key=value (repeatable; `locallibrary plugin
    /// info` shows them)
    #[arg(long = "option", value_name = "KEY=VALUE")]
    options: Vec<String>,
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

/// `--format`'s values: the list formats, and those plugins add for a drive's library.
fn formats() -> PossibleValuesParser {
    let own = FormatArg::value_variants().iter().filter_map(ValueEnum::to_possible_value);
    PossibleValuesParser::new(own.chain(plugin_formats(ExportFrom::Drive)).collect::<Vec<_>>())
}

/// The formats plugins add for what `source` is, as a `--format` takes them.
pub(crate) fn plugin_formats(source: ExportFrom) -> Vec<PossibleValue> {
    let plugins = crate::plugins::checked().into_iter().flat_map(|p| p.manifests());
    let added = plugins.flat_map(|m| &m.contributes.exporters).filter(|e| e.takes(source));
    added.map(|e| PossibleValue::new(&e.format).help(&e.title)).collect()
}

pub fn run(args: ExportArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let format = match args.format.as_deref().map(|f| (f, FormatArg::from_str(f, true))) {
        Some((f, Err(_))) => return run_plugin(f.to_string(), args, store, out, ctx),
        Some((_, Ok(f))) => Some(Format::from(f)),
        None => None,
    };
    if !args.options.is_empty() {
        bail!("--option is for plugins' formats");
    }
    if args.dry_run {
        bail!("--dry-run is for plugins' formats");
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

/// A plugin's format, for a drive's library.
fn run_plugin(
    format: String,
    args: ExportArgs,
    store: &Option<PathBuf>,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    if args.split || args.paths != PathsArg::Relative {
        bail!("--split and --paths are for locallibrary's own formats");
    }
    let source = ExportSource {
        drive: args.drive,
        playlists: args.playlist.into_iter().collect(),
        history: args.history.into_iter().collect(),
        ..ExportSource::default()
    };
    let store = crate::diff_cmd::existing_store(store)?;
    let written =
        Written { output: &args.output, options: &args.options, dry_run: args.dry_run, replace: args.replace };
    plugin_export(&format, source, written, store.as_ref(), out, ctx)
}

#[derive(Args)]
#[command(after_long_help = "The interop plugin writes it, as `export --format rekordbox` does.

Examples:
  locallibrary xml Home -o ~/Desktop/Home.xml
  locallibrary xml Home --only \"Sets/2026\" -o sets.xml")]
pub struct XmlArgs {
    #[arg(help = "A library's name, or a drive's mount path, name or id prefix [default: the only mounted \
rekordbox drive, or the one library plugged in]")]
    drive: Option<String>,
    /// The file to write
    #[arg(long, short, default_value = "rekordbox.xml")]
    output: PathBuf,
    /// Where the audio will live when importing [default: the drive's mount point]
    #[arg(long)]
    location_root: Option<PathBuf>,
    /// Metadata and playlists only (no beat grids or cues)
    #[arg(long)]
    no_analysis: bool,
    /// Only this playlist or folder and its tracks (repeatable)
    #[arg(long = "only", value_name = "PLAYLIST")]
    only: Vec<String>,
    /// Replace the file if it's there already, without asking
    #[arg(long)]
    replace: bool,
}

/// `locallibrary xml`: the interop plugin's rekordbox XML (`export --format rekordbox`), its
/// flags as the exporter's playlists and options.
pub fn xml(args: XmlArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let drive = crate::jobs_cmd::drive_or_default(args.drive, store, "export")?;
    let root = args.location_root.map(|r| format!("location_root={}", r.display()));
    let skip = args.no_analysis.then(|| "skip_analysis=true".to_string());
    let options: Vec<String> = root.into_iter().chain(skip).collect();
    let source = ExportSource { drive, playlists: args.only, ..ExportSource::default() };
    let store = crate::diff_cmd::existing_store(store)?;
    let written = Written { output: &args.output, options: &options, dry_run: false, replace: args.replace };
    plugin_export("rekordbox", source, written, store.as_ref(), out, ctx)
}

/// Where a plugin's export goes and how: its `-o`, its `--option`s, `--dry-run` and `--replace`.
pub(crate) struct Written<'a> {
    pub output: &'a Path,
    pub options: &'a [String],
    pub dry_run: bool,
    pub replace: bool,
}

/// A plugin's format (`--format`), for what `source` names: the plugin says which files it would
/// write, overwriting any is asked about first (unless `--replace`), then it writes them.
pub(crate) fn plugin_export(
    format: &str,
    source: ExportSource,
    w: Written<'_>,
    store: Option<&Store>,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let plugins = crate::plugins::host()?;
    let found = plugins
        .manifests()
        .find_map(|m| Some((m.id.as_str(), m.contributes.exporters.iter().find(|e| e.format == format)?)));
    // `--format` takes the formats there are; `xml` names one, whose plugin could be left out.
    let Some((plugin, exporter)) = found else { bail!("no plugin exports {format}") };
    let options = crate::plugin_cmd::options("format", exporter.options.as_ref(), w.options)?;
    let output = std::path::absolute(w.output)?;
    let plan = plugins.export_plan(plugin, format, source, &output, options, store, ctx)?;
    if !out.json {
        render_export(&plan);
    }
    if plan.noop {
        return Ok(out.nothing(&plan, "nothing to write"));
    }
    let stop = match w.dry_run {
        true => Some(Outcome::DryRun),
        false if plan.destructive && !w.replace => out.decide(false, "Overwrite them?"),
        false => None,
    };
    if let Some(o) = stop {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(store, ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || {
        // Those the plan showed aren't said again.
        for w in report.warnings.iter().filter(|w| !plan.warnings.contains(w)) {
            warn(w);
        }
        ok(format!("wrote {} file(s) to {}", report.files.len(), output.display()))
    }))
}

/// How many of a plugin's export's files are listed before "and N more" (here and in the player).
pub(crate) const FILES_SHOWN: usize = 20;

/// What one of a plugin's export's files holds and its size, as far as the plugin says.
pub(crate) fn about(f: &ExportFile) -> String {
    let about =
        [(!f.file.what.is_empty()).then(|| f.file.what.clone()), (f.file.bytes > 0).then(|| bytes(f.file.bytes))];
    about.into_iter().flatten().collect::<Vec<_>>().join(", ")
}

/// A plugin's export as locallibrary's own were shown: what it writes and what goes in, its first
/// files (what each holds, and those there already), what the format can't hold, then problems
/// with the library.
fn render_export(p: &Planned<PluginExport>) {
    crate::output::summary(&p.summary, &[]);
    for f in p.files.iter().take(FILES_SHOWN) {
        let replaces = if f.exists { " (already there)" } else { "" };
        say!("  {}  {}{replaces}", f.file.path.display(), dim(about(f)));
    }
    if p.files.len() > FILES_SHOWN {
        say!("  {}", dim(format!("... and {} more", p.files.len() - FILES_SHOWN)));
    }
    for n in &p.notes {
        say!("  {}", dim(n));
    }
    for w in &p.warnings {
        warn(w);
    }
}
