//! A track's loudness in three bands, for drawing it: loclib-mixid's frame levels, scaled as its
//! mix waveforms are.

use loclib_mixid::audio::Resampler;
use loclib_mixid::fingerprint::{self, Analyzer, FRAMES_PER_SEC, LevelColumns};
use serde::{Deserialize, Serialize};

/// A column a frame of loclib-mixid's analyser (one every 23 ms, about 43 a second): every level
/// it measures, none pooled away. That's more than locallibrary's player draws at its default
/// zoom (8 seconds across the terminal, 25 columns a second on one 200 cells wide), and
/// previews (400 or 1200 columns a track) are pooled from it. rekordbox's detail waveforms have
/// 150 a second, which locallibrary reaches by repeating columns: more would need frames closer
/// together than the analyser's.
pub const COLUMNS_PER_SEC: f64 = FRAMES_PER_SEC;

/// [`COLUMNS_PER_SEC`] columns a second of a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waveform {
    /// Each column's lows (below 200 Hz), mids and highs (above 2 kHz), 0-255, each band scaled
    /// against its own loudest columns.
    pub columns: Vec<[u8; 3]>,
}

/// Measures a [`Waveform`] as packets stream in: their mono mix, resampled to loclib-mixid's rate
/// from the rate of the first.
pub(crate) struct Levels {
    resampler: Option<Resampler>,
    analyzer: Analyzer,
}

impl Levels {
    pub(crate) fn new() -> Levels {
        // Its peaks go unused: the fewest it finds.
        Levels { resampler: None, analyzer: Analyzer::new(fingerprint::LIBRARY, true) }
    }

    /// `mono` at `rate` Hz.
    pub(crate) fn push(&mut self, mono: &[f32], rate: u32) {
        let analyzer = &mut self.analyzer;
        let resampler = self.resampler.get_or_insert_with(|| Resampler::new(rate, fingerprint::RATE));
        resampler.push(mono, &mut |s| analyzer.push(s));
    }

    pub(crate) fn finish(mut self) -> Waveform {
        if let Some(r) = &mut self.resampler {
            let analyzer = &mut self.analyzer;
            r.finish(&mut |s| analyzer.push(s));
        }
        let (_, levels) = self.analyzer.finish();
        let LevelColumns { bands: [low, mid, high], .. } = fingerprint::columns(&levels, COLUMNS_PER_SEC);
        Waveform { columns: low.into_iter().zip(mid).zip(high).map(|((l, m), h)| [l, m, h]).collect() }
    }
}
