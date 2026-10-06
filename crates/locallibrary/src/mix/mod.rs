//! Which library tracks a mix plays, and when.
//!
//! Give it a recording of a DJ set (an audio file, or the Audacity project it was edited in) and
//! a drive's library. Every track on the drive up to 20 minutes long is fingerprinted (once; see
//! [`index`]: longer ones are recorded sets), the mix is fingerprinted as it decodes, and
//! [`matcher`] finds where each track sits in the mix, at what speed, with or without key lock,
//! and which part of the track was used. The recording itself is left out when the drive holds
//! it too. The [`MixReport`] adds the library's metadata, the transitions between tracks and a
//! waveform of the whole mix.
//!
//! ```text
//! locallibrary mix "Live at the Warehouse.wav" --drive DNA --html set.html
//! ```

pub mod audio;
pub mod aup;
pub mod export;
pub mod fingerprint;
pub mod index;
pub mod matcher;
pub mod report;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use lofty::prelude::*;
use lofty::tag::ItemKey;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use self::aup::{Label, Project};
use self::fingerprint::{Analyzer, FRAMES_PER_SEC, FrameLevels, QUERY};
use self::index::{Cache, IndexReport, LibraryIndex};
use self::matcher::Found;
use crate::device::Device;
use crate::error::IoContext;
use crate::harmony::Camelot;
use crate::model::{Library, Track};
use crate::progress::{Ctx, Phase};
use crate::{Error, Result};

/// What a mix file is: `audio` or `audacity`. (An alias, so serde doesn't take the `&str` as
/// borrowed from what a report is read back from.)
pub type MixKind = &'static str;

/// [`MixInfo::kind`] read back.
fn mix_kind<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<&'static str, D::Error> {
    Ok(if String::deserialize(d)? == "audacity" { "audacity" } else { "audio" })
}

/// Stretches without a track shorter than this are transitions, not missing tracks.
const UNIDENTIFIED_SECS: f64 = 30.0;

/// Default playback speed search: ±16%, enough to play a 130 BPM track in a 150 BPM set.
pub const DEFAULT_TEMPO_RANGE: f64 = 0.16;

/// Plays matched with less confidence than this are tentative, and every tracklist says so.
pub const TENTATIVE: f64 = 0.5;

/// What was analysed.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MixInfo {
    pub path: PathBuf,
    /// `audio` or `audacity`.
    #[serde(deserialize_with = "mix_kind")]
    #[schemars(with = "String")]
    pub kind: MixKind,
    pub title: String,
    pub artist: Option<String>,
    pub genre: Option<String>,
    pub date: Option<String>,
    /// The device that recorded it, when the file says (Pioneer DJ's recorders tag their model).
    pub recorder: Option<String>,
    pub duration_secs: f64,
    pub sample_rate: u32,
    pub channels: u16,
    /// Labels from an Audacity project's label tracks.
    pub labels: Vec<Label>,
    /// Cover image: embedded in the audio file, else an image next to it named like it.
    pub cover: Option<CoverSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum CoverSource {
    Embedded(PathBuf),
    File(PathBuf),
}

impl MixInfo {
    /// The cover a report shared with others carries: the one embedded in the recording. An
    /// image found beside it is a guess, fine to show here but not to publish.
    pub fn shared_cover(&self) -> Option<Vec<u8>> {
        match &self.cover {
            Some(c @ CoverSource::Embedded(_)) => c.read().ok(),
            _ => None,
        }
    }
}

impl CoverSource {
    pub fn read(&self) -> Result<Vec<u8>> {
        match self {
            CoverSource::Embedded(p) => crate::artwork::read_embedded(p)?
                .ok_or_else(|| Error::NotFound(format!("no embedded artwork in {}", p.display()))),
            CoverSource::File(p) => std::fs::read(p).map_err(|source| Error::Io { path: p.clone(), source }),
        }
    }
}

/// One track heard in the mix.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Play {
    pub track: Track,
    /// Seconds into the mix where the track is first and last heard.
    pub start: f64,
    pub end: f64,
    /// Seconds into the track at those points.
    pub track_start: f64,
    pub track_end: f64,
    /// Playback speed: 1.0 as recorded, 1.03 is +3%.
    pub tempo: f64,
    /// The track's BPM at that speed.
    pub played_bpm: Option<f64>,
    /// Whether key lock (master tempo) was on: `None` when the speed is too close to 1.0 to tell.
    pub key_lock: Option<bool>,
    /// Pitch change from the speed, in semitones (0 with key lock).
    pub semitones: f64,
    /// The track's key in Camelot notation (the library may use note names or Open Key).
    pub key: Option<String>,
    /// The key heard: the track's key moved by `semitones`, in Camelot notation.
    pub played_key: Option<String>,
    /// 0-1.
    pub confidence: f64,
    pub votes: u32,
    /// How clearly the track is heard, every [`matcher::PRESENCE_STEP`] seconds from `start` (0-1).
    /// Fades and EQ swaps show up as slopes.
    pub presence: Vec<f32>,
    pub presence_step: f64,
    /// Loops and cue jumps inside the play.
    pub jumps: u32,
    /// Other library tracks matching the same stretch of the mix.
    pub alternates: Vec<Alternate>,
}

