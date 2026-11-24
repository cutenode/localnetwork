#[cfg(feature = "account")]
use std::time::Instant;

use hegel::generators as gs;
use locallibrary::keybinds::{self, Keybinds};
use ratatui::crossterm::event::{KeyCode, KeyModifiers, MouseEventKind};
use serde_json::json;

use super::*;
#[cfg(feature = "account")]
use crate::tui::app::testing::signed_in;
use crate::tui::app::testing::{
    LEFT, app_on, app_with, key, last_message, library, mouse, plugged, plugins_once, press, wait,
};
use crate::tui::app::{Focus, View};
use crate::tui::keymap::Scope;

// A pretend data server, standing in for Local Network's directory too, and an account signed in
// to it: as the library's own tests have them.
#[cfg(feature = "account")]
#[path = "../../../../../crates/locallibrary/tests/support/mock_pds.rs"]
mod mock_pds;
#[cfg(feature = "account")]
#[path = "../../../../../crates/locallibrary/tests/support/session.rs"]
mod session;

fn binds(pairs: &[(&str, &[&str])]) -> Keybinds {
    let mut out = Keybinds::default();
    for (id, keys) in pairs {
        out.keys.insert(id.to_string(), keys.iter().map(|k| k.to_string()).collect());
    }
    out
}

/// What `keybinds.json` holds now.
fn kept(app: &App) -> Keybinds {
    keybinds::load(&app.store_root).unwrap()
}

/// The Keys screen, narrowed to `typed`, waiting for the key for the first row.
fn capturing(app: &mut App, typed: &str) {
    press(app, ":change the keys\n");
    assert!(app.keys.is_some(), "{}", last_message(app));
    press(app, typed);
    press(app, "\n");
    assert!(matches!(app.keys.as_ref().unwrap().stage, Stage::Capture { .. }), "{:?}", app.keys);
}

/// Picked on the Keys screen, the track's details go on `]`: it's kept in keybinds.json, `]`
/// shows and hides them, and `i` does nothing.
#[test]
fn remapped_through_the_screen_then_pressed() {
    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Tracks;
    capturing(&mut app, "library.info");
    press(&mut app, "]");
    let Some(Stage::Answer(a)) = app.keys.as_ref().map(|k| &k.stage) else { panic!("{:?}", app.keys) };
    assert_eq!((a.act, a.key, &a.verdict), (Act::Details, K::Ch(']'), &Verdict::Save));
    press(&mut app, "\n\u{1b}");
    assert!(app.keys.is_none());
    assert_eq!(kept(&app).keys["library.info"], ["]"]);
    assert!(kept(&app).updated_at.is_some());
    let shown = app.show_details;
    press(&mut app, "]");
    assert_ne!(app.show_details, shown, "] does it");
    press(&mut app, "i");
    assert_ne!(app.show_details, shown, "i does nothing now");
}

/// Esc goes back a step: from the answer to the list, from waiting for a key to the list, and
/// closes the screen from the list. A refused key waits for another on enter.
#[test]
fn esc_goes_back_a_step_and_a_refused_key_waits_for_another() {
    let (mut app, _dir) = app_with(library());
    capturing(&mut app, "library.info");
    press(&mut app, "?");
    let Some(Stage::Answer(a)) = app.keys.as_ref().map(|k| &k.stage) else { panic!("{:?}", app.keys) };
    assert_eq!(a.verdict, Verdict::Refused("? always opens the help".into()));
    press(&mut app, "\n");
    assert!(matches!(app.keys.as_ref().unwrap().stage, Stage::Capture { act: Act::Details }));
    press(&mut app, "\u{1b}");
    assert_eq!(app.keys.as_ref().map(|k| &k.stage), Some(&Stage::List));
    press(&mut app, "\u{1b}");
    assert!(app.keys.is_none() && !app.help, "closed, nothing else opened");
    assert_eq!(kept(&app), Keybinds::default(), "nothing kept");
}

