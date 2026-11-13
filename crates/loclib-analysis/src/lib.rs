#![doc = include_str!("../README.md")]

pub mod benchmark;
mod error;
#[cfg(feature = "dsp")]
mod estimate;
#[cfg(feature = "dsp")]
mod loudness;
pub mod tempo;
#[cfg(feature = "dsp")]
mod waveform;

// What the `dsp` feature adds: `analyze` and what it measures.
#[cfg(feature = "dsp")]
pub use {
    estimate::Estimate,
    loudness::Loudness,
    waveform::{COLUMNS_PER_SEC, Waveform},
};
#[cfg(feature = "dsp")]
use {
    estimate::RATE,
    loclib_audio::{self as audio, Damaged, Resampler},
    loclib_progress::Ctx,
    serde::{Deserialize, Serialize},
    std::ops::ControlFlow,
    std::path::Path,
};

pub use error::{Error, Result};

/// Changes whenever [`analyze`] would give different results for the same audio (a new
/// stratum-dsp or ebur128, different settings, a fix here), so cached analyses made with another
/// version can be thrown away.
#[cfg(feature = "dsp")]
pub const VERSION: u32 = 5;

/// What [`analyze`] measures. One decode feeds them all.
#[cfg(feature = "dsp")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Want {
    /// Tempo and key.
    pub estimate: bool,
    pub loudness: bool,
    pub waveform: bool,
}

#[cfg(feature = "dsp")]
impl Want {
    pub const ALL: Want = Want { estimate: true, loudness: true, waveform: true };
}

/// One track's analysis, for caching by the audio's content hash and [`VERSION`].
#[cfg(feature = "dsp")]
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
#[cfg(feature = "dsp")]
pub fn analyze(path: &Path, want: Want, ctx: &Ctx<'_>) -> Result<Analysis> {
    let mut meter = want.loudness.then(loudness::Meter::default);
    let mut bands = want.waveform.then(waveform::Bands::new);
    let mut resampler: Option<Resampler> = None;
    let (mut frame_means, mut mono) = (Vec::new(), Vec::new());
    // The mono mix at RATE: kept whole for the estimate, and measured as it comes for the waveform.
    let mut take = |s: &[f32]| {
        if want.estimate {
            mono.extend_from_slice(s);
        }
        if let Some(bands) = &mut bands {
            bands.push(s);
        }
    };
    let format =
        audio::decode_native(path, Damaged::Skip, ctx, |samples: &[f32], rate, channels| -> Result<ControlFlow<()>> {
            if let Some(meter) = &mut meter {
                meter.push(samples, rate, channels).map_err(|e| Error::analyzer(path, e))?;
            }
            if want.estimate || want.waveform {
                frame_means.clear();
                frame_means.extend(samples.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
                resampler.get_or_insert_with(|| Resampler::new(rate, RATE)).push(&frame_means, &mut take);
            }
            Ok(ControlFlow::Continue(()))
        })?;
    if let Some(r) = &mut resampler {
        r.finish(&mut take);
    }
    ctx.check()?;
    Ok(Analysis {
        version: VERSION,
        duration_secs: format.duration_secs,
        estimate: want.estimate.then(|| estimate::estimate(&mono)).flatten(),
        loudness: meter.map(loudness::Meter::finish).transpose().map_err(|e| Error::analyzer(path, e))?,
        waveform: bands.map(waveform::Bands::finish),
    })
}

#[cfg(all(test, feature = "dsp"))]
mod tests;
