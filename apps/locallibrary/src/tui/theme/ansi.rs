//! The roles on ANSI's 16 colours. A scheme can make them anything, so this leans on what schemes
//! agree on: the terminal's own foreground and background always read together, and on a dark
//! background the bright colours read, on a light one red and blue above all. Each role names its
//! slot by the slot's colour in [`ANSI`]; hue that won't carry a meaning is left to bold, reverse
//! video and symbols (see `flat`).

use std::borrow::Cow;

use locallibrary::theme::Theme;
use ratatui::style::Color;

use super::Mode;
use super::tier::ANSI;

const BLACK: [u8; 3] = ANSI[0];
const RED: [u8; 3] = ANSI[1];
const YELLOW: [u8; 3] = ANSI[3];
const BLUE: [u8; 3] = ANSI[4];
const MAGENTA: [u8; 3] = ANSI[5];
const CYAN: [u8; 3] = ANSI[6];
const WHITE: [u8; 3] = ANSI[7];
const GREY: [u8; 3] = ANSI[8];
const BRIGHT_RED: [u8; 3] = ANSI[9];
const BRIGHT_GREEN: [u8; 3] = ANSI[10];
const BRIGHT_YELLOW: [u8; 3] = ANSI[11];
const BRIGHT_BLUE: [u8; 3] = ANSI[12];
const BRIGHT_MAGENTA: [u8; 3] = ANSI[13];
const BRIGHT_CYAN: [u8; 3] = ANSI[14];
const BRIGHT_WHITE: [u8; 3] = ANSI[15];

const SPECTRUM_DARK: &[[u8; 3]] = &[BRIGHT_BLUE, BRIGHT_MAGENTA, BRIGHT_RED, BRIGHT_YELLOW];
const SCOPE_DARK: &[[u8; 3]] = &[BRIGHT_CYAN, BRIGHT_MAGENTA, BRIGHT_RED];
const SPECTRUM_LIGHT: &[[u8; 3]] = &[BLUE, MAGENTA, RED];

/// For a dark background. `WHITE` is the terminal's foreground ([`readable`]).
const DARK: Theme = Theme {
    text: WHITE,
    dim: WHITE,
    faint: WHITE,
    rule: GREY,
    accent: BRIGHT_MAGENTA,
    info: BRIGHT_CYAN,
    secondary: CYAN,
    ink: BLACK,
    selection: GREY,
    idle: GREY,
    marked: GREY,
    mixes: GREY,
    vinyl: GREY,
    good: BRIGHT_GREEN,
    warning: BRIGHT_YELLOW,
    critical: BRIGHT_RED,
    bands: [BRIGHT_BLUE, BRIGHT_RED, BRIGHT_WHITE],
    phrases: [BRIGHT_BLUE, BRIGHT_YELLOW, BRIGHT_YELLOW, BRIGHT_MAGENTA, BRIGHT_CYAN, BRIGHT_CYAN, MAGENTA, GREY],
    hot_cue: BRIGHT_GREEN,
    memory_cue: BRIGHT_RED,
    loop_cue: BRIGHT_YELLOW,
    track_colors: [BRIGHT_MAGENTA, BRIGHT_RED, YELLOW, BRIGHT_YELLOW, BRIGHT_GREEN, BRIGHT_CYAN, BRIGHT_BLUE, MAGENTA],
    playhead: BRIGHT_WHITE,
    spectrum: Cow::Borrowed(SPECTRUM_DARK),
    scope: Cow::Borrowed(SCOPE_DARK),
};

/// For a light background, where green, yellow and cyan don't read on the likes of xterm's white:
/// what they'd mean is said in the terminal's own foreground, by bold and symbols. `BLACK` is that
/// foreground ([`readable`]).
const LIGHT: Theme = Theme {
    text: BLACK,
    dim: BLACK,
    faint: BLACK,
    rule: GREY,
    accent: BLUE,
    info: BLUE,
    secondary: MAGENTA,
    ink: WHITE,
    selection: WHITE,
    idle: WHITE,
    marked: WHITE,
    mixes: WHITE,
    vinyl: BLACK,
    good: BLACK,
    warning: RED,
    critical: RED,
    bands: [BLUE, RED, BLACK],
    phrases: [BLUE, BLUE, BLUE, RED, GREY, GREY, GREY, GREY],
    hot_cue: BLUE,
    memory_cue: RED,
    loop_cue: MAGENTA,
    track_colors: [MAGENTA, RED, RED, GREY, GREY, BLUE, BLUE, MAGENTA],
    playhead: BLACK,
    spectrum: Cow::Borrowed(SPECTRUM_LIGHT),
    scope: Cow::Borrowed(SPECTRUM_LIGHT),
};

/// The 16-colour palette for `mode`.
pub fn theme(mode: Mode) -> &'static Theme {
    if mode == Mode::Light { &LIGHT } else { &DARK }
}

/// What the terminal draws for ANSI colour `i` in `mode`: text's slot is the terminal's own
/// foreground, and the hues that read badly on that mode's background (the bright ones on a light
/// one, the deep ones on a dark one) are their other selves.
pub fn readable(i: u8, mode: Mode) -> Color {
    let (text, moved) = if mode == Mode::Light {
        (0, [0, 1, 2, 3, 4, 5, 6, 7, 8, 1, 2, 3, 4, 5, 6, 15])
    } else {
        (7, [0, 9, 2, 3, 12, 13, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])
    };
    match moved[usize::from(i)] {
        slot if slot == text => Color::Reset,
        slot => Color::Indexed(slot),
    }
}

#[cfg(test)]
mod tests;
