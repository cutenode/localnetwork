//! A drive's library for other DJ software: Traktor's `collection.nml` ([`traktor`]), Serato's
//! crates, with cue points in tags of copies of the audio on request ([`serato`]), and a new
//! Engine DJ library ([`engine`]).
//!
//! Everything is read from the drive and written below an output folder on this computer, never
//! on the drive. [`plan`] works out every file and what the format leaves out without writing
//! anything; [`apply`] writes them, refusing to replace files already there unless asked (an
//! output folder could be the other software's own).
//!
//! Tracks point at their audio where it will be when the other software reads the files: the
//! drive's mount point, or another folder holding a copy of the drive's `Contents/`
//! ([`InteropOptions::location_root`]).

pub mod engine;
pub mod serato;
pub mod traktor;

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::device::Device;
use crate::export::PathStyle;
use crate::model::{Library, Track};
use crate::paths;
use crate::progress::Ctx;
use crate::{Error, Result, fsutil};

/// The DJ software to write for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Software {
    /// Native Instruments Traktor Pro: `collection.nml`.
    Traktor,
    /// Serato DJ: `_Serato_/Subcrates/*.crate`.
    Serato,
    /// Engine DJ: `Engine Library/Database2/m.db`.
    Engine,
}

impl Software {
    pub fn name(self) -> &'static str {
        match self {
            Software::Traktor => "Traktor",
            Software::Serato => "Serato",
            Software::Engine => "Engine DJ",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct InteropOptions {
    /// The folder that will hold the drive's `Contents/` when the other software reads the files.
    /// Default: the drive's mount point.
    #[serde(default)]
    pub location_root: Option<PathBuf>,
    /// Only these playlists and folders (paths like `Folder/Name`, or unique names) and their
    /// tracks. Everything when empty.
    #[serde(default)]
    pub only: Vec<String>,
    /// Leave out beat grids and cues.
    #[serde(default)]
    pub skip_analysis: bool,
    /// Replace files the output folder already holds.
    #[serde(default)]
    pub replace: bool,
    /// Serato: copy the audio into the output folder and write cue points into the copies' tags
    /// (the drive's files are never changed); the crates list the copies.
    #[serde(default)]
    pub write_tags: bool,
    /// Engine DJ: track paths relative to where the library is written (`relative`, the
    /// default), or to the drive's root (`drive`, for an `Engine Library` folder copied there).
    #[serde(default)]
    pub paths: PathStyle,
}

/// What an export writes, and what it leaves out.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct InteropPlan {
    pub software: Software,
    pub output: PathBuf,
    pub files: Vec<PlannedFile>,
    pub tracks: usize,
    pub playlists: usize,
    pub folders: usize,
    pub beat_grids: usize,
    /// Hot cues, memory cues and loops carried across.
    pub cues: usize,
    /// Bytes of audio copied (Serato with `write_tags`).
    pub copy_bytes: u64,
    /// What the format can't hold, and what to do with the files.
    pub notes: Vec<String>,
    /// Problems with this library: missing audio, cues the format has no room for...
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlannedFile {
    pub path: PathBuf,
    /// What it holds.
    pub what: String,
    pub bytes: u64,
    /// A file is already there (it's replaced only with [`InteropOptions::replace`]).
    pub exists: bool,
}

/// How a planned file is made.
pub(crate) enum Content {
    Bytes(Vec<u8>),
    /// A copy of an audio file on the drive, with Serato's cue tag written into it when there's
    /// one.
    TaggedCopy {
        from: PathBuf,
        markers: Option<Vec<u8>>,
    },
}

/// An export worked out in memory: the plan and how to make every file.
pub(crate) struct Built {
    pub plan: InteropPlan,
    pub files: Vec<(PathBuf, Content)>,
    /// Files that must go when the export replaces what's there (a database's old WAL, which
    /// SQLite would replay into the new file).
    pub remove: Vec<PathBuf>,
}

impl Built {
    pub(crate) fn new(software: Software, output: &Path) -> Built {
        Built {
            plan: InteropPlan {
                software,
                output: output.to_path_buf(),
                files: Vec::new(),
                tracks: 0,
                playlists: 0,
                folders: 0,
                beat_grids: 0,
                cues: 0,
                copy_bytes: 0,
                notes: Vec::new(),
                warnings: Vec::new(),
            },
            files: Vec::new(),
            remove: Vec::new(),
        }
    }

    pub(crate) fn file(&mut self, path: PathBuf, what: impl Into<String>, bytes: Vec<u8>) {
        self.plan.files.push(PlannedFile {
            exists: path.exists(),
            path: path.clone(),
            what: what.into(),
            bytes: bytes.len() as u64,
        });
        self.files.push((path, Content::Bytes(bytes)));
    }

    pub(crate) fn copy(&mut self, path: PathBuf, what: impl Into<String>, bytes: u64, content: Content) {
        self.plan.copy_bytes += bytes;
        self.plan.files.push(PlannedFile { exists: path.exists(), path: path.clone(), what: what.into(), bytes });
        self.files.push((path, content));
    }
}

/// What's read from the drive for an export: the library (cut down to `only`) and where its audio
/// will be.
pub(crate) struct Source<'a> {
    pub device: &'a Device,
    /// The drive's, its trees untangled as they were read (see [`crate::model::untangle`]): every
    /// walk down them ends.
    pub library: Library,
    /// The folder holding `Contents/` when the other software reads the files.
    pub root: PathBuf,
}

impl Source<'_> {
    /// Where a track's audio will be.
    pub(crate) fn audio(&self, t: &Track) -> PathBuf {
        paths::to_fs(&self.root, &paths::key(&t.file_path))
    }