/// Typing narrows the rows by name, id or key; the fixed keys are listed and can't be picked.
#[test]
fn the_rows_narrow_and_the_fixed_keys_stay_put() {
    let e = Effective::defaults();
    let all = rows(&e, &[], "");
    assert!(all.len() > 150, "{}", all.len());
    assert_eq!(
        all.iter().filter(|r| r.fixed).map(|r| r.keys.as_str()).collect::<Vec<_>>(),
        ["esc", "?", ":", "ctrl-c"]
    );
    let next = rows(&e, &[], "playback.next");
    assert_eq!(next.len(), 1);
    assert_eq!((next[0].name.as_str(), next[0].keys.as_str(), next[0].default.as_str()), ("next track", "n", "n"));
    let phrase = rows(&e, &[], "next phrase");
    assert_eq!(phrase[0].keys, ":", "by name, as the help says");
    let (mut app, _dir) = app_with(library());
    press(&mut app, ":change the keys\nctrl-c");
    app.keys.as_mut().unwrap().row = rows(&e, &[], "ctrl-c").len() - 1;
    press(&mut app, "\n");
    assert_eq!(app.keys.as_ref().map(|k| &k.stage), Some(&Stage::List), "a fixed key can't change");
}

/// Its own key back puts an action as it came: nothing's kept for it.
#[test]
fn pressing_the_default_key_resets() {
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(binds(&[("library.info", &["]"])]));
    capturing(&mut app, "library.info");
    press(&mut app, "i");
    let Some(Stage::Answer(a)) = app.keys.as_ref().map(|k| &k.stage) else { panic!("{:?}", app.keys) };
    assert_eq!(a.verdict, Verdict::Reset);
    press(&mut app, "\n");
    assert!(kept(&app).keys.is_empty());
    assert!(!effective::current().changed(Act::Details));
}

/// Putting every key back asks first, listing what changes; yes leaves nothing kept, and the keys
/// in effect are the ones they came as, row for row.
#[test]
fn reset_all_asks_then_restores_the_defaults() {
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(binds(&[("library.info", &["]"]), ("player.jobs", &[])]));
    press(&mut app, ":all keys back to their defaults\n");
    let Some(Modal::Ask(question, _)) = &app.modal else { panic!("{}", last_message(&app)) };
    assert!(question.contains("the keys of 2 actions") && question.contains("] → i"), "{question}");
    press(&mut app, "y");
    assert!(kept(&app).keys.is_empty());
    let (now, came) = (effective::current(), Effective::defaults());
    let keys = |e: &Effective| {
        let own = e.rows().iter().filter(|r| !matches!(r.act, Act::Plugin(..)));
        own.map(|r| (r.act, r.keys.clone())).collect::<Vec<_>>()
    };
    assert_eq!(keys(&now), keys(&came));
}

/// Someone's keys show what they'd change, each marked; unmarking one and saying yes takes the
/// rest, with their DID for credit.
#[test]
fn importing_shows_the_differences_then_takes_the_marked() {
    let (mut app, _dir) = app_with(library());
    let theirs = binds(&[("playback.next", &["]"]), ("library.info", &["I"]), ("someday.thing", &["z"])]);
    app.preview_keys("Keys from @dj.example.com".into(), theirs, Some("did:plc:dj".into()), 0);
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no list") };
    assert_eq!(p.title, "Keys from @dj.example.com");
    let rows: Vec<(&str, &str)> = p.items.iter().map(|i| (i.label.as_str(), i.detail.as_str())).collect();
    assert_eq!(rows, [("show or hide the track's details", "i → I"), ("next track", "n → ]")]);
    assert_eq!(p.marks, Some(vec![true, true]));
    press(&mut app, " \n");
    let Some(Modal::Ask(question, _)) = &app.modal else { panic!("{}", last_message(&app)) };
    assert_eq!(question, "Change the keys of 1 action? next track");
    press(&mut app, "y");
    let now = kept(&app);
    assert_eq!(now.keys.keys().collect::<Vec<_>>(), ["playback.next"]);
    assert_eq!(now.from.as_deref(), Some("did:plc:dj"));
    assert_eq!(effective::current().keys(Act::Next), [K::Ch(']')]);
}

