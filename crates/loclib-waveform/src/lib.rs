#![doc = include_str!("../README.md")]

#[cfg(feature = "analyzer")]
pub mod analyzer;
pub mod color;
mod cues;
mod palette;
mod phrases;
mod pixels;
mod spectrum;
mod wave;
mod waveform;
mod zoom;

pub use cues::{CueLane, Marker, cue_label, cue_letter, place_markers};
pub use palette::{Palette, Rgb};
pub use phrases::PhraseStrip;
#[cfg(feature = "image")]
pub use pixels::Art;
pub use pixels::{PixelBuf, Pixels, vinyl};
pub use spectrum::{Scope, Spectrum, bar_layout};
pub use wave::{Wave, band_gain, playhead_cells};
pub use waveform::{Detail, Overview, WaveOverview};
pub use zoom::{Zoom, ZoomRing};

/// Partial blocks, filled from the bottom, in eighths.
const LOWER: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
/// Partial blocks, filled from the left, in eighths.
const LEFT: [&str; 9] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
