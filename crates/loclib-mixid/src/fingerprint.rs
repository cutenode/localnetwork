//! Audio fingerprints: spectral peaks and the landmark pairs hashed from them.
//!
//! This follows Dan Ellis's audfprint, itself in the line of Wang's Shazam paper. The signal is
//! analysed at 11,025 Hz in 46 ms frames every 23 ms. Each frame's log-magnitude spectrum is
//! high-passed along time, which cancels any fixed EQ (a DJ mixer's tone controls), and the
//! strongest local maxima above a decaying, frequency-spread threshold become peaks. Pairs of
//! nearby peaks are hashed as (frequency, frequency difference, time difference).
//!
//! DJs play tracks faster or slower than recorded. With key lock (master tempo) only time
//! stretches; without it, pitch moves with the tempo. [`Variant`] maps a query landmark heard at
//! a given tempo back to the hash the original recording would have produced, so matching can
//! search a range of tempos.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// Analysis sample rate.
pub const RATE: u32 = 11_025;
const FFT_LEN: usize = 512;
const HOP: usize = 256;
/// Frequency bins used (the Nyquist bin is dropped); bin width is ~21.5 Hz.
pub const BINS: usize = FFT_LEN / 2;
/// Peaks below ~65 Hz are mostly kick drums every track shares.
const MIN_BIN: usize = 3;
/// Landmark pairs: the target peak lies 2..63 frames after the anchor, within 31 bins of it.
pub const MIN_DT: u32 = 2;
pub const MAX_DT: u32 = 63;
pub const MAX_DF: i32 = 31;
/// Threshold spread across frequency (standard deviation in bins).
const SPREAD_SD: f32 = 30.0;
/// Time high-pass pole on the log spectrogram.
const HPF_POLE: f32 = 0.98;

/// Frames per second of audio.
pub const FRAMES_PER_SEC: f64 = RATE as f64 / HOP as f64;

/// A spectral peak: frame index and frequency bin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Peak {
    pub t: u32,
    pub f: u8,
}

/// How many peaks to keep. Library tracks are sparse to keep the index small; mixes are dense,
/// since a peak from a track can be masked by the other deck or moved by the mixer.
#[derive(Debug, Clone, Copy)]
pub struct Density {
    /// Roughly the peaks per second wanted (audfprint's `density`).
    pub per_sec: f32,
    pub max_per_frame: usize,
}

pub const LIBRARY: Density = Density { per_sec: 20.0, max_per_frame: 5 };
pub const QUERY: Density = Density { per_sec: 40.0, max_per_frame: 8 };

/// Coarse loudness of one frame, for drawing the mix's waveform.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameLevels {
    /// Peak absolute sample value.
    pub peak: f32,
    /// RMS magnitude below 200 Hz, 200 Hz-2 kHz and above 2 kHz.
    pub bands: [f32; 3],
}

/// Streaming analyser: push samples at [`RATE`], then [`Analyzer::finish`] for the peaks.
pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    pending: Vec<f32>,
    scratch: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
    prev_log: Vec<f32>,
    hpf: Vec<f32>,
    thresh: Vec<f32>,
    spread: Vec<f32>,
    decay: f32,
    density: Density,
    frame: u32,
    /// Forward-pass peaks with their (high-passed) values, in frame order.
    found: Vec<(Peak, f32)>,
    levels: Option<Vec<FrameLevels>>,
}

