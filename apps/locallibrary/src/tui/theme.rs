//! Colours. Truecolor where the terminal supports it, the nearest xterm-256 colour otherwise
//! (macOS Terminal.app, older tmux setups). The palette is made for a dark terminal; on a light
//! one each colour is drawn with its lightness turned round (see `for_light`). With NO_COLOR set
//! there are no colours at all, and what colour says is also said with bold, reverse video and
//! symbols.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use locallibrary::anlz::PhraseKind;
use locallibrary::model::TrackColor;
use locallibrary::tokens::{phrases, status};
use loclib_waveform::Palette;
pub use loclib_waveform::color::{gradient, mix, scale};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::BorderType;

/// How the player draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The palette as made, for a dark background.
    Dark,
    /// The palette turned round for a light background.
    Light,
    /// No colours (NO_COLOR).
    Plain,
}

static MODE: AtomicU8 = AtomicU8::new(0);
static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    /// A test's own mode, so tests drawing in different modes can run side by side.
    static TEST_MODE: std::cell::Cell<Option<Mode>> = const { std::cell::Cell::new(None) };
}

/// Read how to draw from the environment, as the player starts (tests draw dark unless they
/// say otherwise).
pub fn init() {
    let var = |k: &str| std::env::var(k).ok();
    let mode = detect(var("NO_COLOR"), var("LOCALLIBRARY_THEME"), var("COLORFGBG"));
    MODE.store(mode as u8 + 1, Ordering::Relaxed);
    let reduced = var("LOCALLIBRARY_REDUCED_MOTION").is_some_and(|v| !matches!(v.trim(), "" | "0" | "false" | "no"));
    REDUCED_MOTION.store(reduced, Ordering::Relaxed);
}

/// The mode the environment asks for: no colours when NO_COLOR is set to anything (see
/// no-color.org); else the palette LOCALLIBRARY_THEME names (`light` or `dark`); else light when
/// COLORFGBG (`foreground;background`, set by rxvt, Konsole and others) has a light
/// background; else dark.
pub fn detect(no_color: Option<String>, theme: Option<String>, colorfgbg: Option<String>) -> Mode {
    if no_color.is_some_and(|v| !v.is_empty()) {
        return Mode::Plain;
    }
    match theme.as_deref().map(|t| t.trim().to_ascii_lowercase()).as_deref() {
        Some("light") => return Mode::Light,
        Some("dark") => return Mode::Dark,
        _ => {}
    }
    // The background is the last field: 7 (light grey) and 9-15 (the bright colours, white) are
    // light, 0-6 and 8 (dark grey) dark.
    let bg = colorfgbg.and_then(|v| v.rsplit(';').next().and_then(|b| b.trim().parse::<u8>().ok()));
    match bg {
        Some(7 | 9..=15) => Mode::Light,
        _ => Mode::Dark,
    }
}

pub fn mode() -> Mode {
    #[cfg(test)]
    if let Some(m) = TEST_MODE.with(std::cell::Cell::get) {
        return m;
    }
    match MODE.load(Ordering::Relaxed) {
        2 => Mode::Light,
        3 => Mode::Plain,
        _ => Mode::Dark,
    }
}

/// Draw this thread's frames in `mode` (`None`: as the player would).
#[cfg(test)]
pub fn set_test_mode(mode: Option<Mode>) {
    TEST_MODE.with(|m| m.set(mode));
}

/// No colours: say it with bold, reverse video and symbols instead.
pub fn plain() -> bool {
    mode() == Mode::Plain
}

/// LOCALLIBRARY_REDUCED_MOTION: redraw only as often as when nothing moves, and leave out
/// motion that's only decoration (the spinning record, the pulsing logo, cue flashes).
pub fn reduced_motion() -> bool {
    REDUCED_MOTION.load(Ordering::Relaxed)
}

