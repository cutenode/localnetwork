//! Property tests of the player as a whole: random keys, words, clicks and terminal sizes on the
//! fixture drive with every view's data in (see `screens.rs`), with what must always hold checked
//! after every step: the screen draws at any size with its header and footer, what's selected is
//! there, the list shown is the library's, sorted as it says; and at the end, esc gets back out of
//! whatever is open and q quits.
//!
//! The walks stay off the machine: nothing they press opens a browser, signs in, reads the
//! machine's drives, goes online, ejects or writes outside the test's folder (see [`allowed`]).

use std::ffi::OsString;
use std::path::Path;
use std::sync::OnceLock;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};
use rustc_hash::FxHashSet;

use super::app::testing::{Template, mouse, overlay, wait};
use super::app::{App, Focus, View};
use super::browse;
use super::edits::{Modal, PromptFor};
use super::keymap::Act;
use super::palette::{Then, fuzzy};
use super::profile::Scope;
use super::screens::{Fixture, fixture, shown};
use super::ui::screen;

/// Keys a walk presses, as [`key_event`] reads them; the common ones more than once, to come up
/// more often.
#[rustfmt::skip]
const KEYS: &[&str] = &[
    "j", "j", "k", "<up>", "<down>", "<down>", "<pgup>", "<pgdn>", "<home>", "<end>", "g", "G", "<left>", "<right>",
    "<tab>", "<tab>", "<backtab>", "<enter>", "<enter>", "<enter>", "<esc>", "<esc>", "<del>", "<bs>", "<space>",
    "/", "b", "B", "*", "#", "i", "m", "M", "a", "t", "e", "e", "T", "K", "J", "<S-up>", "<S-down>", "w", "X", "u",
    "f", "v", "[", "]", "+", "-", "s", "r", "x", "o", "'", "c", "C", "N", "P", ",", ".", "<", ">", "{", "}", "Q",
    "l", "(", ")", "S", "A", "d", "z", "E", "O", "D", "h", "H", "n", "p", "y", "y", "q", "1", "2", "3", "4", "5",
    "0", "?", "!", ":", "R", "<f5>", "<ins>", "<C-c>",
];

/// Words a walk types: commands for the palette, names for prompts, searches and filters.
#[rustfmt::skip]
const WORDS: &[&str] = &[
    "Friday", "loop", "sort", "rating", "colour", "playlist", "undo", "pull", "check", "export", "smart",
    "Sets/New", "Genre/Deep House", "bpm:120-130", "rating:>=3", "artist:floor", "key:8A", "tag:Genre/House", "-x",
    "playlist:Nope", "(", "zzz", "日本 😹", "Demo", "loopmasters",
];

/// The key `name` (a character, or a key named in angle brackets) as a terminal sends it.
fn key_event(name: &str) -> KeyEvent {
    let (code, mods) = match name {
        "<up>" => (KeyCode::Up, KeyModifiers::NONE),
        "<down>" => (KeyCode::Down, KeyModifiers::NONE),
        "<S-up>" => (KeyCode::Up, KeyModifiers::SHIFT),
        "<S-down>" => (KeyCode::Down, KeyModifiers::SHIFT),
        "<left>" => (KeyCode::Left, KeyModifiers::NONE),
        "<right>" => (KeyCode::Right, KeyModifiers::NONE),
        "<pgup>" => (KeyCode::PageUp, KeyModifiers::NONE),
        "<pgdn>" => (KeyCode::PageDown, KeyModifiers::NONE),
        "<home>" => (KeyCode::Home, KeyModifiers::NONE),
        "<end>" => (KeyCode::End, KeyModifiers::NONE),
        "<tab>" => (KeyCode::Tab, KeyModifiers::NONE),
        "<backtab>" => (KeyCode::BackTab, KeyModifiers::SHIFT),
        "<enter>" => (KeyCode::Enter, KeyModifiers::NONE),
        "<esc>" => (KeyCode::Esc, KeyModifiers::NONE),
        "<del>" => (KeyCode::Delete, KeyModifiers::NONE),
        "<bs>" => (KeyCode::Backspace, KeyModifiers::NONE),
        "<ins>" => (KeyCode::Insert, KeyModifiers::NONE),
        "<space>" => (KeyCode::Char(' '), KeyModifiers::NONE),
        "<f5>" => (KeyCode::F(5), KeyModifiers::NONE),
        "<C-c>" => (KeyCode::Char('c'), KeyModifiers::CONTROL),
        s => {
            let c = s.chars().next().unwrap();
            (KeyCode::Char(c), if c.is_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE })
        }
    };
    KeyEvent::new(code, mods)
}

/// What the palette mustn't run here: signing in, profiling every drive mounted on the machine,
/// opening a report in the browser.
const DENIED: [Act; 3] = [Act::ProfileAccount, Act::ProfileScope, Act::MixReport];

fn denied_pick(then: &Then, item: usize) -> bool {
    matches!(then, Then::Act(acts) if DENIED.contains(&acts[item]))
}

/// Whether a walk may press `key` now (see the module's doc).
fn allowed(app: &App, key: KeyEvent) -> bool {
    let enter = key.code == KeyCode::Enter;
    // A path or a handle being typed: enter would read a folder of the machine's, or sign in.
    if app.mix.input.is_some() || app.profile.account.typing() {
        return !enter;
    }
    if app.help || app.show_log {
        return true;
    }
    match &app.modal {
        Some(Modal::Confirm(c)) => {
            return !(matches!(key.code, KeyCode::Char('y' | 'Y')) && c.plan.operation() == "eject");
        }
        Some(Modal::Pick(p)) => return !(enter && p.selected().is_some_and(|i| denied_pick(&p.then, i))),
        // Providers go online (the cover art plugin to MusicBrainz).
        Some(Modal::Form(f)) => return !(enter && matches!(f.then, super::form::For::Fetch { .. })),
        Some(_) => return true,
        None => {}
    }
    if app.input_taken() {
        return true;
    }
    let ch = |c| key.code == KeyCode::Char(c);
    match app.view {
        View::Profile => !(ch('L') || ch('a')),
        View::Mixes => !ch('w'),
        _ => true,
    }
}

/// Whether a walk may click `kind` at `at`: not on a command the palette mustn't run.
fn click_allowed(app: &App, kind: MouseEventKind, (column, row): (u16, u16)) -> bool {
    let Some(Modal::Pick(p)) = &app.modal else { return true };
    let r = app.areas.menu;
    // Where the picker takes a click to choose (see `on_picker_mouse`).
    let i = row.saturating_sub(r.y + 2) as usize + p.state.offset();
    let on_item = r.contains(Position::new(column, row)) && row >= r.y + 2 && i < p.shown.len();
    !(kind == MouseEventKind::Down(MouseButton::Left) && on_item && denied_pick(&p.then, p.shown[i]))
}

/// An export about to be written goes into the test's folder, not `~/Music`.
fn keep_exports_in(app: &mut App, dir: &Path) {
    if let Some(Modal::Prompt(p)) = &mut app.modal
        && matches!(p.purpose, PromptFor::ExportList { .. })
    {
        let ext = ["m3u8", "csv", "txt"][p.text.len() % 3];
        p.text = dir.join(format!("exported.{ext}")).display().to_string();
    }
}

/// Wait for the job and the reads under way, and let a confirmation take keys at once (it
/// ignores them for a moment from when it shows).
fn settle(app: &mut App) {
    // A sign-in from the palette waits for the browser, which a test never opens: it's stopped,
    // as Esc stops it.
    if app.job.as_ref().is_some_and(|j| j.label == "Signing in") {
        app.cancel_job();
    }
    wait(app, |a| a.job.is_none() && a.loading.is_none());
    if let Some(Modal::Confirm(c)) = &mut app.modal {
        c.shown -= std::time::Duration::from_secs(1);
    }
}