impl Analyzer {
    /// `levels`: also record [`FrameLevels`] for every frame.
    pub fn new(density: Density, levels: bool) -> Analyzer {
        let fft = FftPlanner::new().plan_fft_forward(FFT_LEN);
        let fft_scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let window =
            (0..FFT_LEN).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_LEN as f32).cos()).collect();
        // audfprint's masking-threshold decay for a target density, per hop.
        let decay = 1.0 - 0.01 * (density.per_sec * (HOP as f32 / 352.8).sqrt() / 35.0);
        let spread = (0..2 * BINS + 1)
            .map(|i| {
                let d = i as f32 - BINS as f32;
                (-0.5 * (d / SPREAD_SD).powi(2)).exp()
            })
            .collect();
        Analyzer {
            fft,
            window,
            pending: Vec::new(),
            scratch: vec![Complex::default(); FFT_LEN],
            fft_scratch,
            prev_log: vec![0.0; BINS],
            hpf: vec![0.0; BINS],
            thresh: Vec::new(),
            spread,
            decay,
            density,
            frame: 0,
            found: Vec::new(),
            levels: levels.then(Vec::new),
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        self.pending.extend_from_slice(samples);
        let mut start = 0;
        while start + FFT_LEN <= self.pending.len() {
            self.analyse(start);
            start += HOP;
        }
        self.pending.drain(..start);
    }

    /// Frames analysed so far.
    pub fn frames(&self) -> u32 {
        self.frame
    }

    fn analyse(&mut self, start: usize) {
        let samples = &self.pending[start..start + FFT_LEN];
        for ((c, s), w) in self.scratch.iter_mut().zip(samples).zip(&self.window) {
            *c = Complex::new(s * w, 0.0);
        }
        self.fft.process_with_scratch(&mut self.scratch, &mut self.fft_scratch);

        if let Some(levels) = &mut self.levels {
            // The middle hop of the window is what this frame stands for.
            let mid = &samples[FFT_LEN / 2 - HOP / 2..FFT_LEN / 2 + HOP / 2];
            let peak = mid.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let band = |lo: usize, hi: usize| {
                let e: f32 = self.scratch[lo..hi].iter().map(|c| c.norm_sqr()).sum();
                (e / (hi - lo) as f32).sqrt() / FFT_LEN as f32
            };
            levels.push(FrameLevels { peak, bands: [band(1, 10), band(10, 93), band(93, BINS)] });
        }

        // Log magnitude, high-passed along time per bin.
        const FLOOR: f32 = 1e-5 * FFT_LEN as f32;
        for b in 0..BINS {
            let log = self.scratch[b].norm().max(FLOOR).ln();
            self.hpf[b] = log - self.prev_log[b] + HPF_POLE * self.hpf[b];
            self.prev_log[b] = log;
        }
        let t = self.frame;
        self.frame += 1;
        if t == 0 {
            // The first frame is all onset: use it to seed the threshold, not for peaks.
            self.thresh = vec![0.0; BINS];
            for b in 0..BINS {
                let v = self.hpf[b];
                if v > 0.0 {
                    spread_into(&mut self.thresh, &self.spread, b, v);
                }
            }
            return;
        }

        let col = &self.hpf;
        let diff: Vec<f32> = (0..BINS).map(|b| (col[b] - self.thresh[b]).max(0.0)).collect();
        let mut candidates: Vec<usize> =
            (MIN_BIN..BINS - 1).filter(|&b| diff[b] > 0.0 && diff[b] > diff[b - 1] && diff[b] >= diff[b + 1]).collect();
        candidates.sort_by(|&a, &b| col[b].total_cmp(&col[a]));
        let mut kept = 0;
        for b in candidates {
            if kept == self.density.max_per_frame {
                break;
            }
            let v = col[b];
            if v >= self.thresh[b] {
                self.found.push((Peak { t, f: b as u8 }, v));
                spread_into(&mut self.thresh, &self.spread, b, v);
                kept += 1;
            }
        }
        for x in &mut self.thresh {
            *x *= self.decay;
        }
    }

    /// Peaks in (frame, bin) order, after a backward pruning pass, and per-frame levels if they
    /// were asked for.
    pub fn finish(mut self) -> (Vec<Peak>, Vec<FrameLevels>) {
        // Pad so the tail of the signal gets analysed too.
        if !self.pending.is_empty() {
            let pad = FFT_LEN - HOP;
            self.push(&vec![0.0; pad]);
        }
        let peaks = backward_prune(&self.found, &self.spread, self.decay);
        (peaks, self.levels.unwrap_or_default())
    }
}

fn spread_into(thresh: &mut [f32], spread: &[f32], bin: usize, value: f32) {
    let offset = BINS - bin;
    for (i, t) in thresh.iter_mut().enumerate() {
        let s = value * spread[offset + i];
        if s > *t {
            *t = s;
        }
    }
}

