//! The two rows at the bottom. The status row: what's under way on the left, where things are on
//! the right (only where that means something), and the latest message between them. The keys
//! row: what the keys do now, in the same order in every view, with the commands and the help
//! always at its end; a chip leads it when the keys mean something else (searching, typing,
//! picking a playlist, the pads).

use std::rc::Rc;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::output::bytes;
use crate::tui::app::{App, Focus, View, typed_tail};
use crate::tui::choices::Hints;
use crate::tui::edits::Modal;
use crate::tui::first_run::intro_line;
use crate::tui::icons;
use crate::tui::keymap::{self, Act, Scope};
use crate::tui::theme::{self, bold, dim, faint, fg};

use super::{fit, hint_bar, spinner};

/// The footer: the status row's two ends, and what the keys row says.
pub struct Footer {
    /// What's under way: the job, or the first of the job list.
    pub doing: Vec<Span<'static>>,
    /// Where things are: the deck's modes in player mode, the row selected, the space left.
    pub place: Vec<Span<'static>>,
    /// What the keys mean now, when it's not what they mean in the view.
    pub mode: Option<&'static str>,
    pub keys: Keys,
}

/// What the keys row says.
pub enum Keys {
    /// What the keys do, each a button.
    Hints(Rc<HintsRow>),
    /// A line being typed: what's asked, what's typed so far, and what follows it.
    Typed { asked: Vec<Span<'static>>, typed: String, after: Vec<Span<'static>> },
    /// A line of its own: the first run's, or that esc again quits.
    Said(Vec<Span<'static>>),
}

/// The keys row when it's the hints, as drawn: made once while the keys, where the player is, the
/// hints setting, the chip, the width and how it looks stay as they are.
pub struct HintsRow {
    /// The row fitted to its width, the chip first when there is one.
    line: Line<'static>,
    /// Where each hint is on it: its column, its width, what it does.
    at: Vec<(u16, u16, Act)>,
}

impl Footer {
    /// The footer `width` columns wide.
    pub fn of(app: &mut App, width: u16) -> Footer {
        let mode = mode(app);
        let keys = keys(app, mode, width);
        Footer { doing: doing(app), place: place(app), mode, keys }
    }

    /// Where the latest message goes in `area`: the status row between what's under way and where
    /// things are.
    pub fn room(&self, area: Rect) -> Rect {
        let width = area.width as usize;
        let doing: usize = 1 + fit(self.doing.clone(), width / 2).iter().map(Span::width).sum::<usize>();
        let place: usize = self.place.iter().map(Span::width).sum();
        let room = width.saturating_sub(doing + place + 1);
        Rect { x: area.x + doing.min(width) as u16, y: area.y, width: room as u16, height: area.height.min(1) }
    }

    /// Where each hint is on the keys row of `area`, as clicks find them.
    pub fn hint_areas(&self, area: Rect) -> impl Iterator<Item = (Rect, Act)> + '_ {
        let at = match &self.keys {
            Keys::Hints(row) if area.height > 0 => &row.at[..],
            _ => &[],
        };
        let inside = at.iter().filter(move |(x, w, _)| x + w <= area.width);
        inside.map(move |&(x, w, act)| (Rect::new(area.x + x, area.bottom() - 1, w, 1), act))
    }
}

impl Widget for &Footer {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let width = area.width as usize;
        let rows = area.rows().collect::<Vec<_>>();
        if let [status, ..] = rows[..]
            && rows.len() > 1
        {
            let mut left = vec![Span::raw(" ")];
            left.extend(fit(self.doing.clone(), width / 2));
            super::spread(left, self.place.clone(), area.width).render(status, buf);
        }
        let keys = *rows.last().unwrap_or(&area);
        let asked = match &self.keys {
            Keys::Hints(row) => return (&row.line).render(keys, buf),
            Keys::Typed { asked, .. } | Keys::Said(asked) => asked,
        };
        let mut spans = lead(self.mode);
        spans.extend(asked.iter().cloned());
        if let Keys::Typed { typed, after, .. } = &self.keys {
            // What's typed keeps its end in view, its cursor after it, then what follows if it fits.
            let used: usize = spans.iter().map(Span::width).sum();
            let room = width.saturating_sub(used + 1);
            spans.push(Span::styled(typed_tail(typed, room).to_string(), bold(theme::current().text)));
            spans.push(Span::styled(icons::CURSOR.get(), fg(theme::current().accent)));
            let used: usize = spans.iter().map(Span::width).sum();
            spans.extend(fit(after.clone(), width.saturating_sub(used)));
        }
        Line::from(fit(spans, width)).render(keys, buf);
    }
}

