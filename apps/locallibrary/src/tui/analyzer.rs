//! Turns the playback tap into what the visualisers draw: log-spaced spectrum bands with
//! falling peak caps, and a trigger-aligned oscilloscope trace. One FFT per frame feeds every
//! spectrum on screen, each with its own number of bands.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use super::audio::Tap;

const FFT_SIZE: usize = 4096;
const SCOPE_LEN: usize = 2048;
const MIN_HZ: f32 = 30.0;
const MAX_HZ: f32 = 16_000.0;
/// Displayed range, after the slope below: quieter than FLOOR_DB is empty, CEIL_DB is full.
const FLOOR_DB: f32 = -66.0;
const CEIL_DB: f32 = -6.0;
/// Music falls off with frequency; tilt the display so the highs stay visible.
const SLOPE_DB_PER_OCTAVE: f32 = 3.0;

/// One spectrum layout's smoothed levels.
#[derive(Default)]
pub struct Bands {
    /// 0-1 per band, bass first.
    pub levels: Vec<f32>,
    /// Peak caps: level and how long they've been held (s).
    pub peaks: Vec<(f32, f32)>,
}

impl Bands {
    fn new(n: usize) -> Bands {
        Bands { levels: vec![0.0; n], peaks: vec![(0.0, 0.0); n] }
    }

    /// Fast attack, gravity-like release; peaks hold, then fall.
    fn approach(&mut self, target: &[f32], dt: f32) {
        for ((b, (peak, held)), &t) in self.levels.iter_mut().zip(self.peaks.iter_mut()).zip(target) {
            if t > *b {
                *b += (t - *b) * 0.65;
            } else {
                *b = (*b - dt * 1.4).max(t);
            }
            if *b >= *peak {
                *peak = *b;
                *held = 0.0;
            } else {
                *held += dt;
                if *held > 0.35 {
                    *peak = (*peak - dt * 0.8).max(*b);
                }
            }
        }
    }
}

pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    window_sum: f32,
    samples: Vec<f32>,
    buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    magnitudes: Vec<f32>,
    sample_rate: f32,
    /// One set per band count in use.
    sets: Vec<Bands>,
    /// Oscilloscope trace, -1..1, starting at a rising zero crossing so it stands still.
    pub scope: Vec<f32>,
    /// Overall loudness, 0-1, smoothed.
    pub level: f32,
    last_written: usize,
}

impl Default for Analyzer {
    fn default() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos())
            .collect();
        let window_sum = window.iter().sum();
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        Analyzer {
            fft,
            window,
            window_sum,
            samples: vec![0.0; FFT_SIZE],
            buf: vec![Complex::default(); FFT_SIZE],
            scratch,
            magnitudes: vec![0.0; FFT_SIZE / 2],
            sample_rate: 44_100.0,
            sets: Vec::new(),
            scope: vec![0.0; SCOPE_LEN / 2],
            level: 0.0,
            last_written: 0,
        }
    }
}

impl Analyzer {
    /// Advance by `dt` seconds, keeping a set of bands for each count in `band_counts` (one per
    /// spectrum on screen). When nothing new was played (paused, stopped) the displays fall back to
    /// silence.
    pub fn update(&mut self, tap: &Tap, playing: bool, band_counts: &[usize], dt: f32) {
        let written = tap.written();
        let fresh = playing && written != self.last_written;
        self.last_written = written;
        if fresh {
            tap.latest(&mut self.samples);
            self.sample_rate = tap.sample_rate() as f32;
            self.update_scope();
            self.transform();
        } else {
            self.scope.iter_mut().for_each(|s| *s *= 0.8);
        }

        let rms = if fresh {
            (self.samples[FFT_SIZE - 1024..].iter().map(|s| s * s).sum::<f32>() / 1024.0).sqrt()
        } else {
            0.0
        };
        let loud = (rms * 2.5).min(1.0);
        self.level += (loud - self.level) * if loud > self.level { 0.5 } else { (dt * 4.0).min(1.0) };

        self.sets.retain(|b| band_counts.contains(&b.levels.len()));
        for &n in band_counts {
            if n > 0 && self.bands(n).is_none() {
                self.sets.push(Bands::new(n));
            }
        }
        for i in 0..self.sets.len() {
            let n = self.sets[i].levels.len();
            let target = if fresh { self.band_levels(n) } else { vec![0.0; n] };
            self.sets[i].approach(&target, dt);
        }
    }

    /// The smoothed levels for a spectrum of `n` bands, once `update` has been asked for them.
    pub fn bands(&self, n: usize) -> Option<&Bands> {
        self.sets.iter().find(|b| b.levels.len() == n)
    }

    /// Whether there is anything left to animate.
    pub fn is_idle(&self) -> bool {
        self.sets.iter().all(|b| b.peaks.iter().all(|(p, _)| *p < 0.001)) && self.level < 0.001
    }

    fn update_scope(&mut self) {
        // Look for a rising zero crossing in the first half so the trace starts in phase.
        let n = self.scope.len();
        let search = &self.samples[FFT_SIZE - SCOPE_LEN..FFT_SIZE - n];
        let start = search.windows(2).position(|w| w[0] <= 0.0 && w[1] > 0.0).unwrap_or(0);
        let from = FFT_SIZE - SCOPE_LEN + start;
        self.scope.copy_from_slice(&self.samples[from..from + n]);
    }

    /// Windowed FFT of the latest samples into `magnitudes`.
    fn transform(&mut self) {
        for (i, (b, s)) in self.buf.iter_mut().zip(&self.samples).enumerate() {
            *b = Complex::new(s * self.window[i], 0.0);
        }
        self.fft.process_with_scratch(&mut self.buf, &mut self.scratch);
        // Amplitude of a full-scale sine is 1.0.
        let norm = 2.0 / self.window_sum;
        for (m, c) in self.magnitudes.iter_mut().zip(&self.buf) {
            *m = c.norm() * norm;
        }
    }

    /// The current spectrum in `bands` log-spaced bands, 0-1 each.
    fn band_levels(&self, bands: usize) -> Vec<f32> {
        let sample_rate = self.sample_rate;
        let bin_hz = sample_rate / FFT_SIZE as f32;
        let max_hz = MAX_HZ.min(sample_rate / 2.0 - bin_hz);
        let ratio = (max_hz / MIN_HZ).powf(1.0 / bands as f32);
        let mut out = Vec::with_capacity(bands);
        for i in 0..bands {
            let lo = MIN_HZ * ratio.powi(i as i32);
            let hi = lo * ratio;
            let (a, b) = ((lo / bin_hz).floor() as usize, (hi / bin_hz).ceil() as usize);
            let amp = if b > a + 1 {
                self.magnitudes[a.max(1)..b.min(self.magnitudes.len())].iter().copied().fold(0.0, f32::max)
            } else {
                // Narrower than one bin (the bass end): interpolate at the band's centre.
                let x = (lo * hi).sqrt() / bin_hz;
                let k = (x.floor() as usize).clamp(1, self.magnitudes.len() - 2);
                let f = x - k as f32;
                self.magnitudes[k] * (1.0 - f) + self.magnitudes[k + 1] * f
            };
            let centre = (lo * hi).sqrt();
            let db = 20.0 * amp.max(1e-9).log10() + SLOPE_DB_PER_OCTAVE * (centre / 1000.0).log2();
            out.push(((db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)).clamp(0.0, 1.0));
        }
        out
    }
}

#[cfg(test)]
mod tests;