/// Taking a row whose key another holds, without that one's row, takes nothing and says which.
#[test]
fn an_import_that_would_double_a_key_says_which_row_to_take_too() {
    let (mut app, _dir) = app_with(library());
    app.preview_keys("Keys".into(), binds(&[("playback.next", &["m"]), ("library.mark", &["n"])]), None, 0);
    press(&mut app, " \n");
    assert!(app.modal.is_none());
    assert_eq!(last_message(&app), "playback.next: m is mark's already: mark that one's row too, or leave this one");
    assert!(kept(&app).keys.is_empty());
}

/// AZERTY: the views and the hot cues on the digit row's unshifted keys, after showing them.
#[test]
fn the_azerty_keys_go_on_the_digit_row() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, ":keys for an azerty keyboard\n");
    let Some(Modal::Pick(p)) = &app.modal else { panic!("{}", last_message(&app)) };
    assert_eq!(p.title, "Keys for an AZERTY keyboard");
    press(&mut app, "\ny");
    let e = effective::current();
    assert_eq!(e.problems, Vec::<String>::new());
    let pressed = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
    assert_eq!(
        keymap::lookup(&[Scope::Library, Scope::Global], &pressed('é')).map(|h| h.act),
        Some(Act::ShowView(View::Prepare))
    );
    assert_eq!(keymap::pad_of(&pressed('è')), Some(6));
    assert_eq!(keymap::lookup(&[Scope::Global], &pressed('2')), None, "the digits are free");
}

/// At the start, keybinds.json is read and put in effect, what it can't do said; one that can't
/// be read leaves the keys as they came and is never written over.
#[test]
fn the_keys_are_read_at_the_start_and_an_unreadable_file_kept() {
    let (mut app, _dir) = app_with(library());
    keybinds::save(&app.store_root, &binds(&[("playback.next", &["]"]), ("playback.stop", &["alt-x"])])).unwrap();
    app.load_keybinds();
    assert_eq!(effective::current().keys(Act::Next), [K::Ch(']')]);
    assert_eq!(last_message(&app), "keybinds.json: playback.stop: \"alt-x\": the player doesn't take alt- keys");

    let (mut app, _dir) = app_with(library());
    let file = locallibrary::store::player_dir(&app.store_root).unwrap().join(locallibrary::store::KEYBINDS_FILE);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "{ not json").unwrap();
    app.load_keybinds();
    assert!(app.keybinds_unreadable);
    assert!(last_message(&app).starts_with("the keys are as they came"), "{}", last_message(&app));
    app.set_keybinds(binds(&[("playback.next", &["]"])]));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
}

/// A key a plugin's settings kept for one of its commands moves to keybinds.json at the start,
/// and runs the command.
#[test]
fn a_plugins_key_moves_to_keybinds_json_and_runs_its_command() {
    plugins_once();
    let drive = plugged();
    let mut app = app_on(&drive.0);
    let store = Store::open_or_init(&app.store_root).unwrap();
    let settings = app.store_root.join("plugins/echo/settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, json!({ "keys.titles": "%" }).to_string()).unwrap();
    drop(store);
    app.load_keybinds();
    assert_eq!(kept(&app).keys["plugin.echo.titles"], ["%"]);
    app.modal = None;
    press(&mut app, "%");
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Report(r)) = &app.modal else { panic!("no panel: {}", last_message(&app)) };
    assert_eq!(r.title, app.drive_label(&drive.0));
}

/// Signed out, a change is kept here and nothing's due to be published.
#[test]
fn signed_out_nothing_is_published() {
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(binds(&[("playback.next", &["]"])]));
    assert_eq!(kept(&app).keys["playback.next"], ["]"]);
    assert!(app.publishing.due.is_none() && !app.publishing.running);
}

