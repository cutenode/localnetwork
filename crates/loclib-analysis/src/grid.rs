//! A fixed-tempo beat grid worked out from a track's onsets, placed the way rekordbox places
//! its own: the tempo to 0.01 BPM, the beats where rekordbox's would be, bars counted from the
//! first beat.
//!
//! The onsets are the spectral flux of the mono mix's log magnitude (a 2048-sample window every
//! 128 samples, 2.9 ms), with a half-second moving mean taken away. The tempo is the one the
//! whole band's flux folds sharpest at: every 0.25 BPM from 90 to 180, then every 0.01 within 1%
//! of the best. The beats are where the flux under 2 kHz peaks at that tempo: the whole band's
//! sharpest is often the hats' off-beat.
//!
//! Scored against the grids rekordbox made for 588 tracks of electronic music (the
//! `grid_vs_rekordbox` example), its tempo was in rekordbox's octave for 87% of them and within
//! 0.01 BPM of rekordbox's for 80% of those; 77% had their beats within 10 ms of rekordbox's, and
//! 12% sat half a beat off them. rekordbox started bars on the first beat for 78%: none of the
//! guesses tried at where a bar starts did better.

use loclib_model::analysis::Beat;
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use serde::{Deserialize, Serialize};

use crate::RATE;

const WINDOW: usize = 2048;
const HOP: usize = 128;
const FRAMES_PER_SEC: f64 = RATE as f64 / HOP as f64;

/// The slowest tempo a grid gets; the fastest is just under twice it.
const SLOWEST: f64 = 90.0;

/// How much later rekordbox puts a beat than where the flux here peaks: the median over the
/// tracks it was scored on.
const LATER_SECS: f64 = 0.0103;

/// The shortest audio a grid is worked out for.
const SHORTEST_SECS: f64 = 5.0;

/// A beat grid of one tempo.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Grid {
    /// To 0.01, as rekordbox stores them.
    pub bpm: f64,
    /// The first beat at or after the start, which starts a bar.
    pub first_beat_secs: f64,
}

impl Grid {
    /// Seconds from one beat to the next.
    pub fn period_secs(&self) -> f64 {
        60.0 / self.bpm
    }

    /// Its beats to `duration_secs`, numbered 1 to 4 from the first.
    pub fn beats(&self, duration_secs: f64) -> Vec<Beat> {
        let period = self.period_secs();
        (0..)
            .map(|i| (i, self.first_beat_secs + i as f64 * period))
            .take_while(|(_, t)| *t < duration_secs)
            .map(|(i, t)| Beat { beat_number: (i % 4 + 1) as u16, bpm: self.bpm, time_ms: (t * 1000.0).round() as u32 })
            .collect()
    }
}

/// The grid of `mono` (at [`RATE`]), whose first sample is `lead_in_secs` into rekordbox's
/// timeline (see loclib-audio's `lame_frame_secs`). `None` for audio too short or too quiet to
/// have one.
pub(crate) fn grid(mono: &[f32], lead_in_secs: f64) -> Option<Grid> {
    if (mono.len() as f64) < SHORTEST_SECS * RATE as f64 {
        return None;
    }
    let Flux { full, under_2k } = flux(mono);
    if !full.iter().any(|v| *v > 0.0) || !under_2k.iter().any(|v| *v > 0.0) {
        return None;
    }
    let coarse = (SLOWEST * 4.0) as u32..(SLOWEST * 8.0) as u32;
    let rough = coarse.map(|q| f64::from(q) / 4.0).max_by(|a, b| fold(&full, *a).total_cmp(&fold(&full, *b)))?;
    let (lo, hi) = ((rough * 99.0).round() as u32, (rough * 101.0).round() as u32);
    let bpm = (lo..=hi).map(|c| f64::from(c) / 100.0).max_by(|a, b| fold(&full, *a).total_cmp(&fold(&full, *b)))?;
    let period = FRAMES_PER_SEC * 60.0 / bpm;
    // The phase, to a tenth of a frame.
    let phase = (0..(period * 10.0) as u32)
        .map(|s| f64::from(s) / 10.0)
        .max_by(|a, b| comb(&under_2k, *a, period).total_cmp(&comb(&under_2k, *b, period)))?;
    let at = frame_secs(phase) + LATER_SECS + lead_in_secs;
    let period_secs = 60.0 / bpm;
    Some(Grid { bpm, first_beat_secs: at.rem_euclid(period_secs) })
}