/// One step of a walk.
enum Step {
    Key(&'static str),
    Type(&'static str),
    Click(u8, u16, u16),
    Resize(u16, u16),
    Settle,
}

#[hegel::test_helper]
fn draw_size(tc: &TestCase) -> (u16, u16) {
    let w = tc.draw(gs::integers::<u16>().min_value(1).max_value(220));
    let h = tc.draw(gs::integers::<u16>().min_value(1).max_value(70));
    (w, h)
}

#[hegel::test_helper]
fn draw_step(tc: &TestCase, keys: &'static [&'static str], (w, h): (u16, u16)) -> Step {
    match tc.draw(gs::integers::<u8>().max_value(19)) {
        0..=12 => Step::Key(tc.draw(gs::sampled_from(keys))),
        13 => Step::Type(tc.draw(gs::sampled_from(WORDS))),
        14..=16 => {
            let kind = tc.draw(gs::integers::<u8>().max_value(4));
            let x = tc.draw(gs::integers::<u16>().max_value(w - 1));
            let y = tc.draw(gs::integers::<u16>().max_value(h - 1));
            Step::Click(kind, x, y)
        }
        17 => {
            let (w, h) = draw_size(tc);
            Step::Resize(w, h)
        }
        _ => Step::Settle,
    }
}

/// Take `step`, as a person would (but not where [`allowed`] says not to).
fn take(app: &mut App, step: &Step, size: &mut (u16, u16), dir: &Path) {
    match *step {
        Step::Key(name) => {
            let key = key_event(name);
            if allowed(app, key) {
                if key.code == KeyCode::Enter {
                    keep_exports_in(app, dir);
                }
                app.on_key(key);
            }
        }
        Step::Type(word) => {
            for c in word.chars() {
                let key = key_event(&c.to_string());
                if allowed(app, key) {
                    app.on_key(key);
                }
            }
        }
        Step::Click(kind, x, y) => {
            let kind = match kind {
                0 => MouseEventKind::Down(MouseButton::Left),
                1 => MouseEventKind::Down(MouseButton::Right),
                2 => MouseEventKind::ScrollUp,
                3 => MouseEventKind::ScrollDown,
                _ => MouseEventKind::Drag(MouseButton::Left),
            };
            // A click lands on what was drawn last, at the size it was drawn.
            let at = (x.min(size.0 - 1), y.min(size.1 - 1));
            if click_allowed(app, kind, at) {
                mouse(app, kind, at);
            }
        }
        Step::Resize(w, h) => *size = (w, h),
        Step::Settle => settle(app),
    }
    // What the background has sent, as the event loop runs it between keys.
    app.tick();
}

/// What must hold of the screen and the app at any moment, drawn at `size`.
fn check(app: &mut App, (w, h): (u16, u16)) {
    let s = screen(app, w, h);
    let lines: Vec<&str> = s.lines().collect();
    assert_eq!(lines.len(), h as usize, "{w}x{h}");
    if !overlay(app) && w >= 80 && h >= 3 {
        assert!(lines[0].contains("◉ locallibrary"), "no header at {w}x{h}:\n{s}");
        assert!(!lines[h as usize - 1].trim().is_empty(), "no footer at {w}x{h}:\n{s}");
    }
    // What clicks are matched against is where it was drawn, on the screen.
    let screen_rect = Rect::new(0, 0, w, h);
    let a = &app.areas;
    let named = [
        ("drives", a.drives),
        ("playlists", a.playlists),
        ("tracks", a.tracks),
        ("details", a.details),
        ("matches", a.matches),
        ("crate", a.crate_panel),
        ("crate rows", a.crate_rows),
        ("menu", a.menu),
        ("overview", a.overview),
    ];
    // The Drives and Snapshots views' lists are only looked at there.
    let own = match app.view {
        View::Drives => Some(("drive list", a.drive_list)),
        View::Snapshots => Some(("snapshots", a.history)),
        _ => None,
    };
    for (what, r) in named.into_iter().chain(own).chain(a.columns.iter().map(|c| ("column", c.1))) {
        assert!(r.is_empty() || screen_rect.intersection(r) == r, "{what} {r:?} is off the {w}x{h} screen:\n{s}");
    }
    for (v, r) in &a.tabs {
        assert_eq!(r.y, 0, "the {v:?} tab");
    }
    state(app);
}

/// What must hold of the app's state at any moment.
fn state(app: &mut App) {
    let within = |sel: Option<usize>, len: usize, what: &str| {
        assert!(sel.is_none_or(|s| s < len), "{what}: {sel:?} selected of {len}");
    };
    within(app.crate_state.selected(), app.crate_list.len(), "crate");
    within(app.drive_state.selected(), app.drives.len(), "drives");
    // Snapshots dropped to be read again (after a save) keep the selection until they're back.
    if app.history.is_some() {
        within(app.history_state.selected(), app.history_rows(), "snapshots");
    }
    assert!((0.0..=1.5).contains(&app.volume), "volume {}", app.volume);
    assert!((1.5..=60.0).contains(&app.deck_secs), "deck zoom {}", app.deck_secs);
    if app.focus == Focus::Details {
        assert!(app.show_details, "focus on the details, hidden");
    }
    if app.focus == Focus::Crate {
        assert!(app.show_crate, "focus on the crate, hidden");
    }
    match &app.modal {
        Some(Modal::Menu(m)) => within(m.state.selected(), m.items.len(), "menu"),
        Some(Modal::Pick(p)) => {
            within(p.state.selected(), p.shown.len(), "picker");
            for &i in &p.shown {
                let it = &p.items[i];
                let found = fuzzy(&p.query, &it.label).or(fuzzy(&p.query, &it.detail));
                assert!(found.is_some(), "{:?} shown for {:?}", it.label, p.query);
            }
        }
        _ => {}
    }
    let (sort, searching, query, search_all, error) =
        (app.sort, app.searching, app.query.clone(), app.search_all, app.filter_error.is_some());
    let Some(l) = &mut app.loaded else { return };
    let n = l.library.tracks.len();
    within(app.playlist_state.selected(), l.tree.len(), "tree");
    within(app.track_state.selected(), app.visible.len(), "tracks");
    assert!(app.visible.iter().all(|&i| i < n), "{:?} of {n} tracks", app.visible);
    let paths: FxHashSet<&str> = l.library.tracks.iter().map(|t| t.file_path.as_str()).collect();
    for m in &app.marks {
        assert!(paths.contains(m.as_str()), "marked {m:?}, not on the drive");
    }
    // With nothing typed, the list is the playlist's (or folder's) own, in the order it says.
    if query.trim().is_empty() {
        let mut base = l.base();
        if let Some(k) = sort {
            browse::sort_list(&mut base, &browse::ranks(&l.library, k.field), k.descending);
        }
        assert_eq!(app.visible, base, "the list shown, sorted by {sort:?}");
    } else if let Some(k) = sort.filter(|_| !(searching && error)) {
        let ranks = browse::ranks(&l.library, k.field);
        let mut sorted = app.visible.clone();
        browse::sort_list(&mut sorted, &ranks, k.descending);
        let order = |v: &[usize]| v.iter().map(|&i| ranks[i]).collect::<Vec<_>>();
        assert_eq!(order(&app.visible), order(&sorted), "the search {query:?} (all: {search_all}), by {sort:?}");
    }
}

/// Esc, as often as it takes, puts away whatever's open (a dialog, a typed line, the help, full
/// screen) once the background is done; then q quits.
fn back_out(app: &mut App) {
    let out = |a: &App| !a.input_taken() && !a.fullscreen;
    settle(app);
    // A quit asked for while the drive was written happens once the write is done: the walk
    // ended there.
    if app.quit {
        return;
    }
    for _ in 0..40 {
        if out(app) {
            break;
        }
        app.on_key(key_event("<esc>"));
        app.tick();
    }
    assert!(out(app), "esc didn't get out: modal {}, searching {}", app.modal.is_some(), app.searching);
    assert!(!app.quit, "esc quit");
    app.on_key(key_event("q"));
    assert!(app.quit, "q didn't quit");
}

/// The fixture's drive and store, and the drive's folder name: made once (pulling the drive into
/// the store takes most of the time) and written out for each case.
fn template() -> &'static (Template, OsString) {
    static TEMPLATE: OnceLock<(Template, OsString)> = OnceLock::new();
    TEMPLATE.get_or_init(|| {
        let f = fixture();
        (Template::of(f._dir.path()), f.root.file_name().unwrap().to_owned())
    })
}

/// The fixture, its profile read from its own drive only, its Mixes view answering to it.
pub(super) fn walk_fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let (template, name) = template();
    let root = template.write(dir.path()).join(name);
    let mut f = shown(dir, root);
    let name = f.app.loaded.as_ref().unwrap().device.name();
    f.app.profile.scope = Scope::Drive(f.root.clone(), name);
    f.app.mix = super::mix::tests::tab_on(f.app.sender());
    f
}

/// So many steps with `keys`, checking the player after each, then backing out (see [`back_out`]).
/// A walk that quits (q, ctrl-c) ends there, as the player does.
fn walk(tc: &TestCase, app: &mut App, dir: &Path, keys: &'static [&'static str], steps: usize) {
    let mut size = draw_size(tc);
    check(app, size);
    for _ in 0..steps {
        let step = draw_step(tc, keys, size);
        take(app, &step, &mut size, dir);
        check(app, size);
        if app.quit {
            return;
        }
    }
    back_out(app);
}

/// A walk in the view `start`, with these keys.
fn walk_in(tc: &TestCase, start: &str, keys: &'static [&'static str], steps: usize) {
    use super::theme::{Mode, set_test_mode};
    let Fixture { _dir, mut app, .. } = walk_fixture();
    // On a dark terminal, a light one, or one without colours.
    set_test_mode(Some(tc.draw(gs::sampled_from(&[Mode::Dark, Mode::Light, Mode::Plain][..]))));
    app.on_key(key_event(start));
    walk(tc, &mut app, _dir.path(), keys, steps);
    set_test_mode(None);
}

/// Anything pressed, typed or clicked, in any view, at any size, keeps the player whole.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn any_walk_keeps_the_player_whole(tc: TestCase) {
    walk_in(&tc, "1", KEYS, 30);
}

/// The library's keys: every key but those for the other views.
#[rustfmt::skip]
const LIBRARY_KEYS: &[&str] = &[
    "j", "j", "k", "<up>", "<down>", "<pgup>", "<pgdn>", "<home>", "<end>", "g", "G", "<left>", "<right>", "<tab>",
    "<tab>", "<backtab>", "<enter>", "<enter>", "<enter>", "<esc>", "<esc>", "<del>", "<bs>", "<space>", "/", "/",
    "b", "B", "*", "#", "i", "m", "M", "a", "t", "e", "e", "e", "T", "K", "J", "<S-up>", "<S-down>", "w", "X", "u",
    "f", "v", "[", "]", "+", "-", "s", "r", "x", "o", "'", "c", "C", "N", "P", ",", ".", "<", ">", "{", "}", "Q",
    "l", "(", ")", "S", "A", "d", "n", "p", "y", "y", "y", "0", "3", "5", "?", "!", ":", ":", "R", "<ins>",
];

/// The library: browsing, searching, sorting, marking, the crate, the menus, dialogs and edits.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn library_walks_keep_the_player_whole(tc: TestCase) {
    walk_in(&tc, "1", LIBRARY_KEYS, 30);
}

/// What a library walk found: the outputs list (`d`) in a terminal three rows tall is drawn over
/// the header, as a dialog is.
#[test]
fn the_outputs_list_may_cover_the_header() {
    let Fixture { _dir, mut app, .. } = walk_fixture();
    app.on_key(key_event("1"));
    app.on_key(key_event("d"));
    assert!(screen(&mut app, 80, 3).contains("the system's default"));
    check(&mut app, (80, 3));
}

/// The Drives and Snapshots views' keys, and their menus and dialogs'.
const DRIVES_KEYS: &[&str] = &[
    "j", "k", "<up>", "<down>", "<pgup>", "<pgdn>", "g", "G", "<enter>", "<enter>", "e", "e", "e", "<esc>", "u", "R",
    "y", "y", "n", "2", "2", "3", "<tab>", "?", "!", ":", "<bs>",
];

/// The Drives and Snapshots views: their lists, the drive's actions and what they plan.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn drives_walks_keep_the_player_whole(tc: TestCase) {
    walk_in(&tc, "2", DRIVES_KEYS, 25);
}

/// The Profile view's keys.
const PROFILE_KEYS: &[&str] = &[
    "j", "j", "k", "<up>", "<down>", "<pgup>", "<pgdn>", "g", "G", "<tab>", "<tab>", "<enter>", "<esc>", "i", "m", "h",
    "H", "D", "D", "w", "#", "R", "?", ":",
];

/// The Profile view: its sections and lists, what goes in the document, pins and hidden names.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn profile_walks_keep_the_player_whole(tc: TestCase) {
    walk_in(&tc, "4", PROFILE_KEYS, 25);
}

