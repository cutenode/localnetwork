//! How the player looks, as the palette offers it: how keys are written and coloured, how waveforms
//! are coloured, when it warns of a track's end, cover art (and whether it's in half blocks) and the
//! footer's hints. What's chosen is kept in tui.json.

use super::app::{App, Level};
use super::deck::BandMode;
use super::keymap::Act;
use super::keys::{KeyPalette, Notation};
use super::palette::Item;

/// The warnings on offer, in seconds.
const WARNINGS: [u16; 5] = [0, 15, 30, 45, 60];

/// How much the footer says. Errors and a job's progress show whatever it's set to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Hints {
    #[default]
    Full,
    /// The few that matter most.
    Compact,
    Off,
}

impl Hints {
    const ALL: [Hints; 3] = [Hints::Full, Hints::Compact, Hints::Off];

    pub fn name(self) -> &'static str {
        match self {
            Hints::Full => "full",
            Hints::Compact => "compact",
            Hints::Off => "off",
        }
    }

    pub fn parse(s: &str) -> Option<Hints> {
        Hints::ALL.into_iter().find(|h| h.name() == s)
    }

    fn label(self) -> &'static str {
        match self {
            Hints::Full => "full hints",
            Hints::Compact => "compact hints",
            Hints::Off => "no hints",
        }
    }
}

/// How the palette names a way of colouring waveforms.
fn bands_label(mode: Option<BandMode>) -> String {
    mode.map_or("waveforms as the drive says".into(), |m| format!("waveforms in {}", m.title()))
}

fn notation_label(n: Notation) -> &'static str {
    match n {
        Notation::Camelot => "keys in Camelot",
        Notation::OpenKey => "keys in Open Key",
        Notation::Musical => "keys as notes",
    }
}

fn palette_label(k: KeyPalette) -> &'static str {
    match k {
        KeyPalette::Theme => "key colours from the theme",
        KeyPalette::Wheel => "key colours round a wheel",
        KeyPalette::Protanopia => "key colours for protanopia",
        KeyPalette::Deuteranopia => "key colours for deuteranopia",
        KeyPalette::Tritanopia => "key colours for tritanopia",
    }
}

fn halfblocks_label(on: bool) -> &'static str {
    if on { "cover art in half blocks" } else { "cover art in graphics" }
}

fn warning_label(secs: u16) -> String {
    if secs == 0 { "no warning before a track ends".into() } else { format!("warn {secs} s before a track ends") }
}

/// The choices, as the palette lists them.
pub(super) fn items() -> Vec<(Act, Item)> {
    let item = |label: String| Item::new(label, "Player");
    let modes = [None].into_iter().chain(BandMode::ALL.map(Some));
    let bands = modes.map(|m| (Act::Bands(m), item(bands_label(m))));
    let notations = Notation::ALL.map(|n| (Act::KeyNotation(n), item(notation_label(n).into())));
    let palettes = KeyPalette::ALL.map(|k| (Act::KeyPalette(k), item(palette_label(k).into())));
    let warnings = WARNINGS.map(|secs| (Act::EndWarning(secs), item(warning_label(secs))));
    let art = [(Act::Art(true), item("cover art".into())), (Act::Art(false), item("no cover art".into()))];
    let halfblocks = [false, true].map(|on| (Act::ArtHalfblocks(on), item(halfblocks_label(on).into())));
    let hints = Hints::ALL.map(|h| (Act::Hints(h), item(h.label().into())));
    bands.chain(notations).chain(palettes).chain(warnings).chain(art).chain(halfblocks).chain(hints).collect()
}

impl App {
    /// Colour waveforms as chosen, and keep it for next time.
    pub(super) fn use_bands(&mut self, mode: Option<BandMode>) {
        self.deck.bands = mode;
        self.toast(bands_label(mode), Level::Info);
    }

    /// The next way round BLUE, RGB, 3Band and what the drive says.
    pub(super) fn next_bands(&mut self) {
        self.use_bands(match self.deck.bands {
            None => Some(BandMode::Blue),
            Some(BandMode::Blue) => Some(BandMode::Rgb),
            Some(BandMode::Rgb) => Some(BandMode::ThreeBand),
            Some(BandMode::ThreeBand) => None,
        });
    }

    pub(super) fn use_notation(&mut self, n: Notation) {
        self.deck.notation = n;
        self.toast(notation_label(n), Level::Info);
    }

    pub(super) fn use_key_palette(&mut self, k: KeyPalette) {
        self.deck.key_palette = k;
        self.toast(palette_label(k), Level::Info);
    }

    pub(super) fn use_end_warning(&mut self, secs: u16) {
        self.deck.end_warning = secs;
        self.toast(warning_label(secs), Level::Info);
    }

    pub(super) fn use_hints(&mut self, hints: Hints) {
        self.hints = hints;
        self.toast(hints.label(), Level::Info);
    }

    /// Forget what's been played, for the rest of a set: nothing's dimmed until it plays again.
    pub(super) fn reset_played(&mut self) {
        self.played_tracks.clear();
        self.toast("nothing counts as played now", Level::Info);
    }

    pub(super) fn use_art(&mut self, on: bool) {
        self.deck.art = on;
        self.toast(if on { "cover art" } else { "no cover art" }, Level::Info);
    }

    /// Draw cover art in half blocks, or in the terminal's graphics again, from the next cover on.
    pub(super) fn use_art_halfblocks(&mut self, on: bool) {
        self.deck.art_halfblocks = on;
        self.covers.forget_picker();
        self.toast(halfblocks_label(on), Level::Info);
    }
}

#[cfg(test)]
mod tests;
