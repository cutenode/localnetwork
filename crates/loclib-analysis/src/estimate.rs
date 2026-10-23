//! Tempo, key and beat grid from [stratum-dsp](https://github.com/HLLMR/stratum-dsp): a second
//! opinion to compare a stored value with (see the README for how accurate each is).

use loclib_model::analysis::Beat;
use loclib_model::harmony::Camelot;
use serde::{Deserialize, Serialize};
use stratum_dsp::preprocessing::normalization::{NormalizationConfig, normalize};
use stratum_dsp::preprocessing::silence::{SilenceDetector, detect_and_trim};
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
    /// `None` when the best keys tie (a confidence of 0): none is heard over the others.
    pub key: Option<Camelot>,
    /// 0 to 1: how far the best key's template match beats the next best.
    pub key_confidence: f32,
    /// The beats it tracked, in order, each at the tempo above and numbered in fours from the
    /// first: empty when it found no tempo.
    pub beats: Vec<Beat>,
    /// 0 to 1: how evenly the beats fall, `1 / (1 + cv)` where `cv` is the coefficient of variation
    /// of the time between them (stratum-dsp warns of tempo changes under 0.5). 0 without beats.
    pub grid_stability: f32,
}

/// Estimate `mono` (at [`RATE`]); `None` when the analyzer finds nothing to estimate: no audio, or
/// silence. (It refuses nothing else: too little sound comes back with no tempo.)
pub(crate) fn estimate(mut mono: Vec<f32>) -> Option<Estimate> {
    let result = analyze_audio(&mono, RATE, AnalysisConfig::default()).ok()?;
    let grid = &result.beat_grid.beats;
    // Its beat times count from where its leading silence ends.
    let start = if grid.is_empty() { 0.0 } else { trimmed_secs(&mut mono) };
    let mut beats: Vec<Beat> = Vec::with_capacity(grid.len());
    for &secs in grid {
        let time_ms = ((f64::from(secs) + start) * 1000.0).round() as u32;
        // Tracking tempo changes, it can give a beat twice where the stretches it tracks apart
        // meet: grids hold each beat once, in order.
        if beats.last().is_some_and(|b| b.time_ms >= time_ms) {
            continue;
        }
        // Its first beat is always its first downbeat. The later ones follow its guess at the
        // time signature, which puts bars of six on steady 4/4 kicks (at 126 and 140 BPM), so
        // bars are counted in fours from the first, as rekordbox's grids are.
        let beat_number = (beats.len() % 4 + 1) as u16;
        beats.push(Beat { beat_number, bpm: result.bpm.into(), time_ms });
    }
    Some(Estimate {
        bpm: result.bpm,
        bpm_confidence: result.bpm_confidence,
        // stratum-dsp picks one of tied keys in the order of a `HashMap` (seeded afresh by every
        // process), so a tie has no key here: the same audio always gives the same estimate.
        key: (result.key_confidence > 0.0).then(|| camelot(result.key)),
        key_confidence: result.key_confidence,
        beats,
        grid_stability: result.grid_stability,
    })
}

/// How much leading silence [`analyze_audio`] trims from `mono` before it looks for beats, in
/// seconds: worked out as it does, with the default settings (peak normalisation to 1 dB below
/// full scale, then silence under -40 dB in frames of 2048 samples), which it doesn't report.
/// Normalises `mono` in place.
fn trimmed_secs(mono: &mut [f32]) -> f64 {
    let config = AnalysisConfig::default();
    let peak = NormalizationConfig { target_loudness_lufs: -14.0, max_headroom_db: 1.0, method: config.normalization };
    let silence =
        SilenceDetector { threshold_db: config.min_amplitude_db, min_duration_ms: 500, frame_size: config.frame_size };
    let trimmed = normalize(mono, peak, RATE as f32).and_then(|_| detect_and_trim(mono, RATE, silence));
    // It refused neither when it ran them itself.
    let (_, silences) = trimmed.unwrap_or_default();
    match silences.first() {
        Some(&(0, end)) => end as f64 / f64::from(RATE),
        _ => 0.0,
    }
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
