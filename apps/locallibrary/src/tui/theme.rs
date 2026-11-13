//! Colours. A theme's colours are fitted once to what the terminal is (see `fit`): its colour tier
//! (`tier`), its mode and its real background, read at startup (`terminal`). After that a colour
//! only has to be turned into what the terminal draws, which is arithmetic and never a search, so
//! a frame costs the same whatever the tier. On a light terminal the palette is `Theme::LIGHT`, and
//! with 16 colours the roles of `ansi`. With NO_COLOR set there are no colours at all, and what
//! colour says is also said with bold, reverse video and symbols, as it is where 16 colours can't
//! put one colour behind another.

mod ansi;
mod fit;
mod tier;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use locallibrary::model::TrackColor;
use locallibrary::theme::Theme;
use loclib_waveform::Palette;
use loclib_waveform::color::fade as faded;
pub use loclib_waveform::color::{better_ink, contrast, gradient, luminance, mix, scale};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::BorderType;

use self::fit::{DARK_PAGE, Fit, LIGHT_PAGE, MARK, legible, resolve};
pub use self::tier::Tier;
use self::tier::{ansi16, from_env, xterm256};
use super::icons;
use super::terminal::{Answers, Colours};

/// How the player draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The palette made for a dark background.
    Dark,
    /// The palette made for a light background.
    Light,
    /// No colours (NO_COLOR).
    Plain,
}

static MODE: AtomicU8 = AtomicU8::new(0);
static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);
/// `LOCALLIBRARY_COLOR`, and what's asked for in the settings, by `Tier as u8 + 1`.
static FORCED_TIER: AtomicU8 = AtomicU8::new(0);
static CHOSEN_TIER: AtomicU8 = AtomicU8::new(0);
/// What the terminal or its environment says.
static DETECTED_TIER: AtomicU8 = AtomicU8::new(0);
/// The one of those that counts by `Tier as u8`, kept apart as every coloured cell reads it, and
/// worked out when first asked for unless the player's start has (tests draw as the environment
/// says).
static TIER: AtomicU8 = AtomicU8::new(UNSETTLED);
const UNSETTLED: u8 = u8::MAX;
/// Whether LOCALLIBRARY_THEME named a mode, which the terminal's own background doesn't overrule.
static MODE_NAMED: AtomicBool = AtomicBool::new(false);
/// The terminal's real colours, once it has said.
static SAID: OnceLock<Colours> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static TEST_REDUCED_MOTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A test's own mode, so tests drawing in different modes can run side by side.
    static TEST_MODE: std::cell::Cell<Option<Mode>> = const { std::cell::Cell::new(None) };
    /// A test's own colour tier, whatever terminal runs it.
    static TEST_TIER: std::cell::Cell<Option<Tier>> = const { std::cell::Cell::new(None) };
    /// A test's terminal: the colours it says it has.
    static TEST_SAID: std::cell::Cell<Option<Colours>> = const { std::cell::Cell::new(None) };
    /// A test's own theme, so tests drawing in different themes can run side by side.
    static TEST_THEME: std::cell::Cell<Option<&'static Theme>> = const { std::cell::Cell::new(None) };
    /// What those come to, worked out as each is set.
    static TEST_CURRENT: std::cell::Cell<Option<&'static Theme>> = const { std::cell::Cell::new(None) };
}

/// Read how to draw from the environment, as the player starts (tests draw dark unless they
/// say otherwise). The terminal's own answers, when it gives any, come after ([`answered`]).
pub fn init() {
    let var = |k: &str| std::env::var(k).ok();
    let mode = detect(var("NO_COLOR"), var("LOCALLIBRARY_THEME"), var("COLORFGBG"));
    MODE.store(mode as u8 + 1, Ordering::Relaxed);
    MODE_NAMED.store(names_a_mode(var("LOCALLIBRARY_THEME").as_deref()), Ordering::Relaxed);
    let reduced = var("LOCALLIBRARY_REDUCED_MOTION").is_some_and(|v| !matches!(v.trim(), "" | "0" | "false" | "no"));
    REDUCED_MOTION.store(reduced, Ordering::Relaxed);
    FORCED_TIER.store(tier_code(var("LOCALLIBRARY_COLOR").and_then(|t| Tier::parse(&t))), Ordering::Relaxed);
    DETECTED_TIER.store(from_env(var) as u8 + 1, Ordering::Relaxed);
    settle_tier();
}

