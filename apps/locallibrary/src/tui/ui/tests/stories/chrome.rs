//! The header's and the footer's stories: each view and each state that changes what they say, 80
//! and 160 columns wide. Each draws the player's demo fixture (see `screens::fixture`) put in that
//! state, so a story is what the player shows.

use std::cell::RefCell;
use std::time::Instant;

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;

use super::Story;
use crate::tui::app::testing::job;
#[cfg(feature = "account")]
use crate::tui::app::testing::signed_in;
use crate::tui::app::{App, View};
use crate::tui::edits::{Action, Modal};
use crate::tui::screens::{Fixture, fixture};
use crate::tui::ui::{Footer, Header};

/// A state the player can be put in: its name in a story's id, and how to get there.
type State = (&'static str, fn(&mut App));

/// What makes a widget's story: its id, its width and the state it's drawn in.
type Make = fn(String, u16, fn(&mut App)) -> Story<'static>;

/// Each view as it opens (player mode is the library's other layout).
const VIEWS: [State; 6] = [
    ("library", |_| {}),
    ("prepare", |app| app.set_view(View::Prepare)),
    ("drives", |app| app.set_view(View::Drives)),
    ("recordings", |app| app.set_view(View::Mixes)),
    ("localnetwork", |app| app.view = View::Profile),
    ("player-mode", |app| app.deck_layout = true),
];

/// The header's other states (signed in only with the account: stories.json lists the default
/// features' stories).
const HEADER: &[State] = &[
    ("search-and-marks", |app| {
        app.query = "demo".into();
        app.refresh_visible();
        app.marks.extend(app.visible.iter().map(|&i| app.loaded.as_ref().unwrap().library.tracks[i].file_path.clone()));
    }),
    ("search-all", search_all),
    #[cfg(feature = "account")]
    ("signed-in", |app| {
        app.view = View::Profile;
        app.profile.account.info = Some(signed_in());
    }),
    ("no-drive", |app| {
        app.loaded = None;
        app.set_view(View::Prepare);
    }),
];

/// The footer's other states.
const FOOTER: [State; 9] = [
    ("search", |app| {
        app.searching = true;
        app.query = "demo".into();
        app.refresh_visible();
    }),
    ("prompt", |app| {
        app.track_state.select(Some(0));
        let target = app.target().unwrap();
        app.run_action(Action::NewPlaylistWith, target);
        if let Some(Modal::Prompt(p)) = &mut app.modal {
            p.text = "Warm-up".into();
        }
    }),
    ("picking", |app| {
        app.track_state.select(Some(0));
        let target = app.target().unwrap();
        app.run_action(Action::AddToPlaylist, target);
    }),
    ("pads", |app| app.deck.pads = true),
    ("hints-compact", |app| app.hints = crate::tui::choices::Hints::Compact),
    ("search-all", |app| {
        app.searching = true;
        search_all(app);
    }),
    ("search-error", |app| {
        app.searching = true;
        app.query = "bpm:fast".into();
        app.filter_error = Some("bpm: needs a number".into());
    }),
    // The first line someone new reads.
    ("intro", |app| app.intro = true),
    ("no-drive", |app| app.loaded = None),
];

/// The footer's states that tell the time, put in each as it's drawn so the seconds and the
/// spinner read the same every run: one esc with nothing open, and a job, then one too far in to
/// stop.
const AS_DRAWN: [State; 3] = [
    ("quit", |app| {
        // Any other key starts over, then one esc.
        app.on_key(KeyCode::Char('j').into());
        app.on_key(KeyCode::Esc.into());
    }),
    ("job", |app| (app.started, app.job) = (Instant::now(), Some(job(true, false)))),
    ("job-committed", |app| (app.started, app.job) = (Instant::now(), Some(job(true, true)))),
];

/// Searching the whole drive, not the list shown.
fn search_all(app: &mut App) {
    app.query = "demo".into();
    app.search_all = true;
    app.refresh_visible();
}

/// The fixture put in `state`, kept for a story to draw from.
pub(super) fn put(state: fn(&mut App)) -> RefCell<Fixture> {
    let mut f = fixture();
    state(&mut f.app);
    RefCell::new(f)
}

fn header(id: String, width: u16, state: fn(&mut App)) -> Story<'static> {
    let at = put(state);
    Story::still(Box::leak(id.into_boxed_str()), "Header", (width, 1), move |f: &mut Frame| {
        f.render_widget(&Header::of(&at.borrow().app), f.area());
    })
}

fn footer(id: String, width: u16, state: fn(&mut App)) -> Story<'static> {
    let at = put(state);
    Story::still(Box::leak(id.into_boxed_str()), "Footer", (width, 2), move |f: &mut Frame| {
        let footer = Footer::of(&mut at.borrow_mut().app, f.area().width);
        f.render_widget(&footer, f.area());
    })
}

/// The footer put in `state` as it's drawn.
fn footer_as_drawn(id: String, width: u16, state: fn(&mut App)) -> Story<'static> {
    let at = put(|_| {});
    Story::still(Box::leak(id.into_boxed_str()), "Footer", (width, 2), move |f: &mut Frame| {
        let app = &mut at.borrow_mut().app;
        state(app);
        let footer = Footer::of(app, f.area().width);
        f.render_widget(&footer, f.area());
    })
}

/// Every view's header and footer, and the states each has, narrow and wide.
pub(super) fn stories() -> Vec<Story<'static>> {
    let sizes = [("narrow", 80), ("wide", 160)];
    let headers = VIEWS.iter().chain(HEADER).map(|s| ("header", header as Make, s));
    let footers = VIEWS.iter().chain(&FOOTER).map(|s| ("footer", footer as Make, s));
    let as_drawn = AS_DRAWN.iter().map(|s| ("footer", footer_as_drawn as Make, s));
    let all = headers.chain(footers).chain(as_drawn);
    all.flat_map(|(widget, make, &(name, state))| {
        sizes.map(|(size, w)| make(format!("{widget}/{name}-{size}"), w, state))
    })
    .collect()
}
