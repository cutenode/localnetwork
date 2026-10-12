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
mod tests;
