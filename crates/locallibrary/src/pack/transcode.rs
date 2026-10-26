//! Player-safe copies on a packed drive: tracks the drive's players can't play (a 24/96 FLAC on
//! a CDJ-2000NXS) packed as lossless copies they can ([`loclib_transcode`]), AIFF unless asked
//! otherwise. A lossy file is never encoded lossy again, and is decoded to a lossless copy only
//! when asked. The copy keeps the track's row (its playlists, history, My Tags, cues and grid):
//! the packed databases point it at the copy, with the copy's format, size, rate, depth and bit
//! rate, and its analysis files record the copy's path.
//!
//! Each copy is recorded on the target (`.locallibrary/transcodes.json`) with the content hash of
//! the audio it was made from and its recipe. Packing again keeps a copy whose master and recipe
//! are what they were, and makes it again when either changed: a master replaced by a better
//! one at the same path (same name, new audio) is told by its content, not its path or time. The
//! record lives on the drive because the copies do: a drive erased or packed elsewhere loses
//! both together, where a record in the store would outlive the copies it describes.

use indexmap::IndexMap;
use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use loclib_transcode::{Container, Decision, Options, Reason, Recipe, Source, decide, estimate_size};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::check;
use crate::device::Device;
use crate::edit::{AudioFile, Move};
use crate::equipment::{self, Codec, Model, Unplayable};
use crate::error::IoContext;
use crate::fsutil;
use crate::hash::Hash;
use crate::library_write;
use crate::model::{Track, TrackRef};
use crate::paths;
use crate::progress::{Ctx, Event, Phase};
use crate::tree::FileTree;
use crate::{Error, Result};

const RECORD_FILE: &str = "transcodes.json";

/// A track packed as a copy its players play.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PackTranscode {
    pub track: TrackRef,
    /// The source's audio file and the copy's, as keys.
    pub from: String,
    pub to: String,
    /// The players that don't play the source, and why.
    pub why: String,
    /// What the copy is (`aiff-48000-24-tpdf-v1`): container, rate, depth, dither, version.
    pub recipe: String,
    /// The copy's size: estimated, or what it is when `current`.
    pub size: u64,
    /// A copy of the same audio by the same recipe is on the target already, and stays.
    pub current: bool,
    #[serde(skip)]
    pub(super) job: Job,
}

/// What making (or keeping) a copy needs.
#[derive(Debug, Clone)]
pub(super) struct Job {
    /// The track's audio file as the packed databases name it (a lower-cased key).
    track_key: String,
    source: PathBuf,
    /// Its size, for progress.
    pub(super) source_size: u64,
    hash: Hash,
    recipe: Recipe,
    /// How much later its times are in the copy.
    shift_ms: i32,
    /// What a current copy is, from its record.
    kept: Option<AudioFile>,
}

/// A copy [`plan`] will make, before its master's hash is known.
struct Wanted<'t> {
    track: &'t Track,
    to: String,
    recipe: Recipe,
    why: String,
    estimate: u64,
    shift_ms: i32,
    source_size: u64,
}

/// A track none of the players play that packing leaves as it is.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PackUnplayable {
    pub track: TrackRef,
    pub why: String,
}

/// What [`plan`] found.
#[derive(Debug, Default)]
pub(super) struct Copies {
    pub transcodes: Vec<PackTranscode>,
    pub unplayable: Vec<PackUnplayable>,
    pub warnings: Vec<String>,
    /// Source audio files (keys) the copies stand in for: not packed themselves.
    pub replaced: FxHashSet<String>,
    /// The analysis files (keys) of those tracks, recording their copies' paths and times (a
    /// decoded lossy file's are later: see [`crate::timeline`]). A new copy's are written whether
    /// or not they look current.
    pub rewrites: IndexMap<String, library_write::Source>,
}

/// A copy as recorded on the target.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    source: String,
    hash: Hash,
    recipe: String,
    file: AudioFile,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Records {
    copies: IndexMap<String, Record>,
}