/// Signed in, a change is published once the changes settle, one publish at a time: a change
/// while one runs is published after it. What went wrong is kept to show.
#[cfg(feature = "account")]
#[test]
fn a_change_signed_in_publishes_once_settled() {
    let (mut app, _dir) = app_with(library());
    app.profile.account.info = Some(signed_in());
    app.set_keybinds(binds(&[("playback.next", &["]"])]));
    assert!(app.publishing.due.is_some_and(|at| at > Instant::now()));
    app.publish_keys();
    assert!(!app.publishing.running, "not settled yet");
    app.publishing.due = Some(Instant::now());
    app.publish_keys();
    assert!(app.publishing.running && app.publishing.due.is_none());
    app.set_keybinds(binds(&[("playback.next", &["["])]));
    app.publishing.due = Some(Instant::now());
    app.publish_keys();
    assert!(app.publishing.due.is_some(), "waits for the one running");
    wait(&mut app, |a| !a.publishing.running);
    assert!(app.publishing.error.is_some(), "no session here to publish with");
}

/// Signing in on another computer loads the keys the account published.
#[cfg(feature = "account")]
#[test]
fn signing_in_installs_the_published_keys() {
    let (mut app, _dir) = app_with(library());
    app.profile.account.info = Some(signed_in());
    let theirs = binds(&[("playback.next", &["]"])]);
    app.on_keys_event(KeysEvent::AfterSignIn(Ok(locallibrary::api::AfterSignIn::Loaded(theirs))));
    assert_eq!(effective::current().keys(Act::Next), [K::Ch(']')]);
    assert_eq!(last_message(&app), "loaded your keys from @dj.example.com");
}

/// A session from before the player published keys is told once to sign in again.
#[cfg(feature = "account")]
#[test]
fn a_session_without_the_scope_is_told_to_sign_in_again_once() {
    let (mut app, _dir) = app_with(library());
    for _ in 0..2 {
        app.publishing.running = true;
        app.on_keys_event(KeysEvent::Published(Err(("no scope".into(), true))));
    }
    let said = app.log.iter().filter(|(m, ..)| m == "sign in again to publish your keys").count();
    assert_eq!(said, 1);
    assert_eq!(app.publishing.error.as_deref(), Some("sign in again to publish your keys"));
}

/// The mouse on the Keys screen does what it does on the help: the wheel moves through the list,
/// and a click is esc, a step back while waiting for a key and closed from the list.
#[test]
fn the_wheel_moves_and_a_click_steps_back() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, ":change the keys\n");
    mouse(&mut app, MouseEventKind::ScrollDown, (10, 10));
    assert_eq!(app.keys.as_ref().map(|k| k.row), Some(3));
    mouse(&mut app, MouseEventKind::ScrollUp, (10, 10));
    assert_eq!(app.keys.as_ref().map(|k| k.row), Some(0));
    press(&mut app, "\n");
    mouse(&mut app, LEFT, (10, 10));
    assert_eq!(app.keys.as_ref().map(|k| &k.stage), Some(&Stage::List), "back to the list");
    mouse(&mut app, LEFT, (10, 10));
    assert!(app.keys.is_none(), "closed");
}

/// What's said of a key the player can't take while waiting for one.
const NOT_TAKEN: &str = "the player doesn't take that key (alt- keys aren't kept)";

/// Waiting for a key, one the player can't take (a key with no name, or alt held) is said, and
/// it goes on waiting.
#[test]
fn a_key_the_player_cant_take_is_said_and_it_waits_on() {
    let (mut app, _dir) = app_with(library());
    capturing(&mut app, "library.info");
    key(&mut app, KeyCode::CapsLock);
    app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
    assert_eq!(app.log.iter().filter(|(m, ..)| m == NOT_TAKEN).count(), 2);
    assert!(matches!(app.keys.as_ref().unwrap().stage, Stage::Capture { act: Act::Details }), "{:?}", app.keys);
}

