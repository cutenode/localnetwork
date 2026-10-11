//! Decoding audio: [`decode_native`] at a file's own rate and channel count (the one loop over
//! symphonia, which loclib-analysis uses too), and [`decode`] to mono at a
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

/// Streaming windowed-sinc resampler (mono).
pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Kernel half-width in input samples.
    half: f64,
    table: Vec<f32>,
    /// Input not yet fully consumed; `pos` indexes into it.
    pending: Vec<f32>,
    pos: f64,
    out: Vec<f32>,
    passthrough: bool,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Resampler {
        assert!(from > 0 && to > 0, "resampling from {from} Hz to {to} Hz");
        let step = from as f64 / to as f64;
        // Low-pass just under the lower of the two Nyquist rates, in cycles per input sample.
        let cutoff = 0.5 / step.max(1.0) * 0.94;
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
        Resampler { step, half, table, pending: Vec::new(), pos: 0.0, out: Vec::new(), passthrough: from == to }
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
        let limit = if flush { n as f64 } else { n as f64 - self.half - 1.0 };
        self.out.clear();
        while self.pos < limit {
            let lo = (self.pos - self.half).ceil().max(0.0) as usize;
            let hi = ((self.pos + self.half).floor() as usize).min(n.saturating_sub(1));
            let mut acc = 0.0f32;
            for i in lo..=hi {
                let k = ((i as f64 - self.pos).abs() * PHASES + 0.5) as usize;
                acc += self.pending[i] * self.table[k.min(self.table.len() - 1)];
            }
            self.out.push(acc);
            self.pos += self.step;
        }
        if !self.out.is_empty() {
            sink(&self.out);
        }
        // Keep what later outputs still need.
        let keep_from = ((self.pos - self.half).floor().max(0.0) as usize).min(n);
        self.pending.drain(..keep_from);
        self.pos -= keep_from as f64;
    }
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

    #[test]
    fn removes_what_would_alias() {
        // 9 kHz is above the 5.5 kHz output Nyquist rate: it must not fold back down.
        let out = resample(&tone(9000.0, 44_100, 1.0), 44_100, 11_025);
        assert!(rms(&out[1000..out.len() - 1000]) < 0.01);
    }
}
