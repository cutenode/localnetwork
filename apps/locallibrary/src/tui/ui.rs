//! Drawing.

mod chrome;
mod deck;
mod drives;
mod footer;
mod header;
mod keybinds;
mod library;
mod overlays;
mod player;
mod shell;
mod strip;

pub(super) use chrome::*;
use deck::{draw_deck, end_style};
use drives::*;
pub(super) use footer::Footer;
pub(super) use header::Header;
pub(super) use keybinds::KeysDialog;
pub(super) use library::*;
pub(super) use overlays::*;
pub(super) use player::*;
pub(super) use shell::Density;
use shell::{Shell, draw_too_small};
pub(super) use strip::DeckWaves;
use strip::{draw_overview, draw_strip, keep_waves};

use ratatui::Frame;

use super::app::{App, View};
use super::edits::Modal;
use super::icons;
use super::transitions::Shown;
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
    a.hints.clear();
    a.loop_chip = Default::default();
    a.columns.clear();
    (a.drives, a.playlists, a.tracks, a.details, a.matches, a.crate_panel, a.crate_rows, a.menu, a.overview, a.zoom) =
        Default::default();
    (a.drive_list, a.drive_side, a.history, a.played, a.prepare, a.steps) = Default::default();
    if shell.density == Density::DeckOnly {
        draw_strip(f, app, shell.strip);
        draw_too_small(f, shell.body);
        // With no footer, the bottom row says what a second esc does; the strip stays, the deck being
        // what this size is for.
        if app.quit_waiting() && !shell.body.is_empty() {
            let line = ratatui::layout::Rect { y: shell.body.bottom() - 1, height: 1, ..shell.body };
            f.render_widget(ratatui::widgets::Clear, line);
            f.render_widget(ratatui::text::Line::from(footer::quit_line()), line);
        }
    } else {
        let header = Header::of(app);
        app.areas.tabs = header.tab_areas(shell.header);
        f.render_widget(&header, shell.header);
        match app.view {
            // The deck has the screen: the strip would only say it twice.
            View::Library if app.deck_showing() => draw_deck(f, app, shell.body.union(shell.strip)),
            View::Library => draw_library(f, app, shell.body),
            View::Prepare => super::prepare::draw(f, app, shell.body),
            View::Drives => draw_drives(f, app, shell.body),
            View::Profile => super::profile::draw(f, app, shell.body),
            View::Mixes => super::mix::draw(f, app, shell.body),
        }
        if !app.deck_showing() {
            draw_strip(f, app, shell.strip);
        }
    }
    let over = shell.over();
    // Over the deck, a dialog stays below its head, what's playing, as other views' dialogs leave the strip.
    let over = match shell.density != Density::DeckOnly && app.deck_showing() {
        true => {
            let below = app.areas.deck_head.bottom().max(over.y);
            ratatui::layout::Rect { y: below, height: over.bottom().saturating_sub(below), ..over }
        }
        false => over,
    };
    let footer = Footer::of(app, shell.footer.width);
    app.areas.hints.extend(footer.hint_areas(shell.footer));
    f.render_widget(&footer, shell.footer);
    let message = footer.room(shell.status());
    match &app.modal {
        Some(Modal::Menu(_)) => draw_menu(f, app, over),
        Some(Modal::Confirm(_)) => draw_confirm(f, app, over),
        Some(Modal::Ask(..)) => draw_ask(f, app, over),
        Some(Modal::Pick(_)) => draw_picker(f, app, over),
        Some(Modal::Report(_)) => draw_report(f, app, over),
        Some(Modal::Form(_)) => draw_form(f, app, over),
        Some(Modal::Jobs(_)) => super::jobs::draw(f, app, over),
        Some(Modal::Prompt(_)) | None => {}
    }
    if app.deck.outputs.is_some() {
        draw_outputs(f, app, over);
    }
    if let Some(keys) = KeysDialog::of(app) {
        f.render_widget(&keys, over);
    }
    if app.help {
        draw_help(f, app, over);
    } else if app.show_log {
        draw_log(f, app, over);
    } else if shell.density != Density::DeckOnly {
        draw_toasts(f, app, over, message);
    }
    app.transitions.follow(shown(app), shell.body.union(shell.strip));
    // The overview's where this frame drew it, the deck's or the strip's: none, none washed.
    app.transitions.follow_warning(end_style(app).is_some(), app.areas.overview);
    app.transitions.draw(f.buffer_mut());
    if icons::ascii() {
        icons::fold(f.buffer_mut());
    }
}

/// What a change of starts a fade.
fn shown(app: &App) -> Shown {
    let loaded = app.now.as_ref().filter(|n| !n.loading).map(|n| n.id);
    Shown {
        deck: app.deck_showing(),
        details: app.show_details,
        crate_panel: app.show_crate,
        help: app.help,
        track: loaded,
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
