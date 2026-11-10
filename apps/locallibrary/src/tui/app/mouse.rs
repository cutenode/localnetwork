//! Clicks, the wheel and drags, and where the panes were drawn for them to land on.

use std::time::{Duration, Instant};

use locallibrary::query::SortField;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use crate::tui::browse::{self};
use crate::tui::edits::{Menu, Modal};
use crate::tui::ui::Density;

use super::*;

/// Where things were drawn last frame, for mouse hit-testing.
#[derive(Default)]
pub struct Areas {
    pub tabs: Vec<(View, Rect)>,
    /// How much room the panes had.
    pub density: Density,
    /// The titles of what's beside the Drives view's list.
    pub drive_tabs: Vec<(DrivePane, Rect)>,
    pub drives: Rect,
    pub playlists: Rect,
    pub tracks: Rect,
    /// The track list's column headers, to sort by clicking one.
    pub columns: Vec<(SortField, Rect)>,
    pub details: Rect,
    /// The rows of the details panel's list of matches.
    pub matches: Rect,
    pub crate_panel: Rect,
    /// The rows of the crate.
    pub crate_rows: Rect,
    /// The actions menu, when open.
    pub menu: Rect,
    pub overview: Rect,
    /// Every spectrum drawn: width and whether it's mirrored (each needs its own bands).
    pub spectra: Vec<(u16, bool)>,
    pub drive_list: Rect,
    /// What's beside the Drives view's list.
    pub drive_side: Rect,
    /// The rows of the Drives view's snapshots.
    pub history: Rect,
    /// The rows of the Drives view's list of what players recorded.
    pub played: Rect,
    /// The Prepare view's list.
    pub prepare: Rect,
}

/// A row clicked: its view, its pane (none for the full-screen player's matches) and its index.
pub(super) type ClickedRow = (View, Option<Focus>, usize);

