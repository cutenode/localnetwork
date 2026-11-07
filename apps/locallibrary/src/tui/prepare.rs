//! The Prepare view: the jobs that get a drive ready for a set, each what the palette runs.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem};
use unicode_width::UnicodeWidthStr;

use super::app::App;
use super::icons;
use super::keymap::{self, Act};
use super::theme::{self, dim, faint, fg, text};
use super::ui::{Scrolled, block, clip, pane_title};

const JOBS: [Act; 7] = [Act::Check, Act::SetPrep, Act::SyncGroup, Act::Pack, Act::Pull, Act::SmartApply, Act::Refresh];

/// The gap between a job's name and what it does.
const GAP: usize = 3;

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
    let name_w = JOBS.iter().map(|a| keymap::name(*a).width()).max().unwrap_or(0);
    let items: Vec<ListItem> = JOBS
        .iter()
        .map(|&act| {
            let name = format!("{:<name_w$}", keymap::name(act));
            let about = clip(keymap::about(act), width.saturating_sub(name_w + GAP));
            ListItem::new(Line::from(vec![
                Span::styled(name, text()),
                Span::raw(" ".repeat(GAP)),
                Span::styled(about, dim()),
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
