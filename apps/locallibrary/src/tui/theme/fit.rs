//! A theme fitted to the terminal it's drawn on, once: what it left out comes from the palette for
//! the mode and tier, and every colour that doesn't read on the terminal's real background is
//! blended towards its foreground until it does.

use locallibrary::theme::Theme;

use super::ansi;
use super::tier::{Tier, drawn};
use super::{Mode, contrast, mix};

/// What text and marks have to reach against the background (WCAG's AA for text and for graphics).
pub const TEXT: f32 = 4.5;
pub const MARK: f32 = 3.0;

/// The backgrounds the palettes were made on, for a terminal that doesn't say: the tokens' canvas
/// for each mode.
pub const DARK_PAGE: [u8; 3] = [9, 9, 11];
pub const LIGHT_PAGE: [u8; 3] = [255, 255, 255];

/// What a theme is fitted to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub mode: Mode,
    pub tier: Tier,
    pub page: [u8; 3],
    /// The terminal's own foreground, if it said.
    pub foreground: Option<[u8; 3]>,
}

/// `c`, blended towards `toward` (or black or white if that won't do) just enough to reach `ratio`
/// on the background, as the terminal will show it.
pub fn legible(c: [u8; 3], fit: &Fit, toward: [u8; 3], ratio: f32) -> [u8; 3] {
    let reads = |c: [u8; 3]| contrast(drawn(c, fit.tier), fit.page) >= ratio;
    if reads(c) {
        return c;
    }
    // One of the two always reads: the worst background still gives either 4.5:1.
    let extreme = if contrast([0; 3], fit.page) >= contrast([255; 3], fit.page) { [0; 3] } else { [255; 3] };
    (1..=10).map(|i| mix(c, toward, i as f32 / 10.0)).find(|c| reads(*c)).unwrap_or(extreme)
}

/// A colour behind rows, made on the mode's usual background, moved as far as the real one is from
/// it and taken towards it until `over` reads on top.
fn panel(c: [u8; 3], made_on: [u8; 3], over: [u8; 3], fit: &Fit) -> [u8; 3] {
    let moved = [0, 1, 2].map(|i| (c[i] as i16 + fit.page[i] as i16 - made_on[i] as i16).clamp(0, 255) as u8);
    let reads = |p: &[u8; 3]| contrast(over, drawn(*p, fit.tier)) >= TEXT;
    (0..=10).map(|i| mix(moved, fit.page, i as f32 / 10.0)).find(reads).unwrap_or(fit.page)
}

/// `theme` as it's drawn: its palette for the mode and tier, fitted to the background. With 16
/// colours the scheme decides how anything reads, so its slots are left as they are.
pub fn resolve(theme: &Theme, fit: &Fit) -> Theme {
    let (mut t, made_on) = match (fit.mode, fit.tier) {
        (Mode::Plain, _) => return theme.clone(),
        (mode, Tier::Colors16) => return theme.over(ansi::theme(mode)),
        (Mode::Light, _) => (theme.over(&Theme::LIGHT), LIGHT_PAGE),
        (Mode::Dark, _) => (theme.clone(), DARK_PAGE),
    };
    let toward = fit.foreground.unwrap_or(t.text);
    for c in [&mut t.text, &mut t.dim, &mut t.faint, &mut t.accent, &mut t.info, &mut t.secondary] {
        *c = legible(*c, fit, toward, TEXT);
    }
    for c in [&mut t.good, &mut t.warning, &mut t.critical] {
        *c = legible(*c, fit, toward, TEXT);
    }
    for c in [&mut t.rule, &mut t.playhead, &mut t.hot_cue, &mut t.memory_cue, &mut t.loop_cue] {
        *c = legible(*c, fit, toward, MARK);
    }
    let marks = t.bands.iter_mut().chain(&mut t.phrases).chain(&mut t.track_colors);
    marks.chain(t.spectrum.to_mut()).chain(t.scope.to_mut()).for_each(|c| *c = legible(*c, fit, toward, MARK));
    let dim = t.dim;
    for c in [&mut t.selection, &mut t.idle, &mut t.marked] {
        *c = panel(*c, made_on, dim, fit);
    }
    t.mixes = panel(t.mixes, made_on, t.good, fit);
    t
}

#[cfg(test)]
mod tests;