/// While what a key would do shows, keys but enter and esc leave it be, and nothing's kept.
#[test]
fn other_keys_leave_the_answer_be() {
    let (mut app, _dir) = app_with(library());
    capturing(&mut app, "library.info");
    press(&mut app, "]");
    let shown = app.keys.clone();
    press(&mut app, "x");
    key(&mut app, KeyCode::Down);
    assert_eq!(app.keys, shown);
    assert_eq!(kept(&app), Keybinds::default());
}

/// A key changed where keybinds.json can't be written says it wasn't saved, and works till the
/// player quits.
#[test]
fn a_key_that_cant_be_saved_says_so_and_works_for_now() {
    let (mut app, _dir) = app_with(library());
    let file = locallibrary::store::player_dir(&app.store_root).unwrap().join(locallibrary::store::KEYBINDS_FILE);
    // A folder where the file goes.
    std::fs::create_dir_all(&file).unwrap();
    capturing(&mut app, "library.info");
    press(&mut app, "]\n");
    assert!(last_message(&app).starts_with("the keys weren't saved: "), "{}", last_message(&app));
    assert_eq!(effective::current().keys(Act::Details), [K::Ch(']')]);
}

/// A plugin's setting that isn't a key stays out of keybinds.json, and the start says so; with
/// keybinds.json unreadable nothing moves, and that's said too.
#[test]
fn a_plugins_key_that_cant_move_is_said() {
    let plugin_settings = |app: &App, settings: serde_json::Value| {
        Store::open_or_init(&app.store_root).unwrap();
        let file = app.store_root.join("plugins/gone/settings.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, settings.to_string()).unwrap();
    };
    let (mut app, _dir) = app_with(library());
    plugin_settings(&app, json!({ "keys.go": 5 }));
    app.load_keybinds();
    assert_eq!(last_message(&app), "a plugin's key wasn't moved to keybinds.json: plugin.gone.go: 5 isn't a key");
    assert!(kept(&app).keys.is_empty());

    let (mut app, _dir) = app_with(library());
    plugin_settings(&app, json!({ "keys.go": "%" }));
    let file = locallibrary::store::player_dir(&app.store_root).unwrap().join(locallibrary::store::KEYBINDS_FILE);
    std::fs::write(&file, "{ not json").unwrap();
    app.load_keybinds();
    let said: Vec<&str> = app.log.iter().map(|(m, ..)| m.as_str()).collect();
    assert!(said.iter().any(|m| m.starts_with("plugins' keys weren't moved to keybinds.json: ")), "{said:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
}

/// Putting the keys back when none has changed asks nothing, and says so.
#[test]
fn putting_the_keys_back_when_none_changed_says_so() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, ":all keys back to their defaults\n");
    assert!(app.modal.is_none());
    assert_eq!(last_message(&app), "every key is as it came");
}

/// Someone's keys that are the ones here have nothing to show.
#[test]
fn importing_the_keys_you_have_says_so() {
    let (mut app, _dir) = app_with(library());
    let mine = binds(&[("playback.next", &["]"])]);
    app.set_keybinds(mine.clone());
    app.preview_keys("Keys".into(), mine, None, 0);
    assert!(app.modal.is_none());
    assert_eq!(last_message(&app), "those keys are the ones you have");
}

/// Taking a row where they have the key as it came puts that key back here.
#[test]
fn taking_a_row_they_left_as_it_came_puts_it_back() {
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(binds(&[("playback.next", &["]"]), ("library.info", &["I"])]));
    app.preview_keys("Keys".into(), binds(&[("library.info", &["I"])]), None, 0);
    let Some(Modal::Pick(p)) = &app.modal else { panic!("{}", last_message(&app)) };
    let rows: Vec<(&str, &str)> = p.items.iter().map(|i| (i.label.as_str(), i.detail.as_str())).collect();
    assert_eq!(rows, [("next track", "] → n")]);
    press(&mut app, "\ny");
    assert_eq!(kept(&app).keys.keys().collect::<Vec<_>>(), ["library.info"]);
    assert_eq!(effective::current().keys(Act::Next), [K::Ch('n')]);
}

