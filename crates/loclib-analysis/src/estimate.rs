//! Tempo and key from [stratum-dsp](https://github.com/HLLMR/stratum-dsp): a second opinion to
//! compare a stored value with (see the README for how accurate each is).

use loclib_model::harmony::Camelot;
use serde::{Deserialize, Serialize};
use stratum_dsp::{AnalysisConfig, Key, analyze_audio};

/// The rate tracks are resampled to for the analyzer: what it's tuned at (its frame and hop
/// sizes are in samples), and one rate whatever the file's, so a result doesn't depend on it.
pub(crate) const RATE: u32 = 44_100;

/// What stratum-dsp makes of a track, at its precision (`f32`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    /// Usually a whole number (its tempogram tries tempos 1 BPM apart; the estimator it falls
    /// back on can give fractions). 0 when it found no tempo.
    pub bpm: f32,
    /// 0 to 1.
    pub bpm_confidence: f32,
    pub key: Camelot,
    /// 0 to 1: how far the best key's template match beats the next best.
    pub key_confidence: f32,
}

/// Estimate `mono` (at [`RATE`]); `None` when the analyzer finds nothing to estimate: no audio, or
/// silence. (It refuses nothing else: too little sound comes back with no tempo.)
pub(crate) fn estimate(mono: &[f32]) -> Option<Estimate> {
    let result = analyze_audio(mono, RATE, AnalysisConfig::default()).ok()?;
    Some(Estimate {
        bpm: result.bpm,
        bpm_confidence: result.bpm_confidence,
        key: camelot(result.key),
        key_confidence: result.key_confidence,
    })
}

/// stratum-dsp's keys count semitones above C. (Its own `numerical()` names majors `A` and minors
/// `B`, the other way round from the Camelot wheel, so it isn't used.)
fn camelot(key: Key) -> Camelot {
    match key {
        Key::Major(pitch) => Camelot::from_pitch(pitch, true),
        Key::Minor(pitch) => Camelot::from_pitch(pitch, false),
    }
}

#[cfg(test)]
mod tests;