/// The Mixes view's keys.
const MIX_KEYS: &[&str] = &[
    "j", "j", "k", "<up>", "<down>", "<pgup>", "<pgdn>", "g", "G", "<tab>", "<tab>", "<enter>", "<esc>", "<space>",
    "n", "p", "O", "z", "z", "<del>", "o", "E", "x", "R", "?", ":", "<bs>",
];

/// The Mixes view: the recordings, the set's timeline and tracks, playing from them.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn mix_walks_keep_the_player_whole(tc: TestCase) {
    walk_in(&tc, "5", MIX_KEYS, 25);
}

/// The keys that turn something on or off, or step through settings, as the library takes them.
const TOGGLES: &[&str] =
    &["*", "#", "i", "t", "s", "r", "v", "f", "[", "]", "+", "-", "Q", "S", "A", "'", "<tab>", "<backtab>"];

/// What the toggles set, as a person keeping count would see it.
#[derive(Debug, Clone, PartialEq)]
struct Toggles {
    ratings: bool,
    tags: bool,
    details: bool,
    crate_panel: bool,
    shuffle: bool,
    /// Off, all, one.
    repeat: u8,
    /// Spectrum, mirror, scope, deck.
    vis: u8,
    fullscreen: bool,
    quantize: bool,
    single: bool,
    auto_cue: bool,
    pads: bool,
    focus: Focus,
}

impl Toggles {
    fn of(app: &App) -> Toggles {
        use super::app::{Repeat, Vis};
        Toggles {
            ratings: app.show_ratings,
            tags: app.show_tags_column,
            details: app.show_details,
            crate_panel: app.show_crate,
            shuffle: app.shuffle,
            repeat: match app.repeat {
                Repeat::Off => 0,
                Repeat::All => 1,
                Repeat::One => 2,
            },
            vis: match app.vis {
                Vis::Bars => 0,
                Vis::Mirror => 1,
                Vis::Scope => 2,
                Vis::Deck => 3,
            },
            fullscreen: app.fullscreen,
            quantize: app.deck.quantize,
            single: app.deck.single,
            auto_cue: app.deck.auto_cue,
            pads: app.deck.pads,
            focus: app.focus,
        }
    }

    fn press(&mut self, key: &str) {
        match key {
            "*" => self.ratings = !self.ratings,
            "#" => self.tags = !self.tags,
            "i" => {
                self.details = !self.details;
                if !self.details && self.focus == Focus::Details {
                    self.focus = Focus::Tracks;
                }
            }
            "t" => {
                self.crate_panel = !self.crate_panel;
                if !self.crate_panel && self.focus == Focus::Crate {
                    self.focus = Focus::Tracks;
                }
            }
            "s" => self.shuffle = !self.shuffle,
            "r" => self.repeat = (self.repeat + 1) % 3,
            "v" => self.vis = (self.vis + 1) % 4,
            "f" => self.fullscreen = !self.fullscreen,
            "Q" => self.quantize = !self.quantize,
            "S" => self.single = !self.single,
            "A" => self.auto_cue = !self.auto_cue,
            "'" => self.pads = !self.pads,
            "<tab>" | "<backtab>" => {
                let mut order = vec![Focus::Drives, Focus::Playlists, Focus::Tracks];
                order.extend(self.details.then_some(Focus::Details));
                order.extend(self.crate_panel.then_some(Focus::Crate));
                let n = order.len();
                let i = order.iter().position(|f| *f == self.focus).unwrap();
                self.focus = order[if key == "<tab>" { (i + 1) % n } else { (i + n - 1) % n }];
            }
            _ => {}
        }
    }
}

/// Each toggle flips what it says it does, and nothing else, in any order, with the volume and
/// the deck's zoom held within their bounds.
#[hegel::test(test_cases = 100)]
fn toggles_do_what_they_say(tc: TestCase) {
    let (mut app, _dir) = super::app::testing::app_with(super::app::testing::library());
    let mut model = Toggles::of(&app);
    let (mut volume, mut zoom) = (app.volume, app.deck_secs);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(40)) {
        let key = tc.draw(gs::sampled_from(TOGGLES));
        app.on_key(key_event(key));
        model.press(key);
        match key {
            "+" => volume = (volume + 0.05).min(1.5),
            "-" => volume = (volume - 0.05).max(0.0),
            "]" => zoom = (zoom / 1.5).max(1.5),
            "[" => zoom = (zoom * 1.5).min(60.0),
            _ => {}
        }
        assert_eq!(Toggles::of(&app), model, "after {key}");
        assert!((app.volume - volume).abs() < 1e-4, "volume {} for {volume}", app.volume);
        assert!((app.deck_secs - zoom).abs() < 1e-9, "zoom {} for {zoom}", app.deck_secs);
    }
    check(&mut app, (100, 30));
}

/// The keys of the marks and the crate, with moving about and between the panes.
const CRATE_KEYS: &[&str] = &["j", "k", "g", "G", "m", "m", "M", "a", "a", "t", "<tab>", "K", "J", "<del>", "X"];

/// The marks and the crate as a person keeping count would see them: which tracks are marked,
/// what's in the crate in its order and which of it is selected, and where the cursor is.
#[derive(Debug)]
struct Crated {
    marks: FxHashSet<String>,
    crate_list: Vec<String>,
    crate_sel: Option<usize>,
    track: usize,
    tree: usize,
    list_marks: Vec<String>,
    shown: bool,
    focus: Focus,
}

/// Marking, unmarking, adding to the crate, moving its tracks and taking them out, and clearing
/// it, in any order, do what a person keeping count expects (with the crate showing or not).
#[hegel::test(test_cases = 100)]
fn marks_and_the_crate_keep_count(tc: TestCase) {
    use super::browse::NodeId;
    let (mut app, _dir) = super::app::testing::app_with(super::app::testing::library());
    let l = app.loaded.as_ref().unwrap();
    let paths: Vec<String> = app.visible.iter().map(|&i| l.library.tracks[i].file_path.clone()).collect();
    let all: Vec<String> = l.library.tracks.iter().map(|t| t.file_path.clone()).collect();
    let tree: Vec<Option<String>> = (l.tree.iter())
        .map(|n| match n.id {
            NodeId::Playlist(id) => Some(l.playlist_path(id)),
            _ => None,
        })
        .collect();
    let mut m = Crated {
        marks: FxHashSet::default(),
        crate_list: Vec::new(),
        crate_sel: None,
        track: 0,
        tree: app.playlist_state.selected().unwrap(),
        list_marks: Vec::new(),
        shown: false,
        focus: Focus::Tracks,
    };
    let clamp = |i: usize, d: isize, n: usize| (i as isize + d).clamp(0, n as isize - 1) as usize;
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(40)) {
        let key = tc.draw(gs::sampled_from(CRATE_KEYS));
        // Deleting in the tree plans a change to the drive: not what's counted here.
        if key == "<del>" && m.focus == Focus::Playlists {
            continue;
        }
        app.on_key(key_event(key));
        let n = paths.len();
        let step = match key {
            "j" => 1,
            "k" => -1,
            "g" => -1000,
            "G" => 1000,
            _ => 0,
        };
        match (key, m.focus) {
            ("j" | "k" | "g" | "G", Focus::Tracks) => m.track = clamp(m.track, step, n),
            ("j" | "k" | "g" | "G", Focus::Playlists) => m.tree = clamp(m.tree, step, tree.len()),
            ("j" | "k" | "g" | "G", Focus::Crate) if !m.crate_list.is_empty() => {
                m.crate_sel = Some(clamp(m.crate_sel.unwrap_or(0), step, m.crate_list.len()))
            }
            ("m", Focus::Tracks) => {
                let p = &paths[m.track];
                if !m.marks.remove(p) {
                    m.marks.insert(p.clone());
                }
                m.track = clamp(m.track, 1, n);
            }
            ("m", Focus::Playlists) => {
                if let Some(p) = &tree[m.tree] {
                    match m.list_marks.iter().position(|q| q == p) {
                        Some(i) => drop(m.list_marks.remove(i)),
                        None => m.list_marks.push(p.clone()),
                    }
                    m.tree = clamp(m.tree, 1, tree.len());
                }
            }
            ("M", _) => {
                if m.marks.is_empty() {
                    m.marks.extend(paths.iter().cloned());
                } else {
                    m.marks.clear();
                }
            }
            ("a", f) if f != Focus::Crate => {
                // The marked tracks, those in the list first, else the one selected.
                let add: Vec<&String> = if m.marks.is_empty() {
                    vec![&paths[m.track]]
                } else {
                    paths.iter().chain(&all).filter(|p| m.marks.contains(*p)).collect()
                };
                for p in add {
                    if !m.crate_list.contains(p) {
                        m.crate_list.push(p.clone());
                    }
                }
                if m.crate_sel.is_none() && !m.crate_list.is_empty() {
                    m.crate_sel = Some(0);
                }
            }
            ("t", _) => {
                m.shown = !m.shown;
                if !m.shown && m.focus == Focus::Crate {
                    m.focus = Focus::Tracks;
                }
                if m.crate_sel.is_none() && !m.crate_list.is_empty() {
                    m.crate_sel = Some(0);
                }
            }
            ("<tab>", f) => {
                let order = [Focus::Drives, Focus::Playlists, Focus::Tracks, Focus::Crate];
                let n = if m.shown { 4 } else { 3 };
                m.focus = order[(order.iter().position(|o| *o == f).unwrap() + 1) % n];
            }
            ("K" | "J", Focus::Crate) => {
                if let Some(i) = m.crate_sel {
                    let to = clamp(i, if key == "K" { -1 } else { 1 }, m.crate_list.len());
                    let p = m.crate_list.remove(i);
                    m.crate_list.insert(to, p);
                    m.crate_sel = Some(to);
                }
            }
            ("<del>", Focus::Crate) => {
                if let Some(i) = m.crate_sel {
                    m.crate_list.remove(i);
                    let len = m.crate_list.len();
                    m.crate_sel = (len > 0).then(|| i.min(len - 1));
                }
            }
            // The crate's own key: only in the crate.
            ("X", Focus::Crate) if !m.crate_list.is_empty() => {
                // It asks first: yes.
                assert!(matches!(app.modal, Some(Modal::Ask(..))), "X didn't ask");
                app.on_key(key_event("y"));
                m.crate_list.clear();
                m.crate_sel = None;
            }
            _ => {}
        }
        let crated: Vec<String> = app.crate_list.items.iter().map(|t| t.track.file_path.clone()).collect();
        assert_eq!(app.marks, m.marks, "marks after {key}");
        assert_eq!(app.list_marks, m.list_marks, "marked playlists after {key}");
        assert_eq!(crated, m.crate_list, "the crate after {key}");
        assert_eq!(app.crate_state.selected(), m.crate_sel, "the crate's selection after {key}");
        assert_eq!(app.track_state.selected(), Some(m.track), "the track selected after {key}");
        assert_eq!(app.playlist_state.selected(), Some(m.tree), "the tree's row after {key}");
        assert_eq!((app.focus, app.show_crate), (m.focus, m.shown), "after {key}");
        assert!(app.modal.is_none(), "a dialog after {key}");
    }
    check(&mut app, (120, 30));
}

