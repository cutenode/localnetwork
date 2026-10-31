//! Reading audio files themselves, through symphonia: their format, sample rate and bit depth,
//! for when a track's database row doesn't say, and ([`check_tracks`]) whether they open and
//! decode to the end, which catches files an interrupted copy cut short.
//!
//! symphonia trusts what a file's headers say: a WAV saying 0 Hz panics it. Reading a file's audio
//! ([`format()`], [`check_tracks`], mix fingerprinting) and opening it to play happen inside
//! [`guarded`], so one bad file is that file's error rather than a crash. The player catches a
//! panic while decoding as it plays on its own audio thread. An MP4 claiming more than it holds
//! would make symphonia abort the process, which nothing can catch: [`vet_container`] refuses it
//! first.

use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;
use symphonia::core::codecs::{
    CODEC_TYPE_AAC, CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CodecType, DecoderOptions,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{SeekMode, SeekTo};
use symphonia::core::units::{Time, TimeBase};

/// For the player, which opens files through rodio rather than [`open`].
pub use loclib_mixid::audio::vet_container;
use loclib_mixid::audio::{CRASHED, open};
use loclib_model::audio;

use crate::device::Device;
use crate::equipment::Codec;
use crate::model::{Library, TrackRef};
use crate::paths;
use crate::progress::Ctx;
use crate::progress::{Event, Phase};
use crate::tree::FileTree;
use crate::{Error, Result};

/// What a file's own headers say about its audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Format {
    /// `None`: a codec players don't list (or PCM in a container that isn't WAV or AIFF).
    pub codec: Option<Codec>,
    /// Hz; 0 when the file doesn't say.
    pub sample_rate: u32,
    /// Bits per sample; 0 when the file doesn't say.
    pub bit_depth: u16,
    /// 0 when the file doesn't say.
    pub channels: u16,
    /// Seconds, when the file says how many frames it holds.
    pub duration_secs: Option<f64>,
}

/// PCM, in any of symphonia's sample formats (its PCM decoders are all named `pcm_*`).
fn is_pcm(c: CodecType) -> bool {
    symphonia::default::get_codecs().get_codec(c).is_some_and(|d| d.short_name.starts_with("pcm"))
}

/// Run `f`, which opens or decodes the audio file at `path`, with a panic in it turned into an
/// error about that file.
pub fn guarded<T, E: From<Error>>(
    path: &Path,
    f: impl FnOnce() -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    crate::pdb::catch_quietly(f).unwrap_or_else(|_| Err(Error::parse(path, CRASHED).into()))
}

/// The format of the audio in `path`, from its headers (nothing is decoded).
pub fn format(path: &Path) -> Result<Format> {
    let track = guarded(path, || open(path).map(|(_, track)| track).map_err(Error::from))?;
    let p = &track.codec_params;
    let ext = path.extension().and_then(|e| e.to_str()).and_then(audio::Format::from_extension);
    let codec = match p.codec {
        CODEC_TYPE_MP3 => Some(Codec::Mp3),
        CODEC_TYPE_AAC => Some(Codec::Aac),
        CODEC_TYPE_ALAC => Some(Codec::Alac),
        CODEC_TYPE_FLAC => Some(Codec::Flac),
        c if is_pcm(c) => match ext {
            Some(audio::Format::Wav) => Some(Codec::Wav),
            Some(audio::Format::Aiff) => Some(Codec::Aiff),
            _ => None,
        },
        _ => None,
    };
    let sample_rate = p.sample_rate.unwrap_or(0);
    let duration_secs = match (p.n_frames, sample_rate) {
        (Some(n), r) if r > 0 => Some(n as f64 / r as f64),
        _ => None,
    };
    Ok(Format {
        codec,
        sample_rate,
        bit_depth: p.bits_per_sample.or(p.bits_per_coded_sample).unwrap_or(0).min(u16::MAX.into()) as u16,
        channels: p.channels.map_or(0, |c| c.count() as u16),
        duration_secs,
    })
}

/// Run `work` on every item, on every core, reporting each item (by `describe`: its path and
/// size) as progress in `phase`. Results come back in `items`' order. Stops early when cancelled.
pub(crate) fn parallel<T: Sync, R: Send>(
    items: &[T],
    phase: Phase,
    describe: impl Fn(&T) -> (String, u64) + Sync,
    work: impl Fn(&T) -> R + Sync,
    ctx: &Ctx<'_>,
) -> Result<Vec<R>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    ctx.phase_start(phase, items.len() as u64, 0);
    let results = crate::workers::each(items.iter().collect(), crate::workers::per_core(), |item| -> Result<R> {
        ctx.check()?;
        let (path, bytes) = describe(item);
        ctx.emit(Event::Item { phase, path, bytes });
        Ok(work(item))
    });
    ctx.phase_end(phase);
    let mut out = Vec::with_capacity(results.len());
    for r in results {
        match r {
            Ok(r) => out.push(r?),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    Ok(out)
}

/// What's wrong with an audio file.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AudioProblem {
    /// It doesn't open as audio, or there's no decoder for it.
    Unreadable { error: String },
    /// Its audio stops before the length the database (or its own header) gives: a copy cut
    /// short, most likely.
    Truncated { ends_secs: f64, expected_secs: f64 },
    /// Its last seconds don't decode.
    Undecodable { error: String },
    /// Its header gives a different length from the database's (a replaced file?).
    LengthDiffers { file_secs: f64, expected_secs: f64 },
}

