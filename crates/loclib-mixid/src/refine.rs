//! Lining two recordings of one track up to a sample, from [`align`](crate::matcher::align)'s
//! estimate.
//!
//! `align` compares fingerprint frames 23 ms apart and gets to within ~4 ms. Cue points and beat
//! grids moved by the offset are stored to the millisecond, so [`refine`] measures it on the audio
//! itself: a few loud stretches from the first minute of one file are cross-correlated with the
//! other around the estimate at 44.1 kHz, and a parabola through the peak and its neighbours puts
//! it between samples.

use std::ops::ControlFlow;
use std::path::Path;

use loclib_progress::Ctx;
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::{Error, Result, audio};

/// Rate both files are decoded at. Lower rates (the fingerprints' 11,025 Hz) lose the highs that
/// make the peak sharp.
const RATE: u32 = 44_100;
/// How much of the start of `a` the windows are picked from, seconds.
const SPAN_SECS: f64 = 60.0;
/// Lags searched either side of the estimate (10 ms): `align`'s 4 ms error with room to spare.
const MAX_LAG: usize = 441;
/// A window and the lags either side of it fill the FFT, so no lag wraps around.
const FFT_LEN: usize = 1 << 16;
/// Samples per window (~1.5 s).
const WINDOW: usize = FFT_LEN - 2 * MAX_LAG;
/// One window from each equal part of the span: an edit in between pulls the correlation down.
const WINDOWS: usize = 3;
/// Window starts tried, 50 ms apart.
const HOP: usize = RATE as usize / 20;
/// A part whose loudest window has under this share of the loudest part's energy (-30 dB) is
/// silence or a fade, with nothing to line up.
const QUIET: f64 = 1e-3;

/// The [`Refined::correlation`] below which the two files aren't the same audio at the offset
/// found, so it shouldn't be used. In the tests, copies of one recording measure 0.91 when
/// low-passed at 3.8 kHz and quantised to 1/16 of full scale (harsher than a lossy encode) and
/// 0.99 resampled; another track measures 0.01, and an edit with 2 s cut from its first 5 s, where
/// one window in three still lines up, 0.52.
pub const REFINE_MIN_CORRELATION: f64 = 0.8;

/// Where the same moment is in two recordings of one track, to a fraction of a sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Refined {
    /// Seconds to add to a time in `a` for the same moment in `b`, like
    /// [`Alignment::offset`](crate::matcher::Alignment::offset).
    pub offset: f64,
    /// Normalised cross-correlation at `offset` over all the windows, -1 to 1: 1 when `b` is
    /// `a`'s audio at another level. See [`REFINE_MIN_CORRELATION`].
    pub correlation: f64,
}

/// Measure the offset between `a` and `b` to a fraction of a sample, starting from `coarse` (a
/// precise [`Alignment::offset`](crate::matcher::Alignment::offset), within 10 ms of the truth).
/// Only the first minute or so of each file is decoded. A file too short or quiet to compare is
/// [`Error::Invalid`].
pub fn refine(a: &Path, b: &Path, coarse: f64, ctx: &Ctx<'_>) -> Result<Refined> {
    let span = (SPAN_SECS * RATE as f64) as usize;
    let shift = (coarse * RATE as f64).round() as isize;
    let x = decode_start(a, span, ctx)?;
    // As far into `b` as the span of `a` reaches, and the lags past that.
    let y = decode_start(b, (span as isize + shift).max(0) as usize + MAX_LAG, ctx)?;
    // Decoding checks every few hundred packets, which a short file never reaches.
    ctx.check()?;

    let (x_energy, y_energy) = (cumulative_energy(&x), cumulative_energy(&y));
    let energy = |t: usize| x_energy[t + WINDOW] - x_energy[t];
    // Where window `t` of `a` starts being compared in `b`, when all of its lags were decoded.
    let in_b = |t: usize| {
        let s = usize::try_from(t as isize + shift - MAX_LAG as isize).ok()?;
        (s + FFT_LEN <= y.len()).then_some(s)
    };
    let part = x.len() / WINDOWS;
    let mut starts: Vec<usize> = (0..WINDOWS)
        .filter_map(|p| {
            (p * part..(p + 1) * part)
                .step_by(HOP)
                .filter(|&t| t + WINDOW <= x.len() && in_b(t).is_some())
                .max_by(|&s, &t| energy(s).total_cmp(&energy(t)))
        })
        .collect();
    let loudest = starts.iter().map(|&t| energy(t)).fold(0.0, f64::max);
    starts.retain(|&t| energy(t) > loudest * QUIET);
    if starts.is_empty() {
        return Err(Error::Invalid(format!(
            "{} and {} don't have enough audio in common to line up",
            a.display(),
            b.display()
        )));
    }

    let mut planner = FftPlanner::<f32>::new();
    let (forward, inverse) = (planner.plan_fft_forward(FFT_LEN), planner.plan_fft_inverse(FFT_LEN));
    let lags = 2 * MAX_LAG + 1;
    // Summed over the windows: a's energy, and for each lag the product with b and b's energy.
    let (mut a_energy, mut products, mut b_energy) = (0.0, vec![0.0f64; lags], vec![0.0f64; lags]);
    for t in starts {
        let s = in_b(t).expect("kept only windows that fit");
        let mut fa: Vec<Complex<f32>> = x[t..t + WINDOW].iter().map(|&v| Complex::new(v, 0.0)).collect();
        fa.resize(FFT_LEN, Complex::default());
        let mut fb: Vec<Complex<f32>> = y[s..s + FFT_LEN].iter().map(|&v| Complex::new(v, 0.0)).collect();
        forward.process(&mut fa);
        forward.process(&mut fb);
        // Multiplying by a's conjugate spectrum gives sum(a[n] * b[n + k]) at every lag k.
        fb.iter_mut().zip(&fa).for_each(|(q, p)| *q *= p.conj());
        inverse.process(&mut fb);
        a_energy += energy(t);
        for k in 0..lags {
            products[k] += f64::from(fb[k].re) / FFT_LEN as f64;
            b_energy[k] += y_energy[s + k + WINDOW] - y_energy[s + k];
        }
    }
    let ncc: Vec<f64> =
        products.iter().zip(&b_energy).map(|(p, e)| if *e > 0.0 { p / (a_energy * e).sqrt() } else { 0.0 }).collect();
    let k = (0..lags).max_by(|&i, &j| ncc[i].total_cmp(&ncc[j])).expect("there are lags");
    // A maximum at the edge of the window is where the search stopped, not a peak: the audio
    // lines up (if at all) further from the estimate than it searched.
    if k == 0 || k == lags - 1 {
        return Err(Error::Invalid(format!(
            "they don't line up within {} ms of the estimate",
            MAX_LAG * 1000 / RATE as usize
        )));
    }
    // The vertex of a parabola through the peak and its neighbours.
    let (before, after) = (ncc[k - 1], ncc[k + 1]);
    let between = match before + after < 2.0 * ncc[k] {
        true => 0.5 * (before - after) / (before - 2.0 * ncc[k] + after),
        false => 0.0,
    };
    Ok(Refined { offset: (shift as f64 + k as f64 - MAX_LAG as f64 + between) / RATE as f64, correlation: ncc[k] })
}