/// What keys mean now, when it's not what they mean in the view.
fn mode(app: &App) -> Option<&'static str> {
    let typing = matches!(app.modal, Some(Modal::Prompt(_))) || app.mix.input.is_some() || app.profile.account.typing();
    if app.searching {
        Some("SEARCH")
    } else if typing {
        Some("TYPING")
    } else if app.picking.is_some() {
        Some("PICK")
    } else if app.deck.pads && !app.deck_showing() {
        Some("PADS")
    } else {
        None
    }
}

/// A typed line's keys after it, as a dialog's border says them (but for moving, which the list
/// above shows).
fn typed_keys(scopes: &[Scope], says: &[(Act, &str)]) -> Vec<Span<'static>> {
    let mut hints = keymap::dialog_hints(scopes);
    hints.retain(|h| !matches!(h.act, Act::Move(_)));
    let mut spans = vec![Span::raw("  ")];
    spans.extend(hint_bar(&hints, says).spans);
    spans
}

/// The keys row's first columns: a space, then the chip when keys mean something else.
fn lead(mode: Option<&'static str>) -> Vec<Span<'static>> {
    let mut spans = vec![Span::raw(" ")];
    if let Some(m) = mode {
        spans.extend([Span::styled(format!(" {m} "), theme::chip(theme::current().dim)), Span::raw(" ")]);
    }
    spans
}

/// The hints row `width` columns wide after `mode`'s chip: each hint's keys, then what it does.
fn hints_row(app: &App, mode: Option<&'static str>, width: u16) -> HintsRow {
    let chip = mode.map_or(0, |m| m.width() + 3);
    let mut spans = lead(mode);
    let mut at = Vec::new();
    let mut x: usize = spans.iter().map(Span::width).sum();
    for h in keymap::hints(app, (width as usize).saturating_sub(chip)) {
        let label = format!(" {}  ", h.label);
        if keymap::clicks(h.act) {
            at.push((x as u16, (h.keys.width() + label.trim_end().width()) as u16, h.act));
        }
        x += h.keys.width() + label.width();
        spans.push(Span::styled(h.keys, theme::key()));
        spans.push(Span::styled(label, dim()));
    }
    HintsRow { line: Line::from(fit(spans, width as usize)), at }
}

/// What the keys row says now, `width` columns wide.
fn keys(app: &App, mode: Option<&'static str>, width: u16) -> Keys {
    if let Some(Modal::Prompt(p)) = &app.modal {
        let asked = vec![Span::styled(format!("{}: ", p.label), theme::key())];
        return Keys::Typed { asked, typed: p.text.clone(), after: typed_keys(&keymap::dialogs::PROMPT, &[]) };
    }
    if app.searching {
        let n = app.visible.len();
        let mut after = vec![Span::styled(format!("   {n} match{}", if n == 1 { "" } else { "es" }), dim())];
        if app.search_all {
            after.push(Span::styled("  in the whole drive", fg(theme::current().info)));
        }
        match (&app.filter_error, &app.filter_note) {
            (Some(e), _) => after.push(Span::styled(format!("   {e}"), fg(theme::current().warning))),
            (None, Some(n)) => after.push(Span::styled(format!("   {n}"), dim())),
            (None, None) => {
                let other = if app.search_all { "this list" } else { "whole drive" };
                after.extend(typed_keys(&keymap::dialogs::SEARCH, &[(Act::SearchWhere, other)]));
            }
        }
        return Keys::Typed { asked: vec![Span::styled("/ ", theme::key())], typed: app.query.clone(), after };
    }
    if let Some(p) = &app.picking {
        let mut said =
            vec![Span::styled(format!("Add {} to which playlist?", p.target.what), bold(theme::current().text))];
        said.extend(typed_keys(&keymap::dialogs::PICKING, &[]));
        return Keys::Said(said);
    }
    if app.mix.input.is_some() || app.profile.account.typing() {
        return Keys::Said(typed_keys(&keymap::dialogs::PROMPT, &[]));
    }
    if app.intro {
        return Keys::Said(intro_line(&keymap::scopes(app)));
    }
    if app.quit_waiting() {
        return Keys::Said(quit_line());
    }
    // No hints but the way to the help, unless that's off too.
    if app.hints == Hints::Off && app.help_hint {
        return Keys::Said(vec![Span::styled("? help", faint())]);
    }
    let key = (keymap::situation(app), app.hints, mode, width, theme::look());
    Keys::Hints(keymap::effective::memo(|k| *k == key, || key, || hints_row(app, mode, width)))
}

/// After one esc with nothing open, for the second it lasts.
pub(super) fn quit_line() -> Vec<Span<'static>> {
    vec![Span::styled("esc", theme::key()), Span::styled(" again to quit", dim())]
}