impl AudioProblem {
    /// Plays, but not as the database says: worth a look rather than a failed check.
    pub fn is_warning(&self) -> bool {
        matches!(self, AudioProblem::LengthDiffers { .. })
    }

    /// As serialised: `unreadable`, `truncated`, `undecodable` or `length_differs`.
    pub fn kind(&self) -> &'static str {
        match self {
            AudioProblem::Unreadable { .. } => "unreadable",
            AudioProblem::Truncated { .. } => "truncated",
            AudioProblem::Undecodable { .. } => "undecodable",
            AudioProblem::LengthDiffers { .. } => "length_differs",
        }
    }
}

/// `3:07`.
pub fn minutes(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

impl std::fmt::Display for AudioProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioProblem::Unreadable { error } => write!(f, "can't be opened: {error}"),
            AudioProblem::Truncated { ends_secs, expected_secs } => {
                write!(f, "stops at {} of {}", minutes(*ends_secs), minutes(*expected_secs))
            }
            AudioProblem::Undecodable { error } => write!(f, "its end doesn't decode: {error}"),
            AudioProblem::LengthDiffers { file_secs, expected_secs } => {
                write!(f, "{} long, the database says {}", minutes(*file_secs), minutes(*expected_secs))
            }
        }
    }
}

/// How far two lengths may differ: the database keeps whole seconds, and encoders pad.
fn tolerance(secs: f64) -> f64 {
    2.0 + secs * 0.005
}

/// How much of the end is decoded.
pub const TAIL_SECS: f64 = 10.0;

fn seconds(tb: Option<TimeBase>, rate: u32, ts: u64) -> f64 {
    match (tb, rate) {
        (Some(tb), _) => {
            let t = tb.calc_time(ts);
            t.seconds as f64 + t.frac
        }
        (None, r) if r > 0 => ts as f64 / r as f64,
        _ => 0.0,
    }
}