/// What made-up tracks are called.
const ARTISTS: &[&str] = &["Daft Punk", "Floorplan", "floorplan", "DJ Koze", "Moodymann", ""];
const TITLES: &[&str] = &["Around the World", "Never Grow Old", "Pick Up", "Tell You No Lie", "Old"];
const GENRES: &[&str] = &["House", "Techno", "Deep House", ""];
/// Words to search for: in the tracks, in several of them, in none, negated, either of two, and
/// with nothing to look for.
const TERMS: &[&str] = &[
    "daft",
    "FLOOR",
    "plan",
    "old",
    "the",
    "house",
    "deep",
    "no",
    "o",
    "x",
    "-house",
    "-old",
    "punk|koze",
    "a|zz",
    "|",
    "-|",
];

/// A small library of made-up tracks, and a playlist of some of them in any order.
#[hegel::test_helper]
fn small_library(tc: &TestCase) -> locallibrary::Library {
    use super::app::testing::{strings_from, track};
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(8));
    let tracks: Vec<_> = (1..=n)
        .map(|id| {
            tc.draw(
                loclib_model::test_util::tracks_like(track(id, "", ""))
                    .artist(strings_from(ARTISTS))
                    .title(strings_from(TITLES))
                    .genre(strings_from(GENRES)),
            )
        })
        .collect();
    let mut listed = tc.draw(gs::permutations((1..=n).collect::<Vec<u32>>()));
    listed.truncate(tc.draw(gs::integers::<usize>().max_value(n as usize)));
    let list = locallibrary::Playlist {
        id: 1,
        parent_id: None,
        name: "List".into(),
        is_folder: false,
        sort_order: 0,
        track_ids: listed,
    };
    locallibrary::Library { tracks, playlists: vec![list], ..Default::default() }
}

/// Whether `t` holds every word of `query` (in one of its fields, ignoring case; `a|b` either),
/// and none of the `-word`s, as the filter language reads them: an empty alternative (`a|`) is
/// nothing to look for, a word with nothing to look for is no word, and `-` alone is text.
fn holds(t: &locallibrary::Track, query: &str) -> bool {
    let fields = [&t.artist, &t.title, &t.album, &t.genre, &t.label, &t.key, &t.comment, &t.remixer, &t.mix_name];
    let fields: Vec<String> = fields.iter().map(|f| f.to_lowercase()).chain([t.file_path.to_lowercase()]).collect();
    let has = |w: &str| fields.iter().any(|f| f.contains(&w.to_lowercase()));
    let alts = |w: &str| w.split('|').filter(|a| !a.is_empty()).map(str::to_string).collect::<Vec<_>>();
    query.split_whitespace().all(|word| match word.strip_prefix('-').filter(|not| !not.is_empty()) {
        Some(not) => !alts(not).iter().any(|a| has(a)),
        None => {
            let either = alts(word);
            either.is_empty() || either.iter().any(|a| has(a))
        }
    })
}

/// What the search box shows is the tracks of the list (or, tab, the whole drive) holding every
/// word typed, in the list's order, as it's typed, cleared and typed again; esc clears it.
#[hegel::test(test_cases = 100)]
fn the_search_shows_what_holds_the_words(tc: TestCase) {
    use super::browse::NodeId;
    let lib = small_library(&tc);
    let (mut app, _dir) = super::app::testing::app_with(lib.clone());
    if tc.draw(gs::booleans()) {
        let at = app.loaded.as_ref().unwrap().tree_position(&NodeId::Playlist(1));
        app.focus = Focus::Playlists;
        app.playlist_state.select(at);
        app.on_key(key_event("<enter>"));
    }
    let on_list = matches!(app.loaded.as_ref().unwrap().node, NodeId::Playlist(_));
    let list: Vec<usize> = match on_list {
        true => lib.playlists[0].track_ids.iter().map(|&id| id as usize - 1).collect(),
        false => (0..lib.tracks.len()).collect(),
    };
    app.on_key(key_event("/"));
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        match tc.draw(gs::integers::<u8>().max_value(5)) {
            0..=2 => {
                let term = tc.draw(gs::sampled_from(TERMS));
                for c in format!(" {term}").chars() {
                    app.on_key(key_event(&c.to_string()));
                }
            }
            3 => app.on_key(key_event("<bs>")),
            4 => app.on_key(key_event("<tab>")),
            _ => {
                app.on_key(key_event("<esc>"));
                assert!(!app.searching && app.query.is_empty(), "esc leaves the search empty");
                assert_eq!(app.visible, list, "esc shows the whole list again");
                app.on_key(key_event("/"));
            }
        }
        assert!(app.searching);
        let query = app.query.clone();
        let from: Vec<usize> =
            if app.search_all && !query.trim().is_empty() { (0..lib.tracks.len()).collect() } else { list.clone() };
        let want: Vec<usize> = from.into_iter().filter(|&i| holds(&lib.tracks[i], &query)).collect();
        assert_eq!(app.visible, want, "searching {query:?} (whole drive: {})", app.search_all);
        let footer = format!("{} match", want.len());
        assert!(screen(&mut app, 100, 20).lines().last().unwrap().contains(&footer), "the footer counts {footer}");
    }
}

/// The player's keys: playback, the deck, the visualisers.
const DECK_KEYS: &[&str] = &[
    "<space>", "<space>", "<left>", "<right>", "n", "n", "p", "x", "s", "r", "S", "A", "c", "C", "N", "P", ",", ".",
    "<", ">", "{", "}", "Q", "l", "l", "(", ")", "f", "1", "2", "4", "v", "[", "]", "o", "d", "<enter>", "<esc>", "+",
    "-", "'",
];

/// What must hold of what's playing: the queue's order is its tracks', each once; what plays is
/// the queue's (once it's open); the playhead and a loop are within the track.
fn playing_holds(app: &App) {
    let q = &app.queue;
    let mut order = q.order.clone();
    order.sort_unstable();
    assert_eq!(order, (0..q.len()).collect::<Vec<_>>(), "the play order is the queue's tracks");
    assert!(q.is_empty() || q.pos < q.len(), "at {} of {}", q.pos, q.len());
    let Some(now) = &app.now else { return };
    let len_ms = now.duration.as_secs_f64() * 1000.0;
    if let Some(l) = app.deck.looped {
        assert!(0.0 <= l.start && l.start < l.end, "{l:?}");
        assert!(len_ms == 0.0 || l.end <= len_ms + 1.0, "{l:?} past the end, {len_ms}");
    }
    if !now.opening {
        assert!(app.position() <= now.duration + std::time::Duration::from_millis(50), "{:?}", app.position());
    }
}

/// Playing through the queue with the player's keys, its time passing between them (half-second
/// tracks at two sample rates, at a tempo to loop and jump by), keeps the queue whole and what
/// plays within it, and n, s and x do what they say.
#[hegel::test(test_cases = 20, suppress_health_check = [HealthCheck::TooSlow])]
fn playing_keeps_the_queue_whole(tc: TestCase) {
    let (_dir, mut app, mut out) = super::app::testing::playing_wavs(&[8000, 8000, 11025, 8000]);
    // At a tempo, so loops and beat jumps go by it (the files have no beat grid).
    let mut lib = (*app.queue.library).clone();
    lib.tracks.iter_mut().for_each(|t| t.bpm = 480.0);
    app.queue.library = std::sync::Arc::new(lib);
    app.now.as_mut().unwrap().track.bpm = 480.0;
    let mut size = draw_size(&tc);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(30)) {
        match tc.draw(gs::integers::<u8>().max_value(9)) {
            // Time passes: so many samples played.
            0..=1 => {
                let n = tc.draw(gs::integers::<usize>().max_value(5000));
                (&mut out).take(n).for_each(drop);
            }
            2 => size = draw_size(&tc),
            3 => settle(&mut app),
            _ => {
                let key = tc.draw(gs::sampled_from(DECK_KEYS));
                let (pos, len, item) = (app.queue.pos, app.queue.len(), app.queue.order.get(app.queue.pos).copied());
                let (repeat, playing) = (app.repeat, app.now.is_some());
                // With the outputs listed (d), a key closes the list instead.
                let free = !app.input_taken();
                app.on_key(key_event(key));
                match key {
                    _ if !free => {}
                    "x" => assert!(app.now.is_none(), "x stops"),
                    // In the library (not full screen, where it queues the pick) n is the next.
                    "n" if !app.fullscreen && playing => {
                        let want = if pos + 1 < len {
                            Some(pos + 1)
                        } else {
                            (repeat != super::app::Repeat::Off).then_some(0)
                        };
                        match want {
                            Some(p) => assert_eq!(app.queue.pos, p, "n from {pos} of {len}"),
                            None => assert!(app.now.is_none(), "n past the end stops"),
                        }
                    }
                    "s" => assert_eq!(app.queue.order.get(app.queue.pos).copied(), item, "s keeps the track"),
                    _ => {}
                }
            }
        }
        app.tick();
        playing_holds(&app);
        check(&mut app, size);
    }
}

/// A drive as edits leave it: each track's rating and colour, and each playlist's tracks.
#[derive(Debug, Clone, PartialEq)]
struct Drive {
    ratings: Vec<u8>,
    colours: Vec<Option<locallibrary::model::TrackColor>>,
    playlists: indexmap::IndexMap<String, Vec<String>>,
}

