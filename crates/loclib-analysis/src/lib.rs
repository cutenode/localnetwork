#![doc = include_str!("../README.md")]

pub mod benchmark;
mod error;
mod estimate;
mod loudness;
pub mod tempo;
mod waveform;

use std::path::Path;

use std::ops::ControlFlow;

use loclib_mixid::audio::{self, Damaged, Resampler};
use loclib_progress::Ctx;
use serde::{Deserialize, Serialize};

pub use error::{Error, Result};
pub use estimate::Estimate;
use estimate::RATE;
pub use loudness::Loudness;
pub use waveform::{COLUMNS_PER_SEC, Waveform};

/// Changes whenever [`analyze`] would give different results for the same audio (a new
/// stratum-dsp or ebur128, different settings, a fix here), so cached analyses made with another
/// version can be thrown away.
pub const VERSION: u32 = 5;

/// What [`analyze`] measures. One decode feeds them all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Want {
    /// Tempo and key.
    pub estimate: bool,
    pub loudness: bool,
    pub waveform: bool,
}

impl Want {
    pub const ALL: Want = Want { estimate: true, loudness: true, waveform: true };
}

/// One track's analysis, for caching by the audio's content hash and [`VERSION`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    /// The [`VERSION`] it was made with.
    pub version: u32,
    /// Seconds of audio decoded.
    pub duration_secs: f64,
    /// When asked for and the analyzer found something to estimate (not silence). Its loudness is
    /// measured either way.
    pub estimate: Option<Estimate>,
    /// When asked for.
    pub loudness: Option<Loudness>,
    /// When asked for.
    pub waveform: Option<Waveform>,
}

/// Decode the track at `path` once and measure what `want` asks for.
///
/// Decoding checks `ctx` for cancellation as it goes; the tempo and key analysis that follows
/// runs to the end once started (a few seconds a track). Reporting progress is left to the
/// caller, which knows how many tracks it has, as with loclib-mixid's `fingerprint_file`.
pub fn analyze(path: &Path, want: Want, ctx: &Ctx<'_>) -> Result<Analysis> {
    let mut meter = want.loudness.then(loudness::Meter::default);
    let mut levels = want.waveform.then(waveform::Levels::new);
    let mut resampler: Option<Resampler> = None;
    let (mut frame_means, mut mono) = (Vec::new(), Vec::new());
    let format =
        audio::decode_native(path, Damaged::Skip, ctx, |samples: &[f32], rate, channels| -> Result<ControlFlow<()>> {
            if let Some(meter) = &mut meter {
                meter.push(samples, rate, channels).map_err(|e| Error::analyzer(path, e))?;
            }
            if want.estimate || want.waveform {
                frame_means.clear();
                frame_means.extend(samples.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
            }
            if want.estimate {
                let resampler = resampler.get_or_insert_with(|| Resampler::new(rate, RATE));
                resampler.push(&frame_means, &mut |s| mono.extend_from_slice(s));
            }
            if let Some(levels) = &mut levels {
                levels.push(&frame_means, rate);
            }
            Ok(ControlFlow::Continue(()))
        })?;
    if let Some(r) = &mut resampler {
        r.finish(&mut |s| mono.extend_from_slice(s));
    }
    ctx.check()?;
    Ok(Analysis {
        version: VERSION,
        duration_secs: format.duration_secs,
        estimate: want.estimate.then(|| estimate::estimate(&mono)).flatten(),
        loudness: meter.map(loudness::Meter::finish).transpose().map_err(|e| Error::analyzer(path, e))?,
        waveform: levels.map(waveform::Levels::finish),
    })
}

#[cfg(test)]
mod tests;