/// Where a file's packets end, read without decoding (for a file whose end couldn't be reached).
fn packets_end(path: &Path) -> Option<f64> {
    let (mut format, track) = open(path).ok()?;
    let (tb, rate) = (track.codec_params.time_base, track.codec_params.sample_rate.unwrap_or(0));
    let mut end = 0;
    loop {
        match format.next_packet() {
            Ok(p) if p.track_id() == track.id => end = end.max(p.ts() + p.dur()),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Some(seconds(tb, rate, end))
}

/// Whether `path` opens, is about `expected_secs` long (the database's length; 0 if unknown),
/// and decodes over its last [`TAIL_SECS`]. Only the end is decoded; a file whose end can't be
/// reached is read to its last packet (without decoding) to say where it stops.
///
/// `expected_bytes` (the database's size; 0 if unknown) tells a copy cut short from a file that
/// replaced it when the length comes from the file's size (constant bit rate MP3s without a
/// header saying how long they are): a cut copy lost as much of its size as of its length.
pub fn health(path: &Path, expected_secs: f64, expected_bytes: u64) -> Option<AudioProblem> {
    let problem = health_of(path, expected_secs)?;
    let size = std::fs::metadata(path).map_or(0, |m| m.len());
    match problem {
        AudioProblem::LengthDiffers { file_secs, expected_secs }
            if expected_bytes > 0
                && file_secs < expected_secs
                && (size as f64 / expected_bytes as f64 - file_secs / expected_secs).abs() < 0.05 =>
        {
            Some(AudioProblem::Truncated { ends_secs: file_secs, expected_secs })
        }
        p => Some(p),
    }
}

fn health_of(path: &Path, expected_secs: f64) -> Option<AudioProblem> {
    let unreadable = |e: &dyn std::fmt::Display| Some(AudioProblem::Unreadable { error: e.to_string() });
    let (mut format, track) = match open(path) {
        Ok(f) => f,
        Err(e) => return unreadable(&e),
    };
    let params = &track.codec_params;
    let mut decoder = match symphonia::default::get_codecs().make(params, &DecoderOptions::default()) {
        Ok(d) => d,
        Err(e) => return unreadable(&e),
    };
    let (tb, rate) = (params.time_base, params.sample_rate.unwrap_or(0));
    let header_secs = params.n_frames.map(|n| seconds(tb, rate, n));
    let expected = match (expected_secs, header_secs) {
        (e, _) if e > 0.0 => e,
        (_, Some(h)) => h,
        // No length anywhere: it opens, and its first seconds are decoded below.
        _ => 0.0,
    };
    // A header that disagrees with the database means another file (one that replaced it): its
    // own length is then what its end is checked against.
    let (expected, length_differs) = match header_secs {
        Some(h) if expected_secs > 0.0 && (h - expected_secs).abs() > tolerance(expected_secs) => {
            (h, Some(AudioProblem::LengthDiffers { file_secs: h, expected_secs }))
        }
        _ => (expected, None),
    };
    let from = (expected - TAIL_SECS - tolerance(expected)).max(0.0);
    let start = match from > 0.0 {
        false => 0.0,
        true => {
            match format.seek(SeekMode::Coarse, SeekTo::Time { time: Time::from(from), track_id: Some(track.id) }) {
                Ok(to) => seconds(tb, rate, to.actual_ts),
                Err(_) => {
                    let ends_secs = packets_end(path).unwrap_or(0.0);
                    return Some(AudioProblem::Truncated { ends_secs, expected_secs: expected });
                }
            }
        }
    };
    decoder.reset();
    let (mut decoded, mut failed, mut last_error, mut end) = (0u32, 0u32, None, 0u64);
    let stop = expected + tolerance(expected) + 1.0;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => {
                failed += 1;
                last_error = Some(e.to_string());
                break;
            }
        };
        if packet.track_id() != track.id {
            continue;
        }
        end = end.max(packet.ts() + packet.dur());
        match decoder.decode(&packet) {
            Ok(_) => decoded += 1,
            Err(e) => {
                failed += 1;
                last_error = Some(e.to_string());
            }
        }
        // Enough when there's no length to reach, or the file runs on past it.
        if (expected == 0.0 && decoded >= 16) || (expected > 0.0 && seconds(tb, rate, end) > stop) {
            break;
        }
    }
    let ends_secs = seconds(tb, rate, end).max(start);
    if decoded == 0 && failed > 0 {
        return Some(AudioProblem::Undecodable { error: last_error.unwrap_or_default() });
    }
    if decoded == 0 || (expected > 0.0 && ends_secs < expected - tolerance(expected)) {
        // Where it really stops, when the seek landed past the end.
        let ends_secs = if decoded == 0 { packets_end(path).unwrap_or(start) } else { ends_secs };
        return Some(AudioProblem::Truncated { ends_secs, expected_secs: expected });
    }
    if failed > decoded {
        return Some(AudioProblem::Undecodable { error: last_error.unwrap_or_default() });
    }
    length_differs
}

/// A track whose audio file has a problem.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AudioFinding {
    pub track: TrackRef,
    pub problem: AudioProblem,
}

/// Every audio file `library`'s tracks use (once each), checked with [`health`] on every core,
/// with progress in [`Phase::Verify`]. Files that aren't there are left to the missing-file
/// checks. Returns how many files were checked, and what was found.
pub fn check_tracks(
    device: &Device,
    library: &Library,
    tree: &FileTree,
    ctx: &Ctx<'_>,
) -> Result<(usize, Vec<AudioFinding>)> {
    let mut seen = FxHashSet::default();
    // (track, file, its size, the database's length and size)
    let mut todo: Vec<(TrackRef, PathBuf, u64, f64, u64)> = Vec::new();
    for t in &library.tracks {
        let key = paths::key(&t.file_path);
        if !seen.insert(key.to_lowercase()) {
            continue;
        }
        let file = match tree.resolve(&key) {
            Some(e) => Some((device.path(&e.path), e.size)),
            None => device.checked_path(&key).ok().filter(|p| p.is_file()).map(|p| (p, t.file_size)),
        };
        if let Some((path, size)) = file {
            todo.push((t.into(), path, size, f64::from(t.duration_secs), t.file_size));
        }
    }
    let results = parallel(
        &todo,
        Phase::Verify,
        |(t, _, size, ..)| (t.file_path.clone(), *size),
        |(_, path, _, secs, bytes)| {
            guarded(path, || Ok::<_, Error>(health(path, *secs, *bytes)))
                .unwrap_or_else(|_| Some(AudioProblem::Undecodable { error: CRASHED.into() }))
        },
        ctx,
    )?;
    let findings = todo
        .iter()
        .zip(results)
        .filter_map(|((track, ..), problem)| problem.map(|problem| AudioFinding { track: track.clone(), problem }))
        .collect();
    Ok((todo.len(), findings))
}

#[cfg(test)]
pub(crate) mod tests;