/// With every row unmarked, enter changes nothing and says so.
#[test]
fn taking_nothing_marked_changes_nothing() {
    let (mut app, _dir) = app_with(library());
    app.preview_keys("Keys".into(), binds(&[("playback.next", &["]"])]), None, 0);
    press(&mut app, " \n");
    assert_eq!(last_message(&app), "nothing marked: no key changed");
    assert!(app.modal.is_none());
    assert_eq!(kept(&app), Keybinds::default());
    assert_eq!(effective::current().keys(Act::Next), [K::Ch('n')]);
}

/// An action of a plugin that isn't loaded is called by its plugin and its name.
#[test]
fn a_plugin_not_loaded_names_its_action_plainly() {
    assert_eq!(title(Act::Plugin("gone", "go"), &[]), "gone: go");
}

/// Keybinds giving some of the actions a key each, no key twice, and none an action came with.
fn some_keys(tc: &hegel::TestCase) -> Keybinds {
    let acts = ids::bindable(&manifests());
    let free: Vec<String> = ('A'..='Z')
        .map(String::from)
        .chain((6..=12).map(|n| format!("f{n}")))
        .chain("begortyz".chars().map(|c| format!("ctrl-{c}")))
        .collect();
    let mut out = Keybinds::default();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let act = tc.draw(gs::sampled_from(acts.clone()));
        let key = tc.draw(gs::sampled_from(free.clone()));
        if out.keys.values().flatten().any(|k| *k == key) {
            continue;
        }
        out.keys.insert(ids::id(act).unwrap().into_owned(), vec![key]);
    }
    out
}

/// Whatever their keys and the ones here, taking every row of the import leaves the keys here
/// theirs, action for action, with nothing to say of the file; with none different there's
/// nothing to take.
#[hegel::test(test_cases = 30)]
fn prop_taking_every_row_of_an_import_gives_their_keys(tc: hegel::TestCase) {
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(some_keys(&tc));
    let theirs = some_keys(&tc);
    let plugins = manifests();
    let want = Effective::new(&theirs, &plugins);
    let mine = effective::current();
    let differ = ids::bindable(&plugins).into_iter().any(|a| own(&mine.keys(a)) != own(&want.keys(a)));
    app.preview_keys("Keys".into(), theirs, None, 0);
    if !differ {
        assert!(app.modal.is_none());
        assert_eq!(last_message(&app), "those keys are the ones you have");
        return;
    }
    // Asked with the yes key as it is here, which the file may have changed.
    press(&mut app, "\n");
    let yes = effective::current().keys(Act::Answer(true))[0];
    app.on_key(yes.pressed());
    assert_eq!(last_message(&app), "keys changed");
    let now = effective::current();
    for act in ids::bindable(&plugins) {
        assert_eq!(own(&now.keys(act)), own(&want.keys(act)), "{act:?}");
    }
    assert_eq!(Effective::new(&kept(&app), &plugins).problems, Vec::<String>::new());
}

/// An import that puts yes back on `y`, taken with the yes key as it is here (`A`): the import's own question is
/// answered with the key the file gave yes, not the one it's putting back.
#[test]
fn an_import_that_moves_yes_is_taken_with_yes_as_it_is() {
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(binds(&[("dialog.yes", &["A"])]));
    app.preview_keys("Keys".into(), Keybinds::default(), None, 0);
    press(&mut app, "\ny");
    assert_ne!(last_message(&app), "keys changed", "y isn't yes here");
    press(&mut app, "A");
    assert_eq!(last_message(&app), "keys changed");
    assert_eq!(
        own(&effective::current().keys(Act::Answer(true))),
        own(&Effective::new(&Keybinds::default(), &manifests()).keys(Act::Answer(true)))
    );
}

