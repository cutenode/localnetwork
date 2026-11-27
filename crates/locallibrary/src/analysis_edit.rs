//! Writing a track's beat grid, phrases and waveform into its analysis files, with
//! [`crate::anlz`]'s writers: an edit ([`crate::edit::Edit::SetAnalysis`]) made as a cue edit is,
//! backed up first and journaled, so `undo` takes it back and `edits replay` makes it again after
//! rekordbox exports to the drive. The journal names a waveform staged on the drive by its hash
//! (`.locallibrary/waveforms/<hash>.bands`).
//!
//! A track takes one in the analysis files it has: a beat grid in its `.DAT` (the `.EXT`'s
//! extended grid, which can't be written, goes, see [`anlz::without_extended_grid`]), phrases in
//! its `.EXT`, and a waveform in each file's waveform tags (the blue overview in the `.DAT`, the
//! blue and colour detail in the `.EXT`, the 3-band ones in a `.2EX`). A track without analysis
//! files is refused, and so is a waveform for one whose files hold a tag that can't be worked
//! out from it (rekordbox's colour overview, `PWV4`, and the `.2EX`'s band gains, `PWVC`): its
//! waveforms stay rekordbox's. rekordbox's collection (see [`crate::collection_analysis`]), where
//! every analysed track has both, keeps them instead: the overview still shows the track's audio,
//! and the 3-band waveforms are written at the levels rekordbox's had, so the gains still fit. Cues stay where they are when the grid moves (a warning says how many sat on its
//! beats), phrases not given move with it to the beats nearest their times, and the plan that
//! sets a grid sets the track's BPM in the databases too ([`crate::edit::plan`]).
//!
//! What's written is only what the readers read back as given, in layouts checked against
//! rekordbox's own files; how players take a grid, phrases or a waveform written here is for real
//! players to show.

use rustc_hash::FxHashMap;

use base64::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::anlz::{self, Bands, Beat, CueFile, CuePoint, SongStructure};
use crate::commit::{self, Changes, NewFile, Options};
use crate::cue_edit::{clock, read_optional, sibling_key};
use crate::device::{Device, LineageOp};
use crate::edit::Replayed;
use crate::fsutil;
use crate::hash::Hash;
use crate::model::{Library, Track};
use crate::paths::{self, FileClass};
use crate::progress::Ctx;
use crate::staged::WAVEFORMS;
use crate::{Error, Result};

/// What an analysis edit writes; what it leaves out stays as it is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NewAnalysis {
    /// The beat grid: each beat's number in its bar (1-4), the tempo there and its time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beats: Option<Vec<Beat>>,
    /// The phrases, named as the track's mood names them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phrases: Option<SongStructure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waveform: Option<WaveSource>,
}

/// A waveform an analysis edit writes: three bands, each column's lows, mids and highs (0-255),
/// 150 columns a second ([`Bands`]), from which each of the track's waveform tags is worked out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaveSource {
    /// The bands packed (three bytes a column), in base64.
    Base64(String),
    /// Staged on the drive (`.locallibrary/waveforms/<hash>.bands`), by its hash: how the journal
    /// records the others.
    Staged(Hash),
    /// A file a plugin's answer names in its request's scratch folder, packed: how plugins give
    /// waveforms. The plugin host reads its bytes once it has checked it.
    #[schemars(skip)]
    Blob(String),
}

impl NewAnalysis {
    pub fn is_empty(&self) -> bool {
        self.beats.is_none() && self.phrases.is_none() && self.waveform.is_none()
    }

    /// `set the beat grid and phrases of /Contents/a.mp3`.
    pub fn describe(&self, track: &str) -> String {
        let parts: Vec<&str> = [
            self.beats.as_ref().map(|_| "the beat grid"),
            self.phrases.as_ref().map(|_| "phrases"),
            self.waveform.as_ref().map(|_| "the waveform"),
        ]
        .into_iter()
        .flatten()
        .collect();
        format!("set {} of {track}", crate::op::and_list(&parts))
    }

