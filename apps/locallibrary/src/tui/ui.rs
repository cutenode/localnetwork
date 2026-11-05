//! Drawing.

mod chrome;
mod drives;
mod library;
mod overlays;
mod player;

pub(super) use chrome::*;
use drives::*;
pub(super) use library::*;
pub(super) use overlays::*;
pub(super) use player::*;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};

use super::app::{App, View};
use super::edits::Modal;
// What the test modules find through `use super::*`.
#[cfg(test)]
use {
    super::app::{Focus, Level, Matches, Repeat, Vis},
    super::browse::sort_label,
    super::drives::DriveRow,
    super::keys::{Camelot, Relation as KeyRelation},
    super::theme,
    crate::output::{bytes, clock, plural},
    locallibrary::DeviceInfo,
    locallibrary::api::FleetLibrary,
    locallibrary::compare::Relation,
    locallibrary::fields::FieldValue,
    locallibrary::fleet::{Role, Standing},
    locallibrary::local_time,
    locallibrary::query::{FieldName, SortField, SortKey},
    ratatui::style::Modifier,
    ratatui::text::Span,
    ratatui::widgets::ListState,
    uuid::Uuid,
};

pub fn draw(f: &mut Frame, app: &mut App) {
    let [header, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
    draw_header(f, app, header);
    // Panes register where they are as they're drawn; hidden ones mustn't catch clicks.
    let a = &mut app.areas;
    a.spectra.clear();
    a.columns.clear();
    (a.drives, a.playlists, a.tracks, a.details, a.matches, a.crate_panel, a.crate_rows, a.menu, a.overview) =
        Default::default();
    match app.view {
        View::Library if app.fullscreen => draw_player(f, app, body, true),
        View::Library => {
            let player_h = if body.height >= 30 {
                14
            } else if body.height >= 22 {
                12
            } else {
                9
            };
            let [main, player] = Layout::vertical([Constraint::Min(0), Constraint::Length(player_h)]).areas(body);
            draw_library(f, app, main);
            draw_player(f, app, player, false);
        }
        View::Drives => draw_drives(f, app, body),
        View::Snapshots => draw_history(f, app, body),
        View::Profile => super::profile::draw(f, app, body),
        View::Mixes => super::mix::draw(f, app, body),
    }
    draw_footer(f, app, footer);
    match &app.modal {
        Some(Modal::Menu(_)) => draw_menu(f, app, f.area()),
        Some(Modal::Confirm(_)) => draw_confirm(f, app, f.area()),
        Some(Modal::Ask(q, _)) => draw_ask(f, q.clone(), f.area()),
        Some(Modal::Pick(_)) => draw_picker(f, app, f.area()),
        Some(Modal::Report(_)) => draw_report(f, app, f.area()),
        Some(Modal::Form(_)) => draw_form(f, app, f.area()),
        Some(Modal::Prompt(_)) | None => {}
    }
    if app.deck.outputs.is_some() {
        draw_outputs(f, app, f.area());
    }
    if app.help {
        draw_help(f, app, f.area());
    } else if app.show_log {
        draw_log(f, app, f.area());
    } else {
        draw_toasts(f, app, f.area(), footer);
    }
}

/// For tests: the screen as text.
#[cfg(test)]
pub(super) fn screen(app: &mut App, w: u16, h: u16) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buf = terminal.backend().buffer();
    (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n").collect()
}

#[cfg(test)]
mod tests;
