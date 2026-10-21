//! Colours. Truecolor where the terminal supports it, the nearest xterm-256 colour otherwise
//! (macOS Terminal.app, older tmux setups). The palette is made for a dark terminal; on a light
//! one each colour is drawn with its lightness turned round (see `for_light`). With NO_COLOR set
//! there are no colours at all, and what colour says is also said with bold, reverse video and
//! symbols.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use locallibrary::model::TrackColor;
use locallibrary::theme::Theme;
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

/// The player's themes: the default, then each plugin's (`[[contributes.themes]]`), by
/// `<plugin>.<name>`. Read from the plugins' manifests once, when first drawn: data, never a
/// plugin's process.
pub struct Named {
    /// `<plugin>.<name>`, as tui.json keeps it; empty for the default.
    pub key: String,
    pub title: String,
    /// Its plugin's name; empty for the default.
    pub by: String,
    pub theme: Theme,
}

pub fn themes() -> &'static [Named] {
    static THEMES: OnceLock<Vec<Named>> = OnceLock::new();
    THEMES.get_or_init(|| {
        let default = Named { key: String::new(), title: "default".into(), by: String::new(), theme: Theme::DEFAULT };
        let plugins = crate::plugins::host().into_iter().flat_map(|p| p.manifests());
        let theirs = plugins.flat_map(|m| {
            // Each checked as its manifest was.
            m.contributes.themes.iter().filter_map(move |t| {
                let theme = t.theme().ok()?;
                Some(Named { key: format!("{}.{}", m.id, t.name), title: t.title.clone(), by: m.name.clone(), theme })
            })
        });
        std::iter::once(default).chain(theirs).collect()
    })
}

/// The theme drawn in, by its place in [`themes`].
static CURRENT: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
thread_local! {
    /// A test's own theme, so tests drawing in different themes can run side by side.
    static TEST_THEME: std::cell::Cell<Option<&'static Theme>> = const { std::cell::Cell::new(None) };
}

/// The theme the player draws in: every colour of its chrome, its waveforms and the shared ones.
pub fn current() -> &'static Theme {
    #[cfg(test)]
    if let Some(t) = TEST_THEME.with(std::cell::Cell::get) {
        return t;
    }
    themes().get(CURRENT.load(Ordering::Relaxed)).map_or(&Theme::DEFAULT, |n| &n.theme)
}

/// Draw in the theme `key` names (`<plugin>.<name>`), or the default without one or for one no
/// plugin has (any more): the key of the theme drawn in now, `None` for the default.
pub fn select(key: Option<&str>) -> Option<&'static str> {
    let at = key.and_then(|k| themes().iter().position(|n| !n.key.is_empty() && n.key == k)).unwrap_or(0);
    CURRENT.store(at, Ordering::Relaxed);
    chosen()
}

/// The key of the theme drawn in now, `None` for the default.
pub fn chosen() -> Option<&'static str> {
    themes().get(CURRENT.load(Ordering::Relaxed)).map(|n| n.key.as_str()).filter(|k| !k.is_empty())
}

/// Draw this thread's frames in `theme` (`None`: as the player would).
#[cfg(test)]
pub fn set_test_theme(theme: Option<&'static Theme>) {
    TEST_THEME.with(|t| t.set(theme));
}

/// The palette the player's waveforms, visualisers and cover art draw in, the theme's, for the
/// mode it draws in.
pub fn palette() -> Palette<'static> {
    Palette { paint: rgb3, image, plain: plain(), ..Palette::of(current()) }
}

pub fn fg(c: [u8; 3]) -> Style {
    Style::new().fg(rgb3(c))
}

pub fn text() -> Style {
    fg(current().text)
}

pub fn dim() -> Style {
    fg(current().dim)
}

pub fn faint() -> Style {
    if plain() { Style::new().add_modifier(Modifier::DIM) } else { fg(current().faint) }
}

pub fn bold(c: [u8; 3]) -> Style {
    fg(c).add_modifier(Modifier::BOLD)
}

pub fn border(focused: bool) -> Style {
    if focused { fg(current().info) } else { faint() }
}

/// Rounded borders; without colours, the focused pane's are heavy.
pub fn border_type(focused: bool) -> BorderType {
    if focused && plain() { BorderType::Thick } else { BorderType::Rounded }
}

pub fn selected() -> Style {
    if plain() {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::new().bg(rgb3(current().selection)).add_modifier(Modifier::BOLD)
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
        Span::styled("●", fg(current().track_color(c)))
    }
}

/// A key hint, e.g. the `q` in "q quit".
pub fn key() -> Style {
    bold(current().accent)
}

#[cfg(test)]
mod tests;