    /// The tempo a track with this grid has in the databases: the one most of its beats have
    /// (of those alike, the first's), to hundredths. rekordbox's own `BPM` is that on 1,969 of
    /// 1,990 constant grids in one collection; on a grid whose tempo changes it often keeps a whole
    /// number instead (25 of 48), which this doesn't.
    pub fn tempo(&self) -> Option<f64> {
        let beats = self.beats.as_ref()?;
        let mut counts: Vec<(i64, usize)> = Vec::new();
        for b in beats {
            let hundredths = (b.bpm * 100.0).round() as i64;
            match counts.iter_mut().find(|(t, _)| *t == hundredths) {
                Some((_, n)) => *n += 1,
                None => counts.push((hundredths, 1)),
            }
        }
        // `max_by_key` keeps the last of equals: reversed, that's the first.
        counts.into_iter().rev().max_by_key(|(_, n)| *n).map(|(t, _)| t as f64 / 100.0)
    }
}

/// The waveform an analysis edit writes, as the drive keeps it for the journal, named by its hash,
/// with its bytes unless it was given staged (the plan holds them, as an artwork edit's image).
pub(crate) fn stage(device: &Device, wave: &WaveSource) -> Result<(Hash, Option<Vec<u8>>)> {
    let bytes = match wave {
        WaveSource::Staged(hash) if WAVEFORMS.path(device, hash).is_file() => return Ok((*hash, None)),
        WaveSource::Staged(hash) => {
            return Err(Error::NotFound(format!("the staged waveform {} on {}", hash.short(), device.name())));
        }
        WaveSource::Blob(_) => return Err(Error::invalid("only a plugin's plan gives a waveform as a blob")),
        WaveSource::Base64(text) => {
            BASE64_STANDARD.decode(text).map_err(|e| Error::invalid(format!("the waveform isn't base64: {e}")))?
        }
    };
    if bytes.is_empty() || Bands::unpack(&bytes).is_none() {
        return Err(Error::invalid("a waveform is three bytes a column (lows, mids, highs), at least one column"));
    }
    Ok((Hash::of(&bytes), Some(bytes)))
}

/// A track's analysis files, as keys, with what each holds now: its `.DAT`, `.EXT` and `.2EX`.
struct Files {
    keys: [String; 3],
    bytes: [Option<Vec<u8>>; 3],
    /// The track's cues, where they aren't the ones its files hold (a collection's are in
    /// `djmdCue`): `None` reads them from the files.
    cues: Option<Vec<CuePoint>>,
    /// Whether a waveform keeps the tags rekordbox wrote that can't be worked out from it (its
    /// colour overview, `PWV4`, and 3-band gains, `PWVC`), the 3-band waveforms written at its
    /// levels so the gains still fit, rather than being refused: a collection's.
    keeps_rekordboxs: bool,
}

/// The keys of the analysis files of the track at `analyze_path` (a device path), `.DAT` first,
/// once it's an analysis file's path on the drive.
fn keys(t: &Track) -> Result<[String; 3]> {
    let no_analysis = || {
        Error::invalid(format!(
            "{} has no analysis files (rekordbox hasn't analysed it), so it can't hold a beat grid, phrases or waveforms",
            t.display_name()
        ))
    };
    if t.analyze_path.is_empty() {
        return Err(no_analysis());
    }
    let dat = paths::key(&t.analyze_path);
    let refused = || Error::invalid(format!("refusing analysis path {:?} of {}", t.analyze_path, t.display_name()));
    if !paths::is_safe_key(&dat) || paths::classify(&dat) != Some(FileClass::Analysis) {
        return Err(refused());
    }
    let [ext, two] = ["EXT", "2EX"].map(|e| sibling_key(&dat, e));
    Ok([dat, ext.ok_or_else(refused)?, two.ok_or_else(refused)?])
}