/// The copies recorded on `target`. A record that can't be read counts as none (its copies are
/// made again), with a warning, rather than stopping every later pack.
fn records(target: &Device, warnings: &mut Vec<String>) -> Records {
    fsutil::read_json_opt(&target.state_dir().join(RECORD_FILE))
        .unwrap_or_else(|e| {
            warnings.push(format!("{RECORD_FILE} on {} can't be read ({e}); its copies are made again", target.name()));
            None
        })
        .unwrap_or_default()
}

/// The drive no longer holds a pack (see [`super::forget`]).
pub(super) fn forget(device: &Device) -> Result<()> {
    fsutil::remove_if_exists(&device.state_dir().join(RECORD_FILE)).map(|_| ())
}

/// The players `names` are, refusing none or a name the table doesn't know.
fn players(names: &[String]) -> Result<Vec<&'static Model>> {
    let names = equipment::canonical(names)?;
    if names.is_empty() {
        return Err(Error::invalid("name the players the drive is for, or remember them for its library")
            .because(crate::error::Reason::MissingParam)
            .with("param", "players"));
    }
    Ok(names.iter().filter_map(|n| equipment::find(n)).collect())
}

/// `CDJ-2000NXS plays no FLAC; OMNIS-DUO doesn't play it at 96 kHz`.
fn why(players: &[&Model], codec: Codec, rate: u32, depth: u16) -> String {
    let what = |u: Unplayable| match u {
        Unplayable::Format => format!("plays no {}", codec.name()),
        Unplayable::SampleRate => format!("doesn't play it at {} kHz", f64::from(rate) / 1000.0),
        Unplayable::BitDepth => format!("doesn't play it at {depth} bit"),
        _ => format!("doesn't play it at {} kHz and {depth} bit", f64::from(rate) / 1000.0),
    };
    let reasons: Vec<String> = players
        .iter()
        .filter_map(|m| m.plays(codec, rate, depth).result.err().map(|u| format!("{} {}", m.name, what(u))))
        .collect();
    reasons.join("; ")
}

