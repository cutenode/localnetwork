//! A drive's library for other DJ software, written by loclib-interop: Traktor's `collection.nml`
//! ([`traktor`]), Serato's crates, with cue points in tags of copies of the audio on request
//! ([`serato`]), and a new Engine DJ library ([`engine`]).
//!
//! Everything is read from the drive and written below an output folder on this computer, never
//! on the drive. [`plan`] works out every file and what the format leaves out without writing
//! anything; [`apply`] writes them, refusing to replace files already there unless asked (an
//! output folder could be the other software's own).
//!
//! Tracks point at their audio where it will be when the other software reads the files: the
//! drive's mount point, or another folder holding a copy of the drive's `Contents/`
//! ([`InteropOptions::location_root`]).

pub use loclib_interop::{Software, engine, serato, traktor};

use std::path::{Path, PathBuf};

use loclib_interop::{Content, Export, Source};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::device::Device;
use crate::export::PathStyle;
use crate::model::Track;
use crate::progress::Ctx;
use crate::{Error, Result, anlz, fsutil, paths};

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

/// Work out an export of `device`'s library for `software` into `output`, without writing.
pub fn plan(
    device: &Device,
    software: Software,
    output: &Path,
    opts: &InteropOptions,
    ctx: &Ctx<'_>,
) -> Result<InteropPlan> {
    Ok(build(device, software, output, opts, ctx)?.0)
}