fn read(device: &Device, t: &Track) -> Result<Files> {
    let keys = keys(t)?;
    let bytes = [read_optional(device, &keys[0])?, read_optional(device, &keys[1])?, read_optional(device, &keys[2])?];
    if bytes[0].is_none() {
        return Err(Error::invalid(format!("{} has no .DAT analysis file", t.display_name())));
    }
    Ok(Files { keys, bytes, cues: None, keeps_rekordboxs: false })
}

/// The analysis files an analysis edit of the track of the audio file `file` (a device path) may
/// rewrite, as keys: what the backup made before it copies.
pub(crate) fn analysis_files(device: &Device, library: &Library, file: &str) -> Vec<String> {
    let keys = track(library, file).and_then(keys).into_iter().flatten();
    keys.filter(|k| device.checked_path(k).is_ok_and(|p| p.is_file())).collect()
}

/// The waveform tags each analysis file takes, `.DAT`, `.EXT` and `.2EX`.
const WAVEFORM_TAGS: [&[&[u8; 4]]; 3] = [&[b"PWAV", b"PWV2"], &[b"PWV3", b"PWV5"], &[b"PWV6", b"PWV7"]];

/// The analysis files an edit rewrites, by key, with their new bytes.
type Rewritten = Vec<(String, Vec<u8>)>;

/// The time of the beat at grid index `i` of `grid`, one past its end too (a phrase's end), or
/// further at its last tempo.
fn beat_time(grid: &[Beat], i: usize) -> f64 {
    if let Some(b) = grid.get(i) {
        return f64::from(b.time_ms);
    }
    let last = grid.last().expect("a grid has a beat");
    f64::from(last.time_ms) + (i + 1 - grid.len()) as f64 * 60_000.0 / last.bpm.max(1.0)
}

/// The 1-based beat of `grid` nearest `ms`, one past its last beat when `ms` is nearer that.
fn nearest_beat(grid: &[Beat], ms: f64) -> u16 {
    // Grids are in time order: the first beat at or after `ms`, or the one before it.
    let after = grid.partition_point(|b| f64::from(b.time_ms) < ms);
    let off = |i: usize| (beat_time(grid, i) - ms).abs();
    let i = if after > 0 && off(after - 1) <= off(after) { after - 1 } else { after };
    u16::try_from(i + 1).unwrap_or(u16::MAX)
}

/// The phrases `s` (counted on the beats of `old`) on the grid `new` instead, each beat moved to
/// the new one nearest its time, and what to warn of (the track named `name`): phrases that move by
/// more than a beat, and those that end up on one beat with the next (left out). `None` when none
/// are left.
fn phrases_on(s: &SongStructure, old: &[Beat], new: &[Beat], name: &str) -> (Option<SongStructure>, Vec<String>) {
    let scale = s.beat_scale(old.len());
    let time = |beat: u16| beat_time(old, SongStructure::grid_index(beat, scale));
    let beat_ms = |beat: u16| beat_time(new, usize::from(beat)) - beat_time(new, usize::from(beat).saturating_sub(1));
    let (mut far, mut gone) = (Vec::new(), Vec::new());
    let mut phrases: Vec<anlz::Phrase> = Vec::new();
    for (n, p) in s.phrases.iter().enumerate() {
        let start = nearest_beat(new, time(p.start_beat));
        if (beat_time(new, usize::from(start) - 1) - time(p.start_beat)).abs() > beat_ms(start) {
            far.push(n + 1);
        }
        let fill = p.fill_beat.map(|f| nearest_beat(new, time(f)));
        if phrases.last().is_some_and(|q| q.start_beat >= start) {
            gone.push(n + 1);
            continue;
        }
        if let Some(q) = phrases.last_mut() {
            q.end_beat = start;
        }
        phrases.push(anlz::Phrase { start_beat: start, end_beat: start, fill_beat: fill, ..p.clone() });
    }
    if let (Some(q), Some(p)) = (phrases.last_mut(), s.phrases.last()) {
        q.end_beat = nearest_beat(new, time(p.end_beat));
    }
    // A last one ending where it starts (past the new grid's end) goes too: the one before ends
    // where it started.
    if phrases.last().is_some_and(|q| q.end_beat <= q.start_beat) {
        gone.push(s.phrases.len());
        phrases.pop();
    }
    for q in &mut phrases {
        q.fill_beat = q.fill_beat.filter(|f| (q.start_beat..q.end_beat).contains(f));
    }
    let list = |n: &[usize]| n.iter().map(usize::to_string).collect::<Vec<_>>().join(", ");
    let mut warnings = Vec::new();
    if !far.is_empty() {
        warnings.push(format!("{name}'s phrases {} move by more than a beat onto the new grid", list(&far)));
    }
    if !gone.is_empty() {
        warnings.push(format!(
            "{name}'s phrases {} fall on one beat with the next on the new grid, so they go",
            list(&gone)
        ));
    }
    if phrases.is_empty() {
        warnings.push(format!("{name}'s phrases don't fit the new grid, so the .EXT loses them"));
        return (None, warnings);
    }
    (Some(SongStructure { mood: s.mood, phrases }), warnings)
}