impl Drive {
    fn of(lib: &locallibrary::Library) -> Drive {
        let lists = lib.playlists.iter().filter(|p| !p.is_folder);
        let path = |id: &u32| lib.tracks.iter().find(|t| t.id == *id).map(|t| t.file_path.clone());
        Drive {
            ratings: lib.tracks.iter().map(|t| t.rating).collect(),
            colours: lib.tracks.iter().map(|t| t.color).collect(),
            playlists: lists
                .map(|p| (lib.playlist_path(p.id), p.track_ids.iter().filter_map(path).collect()))
                .collect(),
        }
    }
}

/// An edit made through the player's keys.
#[derive(Debug, Clone)]
enum Edit {
    Rate(usize, u8),
    Colour(usize, u8),
    NewPlaylist(&'static str),
    AddTo(usize, usize),
    Rename(usize, &'static str),
    Delete(usize),
    RemoveFrom(usize, usize),
    Undo,
}

/// Edits made with the keys (the menu, its pickers and prompts, the tree, undo), each planned,
/// confirmed and written, leave the drive as a model of them says, read back by the player and
/// from the drive itself; an edit that changes nothing asks nothing.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn edits_write_what_they_say(tc: TestCase) {
    use super::browse::{COLOURS, NodeId};
    let Fixture { _dir, mut app, root } = walk_fixture();
    app.focus = Focus::Tracks;
    let read = || Drive::of(&locallibrary::Device::open_export(&root).unwrap().library().unwrap());
    let mut model = read();
    let n = model.ratings.len();
    // First a playlist of every track (marked, then "new playlist with them"), to edit after.
    let every: Vec<String> = app.loaded.as_ref().unwrap().library.tracks.iter().map(|t| t.file_path.clone()).collect();
    for k in ["M", "e", "n"] {
        app.on_key(key_event(k));
    }
    "Friday".chars().for_each(|c| app.on_key(key_event(&c.to_string())));
    app.on_key(key_event("<enter>"));
    yes(&mut app);
    app.on_key(key_event("M"));
    assert!(app.marks.is_empty());
    model.playlists.insert("Friday".into(), every);
    assert_eq!(read(), model, "a new playlist with the tracks marked");
    let mut before: Option<Drive> = None;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let edit = match tc.draw(gs::integers::<u8>().max_value(6)) {
            0 => Edit::Rate(
                tc.draw(gs::integers::<usize>().max_value(n - 1)),
                tc.draw(gs::integers::<u8>().max_value(5)),
            ),
            1 => Edit::Colour(
                tc.draw(gs::integers::<usize>().max_value(n - 1)),
                tc.draw(gs::integers::<u8>().max_value(8)),
            ),
            2 => Edit::NewPlaylist(tc.draw(gs::sampled_from(&["Friday", "Warm up", "Friday"][..]))),
            3 if !model.playlists.is_empty() => {
                let list = tc.draw(gs::integers::<usize>().max_value(model.playlists.len() - 1));
                Edit::AddTo(tc.draw(gs::integers::<usize>().max_value(n - 1)), list)
            }
            4 => Edit::Undo,
            5 | 6 if !model.playlists.is_empty() => {
                let list = tc.draw(gs::integers::<usize>().max_value(model.playlists.len() - 1));
                match tc.draw(gs::integers::<u8>().max_value(2)) {
                    0 => Edit::Rename(list, tc.draw(gs::sampled_from(&["Saturday", "Warm up"][..]))),
                    1 => Edit::Delete(list),
                    _ => match model.playlists.values().nth(list).unwrap().len() {
                        0 => continue,
                        n => Edit::RemoveFrom(list, tc.draw(gs::integers::<usize>().max_value(n - 1))),
                    },
                }
            }
            _ => continue,
        };
        let select = |app: &mut App, i: usize| {
            app.on_key(key_event("g"));
            (0..i).for_each(|_| app.on_key(key_event("j")));
        };
        // The playlist `name` selected in the tree.
        let in_tree = |app: &mut App, name: &str| {
            let l = app.loaded.as_ref().unwrap();
            let id = l.library.playlists.iter().find(|p| l.library.playlist_path(p.id) == name).unwrap().id;
            app.focus = Focus::Playlists;
            app.playlist_state.select(l.tree_position(&NodeId::Playlist(id)));
        };
        let mut want = model.clone();
        let path = |i: usize| app.loaded.as_ref().unwrap().library.tracks[i].file_path.clone();
        match edit {
            Edit::Rate(i, stars) => {
                want.ratings[i] = stars;
                select(&mut app, i);
                app.on_key(key_event("e"));
                app.on_key(key_event(&stars.to_string()));
            }
            Edit::Colour(i, c) => {
                want.colours[i] = (c > 0).then(|| COLOURS[c as usize - 1]);
                select(&mut app, i);
                for k in ["e", "c", &c.to_string()] {
                    app.on_key(key_event(k));
                }
            }
            Edit::NewPlaylist(name) => {
                want.playlists.entry(name.to_string()).or_default();
                app.on_key(key_event("e"));
                app.on_key(key_event("N"));
                name.chars().for_each(|c| app.on_key(key_event(&c.to_string())));
                app.on_key(key_event("<enter>"));
            }
            Edit::AddTo(i, list) => {
                let name = want.playlists.keys().nth(list).unwrap().clone();
                let p = path(i);
                let tracks = want.playlists.get_mut(&name).unwrap();
                if !tracks.contains(&p) {
                    tracks.push(p);
                }
                select(&mut app, i);
                app.on_key(key_event("e"));
                app.on_key(key_event("p"));
                assert!(app.picking.is_some(), "p picks a playlist");
                let l = app.loaded.as_ref().unwrap();
                let id = l.library.playlists.iter().find(|p| l.library.playlist_path(p.id) == name).unwrap().id;
                app.playlist_state.select(l.tree_position(&NodeId::Playlist(id)));
                app.on_key(key_event("<enter>"));
            }
            Edit::Rename(list, to) => {
                let name = want.playlists.keys().nth(list).unwrap().clone();
                if !want.playlists.contains_key(to) {
                    let tracks = want.playlists.shift_remove(&name).unwrap();
                    want.playlists.insert(to.to_string(), tracks);
                }
                in_tree(&mut app, &name);
                app.on_key(key_event("e"));
                app.on_key(key_event("r"));
                let Some(Modal::Prompt(p)) = &mut app.modal else { panic!("no prompt to rename {name}") };
                assert_eq!(p.text, name);
                app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
                to.chars().for_each(|c| app.on_key(key_event(&c.to_string())));
                app.on_key(key_event("<enter>"));
            }
            Edit::Delete(list) => {
                let name = want.playlists.keys().nth(list).unwrap().clone();
                want.playlists.shift_remove(&name);
                in_tree(&mut app, &name);
                app.on_key(key_event("e"));
                app.on_key(key_event("d"));
            }
            Edit::RemoveFrom(list, i) => {
                let name = want.playlists.keys().nth(list).unwrap().clone();
                let p = want.playlists.get_mut(&name).unwrap().remove(i);
                // The playlist shown, its track selected (each of its entries for that file goes).
                want.playlists.get_mut(&name).unwrap().retain(|q| *q != p);
                in_tree(&mut app, &name);
                app.on_key(key_event("<enter>"));
                select(&mut app, i);
                app.on_key(key_event("e"));
                app.on_key(key_event("x"));
            }
            Edit::Undo => {
                want = before.clone().unwrap_or(want);
                app.on_key(key_event("u"));
            }
        }
        settle(&mut app);
        let asked = matches!(app.modal, Some(Modal::Confirm(_)));
        match (&edit, &before) {
            // Only what there is a backup for is undone.
            (Edit::Undo, None) => {
                if asked {
                    app.on_key(key_event("n"));
                }
                want = model.clone();
            }
            _ => assert_eq!(asked, want != model, "{edit:?} asks only to change something: {}", {
                super::app::testing::last_message(&app).to_string()
            }),
        }
        if asked && app.modal.is_some() {
            check(&mut app, (120, 40));
            app.on_key(key_event("y"));
            wait(&mut app, |a| a.job.is_none() && a.loading.is_none());
            assert!(app.modal.is_none() || matches!(app.modal, Some(Modal::Report(_))), "after writing");
        }
        app.on_key(key_event("<esc>"));
        // Back to every track, for the next edit.
        let all = app.loaded.as_ref().unwrap().tree_position(&NodeId::All);
        (app.focus, app.playlist_state) = (Focus::Playlists, ratatui::widgets::ListState::default().with_selected(all));
        app.on_key(key_event("<enter>"));
        let shown = Drive::of(&app.loaded.as_ref().unwrap().library);
        assert_eq!(shown, want, "the player after {edit:?}");
        assert_eq!(read(), want, "the drive after {edit:?}");
        before = match edit {
            Edit::Undo => None,
            _ if want != model => Some(model),
            _ => before,
        };
        model = want;
    }
}

/// Exporting the list shown (every track, or a playlist) writes a file of that list's tracks in
/// its order, in the format its name asks for; writing over a file that's there asks first.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn exports_hold_the_list_shown(tc: TestCase) {
    use super::browse::NodeId;
    let Fixture { _dir, mut app, root } = walk_fixture();
    let tracks = app.loaded.as_ref().unwrap().library.tracks.clone();
    // A playlist of some of the tracks, in any order, made on the drive behind the player's back.
    let mut order: Vec<usize> = (0..tracks.len()).collect();
    if tc.draw(gs::booleans()) {
        order.reverse();
    }
    order.truncate(tc.draw(gs::integers::<usize>().max_value(tracks.len())));
    let paths: Vec<String> = order.iter().map(|&i| tracks[i].file_path.clone()).collect();
    let req = locallibrary::api::EditRequest {
        drive: root.display().to_string(),
        edits: super::edits::create("Exported", false, &paths),
        options: Default::default(),
    };
    locallibrary::api::edit_apply(&req, &locallibrary::Ctx::silent()).unwrap();
    app.on_key(key_event("R"));
    wait(&mut app, |a| a.loading.is_none());
    let list: Vec<&locallibrary::Track> = match tc.draw(gs::booleans()) {
        true => {
            let l = app.loaded.as_ref().unwrap();
            let id = l.library.playlists.iter().find(|p| p.name == "Exported").unwrap().id;
            app.focus = Focus::Playlists;
            app.playlist_state.select(l.tree_position(&NodeId::Playlist(id)));
            app.on_key(key_event("<enter>"));
            order.iter().map(|&i| &tracks[i]).collect()
        }
        false => tracks.iter().collect(),
    };
    let ext = tc.draw(gs::sampled_from(&["m3u8", "csv", "txt"][..]));
    let out = _dir.path().join(format!("out/list.{ext}"));
    for again in [false, true] {
        let name = "export the list";
        command(&mut app, name);
        let Some(Modal::Prompt(p)) = &mut app.modal else { panic!("no prompt for {name}") };
        assert!(p.text.starts_with("~/Music/"), "{:?}", p.text);
        p.text = out.display().to_string();
        app.on_key(key_event("<enter>"));
        if again {
            assert!(matches!(app.modal, Some(Modal::Ask(..))), "writing over {out:?} doesn't ask");
            app.on_key(key_event("y"));
        }
        wait(&mut app, |a| a.job.is_none());
        let text = std::fs::read_to_string(&out)
            .unwrap_or_else(|e| panic!("{e}: {}", super::app::testing::last_message(&app)));
        // Each track of the list, once, in its order: by its file (a text file names it).
        let named = |t: &locallibrary::Track| match ext {
            "txt" => t.title.clone(),
            _ => t.file_path.rsplit('/').next().unwrap().to_string(),
        };
        let mut from = 0;
        for t in &list {
            let name = named(t);
            let at = text[from..].find(&name).unwrap_or_else(|| panic!("{name} isn't after {from} in\n{text}"));
            from += at + name.len();
        }
        for t in tracks.iter().filter(|t| !list.iter().any(|l| l.id == t.id)) {
            assert!(!text.contains(&named(t)), "{} isn't in the list:\n{text}", t.title);
        }
    }
}

