//! Decoding audio: [`decode_native`] at a file's own rate and channel count (the one loop over
//! symphonia, which loclib-analysis and loclib-transcode use too), and [`decode`] to mono at a
//! given rate: fingerprints' low one, or 44.1 kHz for [`refine`](fn@crate::refine).
//!
//! Everything a drive or a recorder produces (MP3, AAC/ALAC in `.m4a`, FLAC, WAV, AIFF) decodes
//! through symphonia. Channels are averaged and the result resampled as it streams, so an
//! hour-long recording never sits in memory at full rate. Other sources (an editor's project, a
//! live input) can feed [`Resampler`] themselves.
//!
//! symphonia trusts what a file's headers say: a WAV saying 0 Hz panics it. [`decode`] catches
//! that, so one bad file is that file's error rather than a crash; a program that wants the
//! panic's message kept off stderr installs its own panic hook around the call.

use std::f64::consts::PI;
use std::fs::File;
use std::ops::ControlFlow;
use std::panic::AssertUnwindSafe;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::conv::ConvertibleSample;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, Track};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use loclib_progress::{Cancelled, Ctx};

use crate::{Error, Result};

/// What a decoder that panicked on a file is reported as.
pub const CRASHED: &str = "the decoder crashed on it";

/// Native format of a decoded file.
#[derive(Debug, Clone, Copy, Default)]
pub struct SourceFormat {
    pub sample_rate: u32,
    pub channels: u16,
    /// Seconds of audio decoded.
    pub duration_secs: f64,
}

/// Open `path` and read its first audio track's format, refusing one no decoder can play.
/// symphonia can panic in here on a malformed header: [`decode`] catches that, other callers
/// catch it themselves.
pub fn open(path: &Path) -> Result<(Box<dyn FormatReader>, Track)> {
    let file = File::open(path).map_err(|source| Error::Io { path: path.to_path_buf(), source })?;
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

/// What decoding does with a packet the decoder can't make sense of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Damaged {
    /// Leave it out: a damaged frame in a long recording shouldn't sink it.
    Skip,
    /// Fail: leaving it out would move everything after it earlier (a copy whose cues must stay
    /// in time). A stretch the container's reader skipped (a FLAC frame whose checksum fails is
    /// dropped before it reaches the decoder) fails too, told by the packets' timestamps and the
    /// length the header gives.
    Fail,
}

/// Decode `path` at its own rate and channel count, handing `sink` each packet's samples
/// (interleaved, as `S`) with their rate and channel count, until it breaks. The one decode loop
/// over symphonia every other decode here is made from: a file the decoder can't make sense of
/// is an error, [`CRASHED`] when it panicked on it, and one that decodes to 0 Hz or no channels
/// (an ALAC whose cookie says so) is refused rather than handed on. `ctx` is checked every 256
/// packets. [`SourceFormat::duration_secs`] is what was decoded.
pub fn decode_native<S, E, F>(
    path: &Path,
    damaged: Damaged,
    ctx: &Ctx<'_>,
    mut sink: F,
) -> std::result::Result<SourceFormat, E>
where
    S: ConvertibleSample,
    E: From<Error> + From<Cancelled>,
    F: FnMut(&[S], u32, usize) -> std::result::Result<ControlFlow<()>, E>,
{
    std::panic::catch_unwind(AssertUnwindSafe(|| decode_native_unguarded(path, damaged, ctx, &mut sink)))
        .unwrap_or_else(|_| Err(Error::parse(path, CRASHED).into()))
}