impl Play {
    /// Matched with less confidence than [`TENTATIVE`].
    pub fn is_tentative(&self) -> bool {
        self.confidence < TENTATIVE
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Alternate {
    pub track_id: u32,
    pub name: String,
    pub file_path: String,
    /// The same recording (a duplicate or re-encode) rather than one sharing some material.
    pub same_audio: bool,
}

/// The change from one play to the next.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Transition {
    /// Indexes into [`MixReport::plays`].
    pub from: usize,
    pub to: usize,
    /// When the incoming track is first heard.
    pub at: f64,
    /// Seconds both are heard together (negative: a gap between them).
    pub overlap: f64,
    /// `same key`, `±1`, `relative`, `energy boost`, or `clash`, from the played keys.
    pub harmony: Option<String>,
    /// Played tempo change, percent.
    pub tempo_change: Option<f64>,
}

/// Stretch of the mix with no track found.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

/// The mix's loudness for drawing it: per column, 0-255.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct MixWaveform {
    pub columns_per_sec: f64,
    pub peak: Vec<u8>,
    /// Below 200 Hz, 200 Hz-2 kHz, above 2 kHz.
    pub low: Vec<u8>,
    pub mid: Vec<u8>,
    pub high: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MixReport {
    pub mix: MixInfo,
    pub drive: String,
    /// The drive's identity, to find it again by (absent for a drive locallibrary never adopted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_id: Option<Uuid>,
    pub library: IndexReport,
    pub plays: Vec<Play>,
    pub transitions: Vec<Transition>,
    pub unidentified: Vec<Span>,
    /// Fraction of the mix where a track was identified.
    pub coverage: f64,
    pub waveform: MixWaveform,
    pub tempo_range: f64,
    pub analysed_at: chrono::DateTime<chrono::Utc>,
    pub elapsed_secs: f64,
}

impl MixReport {
    /// Whether `device` is the drive the mix was matched against: by identity, or by name for a
    /// drive without one.
    pub fn matched_against(&self, device: &Device) -> bool {
        match self.drive_id {
            Some(id) => device.id() == Some(id),
            None => device.name() == self.drive,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Playback speeds to search, as a fraction (0.10 is ±10%).
    pub tempo_range: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options { tempo_range: DEFAULT_TEMPO_RANGE }
    }
}

/// A drive's library, fingerprinted and indexed, ready to identify mixes against.
pub struct Identifier {
    pub drive: String,
    pub drive_id: Option<Uuid>,
    pub library: Arc<Library>,
    pub report: IndexReport,
    index: LibraryIndex,
    /// Each indexed track's slot, by its file's key ([`Cache::key`]).
    slots: HashMap<String, u32>,
}

impl Identifier {
    /// Fingerprint the drive's tracks (from `cache` where possible) and index them.
    pub fn new(device: &Device, cache: &Cache, ctx: &Ctx<'_>) -> Result<Identifier> {
        Self::with_library(device, Arc::new(device.library()?), cache, ctx)
    }

    /// [`Identifier::new`], with the drive's library already read.
    pub fn with_library(device: &Device, library: Arc<Library>, cache: &Cache, ctx: &Ctx<'_>) -> Result<Identifier> {
        let (prints, mut report) = index::fingerprint_library(device, &library, cache, ctx)?;
        let slots = prints.iter().enumerate().map(|(slot, f)| (f.key.clone(), slot as u32)).collect();
        ctx.phase_start(Phase::Index, 0, 0);
        let index = LibraryIndex::build(prints.into_iter().map(|f| (f.id, f.print)).collect());
        ctx.phase_end(Phase::Index);
        report.hashes = index.hashes();
        Ok(Identifier { drive: device.name(), drive_id: device.id(), library, report, index, slots })
    }