/// The drive's My Tags, read from it: each track (by file name) with its tags.
fn tags_on(root: &Path) -> std::collections::BTreeSet<(String, String)> {
    let drive = locallibrary::api::DriveRef { drive: root.display().to_string() };
    let by = locallibrary::api::tags_by_track(&drive).unwrap();
    let name = |p: &str| p.rsplit('/').next().unwrap_or(p).to_lowercase();
    by.iter().flat_map(|(p, tags)| tags.iter().map(move |t| (name(p), t.clone()))).collect()
}

/// Giving the selected (or marked) tracks a new My Tag or one the drive has, and taking one off,
/// with `T`, its list and its prompt, leaves the drive's tags as a model of them says; the list
/// offers to take a tag off just when every track chosen has it.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn my_tags_go_on_and_come_off(tc: TestCase) {
    use super::palette::Then;
    let Fixture { _dir, mut app, root } = walk_fixture();
    app.focus = Focus::Tracks;
    let names: Vec<String> = (app.loaded.as_ref().unwrap().library.tracks.iter())
        .map(|t| t.file_path.rsplit('/').next().unwrap().to_lowercase())
        .collect();
    let mut model = tags_on(&root);
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        // Which tracks: the one selected, or all of them marked.
        let all = tc.draw(gs::booleans());
        app.marks.clear();
        app.on_key(key_event("g"));
        if all {
            app.on_key(key_event("M"));
        }
        let chosen: Vec<&String> = if all { names.iter().collect() } else { vec![&names[0]] };
        app.on_key(key_event("T"));
        wait(&mut app, |a| a.job.is_none());
        let Some(Modal::Pick(p)) = &app.modal else {
            panic!("T lists no tags: {}", super::app::testing::last_message(&app))
        };
        let Then::Tag { choices, .. } = &p.then else { panic!("not the tags") };
        // The list: a new tag, then each of the drive's, to take off just where all have it.
        let drive = locallibrary::api::DriveRef { drive: root.display().to_string() };
        let defined = locallibrary::api::tags_list(&drive).unwrap();
        let defined: Vec<&String> = defined.categories.iter().flat_map(|c| &c.tags).map(|t| &t.path).collect();
        assert_eq!(choices.len(), defined.len() + 1);
        for (item, tag) in p.items[1..].iter().zip(&defined) {
            let on = chosen.iter().all(|n| model.contains(&((*n).clone(), (*tag).clone())));
            let want = if on { format!("take {tag} off") } else { tag.to_string() };
            assert_eq!(item.label, want, "for {chosen:?}");
        }
        let pick = tc.draw(gs::integers::<usize>().max_value(choices.len() - 1));
        let label = p.items[pick].label.clone();
        let tag = match label.strip_prefix("take ").and_then(|l| l.strip_suffix(" off")) {
            Some(tag) => {
                assert!(chosen.iter().all(|n| model.contains(&((*n).clone(), tag.to_string()))), "offers {label:?}");
                for n in &chosen {
                    model.remove(&((*n).clone(), tag.to_string()));
                }
                None
            }
            None if pick == 0 => Some(format!("Genre/{}", tc.draw(gs::sampled_from(&["Deep", "Peak", "Dub"][..])))),
            None => Some(label.clone()),
        };
        if let Some(t) = &tag {
            for n in &chosen {
                model.insert(((*n).clone(), t.clone()));
            }
        }
        for _ in 0..pick {
            app.on_key(key_event("<down>"));
        }
        app.on_key(key_event("<enter>"));
        if pick == 0 {
            let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt for a new tag") };
            assert_eq!(p.text, "Genre/");
            let rest = tag.as_ref().unwrap().strip_prefix("Genre/").unwrap().to_string();
            rest.chars().for_each(|c| app.on_key(key_event(&c.to_string())));
            app.on_key(key_event("<enter>"));
        }
        settle(&mut app);
        if matches!(app.modal, Some(Modal::Confirm(_))) {
            check(&mut app, (100, 30));
            app.on_key(key_event("y"));
            wait(&mut app, |a| a.job.is_none() && a.loading.is_none());
        }
        app.on_key(key_event("<esc>"));
        assert_eq!(tags_on(&root), model, "after {label:?} for {chosen:?}");
    }
}

/// Drives plugged in and out, in any order: the Drives view lists what's mounted, keeps the
/// drive selected while it's there, says which drives were plugged in (not those there at the
/// start), and with nothing open, opens the first drive with a library on it.
#[hegel::test(test_cases = 20, suppress_health_check = [HealthCheck::TooSlow])]
fn the_drive_list_follows_what_is_plugged_in(tc: TestCase) {
    use super::app::Msg;
    let (dir, demo) = super::app::testing::demo_drive();
    let mut pool = vec![locallibrary::Device::open(&demo).unwrap().info()];
    for name in ["EMPTY", "SPARE"] {
        let root = dir.path().join(name);
        std::fs::create_dir(&root).unwrap();
        pool.push(locallibrary::Device::open(&root).unwrap().info());
    }
    let (mut app, _store) = super::app::testing::blank(View::Drives);
    let mut shown: Vec<std::path::PathBuf> = Vec::new();
    let mut first = true;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(8)) {
        if tc.draw(gs::booleans()) {
            app.on_key(key_event(tc.draw(gs::sampled_from(&["j", "k", "g", "G"][..]))));
            continue;
        }
        let mut list = tc.draw(gs::subsequences(&pool[..]));
        if tc.draw(gs::booleans()) {
            list.reverse();
        }
        let selected = app.drive_state.selected().map(|i| app.drives[i].root.clone());
        let was_open = app.loaded.is_some() || app.loading.is_some();
        app.toast = None;
        app.handle_msg(Msg::Drives(list.clone()));
        let roots: Vec<_> = list.iter().map(|d| d.root.clone()).collect();
        assert_eq!(app.drives.iter().map(|d| &d.root).collect::<Vec<_>>(), roots.iter().collect::<Vec<_>>());
        let added: Vec<&std::path::PathBuf> = roots.iter().filter(|r| !first && !shown.contains(r)).collect();
        let said = super::app::testing::last_message(&app).to_string();
        match added.is_empty() {
            true => assert!(!said.contains("plugged in"), "{said:?} with nothing new"),
            false => {
                for r in &added {
                    assert!(said.contains(&app.drive_label(r)), "{said:?} doesn't name {r:?}");
                }
            }
        }
        // The drive opened is selected; else the one selected stays so, else the first.
        let opens = !was_open && roots.contains(&demo);
        let want = match selected.and_then(|s| roots.iter().position(|r| *r == s)) {
            _ if opens => roots.iter().position(|r| *r == demo),
            Some(i) => Some(i),
            None => (!roots.is_empty()).then_some(0),
        };
        assert_eq!(app.drive_state.selected(), want, "the selection");
        if opens {
            assert!(app.loading.is_some(), "the drive with a library isn't opened");
            wait(&mut app, |a| a.loading.is_none());
            assert_eq!(app.loaded.as_ref().map(|l| l.root()), Some(demo.as_path()));
        }
        check(&mut app, (100, 30));
        (shown, first) = (roots, false);
    }
}

/// Press `:` and run the command named `name`.
fn command(app: &mut App, name: &str) {
    app.on_key(key_event(":"));
    name.chars().for_each(|c| app.on_key(key_event(&c.to_string())));
    app.on_key(key_event("<enter>"));
}

/// Say yes to the plan up now, once it's planned, and wait for the write and the reads after it.
fn yes(app: &mut App) {
    settle(app);
    assert!(matches!(app.modal, Some(Modal::Confirm(_))), "no plan: {}", super::app::testing::last_message(app));
    check(app, (120, 40));
    app.on_key(key_event("y"));
    wait(app, |a| a.job.is_none() && a.loading.is_none());
}