/// `bands` with each band scaled so its loudest column is as loud as in `theirs` (a band silent in
/// either stays as it is).
fn at_levels(bands: &Bands, theirs: &Bands) -> Bands {
    let peak = |b: &Bands, k: usize| b.0.iter().map(|c| c[k]).max().unwrap_or(0);
    let scale = [0, 1, 2].map(|k| match (peak(bands, k), peak(theirs, k)) {
        (0, _) | (_, 0) => (1, 1),
        (ours, theirs) => (u32::from(theirs), u32::from(ours)),
    });
    Bands(
        bands.0.iter().map(|c| [0, 1, 2].map(|k| (u32::from(c[k]) * scale[k].0 / scale[k].1).min(255) as u8)).collect(),
    )
}

/// What the edit rewrites in a track's analysis files (only those that change), and what to warn
/// of. `bands` is the waveform's.
fn rewrite(t: &Track, files: &Files, a: &NewAnalysis, bands: Option<&Bands>) -> Result<(Rewritten, Vec<String>)> {
    let name = t.display_name();
    let mut new = files.bytes.clone();
    let mut warnings = Vec::new();
    let past_end = |ms: u32| t.duration_secs > 0 && u64::from(ms) >= (u64::from(t.duration_secs) + 1) * 1000;
    let mut grid = anlz::beat_grid(new[0].as_ref().expect("read")).unwrap_or_default();
    if let Some(beats) = &a.beats {
        let beats = anlz::checked_grid(beats)?;
        if let Some((i, b)) = beats.iter().enumerate().find(|(_, b)| past_end(b.time_ms)) {
            return Err(Error::invalid(format!("beat {} at {} is past the end of {name}", i + 1, clock(b.time_ms))));
        }
        if beats != grid {
            let dat = new[0].as_ref().expect("read");
            let cues = match &files.cues {
                Some(cues) => cues.clone(),
                None => anlz::read_cue_points(dat, new[1].as_deref())?,
            };
            let on = |grid: &[Beat], ms: u32| grid.iter().any(|b| b.time_ms.abs_diff(ms) <= 1);
            let moved = cues.iter().filter(|c| on(&grid, c.start_ms) && !on(&beats, c.start_ms)).count();
            if moved > 0 {
                warnings.push(format!(
                    "{moved} cue(s) of {name} sat on beats of its old grid: they stay where they are, off the new one"
                ));
            }
            new[0] = Some(anlz::write_beat_grid(dat, &beats)?);
            if let Some(ext) = &new[1] {
                let mut without = anlz::without_extended_grid(ext)?;
                if without != *ext {
                    warnings.push(format!(
                        "{name}'s .EXT loses its extended beat grid (PQT2), which locallibrary can't write"
                    ));
                }
                // Phrases count the grid's beats: kept, they'd sit on other beats of the new one.
                // Without an old grid to time them by, they stay as they are.
                if a.phrases.is_none()
                    && !grid.is_empty()
                    && let Some(old) = anlz::song_structure(&without)?
                {
                    let (moved, said) = phrases_on(&old, &grid, &beats, &name);
                    warnings.extend(said);
                    without = anlz::write_song_structure(&without, moved.as_ref())?;
                }
                new[1] = Some(without);
            }
            grid = beats;
        }
    }
    if let Some(p) = &a.phrases {
        let Some(ext) = &new[1] else {
            return Err(Error::invalid(format!("{name} has no .EXT analysis file to hold phrases")));
        };
        new[1] = Some(anlz::write_song_structure(ext, Some(p))?);
    }
    let phrases = new[1].as_deref().and_then(|e| anlz::song_structure(e).ok().flatten());
    if let Some(last) =
        phrases.as_ref().and_then(|s| s.phrases.last()).filter(|p| usize::from(p.end_beat) > grid.len() + 1)
    {
        warnings.push(format!(
            "{name}'s phrases run to beat {}, past its beat grid's {} beats",
            last.end_beat,
            grid.len()
        ));
    }
    if let Some(bands) = bands {
        // The 3-band waveforms' bands, at the levels of rekordbox's where its gains are kept.
        let mut three = bands.clone();
        for (i, tag) in [(1, b"PWV4"), (2, b"PWVC")] {
            let Some(bytes) = new[i].as_deref() else { continue };
            if !anlz::has_section(bytes, tag)? {
                continue;
            }
            let theirs = anlz::TrackWaves::read(None, None, Some(bytes)).band_detail;
            let refused = || {
                Error::invalid(format!(
                    "{name}'s analysis files hold a waveform tag ({}) that can't be worked out from another waveform: its waveforms stay rekordbox's",
                    String::from_utf8_lossy(tag)
                ))
            };
            match (files.keeps_rekordboxs, tag, theirs) {
                (false, _, _) => return Err(refused()),
                (true, b"PWV4", _) => warnings.push(format!(
                    "{name}'s colour overview (PWV4) stays rekordbox's: it can't be worked out from a waveform, and \
                     still shows this track's audio"
                )),
                (true, _, Some((theirs, _))) => {
                    three = at_levels(bands, &theirs);
                    warnings.push(format!(
                        "{name}'s 3-band waveforms are written at the levels rekordbox's had, so its band gains (PWVC), \
                         which can't be worked out, still fit them"
                    ));
                }
                (true, _, None) => return Err(refused()),
            }
        }
        let secs = bands.0.len() as f64 / anlz::DETAIL_RATE;
        if t.duration_secs > 0 && (secs - f64::from(t.duration_secs)).abs() > 2.0 {
            warnings.push(format!("{name}'s waveform is {secs:.0} s long, the track {} s", t.duration_secs));
        }
        if new[1].is_none() {
            warnings.push(format!("{name} has no .EXT analysis file, so only its overview waveform is written"));
        }
        for (i, (file, tags)) in new.iter_mut().zip(WAVEFORM_TAGS).enumerate() {
            let Some(bytes) = file else { continue };
            let bands = if i == 2 { &three } else { bands };
            for tag in tags {
                let columns = anlz::waveform_from_bands(tag, bands)?.expect("every tag here is worked out");
                *bytes = anlz::write_waveform(bytes, tag, &columns)?;
            }
        }
    }
    let changed = files
        .keys
        .iter()
        .zip(&files.bytes)
        .zip(new)
        .filter_map(|((key, old), new)| new.filter(|n| Some(n) != old.as_ref()).map(|n| (key.clone(), n)))
        .collect();
    Ok((changed, warnings))
}

