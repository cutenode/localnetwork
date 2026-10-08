//! Reading audio files themselves, through symphonia: their format, sample rate and bit depth,
//! for when a track's database row doesn't say, and ([`check_tracks`]) whether they open and
//! decode to the end, which catches files an interrupted copy cut short.
//!
//! symphonia trusts what a file's headers say: a WAV saying 0 Hz panics it. Reading a file's audio
//! ([`format()`], [`check_tracks`], mix fingerprinting) and opening it to play happen inside
//! [`guarded`], so one bad file is that file's error rather than a crash. The player catches a
//! panic while decoding as it plays on its own audio thread.

use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use schemars::JsonSchema;
use serde::Serialize;
use symphonia::core::codecs::{
    CODEC_TYPE_AAC, CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_NULL, CodecType, DecoderOptions,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, Track};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};

use crate::device::Device;
use crate::equipment::Codec;
use crate::error::IoContext;
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
    /// Seconds, when the file says how many frames it holds.
    pub duration_secs: Option<f64>,
}

/// PCM, in any of symphonia's sample formats (its PCM decoders are all named `pcm_*`).
fn is_pcm(c: CodecType) -> bool {
    symphonia::default::get_codecs().get_codec(c).is_some_and(|d| d.short_name.starts_with("pcm"))
}

/// What a decoder that panicked on a file is reported as.
const CRASHED: &str = "the decoder crashed on it";

/// Run `f`, which opens or decodes the audio file at `path`, with a panic in it turned into an
/// error about that file.
pub fn guarded<T, E: From<Error>>(
    path: &Path,
    f: impl FnOnce() -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    crate::pdb::catch_quietly(f).unwrap_or_else(|_| Err(Error::parse(path, CRASHED).into()))
}

/// Open `path` and read its first audio track's format, refusing one no decoder can play (call
/// it inside [`guarded`]).
pub(crate) fn open(path: &Path) -> Result<(Box<dyn FormatReader>, Track)> {
    let file = File::open(path).at(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| Error::parse(path, e))?;
    let track = probed
        .format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .cloned()
        .ok_or_else(|| Error::parse(path, "no audio track"))?;
    let p = &track.codec_params;
    if p.sample_rate == Some(0) || p.channels.is_some_and(|c| c.count() == 0) {
        return Err(Error::parse(path, "its header says 0 Hz or no channels"));
    }
    Ok((probed.format, track))
}

/// The format of the audio in `path`, from its headers (nothing is decoded).
pub fn format(path: &Path) -> Result<Format> {
    let track = guarded(path, || open(path).map(|(_, track)| track))?;
    let p = &track.codec_params;
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let codec = match p.codec {
        CODEC_TYPE_MP3 => Some(Codec::Mp3),
        CODEC_TYPE_AAC => Some(Codec::Aac),
        CODEC_TYPE_ALAC => Some(Codec::Alac),
        CODEC_TYPE_FLAC => Some(Codec::Flac),
        c if is_pcm(c) => match ext.as_deref() {
            Some("wav") => Some(Codec::Wav),
            Some("aif" | "aiff") => Some(Codec::Aiff),
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
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::with_capacity(items.len()));
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(items.len());
    let cancelled = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| -> Result<()> {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { return Ok(()) };
                        ctx.check()?;
                        let (path, bytes) = describe(item);
                        ctx.emit(Event::Item { phase, path, bytes });
                        let r = work(item);
                        done.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((i, r));
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("probe worker panicked")).any(|r| r.is_err())
    });
    ctx.phase_end(phase);
    if cancelled {
        return Err(Error::Cancelled);
    }
    let mut done = done.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    done.sort_by_key(|(i, _)| *i);
    Ok(done.into_iter().map(|(_, r)| r).collect())
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
    let mut seen = HashSet::new();
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
pub(crate) mod tests {
    use super::*;

    /// A WAV file of `secs` seconds of a quiet tone.
    pub fn wav(rate: u32, bits: u16, secs: f64) -> Vec<u8> {
        let frames = (rate as f64 * secs) as u32;
        wav_of(rate, bits, (0..frames).map(|i| ((i as f64 * 0.05).sin() * 1000.0) as i32))
    }