/// audfprint's backward pass: walking back in time with the same decaying threshold drops peaks
/// that are only there because something louder came just after.
fn backward_prune(found: &[(Peak, f32)], spread: &[f32], decay: f32) -> Vec<Peak> {
    let mut keep = vec![false; found.len()];
    let mut thresh = vec![0.0f32; BINS];
    let mut end = found.len();
    let mut last_t = found.last().map_or(0, |(p, _)| p.t);
    while end > 0 {
        let t = found[end - 1].0.t;
        let mut start = end;
        while start > 0 && found[start - 1].0.t == t {
            start -= 1;
        }
        for _ in t..last_t {
            for x in &mut thresh {
                *x *= decay;
            }
        }
        last_t = t;
        let mut idx: Vec<usize> = (start..end).collect();
        idx.sort_by(|&a, &b| found[b].1.total_cmp(&found[a].1));
        for i in idx {
            let (p, v) = found[i];
            if v >= thresh[p.f as usize] {
                keep[i] = true;
                spread_into(&mut thresh, spread, p.f as usize, v);
            }
        }
        end = start;
    }
    let mut peaks: Vec<Peak> = found.iter().zip(keep).filter(|(_, k)| *k).map(|((p, _), _)| *p).collect();
    peaks.sort_unstable();
    peaks
}

/// A pair of peaks: the anchor's frame and frequency, the target's frequency and how many
/// frames later it comes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landmark {
    pub t: u32,
    pub f1: u8,
    pub f2: u8,
    pub dt: u8,
}

/// Pair every peak with up to `fanout` later peaks in its target zone.
pub fn landmarks(peaks: &[Peak], fanout: usize) -> Vec<Landmark> {
    let mut out = Vec::with_capacity(peaks.len() * fanout);
    for (i, a) in peaks.iter().enumerate() {
        let mut n = 0;
        for b in &peaks[i + 1..] {
            let dt = b.t - a.t;
            if dt >= MAX_DT {
                break;
            }
            if dt < MIN_DT || (b.f as i32 - a.f as i32).abs() > MAX_DF {
                continue;
            }
            out.push(Landmark { t: a.t, f1: a.f, f2: b.f, dt: dt as u8 });
            n += 1;
            if n == fanout {
                break;
            }
        }
    }
    out
}

pub const HASH_BITS: u32 = 20;

/// 8 bits of anchor frequency, 6 of frequency difference, 6 of time difference.
pub fn hash(f1: u32, df: i32, dt: u32) -> u32 {
    ((f1 & 0xff) << 12) | (((df & 0x3f) as u32) << 6) | (dt & 0x3f)
}

impl Landmark {
    pub fn hash(&self) -> u32 {
        hash(self.f1 as u32, self.f2 as i32 - self.f1 as i32, self.dt as u32)
    }
}

/// A playback speed to search: `ratio` > 1 is faster than the recording. With `pitch` the
/// spectrum moved with it (no key lock); without, only time stretched (key lock).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Variant {
    pub ratio: f64,
    pub pitch: bool,
}

impl Variant {
    /// The hash the original recording would have for a landmark heard at this speed, if any.
    pub fn original_hash(&self, lm: &Landmark) -> Option<u32> {
        let dt = (lm.dt as f64 * self.ratio).round() as u32;
        if !(MIN_DT..MAX_DT).contains(&dt) {
            return None;
        }
        let (f1, f2) = if self.pitch {
            ((lm.f1 as f64 / self.ratio).round() as i32, (lm.f2 as f64 / self.ratio).round() as i32)
        } else {
            (lm.f1 as i32, lm.f2 as i32)
        };
        let df = f2 - f1;
        if !(MIN_BIN as i32..BINS as i32).contains(&f1) || df.abs() > MAX_DF {
            return None;
        }
        Some(hash(f1 as u32, df, dt))
    }
}