fn decode_native_unguarded<S, E, F>(
    path: &Path,
    damaged: Damaged,
    ctx: &Ctx<'_>,
    sink: &mut F,
) -> std::result::Result<SourceFormat, E>
where
    S: ConvertibleSample,
    E: From<Error> + From<Cancelled>,
    F: FnMut(&[S], u32, usize) -> std::result::Result<ControlFlow<()>, E>,
{
    let (mut format, track) = open(path)?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| Error::parse(path, e))?;
    let mut fmt = SourceFormat {
        sample_rate: track.codec_params.sample_rate.unwrap_or(0),
        channels: track.codec_params.channels.map_or(0, |c| c.count() as u16),
        duration_secs: 0.0,
    };
    let mut buf: Option<SampleBuffer<S>> = None;
    let mut frames = 0u64;
    let mut packets = 0u32;
    // Where the next packet should start, in the track's time base.
    let mut next_ts = None;
    let missing = || Error::parse(path, "a stretch of it is damaged");
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(Error::parse(path, e).into()),
        };
        packets += 1;
        if packets.is_multiple_of(256) {
            ctx.check()?;
        }
        if packet.track_id() != track.id {
            continue;
        }
        if damaged == Damaged::Fail && next_ts.is_some_and(|ts| packet.ts() > ts) {
            return Err(missing().into());
        }
        next_ts = Some(packet.ts() + packet.dur());
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) if damaged == Damaged::Skip => continue,
            Err(e) => return Err(Error::parse(path, e).into()),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        // A decoder can say 0 Hz or no channels where the container didn't (ALAC reads both
        // from its own cookie): there's nothing to hand on.
        if spec.rate == 0 || channels == 0 {
            return Err(Error::parse(path, "it decodes to 0 Hz or no channels").into());
        }
        if buf.as_ref().is_none_or(|b| b.capacity() < decoded.capacity() * channels) {
            buf = Some(SampleBuffer::new(decoded.capacity() as u64, spec));
        }
        let b = buf.as_mut().expect("allocated above");
        b.copy_interleaved_ref(decoded);
        frames += (b.samples().len() / channels) as u64;
        (fmt.sample_rate, fmt.channels) = (spec.rate, channels as u16);
        if sink(b.samples(), spec.rate, channels)?.is_break() {
            return Ok(with_duration(fmt, frames));
        }
    }
    if damaged == Damaged::Fail && track.codec_params.n_frames.is_some_and(|n| next_ts.unwrap_or(0) < n) {
        return Err(missing().into());
    }
    Ok(with_duration(fmt, frames))
}

/// `fmt` with the length of `frames` at its rate.
fn with_duration(mut fmt: SourceFormat, frames: u64) -> SourceFormat {
    if fmt.sample_rate > 0 {
        fmt.duration_secs = frames as f64 / fmt.sample_rate as f64;
    }
    fmt
}

/// Decode `path` to mono at `rate`, handing the signal to `sink` in chunks as it streams.
/// Undecodable packets are skipped (a damaged frame in a long recording shouldn't sink it), and a
/// file the decoder can't make sense of is an error, [`CRASHED`] when it panicked on it.
pub fn decode(path: &Path, rate: u32, ctx: &Ctx<'_>, sink: &mut dyn FnMut(&[f32])) -> Result<SourceFormat> {
    decode_until(path, rate, ctx, &mut |s| {
        sink(s);
        ControlFlow::Continue(())
    })
}