/// New analysis files for `t`, a track without any, its audio at its `file_path` on the drive:
/// `.DAT` and `.EXT` holding `cues` and what `a` gives, its waveform `bands` (with no `.2EX`, the
/// 3-band ones are left out), each read back as given, and what to warn of. An error when they
/// can't be written so.
pub(crate) fn new_files(
    t: &Track,
    cues: &[CuePoint],
    a: &NewAnalysis,
    bands: Option<&Bands>,
) -> Result<([Vec<u8>; 2], Vec<String>)> {
    let (dat, ext) = anlz::new_analysis_files(&t.file_path)?;
    let mut made =
        [anlz::write_cue_points(&dat, CueFile::Dat, cues)?, anlz::write_cue_points(&ext, CueFile::Ext, cues)?];
    // Not on a drive yet: each file by its extension.
    let keys = ["DAT", "EXT", "2EX"].map(String::from);
    let files = Files {
        keys,
        bytes: [Some(made[0].clone()), Some(made[1].clone()), None],
        cues: None,
        keeps_rekordboxs: false,
    };
    let (changed, warnings) = rewrite(t, &files, a, bands)?;
    for (key, new) in changed {
        made[usize::from(key == "EXT")] = new;
    }
    let order = |c: &CuePoint| (c.hot_cue, c.start_ms, c.end_ms);
    let mut read = anlz::read_cue_points(&made[0], Some(&made[1]))?;
    let mut given = cues.to_vec();
    read.sort_by_key(order);
    given.sort_by_key(order);
    if read.len() != given.len() || read.iter().zip(&given).any(|(r, g)| !r.same_as(g)) {
        return Err(Error::invalid(format!("{}'s cues don't read back as they were given", t.display_name())));
    }
    Ok((made, warnings))
}