/// Export `device`'s library for `software` into `output`.
pub fn apply(
    device: &Device,
    software: Software,
    output: &Path,
    opts: &InteropOptions,
    ctx: &Ctx<'_>,
) -> Result<InteropPlan> {
    let (plan, export) = build(device, software, output, opts, ctx)?;
    let written: Vec<&Path> =
        plan.files.iter().map(|f| f.path.as_path()).chain(export.remove.iter().map(PathBuf::as_path)).collect();
    fsutil::refuse_to_replace(&written, opts.replace)?;
    // Every file is staged next to where it goes before any is put in place, so the files being
    // replaced stay until the new ones are all written.
    let needed: u64 = plan.files.iter().map(|f| f.bytes).sum();
    let out = fsutil::absolute(output);
    let existing_dir = out.ancestors().find(|a| a.exists()).unwrap_or(&out);
    if let Some(available) = fsutil::available_space(existing_dir)
        && needed > available
    {
        return Err(Error::NoSpace { path: out, needed, available });
    }
    let mut staged = Vec::with_capacity(export.files.len());
    for f in &export.files {
        ctx.check()?;
        let tmp = match &f.content {
            Content::Bytes(bytes) => fsutil::stage_bytes(&f.path, bytes)?,
            Content::TaggedCopy { from, markers } => {
                let (tmp, _) = fsutil::stage_copy(from, &f.path, None, None, ctx)?;
                if let Some(m) = markers {
                    serato::write_markers(tmp.path(), &f.path, m)?;
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
    for stale in &export.remove {
        fsutil::remove_if_exists(stale)?;
    }
    for (f, tmp) in export.files.iter().zip(staged) {
        tmp.persist(&f.path).map_err(|e| Error::Io { path: f.path.clone(), source: e.error })?;
    }
    Ok(plan)
}

/// The plan, and the export it's made from.
fn build(
    device: &Device,
    software: Software,
    output: &Path,
    opts: &InteropOptions,
    ctx: &Ctx<'_>,
) -> Result<(InteropPlan, Export)> {
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
    // The drive's, its trees untangled as they were read (see `model::untangle`): every walk down
    // them ends.
    let library = crate::xml::subset(crate::convert::combined_library(device)?, &opts.only)?;
    let root = opts.location_root.clone().unwrap_or_else(|| device.root().to_path_buf());
    let load = |t: &Track| anlz::load_grid(device, t);
    let analysis: &dyn Fn(&Track) -> anlz::TrackAnalysis = &load;
    let source = Source {
        library: &library,
        root: fsutil::resolved(&root),
        analysis: (!opts.skip_analysis).then_some(analysis),
    };
    let export = match software {
        Software::Traktor => traktor::build(&source, output, ctx)?,
        Software::Serato => {
            if opts.write_tags && opts.location_root.is_some() {
                return Err(Error::invalid(
                    "a location root doesn't go with writing tags: the crates list the tagged copies",
                ));
            }
            let audio = |t: &Track| device.checked_path(&paths::key(&t.file_path));
            // With tags, the crates list copies of the audio below the output folder.
            let copies = serato::Copies { root: fsutil::resolved(output), audio: &audio };
            serato::build(&source, output, opts.write_tags.then_some(&copies), ctx)?
        }
        Software::Engine => engine::build(&source, &fsutil::resolved(output), opts.paths, ctx)?,
        // Software a newer loclib-interop writes that this version doesn't build for yet.
        other => return Err(Error::invalid(format!("{} exports aren't supported yet", other.name()))),
    };
    // The formats' notes name loclib-interop's options, which people choose with these flags.
    let mut notes: Vec<String> = (export.notes.iter())
        .map(|n| n.replace("`PathStyle::Drive`", "--paths drive").replace("`Copies`", "--write-tags"))
        .collect();
    if software == Software::Engine && opts.skip_analysis {
        notes.push("--no-analysis changes nothing here: no analysis is written anyway.".into());
    }
    let mut warnings = export.warnings.clone();
    warnings.extend(missing_audio(&source));
    let plan = InteropPlan {
        software,
        output: output.to_path_buf(),
        files: export
            .files
            .iter()
            .map(|f| PlannedFile {
                path: f.path.clone(),
                what: f.what.clone(),
                bytes: f.bytes,
                exists: f.path.exists(),
            })
            .collect(),
        tracks: export.tracks,
        playlists: export.playlists,
        folders: export.folders,
        beat_grids: export.beat_grids,
        cues: export.cues,
        copy_bytes: export.copy_bytes,
        notes,
        warnings,
    };
    Ok((plan, export))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_go_into_wav_and_aiff_tags() {
        use lofty::config::ParseOptions;
        use lofty::file::AudioFile;
        use lofty::id3::v2::{Frame, FrameFlags, GeneralEncapsulatedObject, Id3v2Tag};

        use serato::{Markers, write_markers};

        let dir = tempfile::tempdir().unwrap();
        let m =
            Markers { colour: [0xff; 3], cues: vec![(3, 1234, [0xcc, 0xcc, 0x00], "x".into())], ..Default::default() };
        let (wav, aiff) = (crate::probe::tests::wav(44_100, 16, 0.0001), crate::probe::tests::aiff(4));
        let geob = |tag: &Id3v2Tag| -> Vec<Vec<u8>> {
            tag.into_iter()
                .filter_map(|f| match f {
                    Frame::Binary(b) if b.id().as_str() == "GEOB" => {
                        GeneralEncapsulatedObject::parse(&b.data, FrameFlags::default()).ok().map(|g| g.data)
                    }
                    _ => None,
                })
                .collect()
        };
        let options = ParseOptions::new().read_properties(false);
        let path = dir.path().join("a.wav");
        std::fs::write(&path, &wav).unwrap();
        // Twice: the second replaces the first rather than adding another.
        write_markers(&path, &path, &m.encode()).unwrap();
        write_markers(&path, &path, &m.encode()).unwrap();
        let file = lofty::iff::wav::WavFile::read_from(&mut std::fs::File::open(&path).unwrap(), options).unwrap();
        assert_eq!(geob(file.id3v2().unwrap()), [m.encode()]);

        let path = dir.path().join("a.aiff");
        std::fs::write(&path, &aiff).unwrap();
        write_markers(&path, &path, &m.encode()).unwrap();
        let file = lofty::iff::aiff::AiffFile::read_from(&mut std::fs::File::open(&path).unwrap(), options).unwrap();
        assert_eq!(geob(file.id3v2().unwrap()), [m.encode()]);

        let flac = dir.path().join("a.flac");
        std::fs::write(&flac, b"fLaC").unwrap();
        assert_eq!(Error::from(write_markers(&flac, &flac, &m.encode()).unwrap_err()).code(), "tags");
    }
}