/// [`decode`], stopping once `sink` breaks: for a caller that needs only the start of a file,
/// which then isn't read any further. [`SourceFormat::duration_secs`] is what was decoded.
pub fn decode_until(
    path: &Path,
    rate: u32,
    ctx: &Ctx<'_>,
    sink: &mut dyn FnMut(&[f32]) -> ControlFlow<()>,
) -> Result<SourceFormat> {
    let mut resampler: Option<Resampler> = None;
    let mut mono = Vec::new();
    let mut stopped = false;
    let fmt = decode_native(path, Damaged::Skip, ctx, |samples: &[f32], from, channels| -> Result<_> {
        mono.clear();
        mono.extend(samples.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
        let r = resampler.get_or_insert_with(|| Resampler::new(from, rate));
        r.push(&mono, &mut |s| stopped = stopped || sink(s).is_break());
        Ok(if stopped { ControlFlow::Break(()) } else { ControlFlow::Continue(()) })
    })?;
    if let Some(mut r) = resampler
        && !stopped
    {
        r.finish(&mut |s| {
            let _ = sink(s);
        });
    }
    Ok(fmt)
}

/// Kernel zero crossings on each side; plenty for fingerprints, cheap enough for a whole library.
const ZERO_CROSSINGS: f64 = 8.0;
/// Kernel samples per input sample.
const PHASES: f64 = 64.0;

/// Streaming windowed-sinc resampler (mono). Positions are kept exactly, as whole `1/to`ths of
/// an input sample: the output is the same to the bit however the input is chunked, and an
/// output on a tie between two of the kernel's phases always takes the same one.
pub struct Resampler {
    /// Input samples per output sample: `from / to` in lowest terms, as `step / unit`.
    step: u64,
    unit: u64,
    /// Kernel half-width, in units.
    half: u64,
    table: Vec<f32>,
    /// Input not yet fully consumed; `pos` (in units) indexes into it.
    pending: Vec<f32>,
    pos: u64,
    out: Vec<f32>,
    passthrough: bool,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Resampler {
        assert!(from > 0 && to > 0, "resampling from {from} Hz to {to} Hz");
        let ratio = from as f64 / to as f64;
        // Low-pass just under the lower of the two Nyquist rates, in cycles per input sample.
        let cutoff = 0.5 / ratio.max(1.0) * 0.94;
        let half = ZERO_CROSSINGS / (2.0 * cutoff);
        let len = (half * PHASES).ceil() as usize + 2;
        let table = (0..len)
            .map(|i| {
                let x = i as f64 / PHASES;
                if x >= half {
                    return 0.0;
                }
                let sinc = if x == 0.0 { 1.0 } else { (2.0 * PI * cutoff * x).sin() / (PI * x) / (2.0 * cutoff) };
                // Blackman window.
                let w = x / half;
                let window = 0.42 + 0.5 * (PI * w).cos() + 0.08 * (2.0 * PI * w).cos();
                (2.0 * cutoff * sinc * window) as f32
            })
            .collect();
        let g = gcd(from, to);
        let (step, unit) = (u64::from(from / g), u64::from(to / g));
        Resampler {
            step,
            unit,
            half: (half * unit as f64) as u64,
            table,
            pending: Vec::new(),
            pos: 0,
            out: Vec::new(),
            passthrough: from == to,
        }
    }

    pub fn push(&mut self, input: &[f32], sink: &mut dyn FnMut(&[f32])) {
        if self.passthrough {
            sink(input);
            return;
        }
        self.pending.extend_from_slice(input);
        self.drain(false, sink);
    }

    pub fn finish(&mut self, sink: &mut dyn FnMut(&[f32])) {
        if self.passthrough {
            return;
        }
        self.drain(true, sink);
    }

    fn drain(&mut self, flush: bool, sink: &mut dyn FnMut(&[f32])) {
        let n = self.pending.len();
        let end = n as u64 * self.unit;
        self.out.clear();
        // Until the input runs out, or (still streaming) until an output's taps would.
        while if flush { self.pos < end } else { self.pos + self.half < end } {
            let lo = self.pos.saturating_sub(self.half).div_ceil(self.unit) as usize;
            let hi = (((self.pos + self.half) / self.unit) as usize).min(n - 1);
            let mut acc = 0.0f32;
            for i in lo..=hi {
                // The nearest of the kernel's phases to the tap's distance, ties away from 0.
                let d = (i as u64 * self.unit).abs_diff(self.pos);
                let k = ((2 * d * PHASES as u64 + self.unit) / (2 * self.unit)) as usize;
                acc += self.pending[i] * self.table[k.min(self.table.len() - 1)];
            }
            self.out.push(acc);
            self.pos += self.step;
        }
        if !self.out.is_empty() {
            sink(&self.out);
        }
        // Keep what later outputs still need.
        let keep_from = ((self.pos.saturating_sub(self.half) / self.unit) as usize).min(n);
        self.pending.drain(..keep_from);
        self.pos -= keep_from as u64 * self.unit;
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Audio formats [`decode`] reads, by extension.
pub fn is_decodable(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "mp3" | "m4a" | "mp4" | "aac" | "flac" | "wav" | "wave" | "aif" | "aiff" | "alac")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f64, rate: u32, secs: f64) -> Vec<f32> {
        (0..(rate as f64 * secs) as usize).map(|i| (2.0 * PI * freq * i as f64 / rate as f64).sin() as f32).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
        let mut r = Resampler::new(from, to);
        let mut out = Vec::new();
        // Odd chunk sizes exercise the streaming bookkeeping.
        for chunk in x.chunks(1013) {
            r.push(chunk, &mut |s| out.extend_from_slice(s));
        }
        r.finish(&mut |s| out.extend_from_slice(s));
        out
    }

    #[test]
    fn keeps_passband_and_length() {
        for from in [44_100, 48_000, 96_000, 22_050] {
            let out = resample(&tone(1000.0, from, 2.0), from, 11_025);
            let expected = 2.0 * 11_025.0;
            assert!((out.len() as f64 - expected).abs() < 3.0, "{from}: {} samples", out.len());
            let mid = &out[2000..out.len() - 2000];
            assert!((rms(mid) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.03, "{from}: rms {}", rms(mid));
        }
    }

    /// A sink that has heard enough stops decoding: the rest of the file isn't read.
    #[test]
    fn decoding_stops_when_the_sink_has_enough() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.wav");
        std::fs::write(&path, crate::test_util::wav(44_100, 16, 30.0)).unwrap();
        let mut heard = 0;
        let start = decode_until(&path, 11_025, &Ctx::silent(), &mut |s| {
            heard += s.len();
            if heard < 11_025 { ControlFlow::Continue(()) } else { ControlFlow::Break(()) }
        })
        .unwrap();
        assert!(start.duration_secs < 1.5 && heard < 2 * 11_025, "{start:?}, {heard} samples");
        let whole = decode(&path, 11_025, &Ctx::silent(), &mut |_| {}).unwrap();
        assert!((whole.duration_secs - 30.0).abs() < 0.01, "{whole:?}");
    }

    /// At the file's own rate, the samples come interleaved as they are, with their rate and
    /// channel count; cancelling stops the decode without reading the rest.
    #[test]
    fn native_decoding_hands_on_the_file_and_stops_when_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.wav");
        let samples: Vec<i32> = (0..8_000 * 60).map(|i| (i % 2000) - 1000).collect();
        std::fs::write(&path, crate::test_util::wav_of(8_000, 16, samples.iter().copied())).unwrap();
        let mut heard = Vec::new();
        let format = decode_native(&path, Damaged::Fail, &Ctx::silent(), |s: &[i32], rate, channels| -> Result<_> {
            assert_eq!((rate, channels), (8_000, 2));
            heard.extend(s.iter().step_by(2).map(|v| v >> 16));
            Ok(ControlFlow::Continue(()))
        })
        .unwrap();
        assert_eq!((format.sample_rate, format.channels), (8_000, 2));
        assert!(heard == samples, "every sample, unchanged");

        let ctx = Ctx::silent();
        ctx.cancel.cancel();
        let mut frames = 0;
        let result = decode_native(&path, Damaged::Skip, &ctx, |s: &[i32], _, channels| -> Result<_> {
            frames += s.len() / channels;
            Ok(ControlFlow::Continue(()))
        });
        assert!(matches!(result, Err(Error::Cancelled(_))), "{result:?}");
        assert!(frames < samples.len(), "{frames}");
    }

    /// Fed a sample at a time, the output is the one fed whole, to the bit: 11025 to 48000 Hz
    /// puts outputs on exact ties between two of the kernel's phases, which rounding a drifting
    /// position used to tip either way.
    #[test]
    fn chunks_dont_change_the_output() {
        let x = tone(50.0, 11_025, 0.25);
        let mut r = Resampler::new(11_025, 48_000);
        let mut one_by_one = Vec::new();
        for s in &x {
            r.push(std::slice::from_ref(s), &mut |s| one_by_one.extend_from_slice(s));
        }
        r.finish(&mut |s| one_by_one.extend_from_slice(s));
        assert!(crate::test_util::resample(&x, 11_025, 48_000) == one_by_one);
    }

    #[test]
    fn removes_what_would_alias() {
        // 9 kHz is above the 5.5 kHz output Nyquist rate: it must not fold back down.
        let out = resample(&tone(9000.0, 44_100, 1.0), 44_100, 11_025);
        assert!(rms(&out[1000..out.len() - 1000]) < 0.01);
    }

    use hegel::generators as gs;
    use hegel::{Generator, TestCase};

    const RATES: [u32; 8] = [8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 96_000];

    #[hegel::test_helper]
    fn rate(tc: &TestCase) -> u32 {
        tc.draw(gs::sampled_from(RATES.to_vec()))
    }

    /// Between any two rates, whatever the chunks the input comes in: the output is the input's
    /// length at the new rate (rounded up: an output for every position inside the input), the
    /// same sample for sample however it was chunked; a tone well inside both rates' bands keeps
    /// its level.
    #[hegel::test(test_cases = 40)]
    fn resampling_keeps_length_and_level(tc: TestCase) {
        let (from, to) = (rate(&tc), rate(&tc));
        let freq = tc.draw(gs::floats::<f64>().min_value(50.0).max_value(0.2 * from.min(to) as f64));
        let x = tone(freq, from, tc.draw(gs::floats::<f64>().min_value(0.0).max_value(0.3)));
        let chunk = tc.draw(gs::integers::<usize>().min_value(1).max_value(5000));
        let mut chunked = Vec::new();
        let mut r = Resampler::new(from, to);
        for c in x.chunks(chunk) {
            r.push(c, &mut |s| chunked.extend_from_slice(s));
        }
        r.finish(&mut |s| chunked.extend_from_slice(s));
        let expected = (x.len() as u64 * u64::from(to)).div_ceil(u64::from(from));
        assert_eq!(chunked.len() as u64, expected);
        assert!(crate::test_util::resample(&x, from, to) == chunked, "chunks of {chunk} change it");
        // Away from the ends, where the kernel runs off the signal.
        let edge = (to as usize / 50).max(64);
        if chunked.len() > 4 * edge {
            let level = rms(&chunked[edge..chunked.len() - edge]);
            assert!((level - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.03, "rms {level} at {freq} Hz");
        }
    }

    /// A WAV of any rate, depth and length decodes to its samples exactly, at its own rate and
    /// to mono at it, and says how long it is; a sink that has heard enough stops the decode a
    /// packet or so later.
    #[hegel::test(test_cases = 40)]
    fn wavs_decode_to_their_samples(tc: TestCase) {
        let rate = rate(&tc);
        let bits = tc.draw(gs::sampled_from(vec![16u16, 24, 32]));
        let top = (1i64 << (bits - 1)) - 1;
        let samples: Vec<i32> = tc
            .draw(gs::vecs(gs::integers::<i64>().min_value(-top - 1).max_value(top)).max_size(3000))
            .into_iter()
            .map(|v| v as i32)
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.wav");
        std::fs::write(&path, crate::test_util::wav_of(rate, bits, samples.iter().copied())).unwrap();

        let mut heard = Vec::new();
        let format = decode_native(&path, Damaged::Fail, &Ctx::silent(), |s: &[i32], r, channels| -> Result<_> {
            assert_eq!((r, channels), (rate, 2));
            heard.extend(s.iter().step_by(2).map(|v| v >> (32 - bits)));
            Ok(ControlFlow::Continue(()))
        })
        .unwrap();
        assert_eq!(heard, samples);
        assert_eq!((format.sample_rate, format.channels), (rate, 2));
        assert_eq!(format.duration_secs, samples.len() as f64 / rate as f64);

        let mut mono = Vec::new();
        decode(&path, rate, &Ctx::silent(), &mut |s| mono.extend_from_slice(s)).unwrap();
        let full = (1u64 << (bits - 1)) as f32;
        assert_eq!(mono.len(), samples.len());
        assert!(mono.iter().zip(&samples).all(|(m, s)| (m - *s as f32 / full).abs() < 1e-6));

        let enough = tc.draw(gs::integers::<usize>().min_value(1).max_value(samples.len().max(1)));
        let mut got = 0;
        let start = decode_until(&path, rate, &Ctx::silent(), &mut |s| {
            got += s.len();
            if got < enough { ControlFlow::Continue(()) } else { ControlFlow::Break(()) }
        })
        .unwrap();
        assert!(got >= enough.min(samples.len()) && got < enough + 8192, "{got} for {enough}");
        assert!(start.duration_secs * rate as f64 <= got as f64 + 0.5);
    }

    /// A format is told by the file's last extension alone, in any case: not by the rest of its
    /// name or its folder. rekordbox's formats are decodable; other formats and names without an
    /// extension aren't.
    #[hegel::test(test_cases = 200)]
    fn formats_are_told_by_extension_in_any_case(tc: TestCase) {
        let named = [
            ("a.mp3", true),
            ("B.FLAC", true),
            ("c.Wave", true),
            ("d.aif", true),
            ("e.M4A", true),
            ("f.ogg", false),
            ("g.opus", false),
            ("h.mp3.txt", false),
            ("i.txt.wav", true),
            ("mp3", false),
            (".mp3", false),
            ("j.mp3 ", false),
        ];
        for (name, decodable) in named {
            assert_eq!(is_decodable(Path::new(name)), decodable, "{name}");
        }
        let ext: String = tc.draw(hegel::one_of!(
            gs::sampled_from(vec!["mp3", "flac", "wave", "aiff", "m4a", "ogg"]).map(str::to_string),
            gs::text().alphabet("acdefilmopvwx34. ").max_size(6)
        ));
        let cased: String =
            ext.chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c }).collect();
        let dir = tc.draw(gs::sampled_from(vec!["/music", "/a.flac", "set.mp3"]));
        let stem = tc.draw(gs::sampled_from(vec!["set", "a.b", "mp3", "x.flac"]));
        let plain = is_decodable(&Path::new("/music").join(format!("set.{ext}")));
        let path = Path::new(dir).join(format!("{stem}.{cased}"));
        assert_eq!(is_decodable(&path), plain, "{} and set.{ext}", path.display());
    }

    /// A WAV damaged anywhere (bytes changed, cut short) never panics the decoder: it decodes,
    /// saying how much it decoded, or is that file's error, in either mode.
    #[hegel::test(test_cases = 60)]
    fn damaged_files_are_that_files_error(tc: TestCase) {
        let frames = tc.draw(gs::integers::<usize>().min_value(1).max_value(2000));
        let mut bytes = crate::test_util::wav_of(rate(&tc), 16, (0..frames as i32).map(|i| i * 7 - 3000));
        let flips = tc.draw(gs::vecs(gs::tuples!(gs::integers::<usize>(), gs::integers::<u8>())).max_size(4));
        for (at, v) in flips {
            let n = bytes.len();
            bytes[at % n] = v;
        }
        let keep = tc.draw(gs::integers::<usize>().max_value(bytes.len()));
        bytes.truncate(keep);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.wav");
        std::fs::write(&path, &bytes).unwrap();
        for damaged in [Damaged::Skip, Damaged::Fail] {
            let mut heard = 0;
            let result = decode_native(&path, damaged, &Ctx::silent(), |s: &[i16], _, channels| -> Result<_> {
                heard += s.len() / channels;
                Ok(ControlFlow::Continue(()))
            });
            match result {
                Ok(format) => assert!(format.duration_secs * format.sample_rate as f64 <= heard as f64 + 0.5),
                Err(Error::Parse { path: at, .. }) => assert_eq!(at, path),
                Err(e) => panic!("{e}"),
            }
        }
    }
}
