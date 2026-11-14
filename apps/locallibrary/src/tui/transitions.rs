//! Short fades, for a track that's just loaded and a panel that's just opened, and the wash of the
//! deck's overview as the warning of a track's end starts. They're only ever decoration: with less
//! motion asked for there are none. Fades need colours to fade, 24-bit ones as what's between two
//! colours is drawn as it is; the wash blends into the warning colour in 24-bit, and in fewer
//! colours snaps to it and back.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use tachyonfx::fx::{self};
use tachyonfx::{Effect, Interpolation};

use super::theme::{self, Tier};

/// As long as a fade takes: long enough to see what changed, short enough not to be waited for.
const FADE_MS: u32 = 160;
/// One of the wash's two pulses, into the warning colour and back.
const PULSE: Duration = Duration::from_millis(360);

/// What's on screen that a fade follows: a change here starts one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shown {
    /// The deck has the screen.
    pub deck: bool,
    pub details: bool,
    pub crate_panel: bool,
    pub help: bool,
    /// The playing track, once it has a waveform.
    pub track: Option<u64>,
}

/// The fade under way.
#[derive(Default)]
pub struct Transitions {
    /// Off for tests' apps, whose frames are looked at as they're drawn.
    pub enabled: bool,
    shown: Option<Shown>,
    running: Option<(Effect, Rect, Instant)>,
    /// The warning of a track's end was on in the last frame.
    warned: bool,
    /// The wash under way: over what, how far into it, and when it was last drawn.
    wash: Option<(Rect, Duration, Instant)>,
}

impl Transitions {
    /// Whether a frame has to come soon for a fade. Once one's over this is false, so the player
    /// goes back to its slow tick.
    pub fn active(&self) -> bool {
        self.running.is_some() || self.wash.is_some()
    }

    /// Whether the warning of the track's end is on, with the track's overview drawn at
    /// `overview` this frame: a wash starts each time it comes on, and stops if it goes off first
    /// (a seek back).
    pub fn follow_warning(&mut self, warning: bool, overview: Rect) {
        let was = std::mem::replace(&mut self.warned, warning);
        let starts = warning && !was;
        if !warning {
            self.wash = None;
        } else if starts && self.enabled && !theme::reduced_motion() && !theme::plain() {
            self.wash = Some((overview, Duration::ZERO, Instant::now()));
        } else if let Some((area, _, _)) = &mut self.wash {
            *area = overview;
        }
    }

    /// What's `shown` now, over `area`: start a fade if it's changed since the last frame.
    pub fn follow(&mut self, shown: Shown, area: Rect) {
        let was = self.shown.replace(shown);
        let changed = was.is_some_and(|w| w != shown);
        if !changed || !self.enabled || theme::reduced_motion() || theme::tier() != Tier::Truecolor || theme::plain() {
            return;
        }
        let [r, g, b] = theme::page();
        let page = ratatui::style::Color::Rgb(r, g, b);
        // Text and blocks only: what's behind them is the terminal's own, which has no colour to
        // fade from.
        let fade = fx::fade_from_fg(page, (FADE_MS, Interpolation::QuadOut));
        self.running = Some((fade, area, Instant::now()));
    }

    /// Fade and wash this frame as far as the time since the last one takes them.
    pub fn draw(&mut self, buf: &mut Buffer) {
        if let Some((_, _, last)) = &mut self.wash {
            let since = std::mem::replace(last, Instant::now()).elapsed();
            self.step_wash(since, buf);
        }
        let Some((_, _, last)) = &mut self.running else { return };
        let since = std::mem::replace(last, Instant::now()).elapsed();
        self.step(since, buf);
    }

    /// Two pulses into the warning colour and back, `by` further on: blended in 24-bit, snapped
    /// to it at the height of each in fewer colours.
    fn step_wash(&mut self, by: Duration, buf: &mut Buffer) {
        let Some((area, at, _)) = &mut self.wash else { return };
        *at += by;
        if *at >= 2 * PULSE {
            self.wash = None;
            return;
        }
        // 0 to 1 and back over a pulse.
        let into = at.as_secs_f32() % PULSE.as_secs_f32() / PULSE.as_secs_f32();
        let t = 1.0 - (2.0 * into - 1.0).abs();
        let warn = theme::rgb3(theme::current().critical);
        for pos in area.intersection(buf.area).positions() {
            let cell = &mut buf[pos];
            cell.fg = match (cell.fg, warn) {
                (Color::Rgb(r, g, b), Color::Rgb(wr, wg, wb)) => {
                    let mix =
                        |from: u8, to: u8| (f32::from(from) + (f32::from(to) - f32::from(from)) * t).round() as u8;
                    Color::Rgb(mix(r, wr), mix(g, wg), mix(b, wb))
                }
                _ if t >= 0.5 => warn,
                (fg, _) => fg,
            };
        }
    }

    fn step(&mut self, by: Duration, buf: &mut Buffer) {
        let Some((fade, area, _)) = &mut self.running else { return };
        let (by, area) = (tachyonfx::Duration::from_millis(by.as_millis() as u32), *area);
        fade.process(by, buf, area);
        if fade.done() {
            self.running = None;
        }
    }
}

#[cfg(test)]
mod tests;