/// What the player's doing: the job's progress first, else the first of what the job list (`&`)
/// holds, so the two can't say different things.
fn doing(app: &App) -> Vec<Span<'static>> {
    let mut busy = Vec::new();
    if let Some(j) = &app.job {
        busy.push(Span::styled(format!("{} ", spinner(app)), fg(theme::current().warning)));
        busy.push(Span::styled(j.label.clone(), bold(theme::current().warning)));
        let secs = j.started.elapsed().as_secs();
        if secs > 0 {
            busy.push(Span::styled(format!(" {secs}s"), faint()));
        }
        if j.committed {
            busy.push(Span::styled(" · can't stop now", fg(theme::current().warning)));
        }
        if !j.detail.is_empty() {
            busy.push(Span::styled(format!(" · {}", j.detail), dim()));
        }
        busy.push(Span::raw(" "));
        // Esc cancels it while it can still stop; & lists every job.
        let esc = keymap::hint_for(&[Scope::Global], Act::Escape, "cancel").filter(|_| !j.committed);
        let jobs = keymap::hint_for(&[Scope::Global], Act::Jobs, "jobs");
        busy.extend(hint_bar(&esc.into_iter().chain(jobs).collect::<Vec<_>>(), &[]).spans);
    } else if let Some(row) = crate::tui::jobs::rows(app).first() {
        busy.push(Span::styled(format!("{} ", spinner(app)), fg(theme::current().info)));
        busy.push(Span::styled(row.label.clone(), dim()));
    }
    busy
}

/// Where things are, where that means something: the deck's modes in player mode, the row
/// selected in its list and, in the library and Prepare, the space left on the drive. What's
/// about the drive comes from the last read of the drive list, so a frame touches no disk.
fn place(app: &mut App) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if app.deck_showing() {
        spans.extend(super::deck::deck_states(app));
        spans.push(Span::raw("   "));
    }
    let (selected, len) = match (app.view, app.focus) {
        // With no drive open, the library's list is its first steps.
        (View::Library, _) if app.loaded.is_none() => (None, 0),
        (View::Library, Focus::Drives) if !app.deck_showing() => (app.source_row(), app.source_rows()),
        _ => {
            let (selected, len) = app.list_selection();
            (*selected, len)
        }
    };
    if let Some(at) = selected.filter(|_| len > 0) {
        spans.push(Span::styled(format!("{}/{len}", at.min(len - 1) + 1), dim()));
    }
    if matches!(app.view, View::Library | View::Prepare) {
        let root = app.action_drive();
        let free = app.drives.iter().find(|d| Some(&d.root) == root.as_ref()).and_then(|d| d.free_space);
        if let Some(free) = free {
            spans.push(Span::styled(format!("  {} free", bytes(free)), dim()));
        }
    }
    spans.push(Span::raw(" "));
    spans
}

#[cfg(test)]
mod tests;
