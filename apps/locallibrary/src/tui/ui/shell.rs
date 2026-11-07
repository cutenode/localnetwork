//! How the screen is shared out: what's around every view, and what goes first as a terminal
//! shrinks. Side panes collapse to their title rows, then the details pane goes, then one pane
//! has the room and the rest are title rows; below that there's only the deck.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::tui::theme::dim;

/// How much of the screen's furniture there's room for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Density {
    #[default]
    Full,
    /// Of panes stacked in a column, one has the room and the others show their titles.
    Collapsed,
    /// As `Collapsed`, and the details pane only shows while it has focus.
    NoDetails,
    /// One pane has the room, and every other shows its title.
    Compact,
    /// The deck, and what size it would take to show the rest.
    DeckOnly,
}

/// The sizes each density takes, roomiest first: a terminal gets the first it's at least as big as
/// in both directions.
const TIERS: [(Density, u16, u16); 4] =
    [(Density::Full, 120, 34), (Density::Collapsed, 100, 28), (Density::NoDetails, 80, 24), (Density::Compact, 60, 20)];

/// The smallest terminal that shows anything but the deck.
const SMALLEST: (u16, u16) = (TIERS[3].1, TIERS[3].2);

/// From this tall, the context line has its own row; under it, the header carries it.
const CONTEXT_ROW: u16 = 30;
/// Rows of the deck strip: its line and the waveform's markers and body.
const STRIP: u16 = 3;
const STRIP_FULL: u16 = 4;

impl Density {
    pub fn of(width: u16, height: u16) -> Density {
        let fits = |(_, w, h): &(Density, u16, u16)| width >= *w && height >= *h;
        TIERS.iter().find(|t| fits(t)).map_or(Density::DeckOnly, |t| t.0)
    }

    /// Whether the details pane shows without focus.
    pub fn shows_details(self) -> bool {
        matches!(self, Density::Full | Density::Collapsed)
    }
}

/// Where everything goes.
pub(super) struct Shell {
    pub density: Density,
    pub header: Rect,
    /// No rows when the header carries it.
    pub context: Rect,
    /// What the view draws in. With only the deck, what's left under it.
    pub body: Rect,
    pub strip: Rect,
    pub status: Rect,
    pub footer: Rect,
}

impl Shell {
    pub fn of(area: Rect) -> Shell {
        let density = Density::of(area.width, area.height);
        if density == Density::DeckOnly {
            let strip_h = STRIP.min(area.height.saturating_sub(1)).max(1);
            let [strip, body] = Layout::vertical([Constraint::Length(strip_h), Constraint::Min(0)]).areas(area);
            let none = Rect::default();
            return Shell { density, header: none, context: none, body, strip, status: none, footer: none };
        }
        let context_h = u16::from(area.height >= CONTEXT_ROW);
        let strip_h = if density == Density::Full { STRIP_FULL } else { STRIP };
        let [header, context, body, strip, status, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(context_h),
            Constraint::Min(0),
            Constraint::Length(strip_h),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        Shell { density, header, context, body, strip, status, footer }
    }

    /// Where popups go: anywhere above the deck, or with only the deck, what's left under it.
    pub fn over(&self) -> Rect {
        if self.density == Density::DeckOnly { self.body } else { Rect { height: self.strip.y, ..self.header } }
    }
}

/// What's said under the deck when there's no room for more.
pub(super) fn draw_too_small(f: &mut Frame, area: Rect) {
    let (w, h) = SMALLEST;
    let long = format!("Needs a {w}x{h} terminal");
    let text = if area.width as usize >= long.len() { long } else { format!("{w}x{h}") };
    let row = Rect { y: area.y + area.height.saturating_sub(1) / 2, height: area.height.min(1), ..area };
    f.render_widget(Paragraph::new(Line::styled(text, dim())).alignment(Alignment::Center), row);
}

#[cfg(test)]
mod tests;