/// Speeds to search within ±`range` (0.08 is ±8%). Key-locked playback only shifts time, so a
/// coarse grid of speeds is enough; without key lock every bin moves too, which needs a fine one.
pub fn variants(range: f64) -> Vec<Variant> {
    let mut v = vec![Variant { ratio: 1.0, pitch: false }];
    let mut grid = |step: f64, pitch: bool| {
        let n = ((1.0 + range).ln() / (1.0 + step).ln()).ceil() as i32;
        for k in (-n..=n).filter(|&k| k != 0) {
            v.push(Variant { ratio: (1.0 + step).powi(k), pitch });
        }
    };
    // Rounding error stays under half a frame (dt < 63) / half a bin (f < 256) between steps.
    grid(0.012, false);
    grid(0.0038, true);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::synth;

    fn analyse(x: &[f32], d: Density) -> Vec<Peak> {
        let mut a = Analyzer::new(d, false);
        for c in x.chunks(777) {
            a.push(c);
        }
        a.finish().0
    }

    #[test]
    fn peak_density_is_in_range() {
        let x = synth(20.0, 1);
        let lib = analyse(&x, LIBRARY).len() as f64 / 20.0;
        let query = analyse(&x, QUERY).len() as f64 / 20.0;
        assert!((5.0..60.0).contains(&lib), "library {lib}/s");
        assert!(query > lib, "query {query}/s vs library {lib}/s");
    }

    #[test]
    fn same_audio_shares_hashes() {
        let x = synth(15.0, 7);
        let a: rustc_hash::FxHashSet<u32> = landmarks(&analyse(&x, LIBRARY), 3).iter().map(Landmark::hash).collect();
        // Same audio, quieter and offset by a few samples: most hashes survive.
        let y: Vec<f32> = std::iter::repeat_n(0.0, 37).chain(x.iter().map(|s| s * 0.5)).collect();
        let b = landmarks(&analyse(&y, QUERY), 8);
        let hits = b.iter().filter(|l| a.contains(&l.hash())).count();
        assert!(hits * 3 > a.len(), "{hits} of {} hashes", a.len());
    }

    #[test]
    fn variants_undo_speed_changes() {
        let lm = Landmark { t: 100, f1: 100, f2: 120, dt: 20 };
        let faster = Landmark { t: 95, f1: 105, f2: 126, dt: 19 };
        let v = Variant { ratio: 1.05, pitch: true };
        assert_eq!(v.original_hash(&faster), Some(lm.hash()));
        let stretched = Landmark { dt: 19, ..lm };
        assert_eq!(Variant { ratio: 1.05, pitch: false }.original_hash(&stretched), Some(lm.hash()));
        let vs = variants(0.08);
        assert!(vs.iter().any(|v| (v.ratio - 1.08).abs() < 0.004 && v.pitch));
        assert!(vs.iter().all(|v| (0.9..1.1).contains(&v.ratio)));
    }

    use crate::index::tests::sorted_peaks;
    use hegel::TestCase;
    use hegel::generators as gs;

    /// Each peak is paired with the first `fanout` later peaks in its target zone, and nothing
    /// else: every landmark joins two peaks 2 to 62 frames and at most 31 bins apart.
    #[hegel::test(test_cases = 300)]
    fn landmarks_pair_peaks_in_their_target_zone(tc: TestCase) {
        let peaks = sorted_peaks(&tc, 8, 80);
        let fanout = tc.draw(gs::integers::<usize>().min_value(1).max_value(8));
        let expected: Vec<Landmark> = peaks
            .iter()
            .enumerate()
            .flat_map(|(i, a)| {
                peaks[i + 1..]
                    .iter()
                    .take_while(|b| b.t - a.t < MAX_DT)
                    .filter(|b| b.t - a.t >= MIN_DT && (b.f as i32 - a.f as i32).abs() <= MAX_DF)
                    .take(fanout)
                    .map(|b| Landmark { t: a.t, f1: a.f, f2: b.f, dt: (b.t - a.t) as u8 })
            })
            .collect();
        let found = landmarks(&peaks, fanout);
        assert_eq!(found, expected);
        assert!(found.iter().all(|lm| lm.hash() < 1 << HASH_BITS));
    }

    /// The speeds searched start at the original, come in reciprocal pairs, cover the range and
    /// stay within a step of it, for key-locked and pitched playback alike.
    #[hegel::test(test_cases = 200)]
    fn variants_cover_the_range(tc: TestCase) {
        let range = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(0.25));
        let vs = variants(range);
        assert_eq!(vs[0], Variant { ratio: 1.0, pitch: false });
        for (pitch, step) in [(false, 0.012f64), (true, 0.0038)] {
            let grid: Vec<f64> = vs.iter().filter(|v| v.pitch == pitch && v.ratio != 1.0).map(|v| v.ratio).collect();
            for r in &grid {
                assert!(grid.iter().any(|q| (q * r - 1.0).abs() < 1e-9), "{r} without its reciprocal");
                assert!(r.ln().abs() <= (1.0 + range).ln() + step.ln_1p() + 1e-9, "{r} beyond ±{range}");
            }
            let speed = tc.draw(gs::floats::<f64>().min_value(-1.0).max_value(1.0)) * (1.0 + range).ln();
            let nearest = grid.iter().chain([&1.0]).map(|r| (r.ln() - speed).abs()).fold(f64::MAX, f64::min);
            assert!(nearest <= step.ln_1p() / 2.0 + 1e-9, "{} is {nearest} from the grid", speed.exp());
        }
    }

    /// A landmark heard at any speed within ±8% with key lock (its gap stretched, then rounded to
    /// a frame) maps back to the original's hash through one of the key-locked speeds searched;
    /// at the original speed, any landmark is its own hash.
    #[hegel::test(test_cases = 500)]
    fn key_locked_speeds_undo_a_stretch(tc: TestCase) {
        let f1 = tc.draw(gs::integers::<u8>().min_value(MIN_BIN as u8));
        let df = tc.draw(gs::integers::<i32>().min_value(-MAX_DF).max_value(MAX_DF));
        let f2 = (f1 as i32 + df).clamp(0, 255) as u8;
        let dt = tc.draw(gs::integers::<u8>().min_value(MIN_DT as u8).max_value(MAX_DT as u8 - 1));
        let original = Landmark { t: 0, f1, f2, dt };
        assert_eq!(
            Variant { ratio: 1.0, pitch: tc.draw(gs::booleans()) }.original_hash(&original),
            Some(original.hash())
        );
        let speed = tc.draw(gs::floats::<f64>().min_value(1.0 / 1.08).max_value(1.08));
        let heard = Landmark { dt: (dt as f64 / speed).round() as u8, ..original };
        let undone =
            variants(0.08).iter().filter(|v| !v.pitch).any(|v| v.original_hash(&heard) == Some(original.hash()));
        assert!(undone, "{heard:?} at {speed}");
    }

    /// Peaks don't depend on how the samples arrive: pushed all at once or in any chunks, the
    /// same peaks (in frame and bin order, in the bins searched) and one level per frame.
    #[hegel::test(test_cases = 40)]
    fn peaks_dont_depend_on_how_samples_arrive(tc: TestCase) {
        let secs = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(2.5));
        let x = crate::test_util::synth(secs, tc.draw(gs::integers::<u32>()));
        let whole = {
            let mut a = Analyzer::new(QUERY, true);
            a.push(&x);
            a.finish()
        };
        let sizes = tc.draw(gs::vecs(gs::integers::<usize>().min_value(1).max_value(3000)).min_size(1).max_size(8));
        let mut a = Analyzer::new(QUERY, true);
        let (mut at, mut i) = (0, 0);
        while at < x.len() {
            let n = sizes[i % sizes.len()].min(x.len() - at);
            a.push(&x[at..at + n]);
            (at, i) = (at + n, i + 1);
        }
        assert_eq!(a.frames() as usize, x.len().saturating_sub(FFT_LEN) / HOP + usize::from(x.len() >= FFT_LEN));
        let (peaks, levels) = a.finish();
        assert_eq!(peaks, whole.0);
        assert!(peaks.is_sorted());
        assert!(peaks.iter().all(|p| (MIN_BIN..BINS - 1).contains(&(p.f as usize)) && (p.t as usize) < levels.len()));
        let loudest = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert_eq!(levels.len(), whole.1.len());
        assert!(levels.iter().all(|l| l.peak <= loudest && l.bands.iter().all(|b| *b >= 0.0)));
    }
}
