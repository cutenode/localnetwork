//! The Keys screen's stories (`change the keys`, by name in `:`): each of its stages, a filter that
//! leaves nothing and a key that can't change, the import list, where the keys are kept signed
//! out, published and failing to publish, and a keybinds.json with problems or that can't be
//! read, each 80 and 160 columns wide. Each is the whole player drawn over the demo fixture (see
//! `screens::fixture`), so a story is what the player shows.

use std::cell::RefCell;

use locallibrary::keybinds::Keybinds;
use ratatui::Frame;

use super::Story;
use super::chrome::put;
use crate::tui::app::App;
use crate::tui::app::testing::press;
#[cfg(feature = "account")]
use crate::tui::app::testing::signed_in;
use crate::tui::keybinds::manifests;
use crate::tui::keymap::effective::{self, Effective};
use crate::tui::screens::Fixture;
use crate::tui::ui::draw;

/// A state the player can be put in: its name in a story's id, and how to get there.
type State = (&'static str, fn(&mut App));

/// Signed in only with the account: stories.json lists the default features' stories.
const STATES: &[State] = &[
    ("defaults", |app| app.open_keys()),
    ("filter", |app| {
        app.open_keys();
        press(app, "next");
    }),
    ("no-match", |app| {
        app.open_keys();
        press(app, "zzqx");
    }),
    // The last row ctrl-c finds is a key that can't change.
    ("fixed", |app| {
        app.open_keys();
        press(app, "ctrl-c");
        let rows = crate::tui::keybinds::rows(&effective::current(), &manifests(), "ctrl-c");
        app.keys.as_mut().unwrap().row = rows.len() - 1;
    }),
    ("capture", |app| capturing(app, "playback.next")),
    // m is the track list's mark: the answer offers mark next track's n.
    ("conflict", |app| {
        capturing(app, "playback.next");
        press(app, "m");
    }),
    ("swap", |app| {
        capturing(app, "playback.next");
        press(app, "m\n");
    }),
    ("reset", |app| {
        app.set_keybinds(keys(&[("playback.next", &["]"])]));
        capturing(app, "playback.next");
        press(app, "n");
    }),
    ("refused", |app| {
        capturing(app, "playback.next");
        press(app, "?");
    }),
    // Someone's keys, with one action this version doesn't know and one binding it couldn't read.
    ("import-preview", |app| {
        let theirs = keys(&[("playback.next", &["]"]), ("playback.previous", &["["]), ("future.action", &["f9"])]);
        app.preview_keys("Keys from @dj.example.com".into(), theirs, Some("did:plc:dj".into()), 1);
    }),
    ("signed-out", changed),
    #[cfg(feature = "account")]
    ("signed-in", |app| {
        changed(app);
        app.profile.account.info = Some(signed_in());
        app.publishing.published = true;
    }),
    #[cfg(feature = "account")]
    ("publish-error", |app| {
        changed(app);
        app.profile.account.info = Some(signed_in());
        app.publishing.error = Some("pds.example didn't answer: connection refused".into());
    }),
    // A keybinds.json with three keys, two of them alt- keys the player doesn't take.
    ("file-problems", |app| {
        let bad = keys(&[("playback.next", &["]"]), ("playback.stop", &["alt-x"]), ("playback.shuffle", &["alt-y"])]);
        locallibrary::keybinds::save(&app.store_root, &bad).unwrap();
        loaded(app);
    }),
    ("file-unreadable", |app| {
        let file = locallibrary::store::player_dir(&app.store_root).unwrap().join(locallibrary::store::KEYBINDS_FILE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{ not json").unwrap();
        loaded(app);
    }),
];

fn keys(pairs: &[(&str, &[&str])]) -> Keybinds {
    let mut out = Keybinds::default();
    for (id, keys) in pairs {
        out.keys.insert(id.to_string(), keys.iter().map(|k| k.to_string()).collect());
    }
    out
}

/// The Keys screen narrowed to `typed`, waiting for the key for its first row.
fn capturing(app: &mut App, typed: &str) {
    app.open_keys();
    press(app, typed);
    press(app, "\n");
}

/// keybinds.json read as the player starts, then the Keys screen opened. The toasts it raises are
/// dismissed: one names the file's path, which is a new temporary folder each run.
fn loaded(app: &mut App) {
    app.load_keybinds();
    (app.toast, app.held) = (None, None);
    app.open_keys();
}

/// Next and previous track on ] and [, the screen open on next track.
fn changed(app: &mut App) {
    app.set_keybinds(keys(&[("playback.next", &["]"]), ("playback.previous", &["["])]));
    app.open_keys();
    let rows = crate::tui::keybinds::rows(&effective::current(), &manifests(), "");
    let next = rows.iter().position(|r| r.id.as_deref() == Some("playback.next"));
    app.keys.as_mut().unwrap().row = next.unwrap();
}

/// The player in `state`, `size` wide and high. The keys in effect are kept per thread, so each
/// story puts its own in while it's drawn and the defaults back after, for the stories after it.
fn story(id: String, (cols, rows): (u16, u16), state: fn(&mut App)) -> Story<'static> {
    let at: RefCell<Fixture> = put(state);
    effective::install(Effective::defaults());
    Story::still(Box::leak(id.into_boxed_str()), "KeysDialog", (cols, rows), move |f: &mut Frame| {
        let app = &mut at.borrow_mut().app;
        effective::install(Effective::new(&app.keybinds, &manifests()));
        draw(f, app);
        effective::install(Effective::defaults());
    })
}

/// Every state, narrow and wide.
pub(super) fn stories() -> Vec<Story<'static>> {
    let sizes = [("narrow", (80, 24)), ("wide", (160, 45))];
    let all = STATES.iter().flat_map(|&(name, state)| sizes.map(move |(size, at)| (name, size, at, state)));
    all.map(|(name, size, at, state)| story(format!("keys/{name}-{size}"), at, state)).collect()
}
