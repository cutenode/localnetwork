//! The Prepare view: the jobs that get a drive ready for a set, each what the palette runs. Those
//! with no key elsewhere live here, and a row that can't run now says why.

use std::sync::LazyLock;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem};
use unicode_width::UnicodeWidthStr;

use super::app::App;
use super::icons;
use super::keymap::{self, Act, Scope};
use super::palette::key_name;
use super::theme::{self, dim, faint, fg, text};
use super::ui::{Scrolled, block, clip, pane_title};

const JOBS: [Act; 13] = [
    Act::Check,
    Act::SetPrep,
    Act::SyncGroup,
    Act::Pack,
    Act::Pull,
    Act::RestorePlaylist,
    Act::Replay,
    Act::SmartList,
    Act::SmartApply,
    Act::SmartNew,
    Act::ExportList,
    Act::Eject,
    Act::Refresh,
];

/// The gap between a job's name and what it does.
const GAP: usize = 3;
/// Room for a job's key.
const KEY: usize = 3;

/// A job's row as far as the keymap says it: its name padded to the widest, its key padded to
/// `KEY`, and what it does.
struct Row {
    act: Act,
    name: String,
    key: String,
    about: &'static str,
}

/// The widest job name and each job's row, worked out once: looking them up in the keymap every
/// frame was a third of what listing all thirteen jobs added to a frame.
static ROWS: LazyLock<(usize, Vec<Row>)> = LazyLock::new(|| {
    let name_w = JOBS.iter().map(|a| keymap::name(*a).width()).max().unwrap_or(0);
    let rows = JOBS
        .iter()
        .map(|&act| {
            let key = keymap::key_in(Scope::Prepare, act).map_or(String::new(), key_name);
            Row {
                act,
                name: format!("{:<name_w$}", keymap::name(act)),
                key: format!("{key:<KEY$}"),
                about: keymap::about(act),
            }
        })
        .collect();
    (name_w, rows)
});

/// Whether this view is where `act` lives.
pub fn holds(act: Act) -> bool {
    JOBS.contains(&act)
}

pub fn len() -> usize {
    JOBS.len()
}

/// Enter: run the job selected.
pub fn run(app: &mut App) {
    if let Some(&act) = app.prepare.selected().and_then(|i| JOBS.get(i)) {
        app.perform(act, None);
    }
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    app.areas.prepare = area;
    let drive = app.loaded.as_ref().map(|l| l.root().to_path_buf()).map(|r| app.drive_label(&r));
    let on = match drive {
        Some(d) => Span::styled(format!(" on {d} "), dim()),
        None => Span::styled(" no drive open ", faint()),
    };
    let width = area.width.saturating_sub(3) as usize;
    let (name_w, rows) = &*ROWS;
    let room = width.saturating_sub(name_w + GAP + KEY);
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| {
            let (name, about) = match app.unavailable(row.act) {
                Some(why) => {
                    let said = format!("{why} · {}", row.about);
                    (Span::styled(row.name.as_str(), faint()), Span::styled(clip(&said, room), faint()))
                }
                None => (Span::styled(row.name.as_str(), text()), Span::styled(clip(row.about, room), dim())),
            };
            ListItem::new(Line::from(vec![
                name,
                Span::raw(" ".repeat(GAP)),
                Span::styled(row.key.as_str(), theme::key()),
                about,
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(block(pane_title(1, "Prepare", true), true).title_top(Line::from(on).right_aligned()))
        .highlight_style(theme::selected())
        .highlight_symbol(Span::styled(icons::SELECTED.get(), fg(theme::current().accent)));
    app.prepare.fit(area.height.saturating_sub(2) as usize, JOBS.len());
    f.render_stateful_widget(list, area, &mut app.prepare);
}

#[cfg(test)]
mod tests;