/// The copies `choice`'s players need of `tracks` (the packed library's, both databases'), found
/// on the source in `src_tree`; `tgt_tree` is the target's, which may hold current copies already.
pub(super) fn plan(
    source: &Device,
    target: &Device,
    src_tree: &mut FileTree,
    tgt_tree: &FileTree,
    tracks: &[&Track],
    choice: &super::Transcode,
    ctx: &Ctx<'_>,
) -> Result<Copies> {
    let players = players(&choice.players)?;
    let players = players.as_slice();
    let options = &Options { container: choice.container, decode_lossy: choice.decode_lossy };
    let mut out = Copies::default();
    let taken: FxHashSet<String> = tracks.iter().map(|t| paths::file_key(&t.file_path)).collect();
    let mut seen = FxHashSet::default();
    let mut wanted: Vec<Wanted<'_>> = Vec::new();
    let mut lossy = 0;
    for t in tracks {
        ctx.check()?;
        let Some(entry) = src_tree.resolve(&paths::key(&t.file_path)).cloned() else { continue };
        if !seen.insert(entry.path.to_lowercase()) {
            continue;
        }
        let file = src_tree.fs_path(&entry.path);
        let probed = check::needs_probe(t).then(|| crate::probe::format(&file).ok()).flatten();
        let audio = check::track_audio(t, probed.as_ref());
        let unplayable = |why: String| PackUnplayable { track: TrackRef::from(*t), why };
        let codec = match audio.codec {
            None => continue,
            Some(Ok(codec)) => codec,
            Some(Err(ext)) => {
                out.unplayable.push(unplayable(format!("no player plays {ext} files")));
                continue;
            }
        };
        let facts = |channels| Source { codec, sample_rate: audio.rate, bit_depth: audio.depth, channels };
        let recipe = match decide(&facts(2), players, options) {
            Decision::Copy => continue,
            Decision::Transcode(r) => r,
            Decision::Unplayable(reason) => {
                let why = match reason {
                    Reason::Lossy => {
                        lossy += 1;
                        why(players, codec, audio.rate, audio.depth)
                    }
                    Reason::UnknownRate => "its sample rate isn't known".into(),
                    _ => "no lossless format plays on all of these players".into(),
                };
                out.unplayable.push(unplayable(why));
                continue;
            }
        };
        let stem = entry.path.rsplit_once('.').filter(|(_, ext)| !ext.contains('/')).map_or(&*entry.path, |(s, _)| s);
        let to = format!("{stem}.{}", recipe.container.extension());
        // The copy may take its own master's path, never another track's.
        let own = to.eq_ignore_ascii_case(&entry.path);
        if (!own && taken.contains(&to.to_lowercase())) || wanted.iter().any(|w| w.to.eq_ignore_ascii_case(&to)) {
            out.unplayable.push(unplayable(format!("its copy's name, {to}, is another track's file")));
            continue;
        }
        let probed = probed.or_else(|| crate::probe::format(&file).ok());
        // Stereo when the file doesn't say: the estimate is for planning space, better high.
        let channels = probed.map_or(0, |p| p.channels);
        let facts = facts(if channels == 0 { 2 } else { channels });
        let secs = probed.and_then(|p| p.duration_secs).unwrap_or(f64::from(t.duration_secs));
        let estimate = estimate_size(&facts, entry.size, secs, &recipe);
        if recipe.container != Container::Flac && estimate >= u64::from(u32::MAX) {
            let container = recipe.container.extension().to_uppercase();
            out.unplayable.push(unplayable(format!("it's too long for {container}, which holds 4 GiB at most")));
            continue;
        }
        // A decoded lossy file starts at its first decoded sample, which rekordbox's timeline for
        // it may not.
        let shift_ms = match codec.lossless() {
            true => 0,
            false => match crate::timeline::lead(source, t, ctx)? {
                Some(lead) => (lead * 1000.0).round() as i32,
                None => {
                    out.unplayable.push(unplayable(crate::timeline::UNTOLD.into()));
                    continue;
                }
            },
        };
        let why = why(players, codec, audio.rate, audio.depth);
        wanted.push(Wanted { track: t, to, recipe, why, estimate, shift_ms, source_size: entry.size });
    }
    if lossy > 0 {
        out.warnings.push(format!(
            "{lossy} lossy track(s) these players don't play stay as they are: they're copied to lossless only \
             when that's asked for"
        ));
    }
    if wanted.is_empty() {
        return Ok(out);
    }

    // The masters' content, to tell a copy made from them from one of audio since replaced.
    let keys: FxHashSet<String> = wanted
        .iter()
        .filter_map(|w| src_tree.resolve(&paths::key(&w.track.file_path)).map(|e| e.path.clone()))
        .collect();
    let mut index = source.load_index();
    src_tree.ensure_hashes(&mut index, |e| keys.contains(&e.path), ctx)?;
    source.save_index(&index)?;
    let records = records(target, &mut out.warnings);
    for w in wanted {
        let t = w.track;
        let entry = src_tree.resolve(&paths::key(&t.file_path)).expect("found above");
        let hash = entry.hash.expect("hashed above");
        let kept = records.copies.get(&w.to).filter(|r| {
            r.hash == hash
                && r.recipe == w.recipe.to_string()
                && tgt_tree.resolve(&w.to).is_some_and(|e| e.size == r.file.size)
        });
        let copy_path = paths::device_path(&w.to);
        for analysis in crate::track_files::analysis_files(&t.analyze_path, src_tree) {
            let e = src_tree.resolve(&analysis).expect("listed from the tree");
            let rewritten =
                library_write::Source::analysis(src_tree.fs_path(&e.path), e.mtime, &copy_path, w.shift_ms)?;
            out.rewrites.insert(analysis, library_write::Source { force: kept.is_none(), ..rewritten });
        }
        out.replaced.insert(entry.path.clone());
        out.transcodes.push(PackTranscode {
            track: TrackRef::from(t),
            from: entry.path.clone(),
            to: w.to,
            why: w.why,
            recipe: w.recipe.to_string(),
            size: kept.map_or(w.estimate, |r| r.file.size),
            current: kept.is_some(),
            job: Job {
                track_key: paths::file_key(&t.file_path),
                source: src_tree.fs_path(&entry.path),
                source_size: w.source_size,
                hash,
                recipe: w.recipe,
                shift_ms: w.shift_ms,
                kept: kept.map(|r| r.file),
            },
        });
    }
    Ok(out)
}

