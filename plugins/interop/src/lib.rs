//! The interop plugin: a drive's library for other DJ software, written by loclib-interop into a
//! folder on this computer: Traktor's `collection.nml`, Serato's crates (with cue points in tags of
//! copies of the audio on request) and a new Engine DJ library. It reads the library exports are
//! made from (locallibrary's `combined` one) and the tracks' beat grids and cues through the host,
//! and the audio, for Serato's copies, from the drive itself, which it never writes to.
//!
//! Tracks point at their audio where it will be when the other software reads the files: the
//! drive's mount point, or another folder holding a copy of the drive's `Contents/`
//! (`location_root`).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use loclib_interop::{Error, Export, Software, Source, engine, serato, traktor};
use loclib_model::analysis::{Beat, Cue, TrackAnalysis};
use loclib_model::paths::{self, PathStyle};
use loclib_model::{Library, Track};
use loclib_plugin::{Exported, ExportedFile, Host, Manifest, Plugin, RpcError, WriteExport, code, params};
use rustc_hash::FxHashMap;
use serde::Deserialize;
use serde_json::{Value, json};

mod write;

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    let manifest = Manifest::parse(MANIFEST).expect("the manifest parses");
    loclib_plugin::serve(&manifest, Interop, input, output);
}

struct Interop;

/// An exporter's options, as the RPC sidecar's `interop.plan` names them: each exporter takes
/// those its schema lists.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Options {
    /// The folder that will hold the drive's `Contents/` when the other software reads the files.
    /// Default: the drive's mount point.
    location_root: Option<PathBuf>,
    /// Leave out beat grids and cues.
    skip_analysis: bool,
    /// Serato: copy the audio into the output folder and write cue points into the copies' tags
    /// (the drive's files are never changed); the crates list the copies.
    write_tags: bool,
    /// Engine DJ: track paths relative to where the library is written (`relative`), or to the
    /// drive's root (`drive`, for an `Engine Library` folder copied there).
    paths: PathStyle,
}

/// `drive.info`'s answer, as far as this plugin reads it.
#[derive(Deserialize)]
struct DriveInfo {
    root: PathBuf,
}

/// `drive.library`'s answer, as far as this plugin reads it.
#[derive(Deserialize)]
struct LibraryPage {
    library: Library,
    page: Option<Page>,
}

#[derive(Deserialize)]
struct Page {
    total: usize,
}

/// `tracks.analysis`'s answer, as far as this plugin reads it.
#[derive(Deserialize)]
struct Analyses {
    tracks: Vec<Analysed>,
}

#[derive(Deserialize)]
struct Analysed {
    track: u32,
    analysis: Grid,
}

#[derive(Deserialize)]
struct Grid {
    beats: Vec<Beat>,
    cues: Vec<Cue>,
}

/// The most tracks `tracks.analysis` takes at once.
const ANALYSES_AT_ONCE: usize = 100;

impl Plugin for Interop {
    /// Works out the export in memory, as loclib-interop does, and with `dry_run` answers the
    /// files it would write; otherwise writes them, below `output`.
    fn write_export(&self, req: WriteExport, host: &Host) -> Result<Exported, RpcError> {
        let software: Software = params(Value::from(req.exporter))?;
        let options: Options = params::<Option<Options>>(req.options)?.unwrap_or_default();
        if !req.source.history.is_empty() {
            return Err(invalid(format!("{} doesn't take history lists", software.name())));
        }
        if let Some(root) = options.location_root.as_deref().filter(|r| !r.is_absolute()) {
            return Err(invalid(format!("the location root must be an absolute path, not {}", root.display())));
        }
        if options.write_tags && options.location_root.is_some() {
            return Err(invalid("a location root doesn't go with writing tags: the crates list the tagged copies"));
        }
        let drive = req.source.drive.as_str();
        let drive_root = host.call::<_, DriveInfo>("drive.info", &json!({ "drive": drive }))?.root;
        let mut library = library(host, drive)?;
        let renumbered = library.number_for_files();
        let library =
            library.subset(&req.source.playlists).map_err(|query| invalid(format!("not found: playlist {query:?}")))?;
        // Engine keeps no analysis, and Serato's crates alone none either.
        let carried = match software {
            Software::Traktor => true,
            Software::Serato => options.write_tags,
            _ => false,
        };
        let analyses = match carried && !options.skip_analysis {
            true => analyses(host, drive, &library, &renumbered)?,
            false => FxHashMap::default(),
        };
        let load = |t: &Track| analyses.get(&t.id).cloned().unwrap_or_default();
        let analysis: &dyn Fn(&Track) -> TrackAnalysis = &load;
        let root = options.location_root.as_deref().unwrap_or(&drive_root);
        let source =
            Source { library: &library, root: resolved(root), analysis: (!options.skip_analysis).then_some(analysis) };
        // The writers find tracks from where the files go with symlinks resolved (macOS's `/tmp`
        // is `/private/tmp`); the files are answered below `output` as it was given.
        let output = resolved(&req.output);
        let ctx = host.ctx();
        let export = match software {
            Software::Traktor => traktor::build(&source, &req.output, &ctx),
            Software::Serato => {
                let drive_dir = resolved(&drive_root);
                let audio = |t: &Track| {
                    let from = paths::to_fs(&drive_root, &paths::key(&t.file_path));
                    match dunce::canonicalize(&from) {
                        Ok(real) if !real.starts_with(&drive_dir) => {
                            Err(Error::Invalid(format!("refusing {:?}: it resolves outside the drive", t.file_path)))
                        }
                        _ => Ok(from),
                    }
                };
                // With tags, the crates list copies of the audio below the output folder.
                let copies = serato::Copies { root: output.clone(), audio: &audio };
                serato::build(&source, &req.output, options.write_tags.then_some(&copies), &ctx)
            }
            _ => engine::build(&source, &output, options.paths, &ctx),
        }
        .map_err(failure)?;
        if !req.dry_run {
            write::write(&export, &output, &ctx)?;
        }
        let files = export.files.iter().map(|f| ExportedFile {
            path: f.path.strip_prefix(&output).map_or_else(|_| f.path.clone(), |rest| req.output.join(rest)),
            what: f.what.clone(),
            bytes: f.bytes,
        });
        // Copied audio is in the files, with its size.
        let counts = [
            ("tracks", export.tracks),
            ("playlists", export.playlists),
            ("folders", export.folders),
            ("beat grids", export.beat_grids),
            ("cues", export.cues),
        ];
        Ok(Exported {
            files: files.collect(),
            counts: counts.into_iter().map(|(label, n)| (label.to_string(), n as u64)).collect(),
            notes: notes(&export, software, &options),
            warnings: export.warnings.iter().cloned().chain(missing_audio(&source)).collect(),
        })
    }
}

