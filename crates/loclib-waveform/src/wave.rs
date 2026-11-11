//! What the waveform widgets draw from, and how a column of it fills cells.

use loclib_model::analysis::{Bands, WaveColumn, WaveLayer, Waveform};
use loclib_model::tokens::bands::{HIGH, LOW, MID, SCALES};
use ratatui::buffer::Cell;
use ratatui::style::{Color, Modifier};

use crate::color::mute;
use crate::{LEFT, LOWER, Palette, Rgb};

/// A waveform as a widget reads it.
#[derive(Debug, Clone, Copy)]
pub enum Wave<'a> {
    /// Up to two layers a column in colours of their own: rekordbox's BLUE and RGB.
    Layers(&'a Waveform),
    /// Lows, mids and highs: rekordbox's 3Band, `columns_per_sec` being a scrolling one's rate and
    /// `gain` ([`band_gain`]) what they're turned up by.
    Bands { bands: &'a Bands, columns_per_sec: Option<f64>, gain: f32 },
}

/// What turns `bands` up until the tallest reaches the top of the waveform: as stored each is drawn
/// in its share of the height, which leaves a quiet master filling half of it.
pub fn band_gain(bands: &Bands) -> f32 {
    let heights = bands.0.iter().flatten().zip(SCALES.iter().cycle());
    let tallest = heights.map(|(v, scale)| f32::from(*v) / 255.0 * scale).fold(0.0, f32::max);
    if tallest > 0.0 { 1.0 / tallest } else { 1.0 }
}

/// The layers of a column, back to front.
pub(crate) type Stack = [WaveLayer; 3];

impl Wave<'_> {
    /// How many columns it has.
    pub fn len(&self) -> usize {
        match self {
            Wave::Layers(w) => w.columns.len(),
            Wave::Bands { bands, .. } => bands.0.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn columns_per_sec(&self) -> Option<f64> {
        match self {
            Wave::Layers(w) => w.columns_per_sec,
            Wave::Bands { columns_per_sec, .. } => *columns_per_sec,
        }
    }

    /// What columns `[from, to)` come to, each band or layer at its loudest, or the column at
    /// `from` when the range is empty. `None` outside the waveform.
    pub(crate) fn stack(&self, from: f64, to: f64) -> Option<Stack> {
        if from < 0.0 || from >= self.len() as f64 {
            return None;
        }
        let a = from as usize;
        let b = (to.ceil() as usize).clamp(a + 1, self.len());
        Some(match self {
            Wave::Layers(w) => {
                let loudest = w.columns[a..b].iter().copied().max_by(|x, y| x.back.height.total_cmp(&y.back.height));
                let WaveColumn { back, front } = loudest.unwrap_or_default();
                [back, front, WaveLayer::default()]
            }
            Wave::Bands { bands, gain, .. } => {
                let loudest =
                    bands.0[a..b].iter().fold([0u8; 3], |m, c| [m[0].max(c[0]), m[1].max(c[1]), m[2].max(c[2])]);
                let colors = [LOW, MID, HIGH];
                let mut layers: Stack = std::array::from_fn(|i| WaveLayer {
                    height: f32::from(loudest[i]) / 255.0 * SCALES[i] * gain,
                    color: colors[i],
                });
                // Stable: of two alike, the lower band stays behind.
                layers.sort_by(|a, b| b.height.total_cmp(&a.height));
                layers
            }
        })
    }
}

/// One cell of a column filled by its layers: the eighths `fill` from its base in layer `ink`,
/// with `paper` the layer behind what's left of a cell filled to its far edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fill {
    pub fill: usize,
    pub ink: usize,
    pub paper: Option<usize>,
}

/// The `row`th cell out from a column's base, `heights` (back to front) being in cells. Two
/// colours are all a cell has: where three would meet, the two that reach furthest are shown.
pub(crate) fn fill_of(heights: [f32; 3], row: u16) -> Fill {
    let eighths = heights.map(|h| ((h - f32::from(row)) * 8.0).round().clamp(0.0, 8.0) as usize);
    let reach = eighths.into_iter().max().unwrap_or(0);
    // The frontmost of those that reach that far.
    let top = (0..3).rev().find(|i| eighths[*i] == reach).unwrap_or(0);
    if reach < 8 {
        return Fill { fill: reach, ink: top, paper: None };
    }
    match (top + 1..3).rev().find(|i| eighths[*i] > 0) {
        Some(front) => Fill { fill: eighths[front], ink: front, paper: Some(top) },
        None => Fill { fill: 8, ink: top, paper: None },
    }
}

/// How far a drive's waveform colours are taken towards grey: a wall of pure red at this size is
/// more than the eye wants. The bands' are chosen for this and left alone: muted, the orange falls
/// to a dusty pink among 256 colours.
const MUTED: f32 = 0.15;

impl Palette<'_> {
    /// A waveform layer's colour as it's drawn, read as a marker is. On a light page the near white
    /// of the highs would only come down to grey, so it goes on to most of the way to the page's
    /// ink: the core of the waveform is the strongest thing in it on either page.
    pub(crate) fn wave_colour(&self, c: Rgb) -> Rgb {
        let band = [LOW, MID, HIGH].contains(&c);
        let shown = (self.marker)(if band { c } else { mute(c, MUTED) });
        if c == HIGH && shown != c { self.fade(self.fg, 0.8) } else { shown }
    }

    /// `f` in a cell of a waveform that grows up, `colors` being its layers'.
    pub(crate) fn put_rising(&self, cell: &mut Cell, f: Fill, colors: &[Color; 3]) {
        if f.fill > 0 {
            cell.set_symbol(LOWER[f.fill]).set_fg(colors[f.ink]);
            if let Some(paper) = f.paper {
                cell.set_bg(colors[paper]);
            }
        }
    }

    /// `f` in a cell of a waveform that grows down. Blocks only fill from the bottom, so a part
    /// filled from the top is the rest of a block in reverse video: the block shows what's behind
    /// the cell.
    pub(crate) fn put_falling(&self, cell: &mut Cell, f: Fill, colors: &[Color; 3]) {
        match f.fill {
            0 => {}
            8 => {
                cell.set_symbol(LOWER[8]).set_fg(colors[f.ink]);
            }
            fill => {
                cell.set_symbol(LOWER[8 - fill]).set_fg(colors[f.ink]);
                cell.modifier.insert(Modifier::REVERSED);
                if let Some(paper) = f.paper {
                    cell.set_bg(colors[paper]);
                }
            }
        }
    }
}

/// A playhead a cell wide that moves in eighths of a cell, across the cell it starts in and the
/// next: `eighth` is how far into `head` it starts, and what's under it shows either side. At 0 it
/// fills `head` and leaves `next` alone. The eighths are block widths, so `head` is the reverse of
/// the block that's left of it.
pub fn playhead_cells(head: &mut Cell, next: Option<&mut Cell>, eighth: u8, colour: Color) {
    let under = |cell: &Cell| if cell.symbol() == " " { cell.bg } else { cell.fg };
    let eighth = usize::from(eighth.min(7));
    if eighth == 0 {
        head.set_symbol(LOWER[8]).set_fg(colour);
        return;
    }
    let behind = under(head);
    head.set_symbol(LEFT[eighth]).set_fg(colour).set_bg(behind);
    head.modifier.insert(Modifier::REVERSED);
    if let Some(next) = next {
        let ahead = under(next);
        next.set_symbol(LEFT[eighth]).set_fg(colour).set_bg(ahead);
        next.modifier.remove(Modifier::REVERSED);
    }
}