/// The first `len` samples of `path` at [`RATE`], reading no further than that.
fn decode_start(path: &Path, len: usize, ctx: &Ctx<'_>) -> Result<Vec<f32>> {
    let mut out = Vec::with_capacity(len);
    audio::decode_until(path, RATE, ctx, &mut |s| {
        out.extend_from_slice(s);
        if out.len() < len { ControlFlow::Continue(()) } else { ControlFlow::Break(()) }
    })?;
    out.truncate(len);
    Ok(out)
}

/// `out[i]` is the energy of `x[..i]`, so any stretch's is a subtraction.
fn cumulative_energy(x: &[f32]) -> Vec<f64> {
    std::iter::once(0.0)
        .chain(x.iter().scan(0.0, |sum, &v| {
            *sum += f64::from(v) * f64::from(v);
            Some(*sum)
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_util::{resample, synth, wav_of};

    /// Rate the test signal is taken to be at: 44.1 kHz isn't a multiple of it, so a delay of
    /// whole samples here is a fraction of one there.
    const MASTER: u32 = 16_000;

    /// `x` (at [`MASTER`] Hz) delayed by `delay` of its samples, or started that many in when
    /// negative, saved as a 16-bit WAV at `rate`.
    fn save(dir: &Path, name: &str, x: &[f32], delay: isize, rate: u32) -> PathBuf {
        let shifted: Vec<f32> = match usize::try_from(delay) {
            Ok(d) => std::iter::repeat_n(0.0, d).chain(x.iter().copied()).collect(),
            Err(_) => x[delay.unsigned_abs()..].to_vec(),
        };
        let pcm = resample(&shifted, MASTER, rate);
        let path = dir.join(name);
        std::fs::write(&path, wav_of(rate, 16, pcm.iter().map(|s| (s * 12_000.0) as i32))).unwrap();
        path
    }

    /// About 10 s at [`MASTER`] Hz.
    fn track(seed: u32) -> Vec<f32> {
        synth(14.0, seed)
    }

    /// Copies shifted by fractions of a sample, either way, made at 48 kHz, or low-passed and
    /// coarsely quantised like a lossy encode are lined up to within a microsecond, from an
    /// estimate 4 ms out either way.
    #[test]
    fn copies_are_lined_up_to_a_fraction_of_a_sample() {
        let dir = tempfile::tempdir().unwrap();
        let x = track(7919);
        let original = save(dir.path(), "a.wav", &x, 0, 44_100);
        let lossy: Vec<f32> =
            resample(&resample(&x, MASTER, 8_000), 8_000, MASTER).iter().map(|s| (s * 16.0).round() / 16.0).collect();
        // (copy, its delay, how far out the estimate is). 401 samples is 25.06 ms, 1105.26 samples
        // at 44.1 kHz.
        let copies = [
            (save(dir.path(), "later.wav", &x, 401, 44_100), 401, -0.004),
            (save(dir.path(), "earlier.wav", &x, -173, 44_100), -173, 0.004),
            (save(dir.path(), "48k.wav", &x, 401, 48_000), 401, 0.004),
            (save(dir.path(), "lossy.wav", &lossy, 401, 44_100), 401, -0.004),
        ];
        for (copy, delay, miss) in &copies {
            let truth = *delay as f64 / MASTER as f64;
            let r = refine(&original, copy, truth + miss, &Ctx::silent()).unwrap();
            assert!((r.offset - truth).abs() < 1e-6, "{copy:?}: {r:?}");
            assert!(r.correlation > REFINE_MIN_CORRELATION, "{copy:?}: {r:?}");
        }
        // And the other way round.
        let back = refine(&copies[3].0, &original, -0.021, &Ctx::silent()).unwrap();
        assert!((back.offset + 401.0 / MASTER as f64).abs() < 1e-6, "{back:?}");
        // An estimate a sample over 10 ms out puts the truth past the search's edge, where the
        // correlation still climbs: the edge isn't taken for the peak.
        let far = refine(&original, &copies[3].0, 401.0 / MASTER as f64 + 0.01 + 1.0 / RATE as f64, &Ctx::silent());
        assert!(matches!(far, Err(Error::Invalid(_))), "{far:?}");
    }

    /// Another track, or an edit with 2 s cut from its first few, doesn't correlate at any
    /// offset near the estimate.
    #[test]
    fn other_audio_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let x = track(7919);
        let original = save(dir.path(), "a.wav", &x, 0, 44_100);
        let m = MASTER as usize;
        let others = [
            save(dir.path(), "other.wav", &track(3 * 7919), 0, 44_100),
            save(dir.path(), "edit.wav", &[&x[..3 * m], &x[5 * m..]].concat(), 0, 44_100),
        ];
        for other in &others {
            let r = refine(&original, other, 0.0, &Ctx::silent()).unwrap();
            assert!(r.correlation < REFINE_MIN_CORRELATION, "{other:?}: {r:?}");
        }
    }

    /// Too little audio, or only silence, is an error; so is a missing file and cancelling.
    #[test]
    fn short_quiet_and_missing_files_and_cancelling_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let x = track(7919);
        let original = save(dir.path(), "a.wav", &x, 0, 44_100);
        let short = save(dir.path(), "short.wav", &x[..MASTER as usize], 0, 44_100);
        let silent = save(dir.path(), "silent.wav", &vec![0.0; x.len()], 0, 44_100);
        for (a, b) in [(&short, &original), (&original, &short), (&silent, &original)] {
            let err = refine(a, b, 0.0, &Ctx::silent()).unwrap_err();
            assert!(matches!(err, Error::Invalid(_)), "{a:?} {b:?}: {err}");
        }
        let gone = dir.path().join("gone.wav");
        assert!(matches!(refine(&original, &gone, 0.0, &Ctx::silent()), Err(Error::Io { path, .. }) if path == gone));
        let ctx = Ctx::silent();
        ctx.cancel.cancel();
        assert!(matches!(refine(&short, &short, 0.0, &ctx), Err(Error::Cancelled(_))));
    }

    use hegel::TestCase;
    use hegel::generators as gs;

    /// A copy at any delay either way, at any level and at either rate, is lined up to within a
    /// microsecond from any estimate up to 4 ms out, and correlates as the same audio.
    #[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
    fn any_delay_is_measured_to_a_fraction_of_a_sample(tc: TestCase) {
        let dir = tempfile::tempdir().unwrap();
        let x = synth(8.0, tc.draw(gs::integers::<u32>().min_value(1)));
        // Whole samples here, so a fraction of one at 44.1 kHz but every 160th.
        let delay =
            tc.draw(gs::integers::<isize>().min_value(1).max_value(600)) * if tc.draw(gs::booleans()) { 1 } else { -1 };
        tc.assume(delay % 160 != 0);
        let gain = tc.draw(gs::floats::<f32>().min_value(0.25).max_value(1.0));
        let rate = tc.draw(gs::sampled_from(vec![44_100, 48_000]));
        let miss = tc.draw(gs::floats::<f64>().min_value(-0.004).max_value(0.004));
        let original = save(dir.path(), "a.wav", &x, 0, 44_100);
        let quieter: Vec<f32> = x.iter().map(|s| s * gain).collect();
        let copy = save(dir.path(), "b.wav", &quieter, delay, rate);
        let truth = delay as f64 / MASTER as f64;
        let r = refine(&original, &copy, truth + miss, &Ctx::silent()).unwrap();
        assert!((r.offset - truth).abs() < 1e-6, "{truth}: {r:?}");
        assert!(r.correlation > REFINE_MIN_CORRELATION && r.correlation <= 1.0 + 1e-9, "{r:?}");
        // Under a window's worth of audio has nothing to line up.
        let short = save(dir.path(), "short.wav", &quieter[..MASTER as usize], 0, rate);
        assert!(matches!(refine(&original, &short, truth + miss, &Ctx::silent()), Err(Error::Invalid(_))));
    }
}