    /// Find the library's tracks in the mix at `path`.
    pub fn identify(&self, path: &Path, options: &Options, ctx: &Ctx<'_>) -> Result<MixReport> {
        let started = Instant::now();
        let (mut info, heard) = listen(path, ctx)?;
        // The recording itself, when it's on the drive too (a set imported into rekordbox).
        let itself = Cache::key(path).and_then(|k| self.slots.get(&k).copied());
        let found = matcher::find(&self.index, &heard.peaks, heard.frames, options.tempo_range, itself, ctx)?;
        if info.duration_secs <= 0.0 {
            info.duration_secs = heard.frames as f64 / FRAMES_PER_SEC;
        }
        let duration = info.duration_secs;
        let plays: Vec<Play> = found.iter().filter_map(|f| self.play(f)).collect();
        let transitions = transitions(&plays);
        let unidentified = matcher::gaps(&found, duration, UNIDENTIFIED_SECS)
            .into_iter()
            .map(|(start, end)| Span { start, end })
            .collect();
        Ok(MixReport {
            mix: info,
            drive: self.drive.clone(),
            drive_id: self.drive_id,
            library: self.report.clone(),
            transitions,
            unidentified,
            coverage: matcher::coverage(&found, duration),
            waveform: waveform(&heard.levels),
            plays,
            tempo_range: options.tempo_range,
            analysed_at: chrono::Utc::now(),
            elapsed_secs: started.elapsed().as_secs_f64(),
        })
    }

