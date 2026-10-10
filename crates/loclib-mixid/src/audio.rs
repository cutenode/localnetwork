//! Decoding audio to the mono, low-rate signal fingerprints are computed from.
//!
//! Everything a drive or a recorder produces (MP3, AAC/ALAC in `.m4a`, FLAC, WAV, AIFF) decodes
//! through symphonia. Channels are averaged and the result resampled to
//! [`crate::fingerprint::RATE`] as it streams, so an hour-long recording never sits in memory at
//! full rate. Other sources (an editor's project, a live input) can feed [`Resampler`] themselves.
//!
//! symphonia trusts what a file's headers say: a WAV saying 0 Hz panics it. [`decode`] catches
//! that, so one bad file is that file's error rather than a crash; a program that wants the
//! panic's message kept off stderr installs its own panic hook around the call.

use std::f64::consts::PI;
use std::fs::File;
use std::panic::AssertUnwindSafe;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, Track};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use loclib_progress::Ctx;

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

/// Decode `path` to mono at `rate`, handing the signal to `sink` in chunks as it streams.
/// Undecodable packets are skipped (a damaged frame in a long recording shouldn't sink it), and a
/// file the decoder can't make sense of is an error, [`CRASHED`] when it panicked on it.
pub fn decode(path: &Path, rate: u32, ctx: &Ctx<'_>, sink: &mut dyn FnMut(&[f32])) -> Result<SourceFormat> {
    std::panic::catch_unwind(AssertUnwindSafe(|| decode_unguarded(path, rate, ctx, sink)))
        .unwrap_or_else(|_| Err(Error::parse(path, CRASHED)))
}

fn decode_unguarded(path: &Path, rate: u32, ctx: &Ctx<'_>, sink: &mut dyn FnMut(&[f32])) -> Result<SourceFormat> {
    let (mut format, track) = open(path)?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| Error::parse(path, e))?;
    let mut fmt = SourceFormat {
        sample_rate: track.codec_params.sample_rate.unwrap_or(0),
        channels: track.codec_params.channels.map_or(0, |c| c.count() as u16),
        duration_secs: 0.0,
    };

    let mut resampler: Option<Resampler> = None;
    let mut buf: Option<SampleBuffer<f32>> = None;
    let mut mono = Vec::new();
    let mut frames = 0u64;
    let mut packets = 0u32;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(Error::parse(path, e)),
        };
        packets += 1;
        if packets.is_multiple_of(256) {
            ctx.check()?;
        }
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) => continue,
            Err(e) => return Err(Error::parse(path, e)),
        };
        let spec = *decoded.spec();
        // A decoder can say 0 Hz where the container didn't (ALAC reads its rate from its own
        // cookie): there's nothing to resample from.
        if spec.rate == 0 {
            return Err(Error::parse(path, "it decodes to 0 Hz audio"));
        }
        let channels = spec.channels.count().max(1);
        if buf.as_ref().is_none_or(|b| b.capacity() < decoded.capacity() * channels) {
            buf = Some(SampleBuffer::new(decoded.capacity() as u64, spec));
        }
        let b = buf.as_mut().expect("allocated above");
        b.copy_interleaved_ref(decoded);
        mono.clear();
        mono.extend(b.samples().chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
        frames += mono.len() as u64;
        fmt.sample_rate = spec.rate;
        fmt.channels = channels as u16;
        let r = resampler.get_or_insert_with(|| Resampler::new(spec.rate, rate));
        r.push(&mono, sink);
    }
    if let Some(mut r) = resampler {
        r.finish(sink);
    }
    if fmt.sample_rate > 0 {
        fmt.duration_secs = frames as f64 / fmt.sample_rate as f64;
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

    #[test]
    fn removes_what_would_alias() {
        // 9 kHz is above the 5.5 kHz output Nyquist rate: it must not fold back down.
        let out = resample(&tone(9000.0, 44_100, 1.0), 44_100, 11_025);
        assert!(rms(&out[1000..out.len() - 1000]) < 0.01);
    }
}
