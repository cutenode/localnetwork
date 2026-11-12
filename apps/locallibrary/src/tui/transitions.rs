//! Short fades, for a track that's just loaded and a panel that's just opened. They're only ever
//! decoration: with less motion asked for there are none, and they need colours to fade, 24-bit
//! ones as what's between two colours is drawn as it is.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tachyonfx::fx::{self};
use tachyonfx::{Effect, Interpolation};

use super::theme::{self, Tier};

/// As long as a fade takes: long enough to see what changed, short enough not to be waited for.
const FADE_MS: u32 = 160;

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
}

impl Transitions {
    /// Whether a frame has to come soon for a fade. Once one's over this is false, so the player
    /// goes back to its slow tick.
    pub fn active(&self) -> bool {
        self.running.is_some()
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

    /// Fade this frame as far as the time since the last one takes it.
    pub fn draw(&mut self, buf: &mut Buffer) {
        let Some((_, _, last)) = &mut self.running else { return };
        let since = std::mem::replace(last, Instant::now()).elapsed();
        self.step(since, buf);
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