/// Make the copies `transcodes` plans on `target` (keeping current ones). Each copy is recorded
/// as soon as it's in place, so a pack stopped part way keeps what it made; with none, the
/// record goes. Returns, for the packed databases, each track's file (a lower-cased key), its
/// copy's device path, what the copy is, and how much later its times are.
pub(super) fn make(transcodes: &[PackTranscode], target: &Device, ctx: &Ctx<'_>) -> Result<Vec<Move>> {
    if transcodes.is_empty() {
        forget(target)?;
        return Ok(Vec::new());
    }
    let path = target.state_dir().join(RECORD_FILE);
    // The records of the copies the pack keeps; those of copies it no longer has go.
    let mut records = records(target, &mut Vec::new());
    records.copies.retain(|to, _| transcodes.iter().any(|t| t.to == *to));
    let mut moves = Vec::new();
    for t in transcodes {
        let file = match t.job.kept {
            Some(file) => file,
            None => {
                ctx.check()?;
                ctx.emit(Event::Item { phase: Phase::Copy, path: t.to.clone(), bytes: t.job.source_size });
                let file = transcode(&t.job, &target.checked_path(&t.to)?, ctx)?;
                let record = Record { source: t.from.clone(), hash: t.job.hash, recipe: t.recipe.clone(), file };
                records.copies.insert(t.to.clone(), record);
                fsutil::write_json_atomic(&path, &records)?;
                file
            }
        };
        moves.push(Move {
            track: t.job.track_key.clone(),
            path: paths::device_path(&t.to),
            file,
            shift_ms: t.job.shift_ms,
        });
    }
    fsutil::write_json_atomic(&path, &records)?;
    Ok(moves)
}

/// Write one copy to `dst` through a temp file beside it, renamed into place once it's whole.
fn transcode(job: &Job, dst: &Path, ctx: &Ctx<'_>) -> Result<AudioFile> {
    let dir = dst.parent().ok_or_else(|| Error::invalid(format!("{} has no parent", dst.display())))?;
    std::fs::create_dir_all(dir).at(dir)?;
    let tmp = fsutil::temp_for(dir, dst, "")?;
    let made = loclib_transcode::transcode(&job.source, &job.recipe, tmp.path(), ctx)?;
    tmp.as_file().sync_all().at(tmp.path())?;
    let size = tmp.as_file().metadata().at(tmp.path())?.len();
    fsutil::replace(tmp.into_temp_path(), dst)?;
    let r = &job.recipe;
    let bitrate = match r.container {
        // rekordbox gives uncompressed audio its nominal rate, and FLAC its average.
        Container::Aiff | Container::Wav => r.sample_rate * u32::from(r.bit_depth) * u32::from(made.channels) / 1000,
        Container::Flac if made.frames > 0 => {
            (size as f64 * 8.0 * f64::from(r.sample_rate) / made.frames as f64 / 1000.0).round() as u32
        }
        Container::Flac => 0,
    };
    Ok(AudioFile { size, sample_rate: r.sample_rate, bit_depth: r.bit_depth, bitrate })
}
