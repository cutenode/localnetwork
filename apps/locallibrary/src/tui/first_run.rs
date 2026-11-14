//! What someone new sees. With no drive open, the library says what to do: plug one in (the volume
//! watcher opens it as it arrives), or one of at most three next steps. The first time a version
//! opens a library, the footer names the keys to start with until a key or a click. What's been
//! seen is kept in tui.json, in the store; it isn't a preference, so it stays out of `settings
//! export`. Nothing here shows until the first read of the drives has answered: a drive that's
//! there mustn't be said to be missing for a moment.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph};

use super::app::{App, VERSION, View};
use super::choices::Hints;
use super::edits::{Modal, Prompt, PromptFor};
use super::icons;
use super::keymap::Act;
use super::theme::{self, bold, dim, fg, text};

/// The keys a first library names.
const STARTERS: [(&str, &str); 4] = [("enter", "play"), ("e", "actions"), (":", "commands"), ("?", "help")];

impl App {
    /// What can be done with no drive: a folder can always be opened; the store's snapshots while
    /// it has any; signing in until someone is.
    pub fn next_steps(&self) -> Vec<(Act, &'static str)> {
        let mut steps = vec![(Act::OpenFolder, "open a folder")];
        if !self.library_names.is_empty() {
            steps.push((Act::Snapshots, "browse snapshots"));
        }
        if cfg!(feature = "account") && self.profile.account.handle().is_none() {
            steps.push((Act::SignIn, "sign in"));
        }
        steps
    }

    /// Whether the library shows the empty state.
    pub(super) fn empty(&self) -> bool {
        self.view == View::Library && self.loaded.is_none() && self.loading.is_none() && !self.scanning()
    }

    /// Enter on a step.
    pub(super) fn take_step(&mut self) {
        let steps = self.next_steps();
        if let Some(&(act, _)) = self.steps.selected().and_then(|i| steps.get(i)) {
            self.perform(act, None);
        }
    }

    /// The first read of the drives answered: with nothing to show, a saved player mode opens Browse
    /// over itself, so the empty state shows. Loading a track brings the deck back, as ever.
    pub(super) fn first_answer(&mut self) {
        if self.loaded.is_none() && self.loading.is_none() && self.now.is_none() {
            self.browsing = self.deck_layout;
        }
    }

    /// A library's open: its first this version, the footer says where to start.
    pub(super) fn first_open(&mut self) {
        if self.intro_seen.as_deref() != Some(VERSION) {
            self.intro_seen = Some(VERSION.to_string());
            self.intro = self.hints != Hints::Off;
        }
    }

    /// Ask for the folder to open.
    pub(super) fn open_folder(&mut self) {
        let (label, text, root) = ("Folder holding an export".into(), String::new(), Default::default());
        self.modal = Some(Modal::Prompt(Prompt { label, text, purpose: PromptFor::OpenFolder, root }));
    }

    /// The Drives view, on the store's snapshots.
    pub(super) fn browse_snapshots(&mut self) {
        self.set_view(View::Drives);
        self.show_drive_pane(super::app::DrivePane::Snapshots);
    }

    /// The Publish view, ready for the handle.
    pub(super) fn sign_in(&mut self) {
        self.set_view(View::Profile);
        self.perform(Act::ProfileAccount, None);
    }
}

/// The footer's line for a first library.
pub(super) fn intro_line() -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(" new here?  ", fg(theme::current().accent))];
    for (k, label) in STARTERS {
        spans.push(Span::styled(k, theme::key()));
        spans.push(Span::styled(format!(" {label}  "), dim()));
    }
    spans
}

/// The library with no drive open, inside the track list's `frame`.
pub(super) fn draw_empty(f: &mut Frame, app: &mut App, frame: Block<'static>, area: Rect) {
    let inner = frame.inner(area);
    f.render_widget(frame, area);
    let found = app.drives.len();
    let head = match found {
        0 => "Plug in a USB drive with a rekordbox export.".to_string(),
        n => format!("{n} plugged in, with no rekordbox export."),
    };
    let about = vec![
        Line::raw(""),
        Line::styled("No rekordbox drive open.", bold(theme::current().text)),
        Line::raw(""),
        Line::styled(head, dim()),
        Line::styled("It opens as soon as it's here.", dim()),
        Line::raw(""),
    ];
    let steps = app.next_steps();
    let w = steps.iter().map(|(_, s)| s.len() as u16).max().unwrap_or(0) + 4;
    let [top, list] = Layout::vertical([Constraint::Length(about.len() as u16), Constraint::Min(0)]).areas(inner);
    f.render_widget(Paragraph::new(about).alignment(Alignment::Center), top);
    let [_, list, _] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(w), Constraint::Fill(1)]).areas(list);
    let items: Vec<ListItem> = steps.iter().map(|(_, s)| ListItem::new(Span::styled(*s, text()))).collect();
    let rows = List::new(items)
        .highlight_style(theme::selected())
        .highlight_symbol(Span::styled(icons::SELECTED.get(), fg(theme::current().accent)));
    app.areas.steps = list;
    f.render_stateful_widget(rows, list, &mut app.steps);
}

#[cfg(test)]
mod tests;
