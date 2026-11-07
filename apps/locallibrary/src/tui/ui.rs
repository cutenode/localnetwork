//! Drawing.

mod chrome;
mod drives;
mod library;
mod overlays;
mod player;
mod shell;
mod strip;

pub(super) use chrome::*;
use drives::*;
pub(super) use library::*;
pub(super) use overlays::*;
pub(super) use player::*;
pub(super) use shell::Density;
use shell::{Shell, draw_too_small};
pub(super) use strip::StripWave;
use strip::draw_strip;

use ratatui::Frame;

use super::app::{App, View};
use super::edits::Modal;
use super::icons;
// What the test modules find through `use super::*`.
#[cfg(test)]
use {
    super::app::{DrivePane, Focus, Level, Matches, Repeat, Vis},
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
    let shell = Shell::of(f.area());
    // Panes register where they are as they're drawn; hidden ones mustn't catch clicks.
    let a = &mut app.areas;
    a.density = shell.density;
    a.tabs.clear();
    a.drive_tabs.clear();
    a.spectra.clear();
    a.columns.clear();
    (a.drives, a.playlists, a.tracks, a.details, a.matches, a.crate_panel, a.crate_rows, a.menu, a.overview) =
        Default::default();
    (a.drive_list, a.drive_side, a.history, a.played, a.prepare) = Default::default();
    if shell.density == Density::DeckOnly {
        draw_strip(f, app, shell.strip);
        draw_too_small(f, shell.body);
    } else {
        draw_header(f, app, shell.header, shell.context.is_empty());
        draw_context(f, app, shell.context);
        match app.view {
            // Full screen is the deck: the strip would only say it twice.
            View::Library if app.fullscreen => draw_player(f, app, shell.body.union(shell.strip)),
            View::Library => draw_library(f, app, shell.body),
            View::Prepare => super::prepare::draw(f, app, shell.body),
            View::Drives => draw_drives(f, app, shell.body),
            View::Profile => super::profile::draw(f, app, shell.body),
            View::Mixes => super::mix::draw(f, app, shell.body),
        }
        if !(app.view == View::Library && app.fullscreen) {
            draw_strip(f, app, shell.strip);
        }
        draw_footer(f, app, shell.footer);
    }
    let over = shell.over();
    let message = draw_status(f, app, shell.status);
    match &app.modal {
        Some(Modal::Menu(_)) => draw_menu(f, app, over),
        Some(Modal::Confirm(_)) => draw_confirm(f, app, over),
        Some(Modal::Ask(q, _)) => draw_ask(f, q.clone(), over),
        Some(Modal::Pick(_)) => draw_picker(f, app, over),
        Some(Modal::Report(_)) => draw_report(f, app, over),
        Some(Modal::Form(_)) => draw_form(f, app, over),
        Some(Modal::Prompt(_)) | None => {}
    }
    if app.deck.outputs.is_some() {
        draw_outputs(f, app, over);
    }
    if app.help {
        draw_help(f, app, over);
    } else if app.show_log {
        draw_log(f, app, over);
    } else if shell.density != Density::DeckOnly {
        draw_toasts(f, app, over, message);
    }
    draw_chord(f, app, over);
    if icons::ascii() {
        icons::fold(f.buffer_mut());
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
