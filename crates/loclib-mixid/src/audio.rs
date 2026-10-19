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

use loclib_model::audio::Format;
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
    // Where the next packet should start, in the track's time base: the track's start for the
    // first, so a first frame the reader dropped is missed like any other.
    let start = track.codec_params.start_ts;
    let mut next_ts = start;
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
        if damaged == Damaged::Fail && packet.ts() > next_ts {
            return Err(missing().into());
        }
        next_ts = packet.ts() + packet.dur();
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
    if damaged == Damaged::Fail && track.codec_params.n_frames.is_some_and(|n| next_ts < start + n) {
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
    /// The kernel's weights for an output `r` units past an input sample (`phases[r]`), one a
    /// tap from the first input it reaches: outputs `unit` apart share their taps' distances,
    /// so each phase's weights are worked out once, the first time an output needs them.
    phases: Vec<Vec<f32>>,
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
            phases: vec![Vec::new(); unit as usize],
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
            let (at, r) = (self.pos / self.unit, self.pos % self.unit);
            // The kernel reaches `before` inputs back from input `at` (the half-width is
            // several inputs, so wider than `r`), and as far ahead as the input goes.
            let before = (self.half - r) / self.unit;
            let lo = at.saturating_sub(before);
            let hi = (at + (r + self.half) / self.unit).min(n as u64 - 1);
            if self.phases[r as usize].is_empty() {
                self.phases[r as usize] = self.weights(r);
            }
            let weights = &self.phases[r as usize][(lo + before - at) as usize..];
            let mut acc = 0.0f32;
            for (x, w) in self.pending[lo as usize..=hi as usize].iter().zip(weights) {
                acc += x * w;
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

    /// The weights of [`Resampler::phases`]`[r]`.
    fn weights(&self, r: u64) -> Vec<f32> {
        let before = (self.half - r) / self.unit;
        let taps = before + (r + self.half) / self.unit + 1;
        (0..taps)
            .map(|j| {
                // The nearest of the kernel's phases to the tap's distance, ties away from 0.
                let d = (j * self.unit).abs_diff(before * self.unit + r);
                let k = ((2 * d * PHASES as u64 + self.unit) / (2 * self.unit)) as usize;
                self.table[k.min(self.table.len() - 1)]
            })
            .collect()
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Audio formats [`decode`] reads (symphonia's, as this crate builds it), by extension.
pub fn is_decodable(path: &Path) -> bool {
    let format = path.extension().and_then(|e| e.to_str()).and_then(Format::from_extension);
    matches!(
        format,
        Some(Format::Mp3 | Format::Aac | Format::Mpeg4 | Format::Alac | Format::Wav | Format::Aiff | Format::Flac)
    )
}

#[cfg(test)]
mod tests;