/// What writing `a` (its waveform `bands`) changes in analysis files holding `bytes` (`.DAT`,
/// `.EXT` and `.2EX`, the `.DAT` there) of the track `t` whose cues are `cues`, with what to warn
/// of: each file changed, by its extension (`DAT`, `EXT`, `2EX`), with its new bytes. For files
/// that aren't on a drive: rekordbox's collection's (see [`crate::collection_analysis`]), whose
/// cues are kept apart from them.
pub(crate) fn rewrite_files(
    t: &Track,
    bytes: [Option<Vec<u8>>; 3],
    cues: Vec<CuePoint>,
    a: &NewAnalysis,
    bands: Option<&Bands>,
) -> Result<(Rewritten, Vec<String>)> {
    if bytes[0].is_none() {
        return Err(Error::invalid(format!("{} has no .DAT analysis file", t.display_name())));
    }
    rewrite(
        t,
        &Files { keys: ["DAT", "EXT", "2EX"].map(String::from), bytes, cues: Some(cues), keeps_rekordboxs: true },
        a,
        bands,
    )
}

/// The waveform `wave` gives as bands, when it's given as them (in base64): not one staged on a
/// drive, nor a plugin's blob.
pub(crate) fn given_bands(wave: &WaveSource) -> Result<Bands> {
    let WaveSource::Base64(text) = wave else {
        return Err(Error::invalid("a waveform written here is given as bands, in base64"));
    };
    let bytes = BASE64_STANDARD.decode(text).map_err(|e| Error::invalid(format!("the waveform isn't base64: {e}")))?;
    Bands::unpack(&bytes)
        .filter(|b| !b.0.is_empty())
        .ok_or_else(|| Error::invalid("a waveform is three bytes a column (lows, mids, highs), at least one column"))
}

/// The track of the audio file `file` (a device path) in `library`.
fn track<'a>(library: &'a Library, file: &str) -> Result<&'a Track> {
    let key = paths::file_key(file);
    library
        .tracks
        .iter()
        .find(|t| paths::file_key(&t.file_path) == key)
        .ok_or_else(|| Error::NotFound(format!("no track at {file}")))
}

/// The bands of the waveform an edit gives, staged as `hash` (from `staged`, else the drive's
/// copy).
fn staged_bands(device: &Device, hash: &Hash, staged: &FxHashMap<Hash, Vec<u8>>) -> Result<Bands> {
    let gone = || Error::NotFound(format!("the staged waveform {} on {}", hash.short(), device.name()));
    let bytes = match staged.get(hash) {
        Some(b) => b.clone(),
        None => crate::fsutil::read(&WAVEFORMS.path(device, hash)).map_err(|_| gone())?,
    };
    Bands::unpack(&bytes).filter(|b| !b.0.is_empty()).ok_or_else(gone)
}

