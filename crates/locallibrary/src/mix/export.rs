//! A mix report as a self-contained data document, for other software to render.
//!
//! [`MixExport`] is [`MixReport`] reshaped for publishing: everything a page needs to draw the
//! report (the mix's 3-band waveform, which tracks play when and how clearly, tempo and key as
//! played, transitions, each track's overview waveform, hot cues and phrases, small artwork) and
//! nothing that only makes sense on this computer. File paths, the drive (its name, its other
//! tracks), ratings, play counts and comments are left out, and so is a cover found next to the
//! recording rather than embedded in it or given for it (`mix --cover`). Tracks the DJ's profile
//! hides (see [`ProfilePrefs::may_name`]) are "ID - ID", as DJs list an unreleased track. Local
//! Network renders it on a DJ's profile.
//!
//! The format is versioned: `format` is always [`FORMAT`] and `version` [`VERSION`]. Fields are
//! camelCase; times are seconds into the mix (or into the track, for `trackStart`/`trackEnd`,
//! cues and phrases), rounded to hundredths. A version only ever gains optional fields; anything
//! else is a new version. Version 2 dropped `library` (the drive's name and track count).

use base64::prelude::*;
use schemars::JsonSchema;
use serde::Serialize;

use super::report::Drive;
use super::{MixReport, Play};
use crate::artwork::{self, Artwork};
use crate::model::Track;
use crate::profile::ProfilePrefs;

pub const FORMAT: &str = "locallibrary.mix-report";
pub const VERSION: u32 = 2;

/// What a track the DJ doesn't name is called, for its artist and title.
pub const UNNAMED: &str = "ID";

/// Whether a report shared with others may name `t`: not when the DJ's profile hides the track
/// or its artist.
pub fn may_name(prefs: &ProfilePrefs, t: &Track) -> bool {
    prefs.may_name(&t.display_name(), &t.artist)
}