/// The drive's library as exports read it (with what only OneLibrary has), a page of tracks at a
/// time (the host sizes them).
fn library(host: &Host, drive: &str) -> Result<Library, RpcError> {
    let mut tracks = Vec::new();
    loop {
        let read = json!({ "drive": drive, "combined": true, "offset": tracks.len() });
        let LibraryPage { mut library, page } = host.call("drive.library", &read)?;
        tracks.append(&mut library.tracks);
        if page.is_none_or(|p| tracks.len() >= p.total) {
            return Ok(Library { tracks, ..library });
        }
    }
}

/// The beat grid and cues of each of `library`'s tracks, by its id, asked for by the id the host
/// knows it by (`renumbered` has those that differ).
fn analyses(
    host: &Host,
    drive: &str,
    library: &Library,
    renumbered: &FxHashMap<u32, u32>,
) -> Result<FxHashMap<u32, TrackAnalysis>, RpcError> {
    let host_ids: Vec<u32> = library.tracks.iter().map(|t| renumbered.get(&t.id).copied().unwrap_or(t.id)).collect();
    let ids: FxHashMap<u32, u32> = host_ids.iter().zip(&library.tracks).map(|(host_id, t)| (*host_id, t.id)).collect();
    let mut out = FxHashMap::default();
    for asked in host_ids.chunks(ANALYSES_AT_ONCE) {
        let read = json!({ "drive": drive, "combined": true, "ids": asked, "waveform": "none" });
        let Analyses { tracks } = host.call("tracks.analysis", &read)?;
        out.extend(tracks.into_iter().filter_map(|Analysed { track, analysis: Grid { beats, cues } }| {
            Some((*ids.get(&track)?, TrackAnalysis { beats, cues, ..Default::default() }))
        }));
    }
    Ok(out)
}

/// What the format leaves out and what to do with the files: the formats' notes, naming the
/// options that choose loclib-interop's.
fn notes(export: &Export, software: Software, options: &Options) -> Vec<String> {
    let mut notes: Vec<String> = (export.notes.iter())
        .map(|n| n.replace("`PathStyle::Drive`", "paths = drive").replace("`Copies`", "write_tags"))
        .collect();
    if software == Software::Engine && options.skip_analysis {
        notes.push("skip_analysis changes nothing here: no analysis is written anyway.".into());
    }
    notes
}

/// Tracks whose audio isn't where the files will say it is, as a warning.
fn missing_audio(source: &Source<'_>) -> Option<String> {
    let missing: Vec<&Track> = source.library.tracks.iter().filter(|t| !source.audio(t).is_file()).collect();
    let first = missing.first()?;
    Some(format!(
        "{} track(s) have no audio under {} (e.g. {}); they're listed anyway",
        missing.len(),
        source.root.display(),
        first.file_path
    ))
}

/// `path` (absolute) with its symlinks resolved as far as it's there, so two paths compare, and
/// relative paths between them lead where they say.
fn resolved(path: &Path) -> PathBuf {
    let found = path.ancestors().find_map(|a| Some((dunce::canonicalize(a).ok()?, path.strip_prefix(a).ok()?)));
    found.map_or_else(|| path.to_path_buf(), |(there, rest)| there.join(rest))
}

/// What can't be done as asked, as locallibrary's `invalid` error.
fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(code::INVALID_PARAMS, "invalid", message)
}

/// loclib-interop's error as the host takes it.
fn failure(e: Error) -> RpcError {
    match e {
        Error::Cancelled(_) => RpcError::cancelled(),
        Error::Io { .. } => RpcError::failed("io", e.to_string()),
        Error::Tags(_) => RpcError::failed("tags", e.to_string()),
        // `Invalid`, and a variant a newer loclib-interop adds, with its message.
        other => invalid(other.to_string()),
    }
}

#[cfg(test)]
mod tests;
