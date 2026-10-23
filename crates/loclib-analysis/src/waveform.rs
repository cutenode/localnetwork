//! A track's loudness in three bands, for drawing it: its mono mix at [`RATE`] split at 200 Hz and
//! 2 kHz (where loclib-mixid's mix waveforms split theirs), each band's RMS level a column.

use biquad::{Biquad, Coefficients, DirectForm2Transposed, Q_BUTTERWORTH_F32, ToHertz, Type};
use loclib_mixid::fingerprint::{Scale, scaled};
use serde::{Deserialize, Serialize};

use crate::RATE;

/// Samples a column at [`RATE`]: 23 ms.
const COLUMN: usize = 1024;

/// About 43 columns a second, as many as loclib-mixid's analyser has frames. That's more than
/// locallibrary's player draws at its default zoom (8 seconds across the terminal, 25 columns a
/// second on one 200 cells wide), and players' previews (100, 400 or 1200 columns a track) are
/// pooled from it. rekordbox's detail waveforms have 150 a second, which locallibrary reaches by
/// repeating columns.
pub const COLUMNS_PER_SEC: f64 = RATE as f64 / COLUMN as f64;

/// The RMS level drawn full height when a track's loudest columns are quieter: -40 dBFS, so a
/// near-silent track (dither, a lone fade) is drawn near 0 rather than scaled up to fill it.
const FLOOR: f32 = 0.01;

/// [`COLUMNS_PER_SEC`] columns a second of a track, column `i` the samples from `i` columns in to
/// `i + 1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waveform {
    /// Each column's lows (below 200 Hz), mids and highs (above 2 kHz), 0-255, all three against
    /// the track's loudest columns, so they keep their ratios.
    pub columns: Vec<[u8; 3]>,
}

/// A filter of the crossovers: Butterworth sections in a row, two to a Linkwitz-Riley crossover.
type Sections<const N: usize> = [DirectForm2Transposed<f32>; N];

fn crossover(kind: Type<f32>, at: f32) -> Sections<2> {
    let fitted = Coefficients::<f32>::from_params(kind, (RATE as f32).hz(), at.hz(), Q_BUTTERWORTH_F32);
    [DirectForm2Transposed::new(fitted.expect("crossovers are under the Nyquist frequency")); 2]
}

fn run<const N: usize>(sections: &mut Sections<N>, x: f32) -> f32 {
    sections.iter_mut().fold(x, |x, s| s.run(x))
}

/// Measures a [`Waveform`] as mono samples at [`RATE`] stream in.
pub(crate) struct Bands {
    low: Sections<2>,
    mid: Sections<4>,
    high: Sections<2>,
    /// The column so far: each band's sum of squares, over `samples`.
    squares: [f64; 3],
    samples: usize,
    /// Each column's RMS levels.
    levels: Vec<[f32; 3]>,
}

impl Bands {
    pub(crate) fn new() -> Bands {
        let [a, b] = crossover(Type::HighPass, 200.0);
        let [c, d] = crossover(Type::LowPass, 2_000.0);
        Bands {
            low: crossover(Type::LowPass, 200.0),
            mid: [a, b, c, d],
            high: crossover(Type::HighPass, 2_000.0),
            squares: [0.0; 3],
            samples: 0,
            levels: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, mono: &[f32]) {
        for &x in mono {
            let bands = [run(&mut self.low, x), run(&mut self.mid, x), run(&mut self.high, x)];
            for (sum, b) in self.squares.iter_mut().zip(bands) {
                *sum += f64::from(b * b);
            }
            self.samples += 1;
            if self.samples == COLUMN {
                self.end_column();
            }
        }
    }

    fn end_column(&mut self) {
        let n = self.samples as f64;
        self.levels.push(self.squares.map(|sum| (sum / n).sqrt() as f32));
        (self.squares, self.samples) = ([0.0; 3], 0);
    }

    /// The waveform, its last column however few samples it has.
    pub(crate) fn finish(mut self) -> Waveform {
        if self.samples > 0 {
            self.end_column();
        }
        let series: [Vec<f32>; 3] = std::array::from_fn(|b| self.levels.iter().map(|l| l[b]).collect());
        let [low, mid, high] = scaled(series.each_ref().map(Vec::as_slice), Scale::Together { floor: FLOOR }, [1.0; 3]);
        Waveform { columns: low.into_iter().zip(mid).zip(high).map(|((l, m), h)| [l, m, h]).collect() }
    }
}