fn truecolor() -> bool {
    static TRUECOLOR: OnceLock<bool> = OnceLock::new();
    *TRUECOLOR.get_or_init(|| {
        let colorterm = std::env::var("COLORTERM").unwrap_or_default().to_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return true;
        }
        // Terminals known to support 24-bit colour even without COLORTERM.
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        matches!(program.as_str(), "iTerm.app" | "WezTerm" | "vscode" | "ghostty" | "Hyper" | "Tabby")
            || std::env::var_os("KITTY_WINDOW_ID").is_some()
            || std::env::var_os("WT_SESSION").is_some()
    })
}

/// A palette colour as the terminal should draw it: as made on a dark terminal, turned round on
/// a light one, and nothing without colours.
pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    match mode() {
        Mode::Dark => exact([r, g, b]),
        Mode::Light => exact(for_light([r, g, b])),
        Mode::Plain => Color::Reset,
    }
}

/// A colour that is what it is whatever the terminal's background (cover art).
pub fn image(c: [u8; 3]) -> Color {
    exact(c)
}

fn exact([r, g, b]: [u8; 3]) -> Color {
    if truecolor() { Color::Rgb(r, g, b) } else { Color::Indexed(xterm256(r, g, b)) }
}

/// A colour of the dark palette for a light background: its lightness turned round, so light
/// text goes dark and dark backgrounds go light, with hue and saturation kept. Bright colours
/// (which can be bright at middling lightness, like rekordbox's hot cue green) are taken down
/// until they're as dark as they were bright, so they read on white and under light text.
fn for_light(c: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = c.map(|v| v as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let (d, l) = (max - min, (max + min) / 2.0);
    let s = if d == 0.0 { 0.0 } else { (d / (1.0 - (2.0 * l - 1.0).abs())).min(1.0) };
    // Hue in sixths of the wheel.
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    let most = (1.0 - luminance(c)).powi(3);
    let mut l = (1.0 - l).powf(1.25);
    loop {
        let out = from_hsl(h, s, l);
        if l <= 0.0 || luminance(out) <= most {
            return out;
        }
        l -= 0.02;
    }
}

/// A colour from hue (in sixths of the wheel), saturation and lightness.
fn from_hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let l = l.clamp(0.0, 1.0);
    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = chroma * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = l - chroma / 2.0;
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// How bright a colour looks (sRGB relative luminance, 0 to 1).
fn luminance(c: [u8; 3]) -> f32 {
    let [r, g, b] = c.map(|v| {
        let v = v as f32 / 255.0;
        if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    });
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

pub fn rgb3(c: [u8; 3]) -> Color {
    rgb(c[0], c[1], c[2])
}

/// Nearest colour in the xterm 6x6x6 cube or the grey ramp.
fn xterm256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let nearest = |v: u8| LEVELS.iter().enumerate().min_by_key(|(_, l)| (**l as i32 - v as i32).abs()).unwrap().0;
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube = [LEVELS[ri], LEVELS[gi], LEVELS[bi]];
    let dist = |c: [u8; 3]| -> i32 { c.iter().zip([r, g, b]).map(|(a, b)| (*a as i32 - b as i32).pow(2)).sum() };
    let avg = (r as u32 + g as u32 + b as u32) / 3;
    let gi_ = ((avg.saturating_sub(8)) / 10).min(23) as u8;
    let grey = 8 + gi_ * 10;
    if dist([grey, grey, grey]) < dist(cube) { 232 + gi_ } else { 16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8 }
}

// Palette: the waveforms' dark one, and the player's own colours.
const DARK: Palette = Palette::DARK;
pub const FG: [u8; 3] = DARK.fg;
pub const DIM: [u8; 3] = DARK.dim;
pub const FAINT: [u8; 3] = DARK.faint;
pub const PINK: [u8; 3] = DARK.accent;
pub const CYAN: [u8; 3] = DARK.scope[0];
pub const AMBER: [u8; 3] = status::dark::WARNING;
pub const VIOLET: [u8; 3] = DARK.spectrum[1];
pub const GREEN: [u8; 3] = status::dark::GOOD;
pub const RED: [u8; 3] = status::dark::CRITICAL;
pub const SELECTION: [u8; 3] = [44, 44, 70];
pub const VINYL: [u8; 3] = [40, 40, 52];

/// The palette the player's waveforms, visualisers and cover art draw in, for the mode it draws
/// in.
pub fn palette() -> Palette {
    Palette { paint: rgb3, image, plain: plain(), ..DARK }
}

pub fn fg(c: [u8; 3]) -> Style {
    Style::new().fg(rgb3(c))
}

pub fn text() -> Style {
    fg(FG)
}

pub fn dim() -> Style {
    fg(DIM)
}

pub fn faint() -> Style {
    if plain() { Style::new().add_modifier(Modifier::DIM) } else { fg(FAINT) }
}

pub fn bold(c: [u8; 3]) -> Style {
    fg(c).add_modifier(Modifier::BOLD)
}

pub fn border(focused: bool) -> Style {
    if focused { fg(CYAN) } else { faint() }
}

/// Rounded borders; without colours, the focused pane's are heavy.
pub fn border_type(focused: bool) -> BorderType {
    if focused && plain() { BorderType::Thick } else { BorderType::Rounded }
}

pub fn selected() -> Style {
    if plain() {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::new().bg(rgb3(SELECTION)).add_modifier(Modifier::BOLD)
    }
}

/// `ink` on `paper`, bold (the active tab, chips): reverse video without colours.
pub fn on(paper: [u8; 3], ink: [u8; 3]) -> Style {
    if plain() {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::new().fg(rgb3(ink)).bg(rgb3(paper)).add_modifier(Modifier::BOLD)
    }
}

/// A colour behind something that a symbol already marks (a marked row, the selection in a
/// pane without focus): nothing without colours.
pub fn behind(c: [u8; 3]) -> Style {
    if plain() { Style::new() } else { Style::new().bg(rgb3(c)) }
}

/// A track's rekordbox colour as a dot; without colours, its number (1 pink to 8 purple, as the
/// colour menu numbers them).
pub fn colour_dot(c: TrackColor) -> Span<'static> {
    if plain() {
        Span::styled(c.id().to_string(), Style::new().add_modifier(Modifier::BOLD))
    } else {
        Span::styled("●", fg(track_color(c)))
    }
}