/// A playlist saved in a snapshot (with the player's "save a snapshot") comes back from it as it
/// was, its tracks in their order, however it was changed on the drive since: deleted, emptied
/// of some tracks, or sorted.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_playlist_comes_back_from_a_snapshot_as_it_was(tc: TestCase) {
    use super::edits::{create, delete, remove_from, sort};
    use locallibrary::api::{self, EditRequest};
    use locallibrary::query::SortField;
    let Fixture { _dir, mut app, root } = walk_fixture();
    let edit = |edits| {
        let req = EditRequest { drive: root.display().to_string(), edits, options: Default::default() };
        api::edit_apply(&req, &locallibrary::Ctx::silent()).unwrap();
    };
    let lists = |root: &Path| Drive::of(&locallibrary::Device::open_export(root).unwrap().library().unwrap()).playlists;
    let mut paths: Vec<String> =
        app.loaded.as_ref().unwrap().library.tracks.iter().map(|t| t.file_path.clone()).collect();
    if tc.draw(gs::booleans()) {
        paths.reverse();
    }
    edit(create("Keep", false, &paths));
    let kept = lists(&root);
    app.on_key(key_event("R"));
    wait(&mut app, |a| a.loading.is_none());

    let store = locallibrary::Store::open(&app.store_root).unwrap();
    let snapshots = |store: &locallibrary::Store| store.libraries().unwrap().iter().map(|l| l.head).collect::<Vec<_>>();
    let before = snapshots(&store);
    command(&mut app, "save a snapshot of the drive");
    yes(&mut app);
    assert_ne!(snapshots(&store), before, "the snapshot isn't the library's newest");

    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => edit(vec![delete("Keep")]),
        1 => edit(vec![remove_from("Keep", &paths[..1])]),
        // The other way round from how it's listed.
        _ => {
            let by = locallibrary::query::SortKey { field: SortField::Title, descending: paths.is_sorted() };
            edit(vec![sort("Keep", by)])
        }
    }
    app.on_key(key_event("R"));
    wait(&mut app, |a| a.loading.is_none());

    command(&mut app, "bring back a playlist");
    let Some(Modal::Prompt(p)) = &mut app.modal else { panic!("no prompt to name the playlist") };
    p.text.clear();
    "Keep".chars().for_each(|c| app.on_key(key_event(&c.to_string())));
    app.on_key(key_event("<enter>"));
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Pick(p)) = &app.modal else {
        panic!("no snapshots to pick: {}", super::app::testing::last_message(&app))
    };
    assert_eq!(p.items[0].label, "the newest snapshot that has it");
    // The newest that has it, or the snapshot just saved (newest first).
    if tc.draw(gs::booleans()) {
        app.on_key(key_event("<down>"));
    }
    app.on_key(key_event("<enter>"));
    yes(&mut app);
    assert_eq!(lists(&root), kept, "the playlists after bringing Keep back");
}

/// The Drives view of any drives, of any libraries (or none), with whatever the store says of
/// them: it lists each library by name with its drives under it, then the drives of none; the
/// arrows move along those rows; each row's actions are its own; and every drive and library is
/// named on a screen big enough for them all.
#[hegel::test(test_cases = 25, suppress_health_check = [HealthCheck::TooSlow])]
fn the_drives_view_lists_libraries_and_their_drives(tc: TestCase) {
    use super::drives::DriveRow;
    use locallibrary::api::FleetLibrary;
    use locallibrary::compare::Relation;
    use locallibrary::fleet::{DriveLog, KnownDrive, Role, Standing};
    let (mut app, dir) = super::app::testing::blank(View::Drives);
    let names = ["Home", "away", "Club"];
    let libs: Vec<(uuid::Uuid, &str)> = names[..tc.draw(gs::integers::<usize>().max_value(names.len()))]
        .iter()
        .enumerate()
        .map(|(i, n)| (uuid::Uuid::from_u128(i as u128 + 1), *n))
        .collect();
    let mut status = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        let root = dir.path().join(format!("DRIVE{i}"));
        std::fs::create_dir_all(&root).unwrap();
        let lib = tc.draw(gs::optional(gs::integers::<usize>().max_value(names.len() - 1))).and_then(|i| libs.get(i));
        let info = locallibrary::DeviceInfo {
            has_export: tc.draw(gs::booleans()),
            library_id: lib.map(|l| l.0),
            ..locallibrary::Device::open(&root).unwrap().info()
        };
        let mut f = super::app::testing::fleet(info.clone(), lib.map(|l| l.1));
        f.role = tc.draw(gs::optional(gs::sampled_from(&Role::ALL[..])));
        f.status.relation = tc.draw(gs::optional(gs::sampled_from(vec![
            Relation::Identical,
            Relation::LeftNewer,
            Relation::RightNewer,
            Relation::Diverged,
            Relation::Unrelated,
        ])));
        f.status.back_from_set = tc.draw(gs::optional(gs::integers::<usize>().max_value(3)));
        f.status.settings_drift = tc.draw(gs::integers::<usize>().max_value(2));
        f.status.error = tc.draw(gs::booleans()).then(|| "unreadable".to_string());
        (f.read_only, f.low_space) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
        f.exported_over_edits = tc.draw(gs::integers::<usize>().max_value(2));
        f.log = tc.draw(gs::booleans()).then(|| DriveLog { writes: 3, clean_ejects: 1, ..Default::default() });
        app.drives.push(info);
        status.push(f);
    }
    app.library_names = libs.iter().map(|(id, n)| (*id, n.to_string())).collect();
    app.fleet_libraries = (libs.iter())
        .map(|&(id, name)| FleetLibrary {
            id,
            name: name.into(),
            head: None,
            mounted: (app.drives.iter()).filter(|d| d.library_id == Some(id)).map(|d| d.root.clone()).collect(),
            offline: (0..tc.draw(gs::integers::<usize>().max_value(2)))
                .map(|i| KnownDrive {
                    id: uuid::Uuid::new_v4(),
                    name: format!("Spare{i}"),
                    role: Some(Role::Backup),
                    library: Some(name.into()),
                    mounted: None,
                    last_mount: dir.path().join("gone"),
                    last_seen: chrono::Utc::now(),
                    capacity: None,
                    standing: tc.draw(gs::sampled_from(vec![Standing::UpToDate, Standing::Unsaved, Standing::Unknown])),
                    log: DriveLog::default(),
                })
                .collect(),
            identical: tc.draw(gs::optional(gs::booleans())),
            unsaved: tc.draw(gs::booleans()),
        })
        .collect();
    app.status = tc.draw(gs::booleans()).then_some(Ok(status));
    app.drive_state.select(Some(0));

    // The rows: the libraries with drives here, by name, each with its drives; then the rest.
    let mut named: Vec<&(uuid::Uuid, &str)> =
        libs.iter().filter(|l| app.drives.iter().any(|d| d.library_id == Some(l.0))).collect();
    named.sort_by_key(|l| l.1.to_lowercase());
    let mut rows = Vec::new();
    for l in &named {
        rows.push(DriveRow::Library(l.0));
        rows.extend((0..app.drives.len()).filter(|&i| app.drives[i].library_id == Some(l.0)).map(DriveRow::Drive));
    }
    let loose: Vec<DriveRow> =
        (0..app.drives.len()).filter(|&i| app.drives[i].library_id.is_none()).map(DriveRow::Drive).collect();
    if !named.is_empty() && !loose.is_empty() {
        rows.push(DriveRow::Unlinked);
    }
    rows.extend(loose);
    assert_eq!(app.drive_rows(), rows);

    let mut at = rows.iter().position(|r| *r == DriveRow::Drive(0)).unwrap();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(10)) {
        let key = tc.draw(gs::sampled_from(&["j", "k", "g", "G", "<pgdn>", "<pgup>", "e"][..]));
        let last = rows.len() - 1;
        at = match key {
            "j" => (at + 1).min(last),
            "k" => at.saturating_sub(1),
            "g" | "<pgup>" if key == "g" || at < 10 => 0,
            "<pgup>" => at - 10,
            "G" => last,
            "<pgdn>" => (at + 10).min(last),
            _ => at,
        };
        app.toast = None;
        app.on_key(key_event(key));
        match rows[at] {
            DriveRow::Drive(i) => assert_eq!((app.drive_header, app.drive_state.selected()), (None, Some(i)), "{key}"),
            header => assert_eq!(app.drive_header, Some(header), "{key}"),
        }
        if key == "e" {
            match (rows[at], &app.modal) {
                (DriveRow::Library(id), Some(Modal::Pick(p))) => {
                    assert_eq!(p.title, format!("Library · {}", app.library_names[&id]))
                }
                (DriveRow::Drive(i), Some(Modal::Pick(p))) => {
                    assert_eq!(p.title, format!("Drive · {}", app.drive_label(&app.drives[i].root)))
                }
                (DriveRow::Unlinked, None) => {
                    assert_eq!(super::app::testing::last_message(&app), "select one of these drives")
                }
                (row, _) => panic!("e on {row:?}"),
            }
            app.on_key(key_event("<esc>"));
        }
        check(&mut app, draw_size(&tc));
    }
    let s = screen(&mut app, 160, 60);
    for d in &app.drives {
        let name = d.name.clone().unwrap_or_else(|| d.volume_name.clone());
        assert!(s.contains(&name), "{name} isn't listed:\n{s}");
    }
    for l in &named {
        assert!(s.contains(l.1), "{} isn't listed:\n{s}", l.1);
    }
}

/// A smart playlist made from the search (`:` "smart playlist from the search") keeps the
/// search as its rule, and filling it on the drive gives it just the tracks the search showed.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_smart_playlist_holds_what_the_search_showed(tc: TestCase) {
    let Fixture { _dir, mut app, root } = walk_fixture();
    let filter =
        tc.draw(gs::sampled_from(&["title:2", "-title:2", "artist:loopmasters", "bpm:0-999", "rating:>=1"][..]));
    app.on_key(key_event("/"));
    filter.chars().for_each(|c| app.on_key(key_event(&c.to_string())));
    app.on_key(key_event("<enter>"));
    assert_eq!(app.filter_error, None, "{filter}");
    let l = app.loaded.as_ref().unwrap();
    let shown: Vec<String> = app.visible.iter().map(|&i| l.library.tracks[i].file_path.clone()).collect();
    command(&mut app, "smart playlist from the search");
    let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt: {}", super::app::testing::last_message(&app)) };
    let path = p.text.clone();
    assert_eq!(path, format!("Smart/{filter}"));
    app.on_key(key_event("<enter>"));
    wait(&mut app, |a| a.job.is_none());
    yes(&mut app);
    let lists = Drive::of(&locallibrary::Device::open_export(&root).unwrap().library().unwrap()).playlists;
    assert_eq!(lists.get(&path), Some(&shown), "{lists:?}");
    // The store keeps the rule, listed with `:` "smart playlists".
    command(&mut app, "smart playlists: the rules");
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Report(r)) = &app.modal else { panic!("no rules listed") };
    assert!(
        r.lines.iter().any(|(_, l)| l == &path) && r.lines.iter().any(|(_, l)| l.starts_with(filter)),
        "{:?}",
        r.lines
    );
    // The panel scrolls, and any other key closes it.
    for k in ["j", "<pgdn>", "k", "<pgup>", "<space>"] {
        app.on_key(key_event(k));
        assert!(matches!(app.modal, Some(Modal::Report(_))), "{k} closed the panel");
        check(&mut app, draw_size(&tc));
    }
    app.on_key(key_event("x"));
    assert!(app.modal.is_none(), "x didn't close the panel");
}