/// Onset envelopes: the whole band's and under 2 kHz.
struct Flux {
    full: Vec<f32>,
    under_2k: Vec<f32>,
}

fn flux(mono: &[f32]) -> Flux {
    let fft = FftPlanner::<f32>::new().plan_fft_forward(WINDOW);
    let hann: Vec<f32> =
        (0..WINDOW).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / WINDOW as f32).cos()).collect();
    let frames = (mono.len() - WINDOW) / HOP + 1;
    let bins = WINDOW / 2;
    let below_2k = (2000.0 * WINDOW as f64 / f64::from(RATE)) as usize;
    let mut last = vec![0f32; bins];
    let mut buf = vec![Complex::new(0f32, 0f32); WINDOW];
    let (mut full, mut under_2k) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
    for i in 0..frames {
        for (b, (x, w)) in buf.iter_mut().zip(mono[i * HOP..i * HOP + WINDOW].iter().zip(&hann)) {
            *b = Complex::new(x * w, 0.0);
        }
        fft.process(&mut buf);
        let (mut all, mut low) = (0f32, 0f32);
        for (k, last) in last.iter_mut().enumerate().skip(1) {
            let m = (1.0 + 100.0 * buf[k].norm()).ln();
            let rise = (m - *last).max(0.0);
            all += rise;
            if k < below_2k {
                low += rise;
            }
            *last = m;
        }
        full.push(all);
        under_2k.push(low);
    }
    Flux { full: detrend(&full), under_2k: detrend(&under_2k) }
}

/// What's above a half-second moving mean.
fn detrend(x: &[f32]) -> Vec<f32> {
    let w = (FRAMES_PER_SEC / 4.0) as usize;
    let mut sums = Vec::with_capacity(x.len() + 1);
    sums.push(0f64);
    for v in x {
        sums.push(sums[sums.len() - 1] + f64::from(*v));
    }
    (0..x.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(w), (i + w + 1).min(x.len()));
            (x[i] - ((sums[b] - sums[a]) / (b - a) as f64) as f32).max(0.0)
        })
        .collect()
}

/// When frame `i` (fractional) is centred.
fn frame_secs(i: f64) -> f64 {
    (i * HOP as f64 + WINDOW as f64 / 2.0) / f64::from(RATE)
}

/// How sharply `env` folds at `bpm`: the peak of its phase histogram against the mean.
fn fold(env: &[f32], bpm: f64) -> f64 {
    const BINS: usize = 128;
    let period = FRAMES_PER_SEC * 60.0 / bpm;
    let mut hist = [0f64; BINS];
    for (i, v) in env.iter().enumerate() {
        hist[((i as f64 / period).fract() * BINS as f64) as usize % BINS] += f64::from(*v);
    }
    let mean = hist.iter().sum::<f64>() / BINS as f64;
    let peak =
        (0..BINS).map(|b| hist[(b + BINS - 1) % BINS] + 2.0 * hist[b] + hist[(b + 1) % BINS]).fold(0.0, f64::max);
    peak / (4.0 * mean.max(1e-9))
}

/// The envelope's peaks near every beat of a grid starting `phase` frames in, `period` apart.
fn comb(env: &[f32], phase: f64, period: f64) -> f64 {
    let mut sum = 0.0;
    let mut x = phase;
    while x < env.len() as f64 {
        sum += f64::from((-3..=3).map(|d| at(env, x + f64::from(d))).fold(0f32, f32::max));
        x += period;
    }
    sum
}

/// `env` at fractional frame `x`, linearly; 0 outside it.
fn at(env: &[f32], x: f64) -> f32 {
    if x < 0.0 {
        return 0.0;
    }
    let i = x as usize;
    match (env.get(i), env.get(i + 1)) {
        (Some(a), Some(b)) => {
            let t = (x - i as f64) as f32;
            a * (1.0 - t) + b * t
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests;
