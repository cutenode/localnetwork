#![doc = include_str!("../README.md")]

pub mod color;
mod cues;
mod palette;
mod phrases;
mod pixels;
mod spectrum;
mod waveform;

pub use cues::{CueLane, cue_label, cue_letter};
pub use palette::{Palette, Rgb};
pub use phrases::PhraseStrip;
#[cfg(feature = "image")]
pub use pixels::Art;
pub use pixels::{PixelBuf, Pixels, vinyl};
pub use spectrum::{Scope, Spectrum, bar_layout};
pub use waveform::{Detail, Overview};

/// Partial blocks, filled from the bottom, in eighths.
const LOWER: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