/// Most columns in the mix waveform: enough for a full-width page.
const MIX_COLUMNS: usize = 1200;
/// Columns in each track's overview waveform.
const TRACK_COLUMNS: usize = 200;
const COVER_PX: u32 = 320;
const ARTWORK_PX: u32 = 96;

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MixExport {
    pub format: &'static str,
    pub version: u32,
    pub generator: Generator,
    /// RFC 3339.
    pub analysed_at: String,
    pub mix: ExportMix,
    /// Playback speeds searched, as a fraction either way (0.16 is ±16%).
    pub tempo_range: f64,
    /// Fraction of the mix where a track was identified, 0-1.
    pub coverage: f64,
    pub waveform: ExportWaveform,
    pub plays: Vec<ExportPlay>,
    pub transitions: Vec<ExportTransition>,
    /// Stretches where no track was identified.
    pub unidentified: Vec<ExportSpan>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Generator {
    pub name: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[schemars(rename = "ExportedMix")]
#[serde(rename_all = "camelCase")]
pub struct ExportMix {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// The player that recorded it, when the file says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorder: Option<String>,
    pub duration_secs: f64,
    /// The cover given for the recording (`mix --cover`) or embedded in it: a JPEG `data:` URL,
    /// 320 px square.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
}

/// The mix's loudness per column, 0-255, in three bands.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportWaveform {
    pub seconds_per_column: f64,
    /// Below 200 Hz.
    pub low: Vec<u8>,
    /// 200 Hz to 2 kHz.
    pub mid: Vec<u8>,
    /// Above 2 kHz.
    pub high: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlay {
    pub track: ExportTrack,
    /// Seconds into the mix where the track is first and last heard.
    pub start: f64,
    pub end: f64,
    /// Seconds into the track at those points.
    pub track_start: f64,
    pub track_end: f64,
    /// Playback speed: 1.03 is +3%.
    pub tempo: f64,
    /// The slowest and fastest speeds, when the pitch fader was ridden; `tempo` is their average.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_range: Option<(f64, f64)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub played_bpm: Option<f64>,
    /// `None` when the speed is too close to 1.0 to tell.
    pub key_lock: Option<bool>,
    pub semitones: f64,
    /// Camelot notation (`8A`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub played_key: Option<String>,
    /// 0-1.
    pub confidence: f64,
    /// How clearly the track is heard, every `presenceStep` seconds from `start`, 0-100.
    pub presence: Vec<u8>,
    pub presence_step: f64,
    /// Loops and cue jumps.
    pub jumps: u32,
}

/// A track: "ID" for its artist and title (and no more that names it) when the DJ hides it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportTrack {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remixer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    /// As the library stores it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// As recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    pub duration_secs: f64,
    /// JPEG `data:` URL, 96 px square.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<String>,
    /// rekordbox's overview waveform: heights 0-255 and `#rrggbb` colours, one per column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overview: Option<Overview>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cues: Vec<ExportCue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub phrases: Vec<ExportPhrase>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Overview {
    pub heights: Vec<u8>,
    pub colors: Vec<String>,
}

/// A hot cue (A-H), seconds into the track.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ExportCue {
    pub at: f64,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A phrase from rekordbox's analysis (Intro, Up, Down, Chorus, Verse, Bridge, Outro).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ExportPhrase {
    pub start: f64,
    pub end: f64,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportTransition {
    /// Indexes into `plays`.
    pub from: usize,
    pub to: usize,
    pub at: f64,
    /// Seconds both are heard together (negative: a gap).
    pub overlap: f64,
    /// `same key`, `±1`, `relative`, `energy boost` or `clash`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harmony: Option<String>,
    /// Played tempo change, percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tempo_change: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ExportSpan {
    pub start: f64,
    pub end: f64,
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn data_url(jpeg: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", BASE64_STANDARD.encode(jpeg))
}

fn thumbnail(bytes: &[u8], px: u32) -> Option<String> {
    Artwork::from_bytes(bytes).ok()?.jpeg(px).ok().map(|j| data_url(&j))
}

/// Downsample `values` to at most `n` columns, keeping each column's loudest value.
fn downsample(values: &[u8], n: usize) -> Vec<u8> {
    if values.len() <= n {
        return values.to_vec();
    }
    (0..n)
        .map(|i| {
            let lo = i * values.len() / n;
            let hi = ((i + 1) * values.len() / n).max(lo + 1);
            values[lo..hi].iter().copied().max().unwrap_or(0)
        })
        .collect()
}

/// Reshape a report for publishing (see the module docs), with each track's artwork, overview
/// waveform, cues and phrases from `drive` (without it, the export has none of those) and the
/// tracks `prefs` hides left unnamed.
pub fn build(report: &MixReport, drive: Option<Drive<'_>>, prefs: &ProfilePrefs) -> MixExport {
    let w = &report.waveform;
    let columns = w.low.len().min(MIX_COLUMNS);
    let seconds_per_column = if columns > 0 && w.columns_per_sec > 0.0 {
        w.low.len() as f64 / w.columns_per_sec / columns as f64
    } else {
        0.0
    };
    let cover = report.mix.shared_cover().and_then(|b| thumbnail(&b, COVER_PX));
    MixExport {
        format: FORMAT,
        version: VERSION,
        generator: Generator { name: "locallibrary", version: env!("CARGO_PKG_VERSION") },
        analysed_at: report.analysed_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        mix: ExportMix {
            title: non_empty(&report.mix.title).unwrap_or_else(|| "Untitled mix".into()),
            artist: report.mix.artist.as_deref().and_then(non_empty),
            genre: report.mix.genre.as_deref().and_then(non_empty),
            date: report.mix.date.as_deref().and_then(non_empty),
            recorder: report.mix.recorder.as_deref().and_then(non_empty),
            duration_secs: r2(report.mix.duration_secs),
            cover,
        },
        tempo_range: report.tempo_range,
        coverage: (report.coverage * 1000.0).round() / 1000.0,
        waveform: ExportWaveform {
            seconds_per_column: (seconds_per_column * 10_000.0).round() / 10_000.0,
            low: downsample(&w.low, columns),
            mid: downsample(&w.mid, columns),
            high: downsample(&w.high, columns),
        },
        plays: report.plays.iter().enumerate().map(|(i, p)| play(p, i, drive, prefs)).collect(),
        transitions: report
            .transitions
            .iter()
            .map(|t| ExportTransition {
                from: t.from,
                to: t.to,
                at: r2(t.at),
                overlap: r2(t.overlap),
                harmony: t.harmony.clone(),
                tempo_change: t.tempo_change.map(|c| (c * 10.0).round() / 10.0),
            })
            .collect(),
        unidentified: report.unidentified.iter().map(|s| ExportSpan { start: r2(s.start), end: r2(s.end) }).collect(),
    }
}

fn play(p: &Play, i: usize, drive: Option<Drive<'_>>, prefs: &ProfilePrefs) -> ExportPlay {
    let t = &p.track;
    let named = may_name(prefs, t);
    let name = |s: &str| if named { non_empty(s) } else { None };
    let mut track = ExportTrack {
        title: if named { non_empty(&t.title).unwrap_or_else(|| "Untitled".into()) } else { UNNAMED.into() },
        artist: if named { non_empty(&t.artist) } else { Some(UNNAMED.into()) },
        remixer: name(&t.remixer),
        album: name(&t.album),
        label: name(&t.label),
        genre: non_empty(&t.genre),
        year: (named && t.year > 0).then_some(t.year),
        key: non_empty(&t.key),
        bpm: (t.bpm > 0.0).then(|| (t.bpm * 100.0).round() / 100.0),
        duration_secs: f64::from(t.duration_secs),
        artwork: None,
        overview: None,
        cues: Vec::new(),
        phrases: Vec::new(),
    };
    if let Some(drive) = drive
        && let Some(t) = drive.track(p)
    {
        if named {
            track.artwork = artwork::read_from_drive(drive.device, drive.library, t.id)
                .ok()
                .flatten()
                .and_then(|b| thumbnail(&b, ARTWORK_PX));
        }
        let analysis = drive.analysis(i, t);
        track.overview = analysis.preview.as_ref().filter(|w| !w.columns.is_empty()).map(|w| {
            let n = w.columns.len().min(TRACK_COLUMNS);
            let mut heights = Vec::with_capacity(n);
            let mut colors = Vec::with_capacity(n);
            for i in 0..n {
                let lo = i * w.columns.len() / n;
                let hi = ((i + 1) * w.columns.len() / n).max(lo + 1);
                let c =
                    w.columns[lo..hi].iter().max_by(|a, b| a.back.height.total_cmp(&b.back.height)).expect("non-empty");
                heights.push((c.back.height.clamp(0.0, 1.0) * 255.0) as u8);
                let [r, g, b] = c.back.color;
                colors.push(format!("#{r:02x}{g:02x}{b:02x}"));
            }
            Overview { heights, colors }
        });
        let beat_secs =
            |beat: u16| analysis.beats.get((beat as usize).saturating_sub(1)).map(|b| f64::from(b.time_ms) / 1000.0);
        track.phrases = analysis
            .structure
            .iter()
            .flat_map(|s| &s.phrases)
            .filter_map(|ph| {
                let start = beat_secs(ph.start_beat)?;
                let end = beat_secs(ph.end_beat).unwrap_or(f64::from(t.duration_secs));
                Some(ExportPhrase { start: r2(start), end: r2(end), kind: ph.kind.name() })
            })
            .collect();
        track.cues = analysis
            .cues
            .iter()
            .filter_map(|c| {
                let letter = crate::cue_edit::HotCue(c.hot_cue?).letter();
                Some(ExportCue {
                    at: r2(f64::from(c.start_ms) / 1000.0),
                    label: letter.to_string(),
                    color: c.color.map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}")),
                })
            })
            .collect();
    }
    ExportPlay {
        track,
        start: r2(p.start),
        end: r2(p.end),
        track_start: r2(p.track_start),
        track_end: r2(p.track_end),
        tempo: r4(p.tempo),
        speed_range: p.speed_range.map(|(slowest, fastest)| (r4(slowest), r4(fastest))),
        played_bpm: p.played_bpm.map(|b| (b * 10.0).round() / 10.0),
        key_lock: p.key_lock,
        semitones: r2(p.semitones),
        key: p.key.clone(),
        played_key: p.played_key.clone(),
        confidence: p.confidence,
        presence: p.presence.iter().map(|v| (v.clamp(0.0, 1.0) * 100.0).round() as u8).collect(),
        presence_step: p.presence_step,
        jumps: p.jumps,
    }
}

/// A speed to the ten-thousandth.
fn r4(speed: f64) -> f64 {
    (speed * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests;