/// Signed out before the keys settled, nothing's published, and nothing's due any more.
#[cfg(feature = "account")]
#[test]
fn signed_out_before_they_settle_nothing_is_published() {
    let (mut app, _dir) = app_with(library());
    app.profile.account.info = Some(signed_in());
    app.set_keybinds(binds(&[("playback.next", &["]"])]));
    app.profile.account.info = None;
    app.publishing.due = Some(Instant::now());
    app.publish_keys();
    assert!(app.publishing.due.is_none() && !app.publishing.running && app.publishing.error.is_none());
}

/// The directory and the account's data server on this computer, for this test's process.
#[cfg(feature = "account")]
fn network() -> mock_pds::MockPds {
    let pds = mock_pds::MockPds::start();
    directory_at(&pds.url);
    pds
}

/// Local Network's directory and website at `url`, for this test's process.
#[cfg(feature = "account")]
fn directory_at(url: &str) {
    // SAFETY: nextest runs each test in a process of its own, and nothing running reads the
    // environment yet (the pretend server's thread never does).
    unsafe {
        std::env::set_var("LOCALLIBRARY_HAPPYVIEW_URL", url);
        std::env::set_var("LOCALLIBRARY_WEBSITE_URL", url);
    }
}

/// A keybinds record giving `next` the next track.
#[cfg(feature = "account")]
fn record(next: &str) -> serde_json::Value {
    json!({
        "$type": locallibrary::atmosphere::lexicon::KEYBINDS,
        "bindings": [{ "action": "playback.next", "keys": [next] }],
        "updatedAt": "2026-11-20T18:00:00.000Z"
    })
}

/// Signed in at `pds`, as a sign-in leaves it: the account read again, then its keys (signing in
/// itself needs a browser).
#[cfg(feature = "account")]
fn signs_in(app: &mut App, pds: &str) {
    let scopes = locallibrary::atmosphere::player_scopes().join(" ");
    let session =
        session::session_json(session::DID, "s1", pds, &scopes, Some("refresh-1"), "2099-01-01T00:00:00.000Z");
    session::sign_in_as(&app.store_root, session);
    app.profile.account.load(&app.store_root);
    assert!(app.profile.account.signed_in());
    app.keys_after_sign_in();
}

#[cfg(feature = "account")]
fn said(app: &App, message: &str) -> bool {
    app.log.iter().any(|(m, ..)| m == message)
}

/// Signing in with keys set here and none on the account publishes them, and says so.
#[cfg(feature = "account")]
#[test]
fn signing_in_publishes_the_keys_here_when_the_account_has_none() {
    let pds = network();
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(binds(&[("playback.next", &["]"])]));
    signs_in(&mut app, &pds.url);
    wait(&mut app, |a| said(a, "published your keys"));
    assert!(app.publishing.published);
    let (stored, _) = pds.record(session::DID, locallibrary::atmosphere::lexicon::KEYBINDS, "self").unwrap();
    assert_eq!(stored["bindings"], json!([{ "action": "playback.next", "keys": ["]"] }]));
}

/// Signing in where the account has the keys set here changes nothing, and they count as
/// published.
#[cfg(feature = "account")]
#[test]
fn signing_in_with_the_same_keys_leaves_them_be() {
    let pds = network();
    pds.put(session::DID, locallibrary::atmosphere::lexicon::KEYBINDS, "self", record("]"));
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(binds(&[("playback.next", &["]"])]));
    signs_in(&mut app, &pds.url);
    wait(&mut app, |a| a.publishing.published);
    assert!(app.modal.is_none());
    assert!(!app.log.iter().any(|(m, ..)| m.contains("your keys")), "{:?}", app.log);
    assert_eq!(kept(&app).keys["playback.next"], ["]"]);
}