/// A key hint, e.g. the `q` in "q quit".
pub fn key() -> Style {
    bold(PINK)
}

pub fn track_color(c: TrackColor) -> [u8; 3] {
    (DARK.track_color)(c)
}

/// Phrases by role: intros and outros cool, builds and verses warm, choruses hot, breakdowns
/// light.
pub fn phrase_color(kind: PhraseKind) -> [u8; 3] {
    match kind {
        PhraseKind::Intro => phrases::INTRO,
        PhraseKind::Up => phrases::UP,
        PhraseKind::Verse(_) => phrases::VERSE,
        PhraseKind::Chorus => phrases::CHORUS,
        PhraseKind::Down => phrases::DOWN,
        PhraseKind::Bridge => phrases::BRIDGE,
        PhraseKind::Outro => phrases::OUTRO,
        PhraseKind::Unknown(_) => phrases::UNKNOWN,
    }
}

/// rekordbox's default hot cue colour (green), used for cues without one.
pub const HOT_CUE: [u8; 3] = DARK.hot_cue;
pub const MEMORY_CUE: [u8; 3] = DARK.memory_cue;
pub const LOOP: [u8; 3] = DARK.loop_cue;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_mapping() {
        assert_eq!(xterm256(0, 0, 0), 16);
        assert_eq!(xterm256(255, 255, 255), 231);
        assert_eq!(xterm256(255, 0, 0), 196);
        assert_eq!(xterm256(128, 128, 128), 244);
    }

    #[test]
    fn modes_from_the_environment() {
        let s = |v: &str| Some(v.to_string());
        assert_eq!(detect(None, None, None), Mode::Dark);
        assert_eq!(detect(s("1"), s("light"), None), Mode::Plain);
        assert_eq!(detect(s(""), None, None), Mode::Dark, "an empty NO_COLOR doesn't count");
        assert_eq!(detect(None, s("Light"), s("15;0")), Mode::Light);
        assert_eq!(detect(None, s("dark"), s("0;15")), Mode::Dark);
        assert_eq!(detect(None, None, s("0;15")), Mode::Light, "black on white");
        assert_eq!(detect(None, s("auto"), s("0;default;7")), Mode::Light);
        assert_eq!(detect(None, None, s("15;0")), Mode::Dark);
        assert_eq!(detect(None, None, s("7;8")), Mode::Dark);
        assert_eq!(detect(None, None, s("nonsense")), Mode::Dark);
    }

    #[test]
    fn light_terminals_get_the_palette_turned_round() {
        let luma = |c: [u8; 3]| c.iter().map(|v| *v as u32).sum::<u32>() / 3;
        // Text goes dark, backgrounds light, faint stays faint against the page.
        assert!(luma(for_light(FG)) < 30);
        assert!(luma(for_light(SELECTION)) > 160);
        assert!(luma(for_light(FAINT)) > luma(for_light(DIM)));
        // Accents stay themselves, dark enough to read on white.
        for c in [PINK, CYAN, AMBER, VIOLET, GREEN, RED] {
            let l = for_light(c);
            assert!(luma(l) < 110, "{c:?} -> {l:?}");
            let top = |c: [u8; 3]| (0..3).max_by_key(|&i| c[i]).unwrap();
            assert_eq!(top(l), top(c), "{c:?} -> {l:?} keeps its hue");
        }
        assert_eq!(for_light([128, 128, 128]).iter().collect::<rustc_hash::FxHashSet<_>>().len(), 1, "greys stay grey");
        // Chips (light ink on the colour, turned round) stay readable, bright greens too.
        let contrast = |a: [u8; 3], b: [u8; 3]| {
            let (x, y) = (luminance(a), luminance(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        for c in [HOT_CUE, AMBER, CYAN, track_color(TrackColor::Yellow)] {
            assert!(contrast(for_light(c), for_light([14, 14, 20])) >= 4.5, "{c:?} -> {:?}", for_light(c));
        }
        set_test_mode(Some(Mode::Plain));
        assert_eq!(rgb3(PINK), Color::Reset);
        assert!(selected().add_modifier.contains(Modifier::REVERSED));
        assert_eq!(colour_dot(TrackColor::Aqua).content, "6");
        set_test_mode(None);
        assert_eq!(colour_dot(TrackColor::Aqua).content, "●");
    }

    /// Every colour has an xterm colour from the cube or the grey ramp (not the 16 the terminal
    /// redefines), and it's no further off than the nearest in the cube.
    #[hegel::test(test_cases = 300)]
    fn every_colour_has_a_near_xterm_colour(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let c = [0; 3].map(|_| tc.draw(gs::integers::<u8>()));
        let i = xterm256(c[0], c[1], c[2]);
        assert!(i >= 16, "{c:?} is {i}");
        const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
        let rgb = |i: u8| match i {
            232.. => [8 + 10 * (i as i32 - 232); 3],
            _ => {
                let i = i as usize - 16;
                [LEVELS[i / 36], LEVELS[i / 6 % 6], LEVELS[i % 6]]
            }
        };
        let dist = |x: [i32; 3]| x.iter().zip(c).map(|(a, b)| (a - b as i32).pow(2)).sum::<i32>();
        let nearest = |v: u8| *LEVELS.iter().min_by_key(|l| (*l - v as i32).abs()).unwrap();
        assert!(dist(rgb(i)) <= dist(c.map(nearest)), "{c:?} as {i}: {:?}", rgb(i));
    }

    /// On a light terminal any colour of the palette turns round: no brighter than it was dark
    /// (so light text goes dark), greys stay grey and a lighter grey goes no lighter, and a
    /// colour that's clearly one hue keeps it.
    #[hegel::test(test_cases = 300)]
    fn any_colour_turns_round_for_a_light_terminal(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let c = [0; 3].map(|_| tc.draw(gs::integers::<u8>()));
        let out = for_light(c);
        assert!(luminance(out) <= (1.0 - luminance(c)).powi(3) + 1e-6, "{c:?} -> {out:?}");
        let (a, b) = (tc.draw(gs::integers::<u8>()), tc.draw(gs::integers::<u8>()));
        let (ga, gb) = (for_light([a; 3]), for_light([b; 3]));
        assert!(ga[0] == ga[1] && ga[1] == ga[2], "grey {a} -> {ga:?}");
        assert!(a > b || ga[0] >= gb[0], "grey {a} -> {ga:?}, {b} -> {gb:?}");
        let mut sorted = c;
        sorted.sort_unstable();
        // Clearly one hue: its strongest channel leads the next by 96 (in u16, where 237 + 96
        // doesn't saturate to 255 and let a yellow like [255, 237, 0] in).
        if sorted[2] as u16 >= sorted[1] as u16 + 96 && out != [0; 3] {
            let top = |c: [u8; 3]| (0..3).max_by_key(|&i| c[i]).unwrap();
            assert_eq!(top(out), top(c), "{c:?} -> {out:?} keeps its hue");
        }
    }

    /// The environment's say, in order. NO_COLOR set to anything means no colours, whatever
    /// else is set, and an empty one is as good as none. A theme named `light` or `dark`, in any
    /// case with spaces around, is that theme whatever COLORFGBG says; any other is as good as
    /// none. Then only COLORFGBG's last field counts: light grey and the bright colours (7,
    /// 9-15) are a light background, and anything else, or nothing, is dark.
    #[hegel::test(test_cases = 300)]
    fn the_mode_is_what_the_environment_asks_for(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let maybe = |s: String| tc.draw(gs::booleans()).then_some(s);
        let no_color = maybe(tc.draw(gs::sampled_from(&["", "1", "0"][..])).to_string());
        let named = tc.draw(gs::sampled_from(&["light", "dark", "solarized", "auto", ""][..]));
        let pad = || tc.draw(gs::sampled_from(&["", " ", "\t"][..]));
        let cased: String =
            named.chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c }).collect();
        let theme = maybe(format!("{}{cased}{}", pad(), pad()));
        let bg = tc.draw(gs::sampled_from(&["0", "6", "7", "8", "9", "15", "16", "default", ""][..]));
        let prefix = tc.draw(gs::sampled_from(&["", "15;", "0;default;", "x;"][..]));
        let colorfgbg = maybe(format!("{prefix}{bg}"));

        let got = detect(no_color.clone(), theme.clone(), colorfgbg.clone());
        let why = format!("{no_color:?} {theme:?} {colorfgbg:?}");
        if no_color.as_deref().is_some_and(|n| !n.is_empty()) {
            assert_eq!(got, Mode::Plain, "{why}");
            return;
        }
        assert_eq!(got, detect(None, theme.clone(), colorfgbg.clone()), "{why}");
        let background = detect(None, None, colorfgbg.clone());
        match (theme.is_some(), named) {
            (true, "light" | "dark") => {
                assert_eq!(got, detect(None, Some(named.into()), Some("0;15".into())), "{why}");
                assert_eq!(got, detect(None, Some(named.into()), Some("15;0".into())), "{why}");
                assert_eq!(got, if named == "light" { Mode::Light } else { Mode::Dark }, "{why}");
            }
            _ => assert_eq!(got, background, "{why}"),
        }
        let light = ["7", "9", "15"].contains(&bg);
        assert_eq!(background == Mode::Light, colorfgbg.is_some() && light, "{why}");
    }
}