/// What the terminal answered as the player started: its colour tier and real colours, which also
/// settle the mode unless LOCALLIBRARY_THEME named one. Before the first frame.
pub fn answered(a: &Answers) {
    if a.truecolor {
        DETECTED_TIER.store(Tier::Truecolor as u8 + 1, Ordering::Relaxed);
        settle_tier();
    }
    let Some(said) = a.colours else { return };
    let _ = SAID.set(said);
    let mode = mode_from(mode(), MODE_NAMED.load(Ordering::Relaxed), said);
    MODE.store(mode as u8 + 1, Ordering::Relaxed);
}

/// The mode with what the terminal says of itself: its background decides, unless a mode was named.
/// (It says nothing with colours off: it isn't asked.)
fn mode_from(mode: Mode, named: bool, said: Colours) -> Mode {
    match named {
        true => mode,
        false if luminance(said.background) > luminance(said.foreground) => Mode::Light,
        false => Mode::Dark,
    }
}

fn names_a_mode(theme: Option<&str>) -> bool {
    theme.is_some_and(|t| matches!(t.trim().to_ascii_lowercase().as_str(), "light" | "dark"))
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

fn tier_code(tier: Option<Tier>) -> u8 {
    tier.map_or(0, |t| t as u8 + 1)
}

/// Works out the colour tier drawn in: `LOCALLIBRARY_COLOR`, else the settings', else what was
/// detected.
fn settle_tier() {
    if DETECTED_TIER.load(Ordering::Relaxed) == 0 {
        DETECTED_TIER.store(from_env(|k| std::env::var(k).ok()) as u8 + 1, Ordering::Relaxed);
    }
    let forced = FORCED_TIER.load(Ordering::Relaxed);
    let chosen = CHOSEN_TIER.load(Ordering::Relaxed);
    let code = if forced != 0 {
        forced
    } else if chosen != 0 {
        chosen
    } else {
        DETECTED_TIER.load(Ordering::Relaxed)
    };
    TIER.store(code - 1, Ordering::Relaxed);
}

pub fn tier() -> Tier {
    #[cfg(test)]
    if let Some(t) = TEST_TIER.with(std::cell::Cell::get) {
        return t;
    }
    if TIER.load(Ordering::Relaxed) == UNSETTLED {
        settle_tier();
    }
    Tier::ALL[usize::from(TIER.load(Ordering::Relaxed))]
}

/// Whether LOCALLIBRARY_COLOR holds the player to a tier, so there's no need to ask the terminal.
pub fn tier_forced() -> bool {
    FORCED_TIER.load(Ordering::Relaxed) != 0
}

/// Draw in the colour tier the settings name, from the next frame (`None`: the one detected).
pub fn choose_tier(tier: Option<Tier>) {
    CHOSEN_TIER.store(tier_code(tier), Ordering::Relaxed);
    settle_tier();
}

/// The tier the settings name.
pub fn chosen_tier() -> Option<Tier> {
    Tier::ALL.get(usize::from(CHOSEN_TIER.load(Ordering::Relaxed)).checked_sub(1)?).copied()
}

/// Work out again what this thread's tests draw in.
#[cfg(test)]
fn refit_test() {
    use std::cell::Cell;
    let set = TEST_MODE.with(Cell::get).is_some()
        || TEST_TIER.with(Cell::get).is_some()
        || TEST_SAID.with(Cell::get).is_some()
        || TEST_THEME.with(Cell::get).is_some();
    let drawn_in = set.then(|| {
        // Not `themes()` for the default: that would read plugins before the test has set them up.
        let chosen = CURRENT.load(Ordering::Relaxed);
        let plugins = || if chosen == 0 { &Theme::DEFAULT } else { &themes()[chosen].theme };
        let theme = TEST_THEME.with(Cell::get).unwrap_or_else(plugins);
        &*Box::leak(Box::new(resolve(theme, &fit_now())))
    });
    TEST_CURRENT.with(|c| c.set(drawn_in));
}

/// Ask for less motion on this thread.
#[cfg(test)]
pub fn set_test_reduced_motion(on: bool) {
    TEST_REDUCED_MOTION.with(|m| m.set(on));
}

/// Draw this thread's frames in `mode` (`None`: as the player would).
#[cfg(test)]
pub fn set_test_mode(mode: Option<Mode>) {
    TEST_MODE.with(|m| m.set(mode));
    refit_test();
}

/// Draw this thread's frames in the colour tier `tier` (`None`: as the player would).
#[cfg(test)]
pub fn set_test_tier(tier: Option<Tier>) {
    TEST_TIER.with(|t| t.set(tier));
    refit_test();
}

/// Draw this thread's frames as on a terminal that says it has `said` (`None`: one that says
/// nothing).
#[cfg(test)]
pub fn set_test_terminal(said: Option<Colours>) {
    TEST_SAID.with(|s| s.set(said));
    refit_test();
}

/// Draw this thread's frames in 24-bit colour.
#[cfg(all(test, feature = "account"))]
pub fn set_test_truecolor() {
    set_test_tier(Some(Tier::Truecolor));
}

/// No colours: say it with bold, reverse video and symbols instead.
pub fn plain() -> bool {
    mode() == Mode::Plain
}

/// Colours, but only the 16 the terminal's scheme defines.
fn sixteen() -> bool {
    tier() == Tier::Colors16 && !plain()
}

/// No colour behind anything: without colours, or with 16 whose every pairing can't be trusted to
/// read. What's behind a row or a chip is said with reverse video and symbols, and quieter text
/// with the dim attribute.
pub fn flat() -> bool {
    plain() || tier() == Tier::Colors16
}

/// LOCALLIBRARY_REDUCED_MOTION: redraw only as often as when nothing moves, and leave out
/// motion that's only decoration (the spinning record, the pulsing logo, cue flashes).
pub fn reduced_motion() -> bool {
    #[cfg(test)]
    if TEST_REDUCED_MOTION.with(std::cell::Cell::get) {
        return true;
    }
    REDUCED_MOTION.load(Ordering::Relaxed)
}

/// What the terminal said of its colours, if it did.
fn said() -> Option<Colours> {
    #[cfg(test)]
    if let Some(s) = TEST_SAID.with(std::cell::Cell::get) {
        return Some(s);
    }
    SAID.get().copied()
}

/// The terminal's background: what it said, else what's usual for the mode.
pub fn page() -> [u8; 3] {
    said().map_or(if mode() == Mode::Light { LIGHT_PAGE } else { DARK_PAGE }, |s| s.background)
}

/// What everything is fitted to now.
fn fit_now() -> Fit {
    Fit { mode: mode(), tier: tier(), page: page(), foreground: said().map(|s| s.foreground) }
}

/// A colour of the palette as the terminal should draw it, and nothing without colours.
pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    match (mode(), tier()) {
        (Mode::Plain, _) => Color::Reset,
        (mode, Tier::Colors16) => ansi::readable(ansi16([r, g, b]), mode),
        (_, Tier::Colors256) => Color::Indexed(xterm256(r, g, b)),
        (_, Tier::Truecolor) => Color::Rgb(r, g, b),
    }
}