/// What making the edit on the track of `file` would warn of, planning it, its waveform staged
/// (from `staged`, else the drive's copy): an error when it can't be made.
pub(crate) fn check(
    device: &Device,
    library: &Library,
    file: &str,
    a: &NewAnalysis,
    staged: &FxHashMap<Hash, Vec<u8>>,
) -> Result<Vec<String>> {
    let bands = match &a.waveform {
        Some(WaveSource::Staged(hash)) => Some(staged_bands(device, hash, staged)?),
        Some(_) => return Err(Error::invalid("an analysis edit's waveform is staged as it's planned")),
        None => None,
    };
    Ok(fits(device, track(library, file)?, a, bands.as_ref())?.1)
}

/// Whether writing `a` (its waveform `bands`) into `t`'s analysis files would change them, and
/// what it would warn of: an error when it can't be written.
pub(crate) fn fits(device: &Device, t: &Track, a: &NewAnalysis, bands: Option<&Bands>) -> Result<(bool, Vec<String>)> {
    let (changed, warnings) = rewrite(t, &read(device, t)?, a, bands)?;
    Ok((!changed.is_empty(), warnings))
}

/// Make an analysis edit of a plan's on the track of `file` (a device path), its waveform staged
/// (from `staged`, else the drive's copy), under the backup and lock the plan took (see
/// [`crate::edit::apply`]). The waveform is staged first, for the journal entry to name.
/// Replaying (not `strict`), one that no longer applies is skipped with a warning; an error is a
/// failure while writing.
pub(crate) fn replay(
    device: &mut Device,
    file: &str,
    a: &NewAnalysis,
    staged: &FxHashMap<Hash, Vec<u8>>,
    strict: bool,
    ctx: &Ctx<'_>,
) -> Result<Replayed> {
    let out = Replayed::default();
    let described = a.describe(file);
    let library = device.library()?;
    let hash = match &a.waveform {
        Some(WaveSource::Staged(hash)) => Some(*hash),
        Some(_) => return Err(Error::invalid("an analysis edit's waveform is staged as it's planned")),
        None => None,
    };
    let made = track(&library, file).and_then(|t| {
        let bands = hash.map(|h| staged_bands(device, &h, staged)).transpose()?;
        Ok((rewrite(t, &read(device, t)?, a, bands.as_ref())?, bands))
    });
    let ((changed, warnings), bands) = match made {
        Ok(made) => made,
        Err(e) => return out.skipped(strict, &described, e),
    };
    let mut out = Replayed { warnings, ..out };
    if changed.is_empty() {
        return Ok(out);
    }
    // Not on the drive yet (it was read from `staged`), it's staged as it was read.
    let fresh = hash.zip(bands).map(|(h, b)| (WAVEFORMS.path(device, &h), b)).filter(|(p, _)| !p.is_file());
    if let Some((path, bands)) = &fresh {
        fsutil::write_atomic(path, &bands.pack())?;
    }
    let changes =
        Changes { files: changed.into_iter().map(|(k, b)| NewFile::bytes(k, b)).collect(), ..Default::default() };
    let swaps = device.swaps();
    let done = commit::commit(device, changes, Options::new(LineageOp::Edit, "edit analysis"), ctx);
    // Failing before it changed the drive, it leaves no staged waveform either (best effort: the
    // error that got here is what's reported).
    if done.is_err()
        && device.swaps() == swaps
        && let Some((path, _)) = &fresh
    {
        let _ = fsutil::remove_if_exists(path);
        let _ = path.parent().map(std::fs::remove_dir);
    }
    let done = done?;
    out.databases = done.written;
    out.fingerprint = done.fingerprint;
    Ok(out)
}

#[cfg(test)]
mod tests;