    /// A stereo WAV file holding `samples` in both channels.
    pub fn wav_of(rate: u32, bits: u16, samples: impl ExactSizeIterator<Item = i32>) -> Vec<u8> {
        let bytes = bits as u32 / 8;
        let data_len = samples.len() as u32 * bytes * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * bytes * 2).to_le_bytes());
        out.extend_from_slice(&((bytes * 2) as u16).to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for v in samples {
            for _ in 0..2 {
                out.extend_from_slice(&v.to_le_bytes()[..bytes as usize]);
            }
        }
        out
    }

    #[test]
    fn formats_from_headers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        std::fs::write(&path, wav(96_000, 24, 0.5)).unwrap();
        let f = format(&path).unwrap();
        assert_eq!((f.codec, f.sample_rate, f.bit_depth), (Some(Codec::Wav), 96_000, 24));
        assert!((f.duration_secs.unwrap() - 0.5).abs() < 0.01, "{f:?}");
        // PCM in a container players don't list.
        let odd = dir.path().join("a.bin");
        std::fs::copy(&path, &odd).unwrap();
        assert_eq!(format(&odd).unwrap().codec, None);
        std::fs::write(dir.path().join("b.wav"), b"not audio").unwrap();
        assert!(format(&dir.path().join("b.wav")).is_err());
    }

    #[test]
    fn whole_cut_short_and_unreadable_files() {
        let dir = tempfile::tempdir().unwrap();
        let whole = wav(8_000, 16, 30.0);
        let path = dir.path().join("whole.wav");
        std::fs::write(&path, &whole).unwrap();
        let size = whole.len() as u64;
        assert_eq!(health(&path, 30.0, size), None);
        assert_eq!(health(&path, 31.0, size), None, "the database keeps whole seconds");
        assert_eq!(health(&path, 0.0, 0), None, "no length: the header's");

        // An interrupted copy: the header still promises 30 s.
        let cut = dir.path().join("cut.wav");
        std::fs::write(&cut, &whole[..whole.len() * 2 / 5]).unwrap();
        match health(&cut, 30.0, size) {
            Some(AudioProblem::Truncated { ends_secs, expected_secs }) => {
                assert!((ends_secs - 12.0).abs() < 0.5, "{ends_secs}");
                assert_eq!(expected_secs, 30.0);
            }
            other => panic!("{other:?}"),
        }

        // A whole file that isn't the length the database says: another file took its place.
        match health(&path, 60.0, size) {
            Some(AudioProblem::LengthDiffers { file_secs, expected_secs }) => {
                assert!((file_secs - 30.0).abs() < 0.01 && expected_secs == 60.0)
            }
            other => panic!("{other:?}"),
        }
        assert!(health(&path, 60.0, size).unwrap().is_warning());
        // ...unless it lost as much of its size as of its length: a cut copy whose length comes
        // from its size (as a constant bit rate MP3's does).
        assert!(matches!(health(&path, 60.0, size * 2), Some(AudioProblem::Truncated { .. })));

        let junk = dir.path().join("junk.mp3");
        std::fs::write(&junk, b"not audio at all").unwrap();
        assert!(matches!(health(&junk, 200.0, 0), Some(AudioProblem::Unreadable { .. })));
        assert!(matches!(health(&dir.path().join("gone.mp3"), 1.0, 0), Some(AudioProblem::Unreadable { .. })));
    }

    /// symphonia panics on a WAV saying 0 Hz: reading its format is an error about that file.
    #[test]
    fn a_file_that_crashes_the_decoder_is_its_own_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zero.wav");
        std::fs::write(&path, wav(0, 16, 0.0)).unwrap();
        let err = format(&path).unwrap_err();
        assert!(matches!(err.inner(), Error::Parse { message, .. } if message == CRASHED), "{err}");
    }

    /// An AIFF file of `frames` frames of 48 kHz mono 16-bit silence.
    pub(crate) fn aiff(frames: u32) -> Vec<u8> {
        let mut comm = 1u16.to_be_bytes().to_vec();
        comm.extend_from_slice(&frames.to_be_bytes());
        comm.extend_from_slice(&16u16.to_be_bytes());
        comm.extend_from_slice(&[0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]); // 48000 as an 80-bit float
        let data = vec![0u8; frames as usize * 2];
        let mut body = b"AIFFCOMM".to_vec();
        body.extend_from_slice(&18u32.to_be_bytes());
        body.extend_from_slice(&comm);
        body.extend_from_slice(b"SSND");
        body.extend_from_slice(&(8 + data.len() as u32).to_be_bytes());
        body.extend_from_slice(&[0; 8]);
        body.extend_from_slice(&data);
        let mut out = b"FORM".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// PCM in AIFF, which players list as AIFF.
    #[test]
    fn aiff_headers_give_the_format() {
        let dir = tempfile::tempdir().unwrap();
        let aif = dir.path().join("a.aif");
        std::fs::write(&aif, aiff(24_000)).unwrap();
        let f = format(&aif).unwrap();
        assert_eq!((f.codec, f.sample_rate, f.bit_depth), (Some(Codec::Aiff), 48_000, 16));
        assert!((f.duration_secs.unwrap() - 0.5).abs() < 0.01, "{f:?}");
        assert_eq!(health(&aif, 0.0, 0), None);
    }

    #[test]
    fn problems_read_as_sentences() {
        let cases = [
            (
                AudioProblem::Unreadable { error: "no audio track".into() },
                "unreadable",
                "can't be opened: no audio track",
            ),
            (AudioProblem::Truncated { ends_secs: 72.4, expected_secs: 187.0 }, "truncated", "stops at 1:12 of 3:07"),
            (
                AudioProblem::Undecodable { error: "invalid main_data".into() },
                "undecodable",
                "its end doesn't decode: invalid main_data",
            ),
            (
                AudioProblem::LengthDiffers { file_secs: 241.0, expected_secs: 187.0 },
                "length_differs",
                "4:01 long, the database says 3:07",
            ),
        ];
        for (problem, kind, text) in cases {
            assert_eq!((problem.kind(), problem.to_string().as_str()), (kind, text));
            assert_eq!(problem.is_warning(), kind == "length_differs");
        }
        assert_eq!(minutes(-3.0), "0:00");
    }

    #[test]
    fn parallel_work_stops_when_cancelled() {
        let cancel = crate::progress::CancelToken::new();
        cancel.cancel();
        let ctx = Ctx::new(&crate::progress::NoProgress, cancel);
        let r = parallel(&[1, 2, 3], Phase::Verify, |i| (i.to_string(), 0), |i| i * 2, &ctx);
        assert!(matches!(r, Err(Error::Cancelled)));
        let none: Vec<u32> = Vec::new();
        assert!(parallel(&none, Phase::Verify, |i| (i.to_string(), 0), |i| i * 2, &ctx).unwrap().is_empty());
    }

    #[test]
    fn parallel_keeps_order() {
        let items: Vec<u32> = (0..100).collect();
        let out = parallel(&items, Phase::Scan, |i| (i.to_string(), 0), |i| i * 2, &Ctx::silent()).unwrap();
        assert_eq!(out, items.iter().map(|i| i * 2).collect::<Vec<_>>());
    }
}