    /// Tracks whose audio isn't where the files will say it is, as a warning.
    pub(crate) fn missing_audio(&self) -> Option<String> {
        let missing: Vec<&Track> = self.library.tracks.iter().filter(|t| !self.audio(t).is_file()).collect();
        let first = missing.first()?;
        Some(format!(
            "{} track(s) have no audio under {} (e.g. {}); they're listed anyway",
            missing.len(),
            self.root.display(),
            first.file_path
        ))
    }
}

/// Work out an export of `device`'s library for `software` into `output`, without writing.
pub fn plan(
    device: &Device,
    software: Software,
    output: &Path,
    opts: &InteropOptions,
    ctx: &Ctx<'_>,
) -> Result<InteropPlan> {
    Ok(build(device, software, output, opts, ctx)?.plan)
}

/// Export `device`'s library for `software` into `output`.
pub fn apply(
    device: &Device,
    software: Software,
    output: &Path,
    opts: &InteropOptions,
    ctx: &Ctx<'_>,
) -> Result<InteropPlan> {
    let built = build(device, software, output, opts, ctx)?;
    let written: Vec<&Path> =
        built.plan.files.iter().map(|f| f.path.as_path()).chain(built.remove.iter().map(PathBuf::as_path)).collect();
    fsutil::refuse_to_replace(&written, opts.replace)?;
    // Every file is staged next to where it goes before any is put in place, so the files being
    // replaced stay until the new ones are all written.
    let needed: u64 = built.plan.files.iter().map(|f| f.bytes).sum();
    let out = fsutil::absolute(output);
    let existing_dir = out.ancestors().find(|a| a.exists()).unwrap_or(&out);
    if let Some(available) = fsutil::available_space(existing_dir)
        && needed > available
    {
        return Err(Error::NoSpace { path: out, needed, available });
    }
    let mut staged = Vec::with_capacity(built.files.len());
    for (path, content) in &built.files {
        ctx.check()?;
        let tmp = match content {
            Content::Bytes(bytes) => fsutil::stage_bytes(path, bytes)?,
            Content::TaggedCopy { from, markers } => {
                let (tmp, _) = fsutil::stage_copy(from, path, None, None, ctx)?;
                if let Some(m) = markers {
                    serato::write_markers(tmp.path(), path, m)?;
                }
                tmp
            }
        };
        // Closed until it's put in place: a copy of every track open at once would run out of
        // file handles (macOS gives an app 256).
        staged.push(tmp.into_temp_path());
    }
    // A failure or a cancel before here leaves an old Engine database its WAL (which holds its
    // latest changes).
    for stale in &built.remove {
        fsutil::remove_if_exists(stale)?;
    }
    for ((path, _), tmp) in built.files.iter().zip(staged) {
        tmp.persist(path).map_err(|e| Error::Io { path: path.clone(), source: e.error })?;
    }
    Ok(built.plan)
}

fn build(device: &Device, software: Software, output: &Path, opts: &InteropOptions, ctx: &Ctx<'_>) -> Result<Built> {
    outside(device, output)?;
    if opts.write_tags && software != Software::Serato {
        return Err(Error::invalid(format!(
            "{} keeps cue points in its own files: tags aren't written",
            software.name()
        )));
    }
    if opts.paths != PathStyle::Relative && software != Software::Engine {
        return Err(Error::invalid(format!("{} locates tracks by their full path", software.name())));
    }
    let library = crate::xml::subset(device.library()?, &opts.only)?;
    let root = opts.location_root.clone().unwrap_or_else(|| device.root().to_path_buf());
    let source = Source { device, library, root: fsutil::resolved(&root) };
    match software {
        Software::Traktor => traktor::build(&source, output, opts, ctx),
        Software::Serato => serato::build(&source, output, opts, ctx),
        Software::Engine => engine::build(&source, output, opts, ctx),
    }
}

/// Refuse an output folder on the drive: exports are for this computer.
fn outside(device: &Device, output: &Path) -> Result<()> {
    if fsutil::resolved(output).starts_with(fsutil::resolved(device.root())) {
        return Err(Error::invalid(format!(
            "{} is on the drive; exports are written to this computer, never to the drive",
            output.display()
        )));
    }
    Ok(())
}

/// A track's beat grid as one tempo and the first downbeat, for formats with a constant-tempo
/// grid. `None` without a grid. The flag says the tempo changes within the track.
pub(crate) fn constant_grid(beats: &[crate::anlz::Beat], track_bpm: f64) -> Option<(f64, u32, bool)> {
    let first = beats.iter().find(|b| b.beat_number == 1).or(beats.first())?;
    let bpm = if first.bpm > 0.0 { first.bpm } else { track_bpm };
    let varies = beats.iter().any(|b| (b.bpm - bpm).abs() > 0.005);
    (bpm > 0.0).then_some((bpm, first.time_ms, varies))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anlz::Beat;

    #[test]
    fn grids_start_on_the_first_downbeat() {
        let beat = |n, bpm, ms| Beat { beat_number: n, bpm, time_ms: ms };
        let beats = [beat(3, 128.0, 100), beat(4, 128.0, 569), beat(1, 128.0, 1038)];
        assert_eq!(constant_grid(&beats, 128.0), Some((128.0, 1038, false)));
        let changing = [beat(1, 120.0, 0), beat(2, 121.0, 500)];
        assert_eq!(constant_grid(&changing, 120.0), Some((120.0, 0, true)));
        assert_eq!(constant_grid(&[], 128.0), None);
    }
}