impl App {
    pub fn on_mouse(&mut self, m: MouseEvent) {
        self.follow_queue(); // as for a key
        let p = Position::new(m.column, m.row);
        if self.help {
            match m.kind {
                MouseEventKind::ScrollUp => self.help_scroll = self.help_scroll.saturating_sub(3),
                MouseEventKind::ScrollDown => self.help_scroll = self.help_scroll.saturating_add(3),
                MouseEventKind::Down(_) => self.help = false,
                _ => {}
            }
            return;
        }
        if self.show_log {
            match m.kind {
                MouseEventKind::ScrollUp => self.log_scroll = self.log_scroll.saturating_sub(3),
                MouseEventKind::ScrollDown => self.log_scroll = self.log_scroll.saturating_add(3),
                MouseEventKind::Down(_) => self.show_log = false,
                _ => {}
            }
            return;
        }
        if self.deck.outputs.is_some() {
            if matches!(m.kind, MouseEventKind::Down(_)) {
                self.deck.outputs = None;
            }
            return;
        }
        if self.modal.is_some() {
            self.on_modal_mouse(m);
            return;
        }
        // The tabs and the deck are the same everywhere, and with only the deck, the panes aren't
        // there to click.
        let around = self.areas.density == Density::DeckOnly
            || self.areas.overview.contains(p)
            || self.areas.tabs.iter().any(|(_, r)| r.contains(p));
        if self.view == View::Profile && !around && crate::tui::profile::on_mouse(self, m) {
            return;
        }
        if self.view == View::Mixes && !around && crate::tui::mix::on_mouse(self, m) {
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let delta = if m.kind == MouseEventKind::ScrollUp { -3 } else { 3 };
                if let Some(f) = self.pane_at(p).filter(|f| self.picking.is_none() || *f == Focus::Playlists) {
                    self.point_at(f);
                }
                self.move_selection(delta);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // A second click is one on the row the last click was on (not on the same place in
                // another view or pane, or with the list scrolled), and a third starts over.
                let last = self.last_click.take();
                let again = |app: &mut Self, row| {
                    let again = last.is_some_and(|(t, was)| t.elapsed() < Duration::from_millis(400) && was == row);
                    if !again {
                        app.last_click = Some((Instant::now(), row));
                    }
                    again
                };
                if let Some(&(v, _)) = self.areas.tabs.iter().find(|(_, r)| r.contains(p)) {
                    if self.picking.is_none() {
                        self.set_view(v);
                    }
                } else if let Some(&(pane, _)) = self.areas.drive_tabs.iter().find(|(_, r)| r.contains(p)) {
                    self.show_drive_pane(pane);
                } else if self.areas.overview.contains(p) {
                    self.seek_click(m.column);
                } else if self.deck_showing() && self.areas.matches.contains(p) {
                    let i = self.next_pick.offset() + (m.row - self.areas.matches.y) as usize;
                    if i < self.list_len() {
                        self.next_pick.select(Some(i));
                        if again(self, (self.view, None, i)) {
                            self.play_next();
                        }
                    }
                } else if let Some(&(field, _)) = self.areas.columns.iter().find(|(_, r)| r.contains(p)) {
                    self.sort = browse::click(self.sort, field);
                    self.refresh_visible();
                } else if let Some(f) = self.pane_at(p) {
                    if self.picking.is_some() && f != Focus::Playlists {
                        return;
                    }
                    self.point_at(f);
                    if let Some(i) = self.click_row(f, m.row)
                        && again(self, (self.view, Some(f), i))
                    {
                        if self.picking.is_some() { self.pick() } else { self.activate() }
                    }
                }
            }
            // Right-click: select what's under the pointer and open the actions menu there.
            MouseEventKind::Down(MouseButton::Right) if self.view == View::Library && self.picking.is_none() => {
                if let Some(f) = self.pane_at(p).filter(|f| *f != Focus::Drives) {
                    self.focus = f;
                    self.click_row(f, m.row);
                    self.open_menu(Some((m.column, m.row)));
                }
            }
            MouseEventKind::Drag(MouseButton::Left)
                if self.areas.overview.contains(p) && self.last_seek.elapsed() > Duration::from_millis(60) =>
            {
                self.seek_click(m.column);
            }
            _ => {}
        }
    }

    /// Clicks while a dialog is open: menu items can be clicked; a click elsewhere closes it.
    fn on_modal_mouse(&mut self, m: MouseEvent) {
        match self.modal.take() {
            Some(Modal::Pick(p)) => return self.on_picker_mouse(p, m),
            Some(Modal::Form(f)) => return self.on_form_mouse(f, m),
            other => self.modal = other,
        }
        if m.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        match self.modal.take() {
            Some(Modal::Menu(menu)) => {
                let r = self.areas.menu;
                if !r.contains(Position::new(m.column, m.row)) {
                    return; // closed
                }
                let row = m.row.saturating_sub(r.y + 1) as usize;
                match menu.sub {
                    Some(sub) => {
                        let i = menu.sub_state.offset() + row;
                        match Menu::sub_items(sub).get(i).copied() {
                            Some(action) => self.run_action(action, menu.target),
                            None => self.modal = Some(Modal::Menu(menu)),
                        }
                    }
                    None => {
                        let i = menu.state.offset() + row;
                        if i < menu.items.len() {
                            self.choose(menu, i);
                        } else {
                            self.modal = Some(Modal::Menu(menu));
                        }
                    }
                }
            }
            other => self.modal = other,
        }
    }

    fn pane_at(&self, p: Position) -> Option<Focus> {
        match self.view {
            View::Library => [
                (self.areas.drives, Focus::Drives),
                (self.areas.playlists, Focus::Playlists),
                (self.areas.tracks, Focus::Tracks),
                (self.areas.details, Focus::Details),
                (self.areas.crate_panel, Focus::Crate),
            ]
            .into_iter()
            .find(|(r, _)| r.contains(p))
            .map(|(_, f)| f),
            View::Prepare => self.areas.prepare.contains(p).then_some(Focus::Tracks),
            View::Drives => [
                (self.areas.drive_list, Focus::Drives),
                (self.areas.history, Focus::Tracks),
                (self.areas.played, Focus::Playlists),
                (self.areas.drive_side, Focus::Details),
            ]
            .into_iter()
            .find(|(r, _)| r.contains(p))
            .map(|(_, f)| f),
            View::Profile | View::Mixes => None,
        }
    }

    /// Give the pane `f` focus, as [`Self::pane_at`] names it in the view.
    fn point_at(&mut self, f: Focus) {
        match (self.view, f) {
            (View::Library, _) => self.focus = f,
            (View::Drives, Focus::Drives) => self.show_drive_pane(DrivePane::List),
            (View::Drives, Focus::Tracks) => self.show_drive_pane(DrivePane::Snapshots),
            (View::Drives, Focus::Playlists) => self.show_drive_pane(DrivePane::History),
            (View::Drives, _) => self.show_drive_pane(DrivePane::Info),
            _ => {}
        }
    }

    /// Select the row of `f` drawn at `row`, returning its index, or none when there's no row
    /// there.
    fn click_row(&mut self, f: Focus, row: u16) -> Option<usize> {
        // The first row of each list, and how far it's scrolled.
        let (top, offset) = match (self.view, f) {
            (View::Library, Focus::Tracks) => (self.areas.tracks.y + 2, self.track_state.offset()),
            (View::Library, Focus::Playlists) => (self.areas.playlists.y + 1, self.playlist_state.offset()),
            (View::Library, Focus::Details) => (self.areas.matches.y, self.details_state.offset()),
            (View::Library, Focus::Crate) => (self.areas.crate_rows.y, self.crate_state.offset()),
            (View::Library, Focus::Drives) => (self.areas.drives.y + 1, self.drive_state.offset()),
            (View::Prepare, _) => (self.areas.prepare.y + 1, self.prepare.offset()),
            (View::Drives, Focus::Drives) => (self.areas.drive_list.y + 1, self.drive_rows.offset()),
            (View::Drives, Focus::Tracks) => (self.areas.history.y + 2, self.history_state.offset()),
            (View::Drives, Focus::Playlists) => (self.areas.played.y + 1, self.played.offset()),
            (View::Drives, _) | (View::Profile | View::Mixes, _) => return None,
        };
        if row < top || (f == Focus::Details && self.areas.matches.height == 0) {
            return None;
        }
        // The Drives view shows each row on two lines: a drive's name, then where it's mounted (a
        // library's, then how its clones stand).
        let two_lines = self.view == View::Drives && f == Focus::Drives;
        let i = offset + (row - top) as usize / if two_lines { 2 } else { 1 };
        if two_lines {
            let &r = self.drive_rows().get(i)?;
            self.select_drive_row(r);
            return Some(i);
        }
        let (sel, len) = self.list_selection();
        (i < len).then(|| {
            *sel = Some(i);
            i
        })
    }

    fn seek_click(&mut self, column: u16) {
        let r = self.areas.overview;
        let Some(now) = &self.now else { return };
        let frac = (column.saturating_sub(r.x) as f64 + 0.5) / r.width.max(1) as f64;
        let target = now.duration.mul_f64(frac.clamp(0.0, 1.0));
        self.seek_to(target);
    }
}