/// The details panel of whichever track is selected lists what mixes into it, as the library
/// suggests (the other recordings in a key that mixes, near its tempo, best first), names it and
/// the best of them on screen, and enter there queues those tracks from the one picked.
#[hegel::test(test_cases = 20, suppress_health_check = [HealthCheck::TooSlow])]
fn the_details_list_what_mixes_into_the_track(tc: TestCase) {
    use super::app::testing::{app_with, track};
    let keys = ["8A", "9A", "8B", "3B", "Am", "", "7A"];
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(8));
    let tracks = (1..=n)
        .map(|id| {
            let mut t =
                track(id, tc.draw(gs::sampled_from(ARTISTS)), &format!("{} {id}", tc.draw(gs::sampled_from(TITLES))));
            t.key = tc.draw(gs::sampled_from(&keys[..])).into();
            t.bpm = tc.draw(gs::sampled_from(&[0.0, 120.0, 124.0, 126.0, 128.0, 140.0][..]));
            t
        })
        .collect();
    let lib = locallibrary::Library { tracks, ..Default::default() };
    let (mut app, _dir) = app_with(lib.clone());
    let at = tc.draw(gs::integers::<usize>().max_value(n as usize - 1));
    app.on_key(key_event("g"));
    (0..at).for_each(|_| app.on_key(key_event("j")));
    app.on_key(key_event("i"));
    wait(&mut app, |a| a.details_current());
    let d = app.details.as_ref().unwrap();
    let want: Vec<usize> = (super::keys::suggestions(&lib, &lib.tracks[at], 60).iter())
        .map(|s| lib.tracks.iter().position(|t| t.id == s.track.id).unwrap())
        .collect();
    assert_eq!(d.matches.items.iter().map(|m| m.index).collect::<Vec<_>>(), want, "for {:?}", lib.tracks[at]);
    let s = screen(&mut app, 180, 50);
    assert!(s.contains(&lib.tracks[at].title), "the details don't name it:\n{s}");
    if let Some(&best) = want.first() {
        assert!(s.contains(&lib.tracks[best].title), "the details don't list {}:\n{s}", lib.tracks[best].title);
        app.on_key(key_event("<tab>"));
        assert_eq!(app.focus, Focus::Details);
        let pick = tc.draw(gs::integers::<usize>().max_value(want.len() - 1));
        (0..pick).for_each(|_| app.on_key(key_event("j")));
        app.on_key(key_event("<enter>"));
        assert_eq!((&app.queue.items, app.queue.pos), (&want, pick), "enter queues the matches");
    }
    check(&mut app, draw_size(&tc));
}

/// Packing for a set (the playlist shown, the playlists marked in the tree, or the crate, saved
/// as a playlist first) onto another drive leaves that drive with just those tracks, in that
/// playlist and its order.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn packing_puts_the_set_on_the_other_drive(tc: TestCase) {
    use super::browse::NodeId;
    use super::palette::Then;
    let Fixture { _dir, mut app, root } = walk_fixture();
    let mut paths: Vec<String> =
        app.loaded.as_ref().unwrap().library.tracks.iter().map(|t| t.file_path.clone()).collect();
    if tc.draw(gs::booleans()) {
        paths.reverse();
    }
    paths.truncate(tc.draw(gs::integers::<usize>().min_value(1).max_value(paths.len())));
    let req = locallibrary::api::EditRequest {
        drive: root.display().to_string(),
        edits: super::edits::create("Set", false, &paths),
        options: Default::default(),
    };
    locallibrary::api::edit_apply(&req, &locallibrary::Ctx::silent()).unwrap();
    app.on_key(key_event("R"));
    wait(&mut app, |a| a.loading.is_none());
    let target = _dir.path().join("TARGET");
    std::fs::create_dir(&target).unwrap();
    app.drives.push(locallibrary::Device::open(&target).unwrap().info());

    let l = app.loaded.as_ref().unwrap();
    let id = l.library.playlists.iter().find(|p| p.name == "Set").unwrap().id;
    let row = l.tree_position(&NodeId::Playlist(id));
    let name = match tc.draw(gs::integers::<u8>().max_value(2)) {
        // The playlist shown.
        0 => {
            app.focus = Focus::Playlists;
            app.playlist_state.select(row);
            app.on_key(key_event("<enter>"));
            "Set".to_string()
        }
        // Marked in the tree.
        1 => {
            app.focus = Focus::Playlists;
            app.playlist_state.select(row);
            app.on_key(key_event("m"));
            assert_eq!(app.list_marks, ["Set"]);
            "Set".to_string()
        }
        // The crate, in the order added.
        _ => {
            app.focus = Focus::Tracks;
            for p in &paths {
                let i =
                    app.visible.iter().position(|&i| app.loaded.as_ref().unwrap().library.tracks[i].file_path == *p);
                app.track_state.select(i);
                app.on_key(key_event("a"));
            }
            "Packed".to_string()
        }
    };
    command(&mut app, "pack playlists for a set");
    let Some(Modal::Pick(p)) = &app.modal else {
        panic!("nothing to pack: {}", super::app::testing::last_message(&app))
    };
    let Then::PackWhat { .. } = &p.then else { panic!("not what to pack") };
    app.on_key(key_event("<enter>"));
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no drive to pack onto") };
    assert_eq!(p.items.len(), 1, "the other drive, not the one packed from");
    app.on_key(key_event("<enter>"));
    if name == "Packed" {
        // The crate's saved on the drive packed from first.
        let Some(Modal::Prompt(p)) = &mut app.modal else { panic!("no name asked for the crate") };
        p.text = name.clone();
        app.on_key(key_event("<enter>"));
        yes(&mut app);
    }
    yes(&mut app);
    let packed = locallibrary::Device::open_export(&target).unwrap().library().unwrap();
    let on = Drive::of(&packed).playlists;
    assert_eq!(on.get(&name), Some(&paths), "{on:?}");
    let mut files: Vec<&str> = packed.tracks.iter().map(|t| t.file_path.as_str()).collect();
    files.sort_unstable();
    let mut want: Vec<&str> = paths.iter().map(String::as_str).collect();
    want.sort_unstable();
    assert_eq!(files, want, "just the set's tracks");
}

/// The actions menu answers to its keys as it says: the arrows move within it (and its rating
/// and colour pickers), enter or an item's key chooses (an item that can't be done says why and
/// stays), the pickers go back with esc, and esc, q or e closes it.
#[hegel::test(test_cases = 100)]
fn the_menu_moves_and_closes_as_it_says(tc: TestCase) {
    use super::edits::{Action, Sub};
    let (mut app, _dir) = super::app::testing::app_with(super::app::testing::library());
    // On the tracks, or on the tree's rows (a playlist, a folder, Browse).
    if tc.draw(gs::booleans()) {
        app.focus = Focus::Playlists;
        let rows = app.loaded.as_ref().unwrap().tree.len();
        app.playlist_state.select(Some(tc.draw(gs::integers::<usize>().max_value(rows - 1))));
    }
    if tc.draw(gs::booleans()) {
        app.on_key(key_event("M"));
    }
    app.on_key(key_event("e"));
    const MENU_KEYS: &[&str] =
        &["j", "k", "<up>", "<down>", "<left>", "<bs>", "<esc>", "q", "e", "<enter>", "c", "0", "3", "9", "z"];
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        let Some(Modal::Menu(m)) = &app.modal else { break };
        let (items, sel, sub, sub_sel) = (m.items.len(), m.state.selected().unwrap(), m.sub, m.sub_state.selected());
        let item = |i: usize| (m.items[i].action, m.items[i].unavailable.is_some());
        let key = tc.draw(gs::sampled_from(MENU_KEYS));
        // Where it should be after the key: still open, here (`None`: closed or onto what was chosen).
        let rating_ok = m.items.iter().any(|i| i.action == Action::RatingMenu && i.unavailable.is_none());
        let by_key = |c: char| m.items.iter().position(|i| i.key.len() == 1 && i.key.starts_with(c));
        let after: Option<(usize, Option<Sub>, Option<usize>)> = match (sub, key) {
            (Some(_), "<esc>" | "<bs>" | "<left>") => Some((sel, None, sub_sel)),
            (Some(s), "k" | "<up>") => Some((sel, Some(s), Some(sub_sel.unwrap_or(0).saturating_sub(1)))),
            (Some(s), "j" | "<down>") => {
                let last = super::edits::Menu::sub_items(s).len() - 1;
                Some((sel, Some(s), Some((sub_sel.unwrap_or(0) + 1).min(last))))
            }
            (Some(_), "<enter>") => None,
            (Some(Sub::Rating), "0" | "3") | (Some(Sub::Colour), "0" | "3") => None,
            (Some(s), _) => Some((sel, Some(s), sub_sel)),
            (None, "<esc>" | "q" | "e") => None,
            (None, "k" | "<up>") => Some((sel.saturating_sub(1), None, sub_sel)),
            (None, "j" | "<down>") => Some(((sel + 1).min(items - 1), None, sub_sel)),
            (None, "0" | "3") if rating_ok => None,
            (None, k) => {
                let chosen = match k {
                    "<enter>" => Some(sel),
                    "0" | "3" => m.items.iter().position(|i| i.action == Action::RatingMenu),
                    _ => k.chars().next().and_then(by_key),
                };
                match chosen.map(|i| (i, item(i))) {
                    None => Some((sel, None, sub_sel)),
                    Some((i, (_, true))) => Some((i, None, sub_sel)),
                    Some((i, (Action::RatingMenu, _))) => Some((i, Some(Sub::Rating), Some(0))),
                    Some((i, (Action::ColourMenu, _))) => Some((i, Some(Sub::Colour), Some(0))),
                    Some(_) => None,
                }
            }
        };
        app.on_key(key_event(key));
        match (&app.modal, after) {
            (Some(Modal::Menu(m)), Some((sel, sub, sub_sel))) => {
                assert_eq!(
                    (m.state.selected(), m.sub, m.sub_state.selected()),
                    (Some(sel), sub, sub_sel),
                    "after {key}"
                );
            }
            (Some(Modal::Menu(_)), None) => panic!("{key} didn't close the menu"),
            (_, Some(_)) => panic!("{key} closed the menu"),
            (_, None) => {}
        }
        check(&mut app, draw_size(&tc));
    }
}
