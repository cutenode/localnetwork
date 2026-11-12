//! The job list (`&`): what's running, each with a cancel while it can still stop. A write can only
//! stop until its databases are being written, and a read of a drive or of the drives' status just
//! finishes: those say so instead of offering a cancel that does nothing. The status line says what's
//! running from the same rows.

use std::time::Instant;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState};

use super::app::{App, Level};
use super::edits::Modal;
use super::icons;
use super::theme::{self, dim, faint, fg, text};
use super::ui::{Scrolled, block, clip, popup, spread, title};

/// What can be done about a job now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Can,
    /// Asked to, and not stopped yet.
    Stopping,
    /// Past the point where stopping would be safe.
    TooLate,
    /// Nothing in it can be stopped.
    Finishes,
}

impl Stop {
    fn note(self) -> &'static str {
        match self {
            Stop::Can => "enter cancels",
            Stop::Stopping => "stopping…",
            Stop::TooLate => "can't stop now",
            Stop::Finishes => "finishes on its own",
        }
    }
}

/// Which work a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Work {
    /// The job: a plan, a write or a task.
    Job,
    Identifying,
    Profile,
    SigningIn,
    Reading,
}

pub struct Row {
    pub label: String,
    pub detail: String,
    since: Option<Instant>,
    pub stop: Stop,
    work: Work,
}

/// Everything running now, the job first.
pub fn rows(app: &App) -> Vec<Row> {
    let row = |label: String, stop, work| Row { label, detail: String::new(), since: None, stop, work };
    let mut rows = Vec::new();
    if let Some(j) = &app.job {
        let stop = match (j.committed, j.cancel.is_cancelled()) {
            (true, _) => Stop::TooLate,
            (false, true) => Stop::Stopping,
            (false, false) => Stop::Can,
        };
        let work = Work::Job;
        rows.push(Row { label: j.label.clone(), detail: j.detail.clone(), since: Some(j.started), stop, work });
    }
    if let Some(s) = app.mix.status() {
        rows.push(row(s, Stop::Can, Work::Identifying));
    }
    if app.profile.loading {
        let s = app.profile.status().unwrap_or_else(|| "Building the profile".into());
        rows.push(row(s, Stop::Can, Work::Profile));
    }
    if let Some(s) = &app.loading {
        rows.push(row(s.clone(), Stop::Finishes, Work::Reading));
    }
    if app.profile.account.busy() {
        rows.push(row("Signing in".into(), Stop::Can, Work::SigningIn));
    }
    if app.status_loading {
        rows.push(row("Reading the drives' status".into(), Stop::Finishes, Work::Reading));
    }
    rows
}

impl App {
    pub(super) fn open_jobs(&mut self) {
        self.modal = Some(Modal::Jobs(ListState::default().with_selected(Some(0))));
    }

    /// Keys while the job list is open: the arrows move, enter (or `x`) cancels, anything else
    /// that isn't typed for it closes it.
    pub(super) fn on_jobs_key(&mut self, mut list: ListState, key: KeyEvent) {
        let n = rows(self).len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => list.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => {
                list.select(Some(list.selected().map_or(0, |s| s + 1).min(n.max(1) - 1)))
            }
            KeyCode::Enter | KeyCode::Delete | KeyCode::Char('x') => self.cancel_row(list.selected().unwrap_or(0)),
            KeyCode::Esc | KeyCode::Char('q' | '&') => return,
            _ => {}
        }
        self.modal = Some(Modal::Jobs(list));
    }

    /// Cancel the job at `at`, if there's one and it can stop. Says when it can't.
    fn cancel_row(&mut self, at: usize) {
        let mut rows = rows(self);
        if at >= rows.len() {
            return;
        }
        let row = rows.swap_remove(at);
        match (row.stop, row.work) {
            (Stop::Stopping, _) => {}
            (Stop::TooLate, _) => self.toast(format!("{}: too late to stop", row.label), Level::Info),
            (Stop::Finishes, _) => self.toast(format!("{}: it finishes on its own", row.label), Level::Info),
            (_, Work::Job) => self.cancel_job(),
            (_, Work::Identifying) => {
                self.mix.cancel();
                self.toast("stopped", Level::Info);
            }
            (_, Work::Profile) => {
                super::profile::stop(self);
                self.toast("stopped", Level::Info);
            }
            (_, Work::SigningIn) => {
                if !self.profile.account.stop() {
                    self.toast("signing out: it finishes on its own", Level::Info);
                }
            }
            (_, Work::Reading) => {}
        }
    }
}

/// The job list, in a box over the view.
pub(super) fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let rows = rows(app);
    let Some(Modal::Jobs(selected)) = &mut app.modal else { return };
    // Nothing to select in what says there's nothing.
    let mut none = ListState::default();
    let list = if rows.is_empty() { &mut none } else { selected };
    let w = 76u16.min(area.width.saturating_sub(2));
    let inner = w.saturating_sub(4);
    let items: Vec<ListItem> = if rows.is_empty() {
        vec![ListItem::new(Line::styled("Nothing's running.", dim()))]
    } else {
        rows.iter().map(|r| ListItem::new(row_line(r, inner))).collect()
    };
    let r = popup(area, w, items.len() as u16 + 2, None);
    list.fit(r.height.saturating_sub(2) as usize, items.len());
    let keys = if rows.is_empty() { " esc close " } else { " ↑↓ select · enter cancel · esc close " };
    let b = block(title("Jobs", true), true).title_bottom(Line::styled(keys, faint()).right_aligned());
    let widget = List::new(items)
        .block(b)
        .highlight_style(theme::selected())
        .highlight_symbol(Span::styled(icons::SELECTED.get(), fg(theme::current().accent)));
    f.render_widget(Clear, r);
    f.render_stateful_widget(widget, r, list);
    app.areas.menu = r;
}

/// A job: what it is, how long it's run and how far it's got, then what can be done about it.
fn row_line(r: &Row, width: u16) -> Line<'static> {
    let mut about = r.label.clone();
    if let Some(at) = r.since.map(|at| at.elapsed().as_secs()).filter(|s| *s > 0) {
        about.push_str(&format!(" {at}s"));
    }
    if !r.detail.is_empty() {
        about.push_str(&format!(" · {}", r.detail));
    }
    let note = match r.stop {
        Stop::Can => Span::styled(r.stop.note(), theme::key()),
        Stop::TooLate => Span::styled(r.stop.note(), fg(theme::current().warning)),
        Stop::Stopping | Stop::Finishes => Span::styled(r.stop.note(), faint()),
    };
    let room = (width as usize).saturating_sub(note.width() + 2);
    spread(vec![Span::styled(clip(&about, room), text())], vec![note], width)
}

#[cfg(test)]
mod tests;