    fn play(&self, f: &Found) -> Option<Play> {
        let id = *self.index.track_ids.get(f.slot as usize)?;
        let track = self.library.track(id)?.clone();
        let semitones = if f.key_lock == Some(true) { 0.0 } else { 12.0 * f.tempo.log2() };
        let key = Camelot::parse(&track.key).map(Camelot::code);
        let played_key = Camelot::parse(&track.key).map(|k| {
            // Without key lock the key moves; a semitone is seven steps round the wheel.
            let shift = if f.key_lock == Some(false) { semitones.round() as i32 } else { 0 };
            let number = (k.number as i32 - 1 + 7 * shift).rem_euclid(12) + 1;
            Camelot { number: number as u8, major: k.major }.code()
        });
        let density = f.votes as f64 / f.spanned.max(1) as f64;
        let continuity = f.heard as f64 / f.spanned.max(1) as f64;
        let confidence = ((1.0 - (-density / 14.0).exp()) * continuity.sqrt()).clamp(0.0, 1.0);
        let alternates = f
            .alternates
            .iter()
            .filter_map(|&(slot, same_audio)| {
                let id = *self.index.track_ids.get(slot as usize)?;
                let t = self.library.track(id)?;
                Some(Alternate { track_id: id, name: t.display_name(), file_path: t.file_path.clone(), same_audio })
            })
            .collect();
        Some(Play {
            played_bpm: (track.bpm > 0.0).then_some(track.bpm * f.tempo),
            track,
            start: f.start,
            end: f.end,
            track_start: f.track_start,
            track_end: f.track_end,
            tempo: f.tempo,
            key_lock: f.key_lock,
            semitones,
            key,
            played_key,
            confidence: (confidence * 100.0).round() / 100.0,
            votes: f.votes,
            presence: f.presence.clone(),
            presence_step: matcher::PRESENCE_STEP,
            jumps: f.jumps,
            alternates,
        })
    }
}

/// A mix decoded and fingerprinted.
pub struct Heard {
    pub peaks: Vec<fingerprint::Peak>,
    pub levels: Vec<FrameLevels>,
    pub frames: u32,
}

/// Whether `path` is something [`Identifier::identify`] can read.
pub fn is_mix_file(path: &Path) -> bool {
    aup::is_project(path) || audio::is_decodable(path)
}

/// Decode and fingerprint a mix, and gather what its file says about it.
pub fn listen(path: &Path, ctx: &Ctx<'_>) -> Result<(MixInfo, Heard)> {
    // Absolute: its folder is looked in (for a cover, a project's audio), and a kept report
    // finds it again by this path from anywhere.
    let path = &std::path::absolute(path).at(path)?;
    if !path.is_file() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    let mut analyzer = Analyzer::new(QUERY, true);
    ctx.phase_start(Phase::Listen, 0, 0);
    let mut info = MixInfo {
        path: path.to_path_buf(),
        kind: "audio",
        title: String::new(),
        artist: None,
        genre: None,
        date: None,
        recorder: None,
        duration_secs: 0.0,
        sample_rate: 0,
        channels: 0,
        labels: Vec::new(),
        cover: None,
    };
    let mut tags: Vec<(String, String)> = Vec::new();
    let result = if aup::is_project(path) {
        info.kind = "audacity";
        let project = Project::open(path)?;
        tags = project.tags.clone();
        info.labels = project.labels.clone();
        project.render(fingerprint::RATE, ctx, &mut |s| analyzer.push(s))
    } else {
        if let Ok(tagged) = lofty::read_from_path(path)
            && let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag())
        {
            let mut get = |key: ItemKey, name: &str| {
                if let Some(v) = tag.get_string(key).map(str::trim).filter(|v| !v.is_empty()) {
                    tags.push((name.to_string(), v.to_string()));
                }
            };
            get(ItemKey::TrackTitle, "TITLE");
            get(ItemKey::TrackArtist, "ARTIST");
            get(ItemKey::Genre, "GENRE");
            get(ItemKey::RecordingDate, "YEAR");
            get(ItemKey::Year, "YEAR");
            if !tag.pictures().is_empty() {
                info.cover = Some(CoverSource::Embedded(path.to_path_buf()));
            }
        }
        audio::decode(path, fingerprint::RATE, ctx, &mut |s| analyzer.push(s))
    };
    ctx.phase_end(Phase::Listen);
    let format = result?;
    info.duration_secs = format.duration_secs;
    info.sample_rate = format.sample_rate;
    info.channels = format.channels;
    describe(&mut info, &tags);
    if info.cover.is_none() {
        info.cover = cover_next_to(path).map(CoverSource::File);
    }
    let frames = analyzer.frames();
    let (peaks, levels) = analyzer.finish();
    Ok((info, Heard { peaks, levels, frames }))
}

/// Title, artist and so on from tags. Pioneer DJ's recorders write `REC046` as the title,
/// `PIONEER DJ REC` as the artist and their model as the genre: that's a recorder, not a
/// description, so the title falls back to the folder the recording was filed in.
fn describe(info: &mut MixInfo, tags: &[(String, String)]) {
    let tag = |name: &str| tags.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
    let artist = tag("ARTIST");
    let from_recorder = artist.as_deref().is_some_and(|a| a.eq_ignore_ascii_case("PIONEER DJ REC"));
    if from_recorder {
        info.recorder = tag("GENRE").map(|model| format!("Pioneer DJ {model}"));
    } else {
        info.artist = artist;
        info.genre = tag("GENRE");
    }
    info.date = tag("YEAR");
    let generic = |t: &str| {
        let t = t.trim();
        t.is_empty() || (t.len() <= 8 && t.starts_with("REC") && t[3..].chars().all(|c| c.is_ascii_digit()))
    };
    info.title = match tag("TITLE").filter(|t| !generic(t)) {
        Some(t) => t,
        None if from_recorder => {
            info.path.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        }
        None => info.path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    };
}

/// An image next to the mix that is obviously its cover: named like the file or its folder, or
/// cover/folder/front/artwork.
fn cover_next_to(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_string_lossy().to_lowercase();
    let folder = dir.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut images: Vec<(u8, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter_map(|p| {
            let ext = p.extension()?.to_str()?.to_ascii_lowercase();
            if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp") {
                return None;
            }
            let name = p.file_stem()?.to_string_lossy().to_lowercase();
            let rank = if name == stem {
                0
            } else if name == folder || folder.ends_with(&name) && name.len() >= 4 {
                1
            } else if ["cover", "folder", "front", "artwork"].contains(&name.as_str()) {
                2
            } else {
                return None;
            };
            Some((rank, p))
        })
        .collect();
    images.sort();
    images.into_iter().next().map(|(_, p)| p)
}

fn transitions(plays: &[Play]) -> Vec<Transition> {
    plays
        .windows(2)
        .enumerate()
        .map(|(i, w)| {
            let (a, b) = (&w[0], &w[1]);
            let harmony = match (
                a.played_key.as_deref().and_then(Camelot::parse),
                b.played_key.as_deref().and_then(Camelot::parse),
            ) {
                (Some(x), Some(y)) => {
                    Some(x.relation_with_boost(y).map_or("clash".to_string(), |r| r.describe().to_string()))
                }
                _ => None,
            };
            let tempo_change = match (a.played_bpm, b.played_bpm) {
                (Some(x), Some(y)) if x > 0.0 => Some(((y / x - 1.0) * 1000.0).round() / 10.0),
                _ => None,
            };
            Transition { from: i, to: i + 1, at: b.start, overlap: a.end - b.start, harmony, tempo_change }
        })
        .collect()
}

/// Columns per second of the mix waveform.
const WAVEFORM_RATE: f64 = 4.0;

fn waveform(levels: &[FrameLevels]) -> MixWaveform {
    let per = FRAMES_PER_SEC / WAVEFORM_RATE;
    let columns = (levels.len() as f64 / per).ceil() as usize;
    let mut peak = vec![0f32; columns];
    let mut bands = vec![[0f32; 3]; columns];
    for (i, l) in levels.iter().enumerate() {
        let c = ((i as f64 / per) as usize).min(columns.saturating_sub(1));
        peak[c] = peak[c].max(l.peak);
        for (acc, v) in bands[c].iter_mut().zip(l.bands) {
            *acc = acc.max(v);
        }
    }
    // Scale each series by a high percentile, so one spike doesn't flatten the rest.
    let scale = |v: &[f32], gamma: f32| -> Vec<u8> {
        let mut sorted: Vec<f32> = v.to_vec();
        sorted.sort_by(f32::total_cmp);
        let top = sorted.get(sorted.len() * 995 / 1000).copied().unwrap_or(0.0).max(1e-9);
        v.iter().map(|x| ((x / top).min(1.0).powf(gamma) * 255.0).round() as u8).collect()
    };
    let band = |b: usize| bands.iter().map(|c| c[b]).collect::<Vec<f32>>();
    MixWaveform {
        columns_per_sec: WAVEFORM_RATE,
        peak: scale(&peak, 1.0),
        low: scale(&band(0), 0.8),
        mid: scale(&band(1), 0.8),
        high: scale(&band(2), 0.8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mix given by a path relative to the current folder is kept by its absolute path: its
    /// folder is where a cover and a project's audio are looked for, and a kept report finds it
    /// again from anywhere.
    #[cfg(unix)]
    #[test]
    fn mixes_are_kept_by_their_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let set = dir.path().join("set.wav");
        std::fs::write(&set, crate::probe::tests::wav(8_000, 16, 0.5)).unwrap();
        let up = std::env::current_dir().unwrap().components().count() - 1;
        let relative =
            std::iter::repeat_n(Path::new(".."), up).collect::<PathBuf>().join(set.strip_prefix("/").unwrap());
        let (info, _) = listen(&relative, &Ctx::silent()).unwrap();
        assert!(info.path.is_absolute(), "{}", info.path.display());
    }

    /// A recording that's on the drive too (a set imported into rekordbox) isn't one of the
    /// tracks heard in it: it would match the whole mix and swallow them.
    #[test]
    fn a_recording_on_the_drive_is_left_out_of_its_own_tracks() {
        use crate::mix::fingerprint::{RATE, tests::synth};
        use crate::model::tests::track;
        use crate::probe::tests::wav_of;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("drive");
        std::fs::create_dir_all(root.join("Contents")).unwrap();
        let (a, b) = (synth(40.0, 7919), synth(40.0, 3 * 7919));
        let set = [&a[..], &b[..]].concat();
        for (name, x) in [("a.wav", &a), ("b.wav", &b), ("set.wav", &set)] {
            let pcm = wav_of(RATE, 16, x.iter().map(|s| (s * 12_000.0) as i32));
            std::fs::write(root.join("Contents").join(name), pcm).unwrap();
        }
        let tracks = vec![
            track(1, "A", "One", "/Contents/a.wav"),
            track(2, "B", "Two", "/Contents/b.wav"),
            track(3, "DJ", "Live", "/Contents/set.wav"),
        ];
        let library = Arc::new(Library { tracks, ..Default::default() });
        let device = Device::open(&root).unwrap();
        let cache = Cache::new(dir.path().join("cache"));
        let id = Identifier::with_library(&device, library, &cache, &Ctx::silent()).unwrap();
        let r = id.identify(&root.join("Contents/set.wav"), &Options { tempo_range: 0.02 }, &Ctx::silent()).unwrap();
        assert_eq!(r.plays.iter().map(|p| p.track.id).collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn recorder_tags_are_not_a_title() {
        let mut info = MixInfo {
            path: PathBuf::from("/mixes/half-savage and hardy, and free/dubstep mix one original.WAV"),
            kind: "audio",
            title: String::new(),
            artist: None,
            genre: None,
            date: None,
            recorder: None,
            duration_secs: 0.0,
            sample_rate: 0,
            channels: 0,
            labels: Vec::new(),
            cover: None,
        };
        let tags = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        describe(&mut info, &tags(&[("TITLE", "REC046"), ("ARTIST", "PIONEER DJ REC"), ("GENRE", "XDJ-RX2")]));
        assert_eq!(info.title, "half-savage and hardy, and free");
        assert_eq!(info.recorder.as_deref(), Some("Pioneer DJ XDJ-RX2"));
        assert_eq!(info.artist, None);

        describe(&mut info, &tags(&[("TITLE", "Live at the Warehouse"), ("ARTIST", "Example DJ")]));
        assert_eq!(info.title, "Live at the Warehouse");
        assert_eq!(info.artist.as_deref(), Some("Example DJ"));
    }
}