pub fn rgb3(c: [u8; 3]) -> Color {
    rgb(c[0], c[1], c[2])
}

/// A colour that is what it is whatever the terminal's background (cover art).
pub fn image(c: [u8; 3]) -> Color {
    exact(c)
}

fn exact(c: [u8; 3]) -> Color {
    match tier() {
        Tier::Truecolor => Color::Rgb(c[0], c[1], c[2]),
        Tier::Colors256 => Color::Indexed(xterm256(c[0], c[1], c[2])),
        Tier::Colors16 => Color::Indexed(ansi16(c)),
    }
}

/// `c` faded towards the terminal's background: `f` = 1 gives `c`, 0 the background.
pub fn fade(c: [u8; 3], f: f32) -> [u8; 3] {
    faded(page(), c, f)
}

/// Colours from a drive (a cue's own, a waveform's) are made for an almost black background, which
/// reads them all as they are, and with 16 colours how they read is the scheme's.
fn markers_read_as_they_are() -> bool {
    flat() || page().iter().all(|v| *v < 64)
}

/// A colour from a drive as it can be read on the background.
pub fn marker(c: [u8; 3]) -> [u8; 3] {
    if markers_read_as_they_are() { c } else { legible(c, &fit_now(), current().text, MARK) }
}

/// What reads best on `paper`: the theme's ink, or its text colour.
pub fn ink_on(paper: [u8; 3]) -> [u8; 3] {
    better_ink(current().ink, current().text, paper)
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

/// [`themes`] fitted to the terminal, a list for each tier, worked out when first drawn: every
/// colour search there is happens here, once.
static FITTED: [OnceLock<Vec<Theme>>; 3] = [const { OnceLock::new() }; 3];

/// The theme the player draws in, fitted to the terminal: every colour of its chrome, its waveforms
/// and the shared ones.
pub fn current() -> &'static Theme {
    #[cfg(test)]
    if let Some(t) = TEST_CURRENT.with(std::cell::Cell::get) {
        return t;
    }
    let fitted = FITTED[tier() as usize].get_or_init(|| {
        let fit = fit_now();
        themes().iter().map(|n| resolve(&n.theme, &fit)).collect()
    });
    &fitted[CURRENT.load(Ordering::Relaxed)]
}