/// Signing in where the account has other keys shows what they'd change, to take or not.
#[cfg(feature = "account")]
#[test]
fn signing_in_with_other_keys_published_offers_them() {
    let pds = network();
    pds.put(session::DID, locallibrary::atmosphere::lexicon::KEYBINDS, "self", record("]"));
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(binds(&[("playback.next", &["["])]));
    signs_in(&mut app, &pds.url);
    wait(&mut app, |a| matches!(a.modal, Some(Modal::Pick(_))));
    let Some(Modal::Pick(p)) = &app.modal else { unreachable!() };
    assert_eq!(p.title, format!("Your keys from {}", session::DID));
    let rows: Vec<(&str, &str)> = p.items.iter().map(|i| (i.label.as_str(), i.detail.as_str())).collect();
    assert_eq!(rows, [("next track", "[ → ]")]);
    press(&mut app, "\ny");
    assert_eq!(effective::current().keys(Act::Next), [K::Ch(']')]);
    // The account's own keys aren't someone else's to credit.
    press(&mut app, ":change the keys\n");
    let s = crate::tui::ui::screen(&mut app, 160, 45);
    assert!(!s.contains("Some keys from"), "{s}");

    // Keys the account took from someone keep their credit.
    let mut credited = record("]");
    credited["from"] = json!("did:plc:dj");
    pds.put(session::DID, locallibrary::atmosphere::lexicon::KEYBINDS, "self", credited);
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(binds(&[("playback.next", &["["])]));
    signs_in(&mut app, &pds.url);
    wait(&mut app, |a| matches!(a.modal, Some(Modal::Pick(_))));
    press(&mut app, "\ny");
    assert_eq!(kept(&app).from.as_deref(), Some("did:plc:dj"));
}

/// Signed in but the directory can't be reached, the account's keys can't be read: it says so.
#[cfg(feature = "account")]
#[test]
fn signing_in_when_the_directory_cant_be_reached_says_so() {
    // A port nothing listens on.
    let closed = format!("http://{}", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap());
    directory_at(&closed);
    let (mut app, _dir) = app_with(library());
    signs_in(&mut app, &closed);
    let failed = |a: &App| a.log.iter().any(|(m, ..)| m.starts_with("your published keys couldn't be read: "));
    wait(&mut app, failed);
    assert!(app.modal.is_none());
}

/// `import someone's keys…` asks whose, reads theirs from the directory, and shows what they'd
/// change; someone with none published is said.
#[cfg(feature = "account")]
#[test]
fn importing_someones_keys_reads_them_and_shows_the_changes() {
    let pds = network();
    let dj = "did:plc:someoneelse";
    pds.put(dj, locallibrary::atmosphere::lexicon::KEYBINDS, "self", record("]"));
    let (mut app, _dir) = app_with(library());
    press(&mut app, ":import someone's keys\n");
    let Some(Modal::Prompt(p)) = &app.modal else { panic!("{}", last_message(&app)) };
    assert_eq!(p.label, "Import the keys of (a handle or DID)");
    press(&mut app, &format!("{dj}\n"));
    assert_eq!(last_message(&app), format!("reading {dj}'s keys…"));
    wait(&mut app, |a| matches!(a.modal, Some(Modal::Pick(_))));
    let Some(Modal::Pick(p)) = &app.modal else { unreachable!() };
    assert_eq!(p.title, format!("Keys from {dj}"));
    press(&mut app, "\ny");
    assert_eq!(effective::current().keys(Act::Next), [K::Ch(']')]);
    assert_eq!(kept(&app).from.as_deref(), Some(dj));

    press(&mut app, ":import someone's keys\ndid:plc:nobody\n");
    let reading = last_message(&app).to_string();
    wait(&mut app, |a| last_message(a) != reading);
    assert!(app.modal.is_none(), "{}", last_message(&app));
    assert!(matches!(app.log.back(), Some((_, crate::tui::app::Level::Error, _))), "{:?}", app.log.back());
}