/// Draw in the theme `key` names (`<plugin>.<name>`), or the default without one or for one no
/// plugin has (any more): the key of the theme drawn in now, `None` for the default.
pub fn select(key: Option<&str>) -> Option<&'static str> {
    let at = key.and_then(|k| themes().iter().position(|n| !n.key.is_empty() && n.key == k)).unwrap_or(0);
    CURRENT.store(at, Ordering::Relaxed);
    #[cfg(test)]
    refit_test();
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
    refit_test();
}

/// The palette the player's waveforms, visualisers and cover art draw in, the theme's, for the
/// mode and tier it draws in.
pub fn palette() -> Palette<'static> {
    // Settled here, not for each of a waveform's columns.
    let marker = if markers_read_as_they_are() { |c| c } else { marker };
    Palette { paint: rgb3, image, marker, plain: plain(), flat: flat(), background: page(), ..Palette::of(current()) }
}

pub fn fg(c: [u8; 3]) -> Style {
    Style::new().fg(rgb3(c))
}

pub fn text() -> Style {
    fg(current().text)
}

pub fn dim() -> Style {
    if sixteen() { Style::new().add_modifier(Modifier::DIM) } else { fg(current().dim) }
}

pub fn faint() -> Style {
    if flat() { Style::new().add_modifier(Modifier::DIM) } else { fg(current().faint) }
}

/// A line that isn't read: a rail, an idle pane's border.
pub fn rule() -> Style {
    if flat() { Style::new().add_modifier(Modifier::DIM) } else { fg(current().rule) }
}

pub fn bold(c: [u8; 3]) -> Style {
    fg(c).add_modifier(Modifier::BOLD)
}

pub fn border(focused: bool) -> Style {
    if focused { fg(current().info) } else { rule() }
}

/// Rounded borders; with nothing but the border to show focus by, the focused pane's are heavy.
pub fn border_type(focused: bool) -> BorderType {
    if focused && flat() { BorderType::Thick } else { BorderType::Rounded }
}

pub fn selected() -> Style {
    if flat() {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::new().bg(rgb3(current().selection)).add_modifier(Modifier::BOLD)
    }
}

/// The selection in a pane without focus: there, but quieter than the one that has it.
pub fn unfocused() -> Style {
    if flat() { Style::new().add_modifier(Modifier::REVERSED | Modifier::DIM) } else { behind(current().idle) }
}

/// Bold text on `paper` (the active tab, chips), in what reads best on it. With 16 colours it's
/// `paper` in reverse video, so the text is the terminal's own background, which the scheme made
/// to read with its colours; without colours, reverse video alone.
pub fn chip(paper: [u8; 3]) -> Style {
    let style = Style::new().add_modifier(Modifier::BOLD);
    match (plain(), flat()) {
        (true, _) => style.add_modifier(Modifier::REVERSED),
        (_, true) => style.fg(rgb3(paper)).add_modifier(Modifier::REVERSED),
        _ => style.fg(rgb3(ink_on(paper))).bg(rgb3(paper)),
    }
}

/// A colour behind something that a symbol already marks (a marked row, the selection in a
/// pane without focus): nothing where there's no colour behind things.
pub fn behind(c: [u8; 3]) -> Style {
    if flat() { Style::new() } else { Style::new().bg(rgb3(c)) }
}

/// A track's rekordbox colour as a dot; where a hue alone can't be trusted to read, its number
/// (1 pink to 8 purple, as the colour menu numbers them), in its colour if there are any.
pub fn colour_dot(c: TrackColor) -> Span<'static> {
    if flat() {
        Span::styled(c.id().to_string(), bold(current().track_color(c)))
    } else {
        Span::styled(icons::DOT.get(), fg(current().track_color(c)))
    }
}

/// A key hint, e.g. the `Q` in "Q quit".
pub fn key() -> Style {
    bold(current().accent)
}

#[cfg(test)]
mod screens;
#[cfg(test)]
mod tests;
