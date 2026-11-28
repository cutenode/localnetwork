use locallibrary::keybinds::Keybinds;
use ratatui::crossterm::event::KeyCode;

use crate::tui::app::App;
#[cfg(feature = "account")]
use crate::tui::app::testing::signed_in;
use crate::tui::app::testing::{app_with, key, library, press};

use super::super::screen;

fn binds(pairs: &[(&str, &[&str])]) -> Keybinds {
    let mut out = Keybinds::default();
    for (id, keys) in pairs {
        out.keys.insert(id.to_string(), keys.iter().map(|k| k.to_string()).collect());
    }
    out
}

/// The Keys screen, opened as anyone would, by name in `:`.
fn opened() -> (App, tempfile::TempDir) {
    let (mut app, dir) = app_with(library());
    press(&mut app, ":change the keys\n");
    assert!(app.keys.is_some());
    (app, dir)
}

/// The Keys screen narrowed to `typed`, waiting for the key for its first row.
fn capturing(typed: &str) -> (App, tempfile::TempDir) {
    let (mut app, dir) = opened();
    press(&mut app, &format!("{typed}\n"));
    (app, dir)
}

/// The screen's rows holding `text`.
fn rows_with<'a>(screen: &'a str, text: &str) -> Vec<&'a str> {
    screen.lines().filter(|l| l.contains(text)).collect()
}

/// From 100 columns what's told of the row selected is beside the list, on the same rows; at 80
/// it's under the list, past a rule across it. Either way the screen says how to change a key and
/// where the keys are kept.
#[test]
fn the_row_selected_is_told_of_beside_the_list_or_under_it() {
    let (mut app, _dir) = opened();
    let wide = screen(&mut app, 160, 45);
    for said in ["Change keys", "PLAYBACK", "166 actions", "enter change", "esc close", "Kept on this computer"] {
        assert!(wide.contains(said), "{said}:\n{wide}");
    }
    let heading = rows_with(&wide, "PLAYBACK");
    assert!(heading[0].contains("do what's selected"), "the name beside the first rows:\n{wide}");

    let narrow = screen(&mut app, 80, 24);
    assert!(!rows_with(&narrow, "PLAYBACK")[0].contains("do what's selected"), "{narrow}");
    let lines: Vec<&str> = narrow.lines().collect();
    let rule = lines.iter().position(|l| l.starts_with("│ ────────────────────")).unwrap();
    let list = lines.iter().position(|l| l.contains("PLAYBACK")).unwrap();
    let told = lines.iter().rposition(|l| l.contains("do what's selected")).unwrap();
    assert!(list < rule && rule < told, "the list, the rule, then what's told:\n{narrow}");
    assert!(narrow.contains("Kept on this computer. Sign in to use them anywhere."), "{narrow}");
}

/// Moving down keeps the row selected in view, and the panel follows it.
#[test]
fn the_list_moves_under_the_row_selected() {
    let (mut app, _dir) = opened();
    for _ in 0..12 {
        key(&mut app, KeyCode::Down);
    }
    let narrow = screen(&mut app, 80, 24);
    let row = app.keys.as_ref().unwrap().row;
    assert_eq!(row, 12);
    let rows = crate::tui::keybinds::rows(&crate::tui::keymap::effective::current(), &[], "");
    let name = &rows[row].name;
    assert_eq!(rows_with(&narrow, name).len(), 2, "in the list and told of under it:\n{narrow}");
}

/// Words nothing has leave no rows, and enter nothing to do.
#[test]
fn typing_narrows_the_list_and_says_when_nothing_is_left() {
    let (mut app, _dir) = opened();
    press(&mut app, "next");
    let some = screen(&mut app, 80, 24);
    assert!(some.contains("› next") && some.contains("13 actions") && some.contains("next track"), "{some}");
    press(&mut app, "zzqx");
    for size in [(80, 24), (160, 45)] {
        let none = screen(&mut app, size.0, size.1);
        assert!(none.contains("No action has those words.") && none.contains("0 actions"), "{none}");
        assert!(none.contains("Type less to see more actions.") && none.contains("esc close"), "{none}");
        assert!(!none.contains("enter change"), "enter does nothing:\n{none}");
    }
}

/// A key that can't change is said to, and enter isn't offered for it.
#[test]
fn a_fixed_key_says_it_cant_change() {
    let (mut app, _dir) = opened();
    press(&mut app, "ctrl-c");
    let last = crate::tui::keybinds::rows(&crate::tui::keymap::effective::current(), &[], "ctrl-c").len() - 1;
    app.keys.as_mut().unwrap().row = last;
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("CAN'T CHANGE") && s.contains("can't change") && s.contains("esc close"), "{s}");
    assert!(!s.contains("enter change"), "{s}");
}

/// Enter asks for the key, and only esc is offered; esc goes back to the list.
#[test]
fn capture_asks_for_the_key_and_esc_goes_back() {
    let (mut app, _dir) = capturing("playback.next");
    for size in [(80, 24), (160, 45)] {
        let s = screen(&mut app, size.0, size.1);
        assert!(s.contains("Press the new key for next track"), "{s}");
        assert_eq!(rows_with(&s, "esc cancel").len(), 1, "{s}");
        assert!(!rows_with(&s, "esc cancel")[0].contains("enter"), "only esc means something:\n{s}");
    }
    press(&mut app, "\u{1b}");
    assert!(screen(&mut app, 80, 24).contains("enter change"));
}

/// A key another action has says who loses it where, before anything's kept, and offers to swap.
#[test]
fn a_taken_key_shows_who_loses_it_and_offers_a_swap() {
    let (mut app, _dir) = capturing("playback.next");
    press(&mut app, "m");
    for size in [(80, 24), (160, 45)] {
        let s = screen(&mut app, size.0, size.1);
        for said in ["m for next track", "mark or unmark the track gets n in place", "enter swap", "esc cancel"] {
            assert!(s.contains(said), "{said}:\n{s}");
        }
        assert!(s.contains("m mark or unmark the track → next track"), "{s}");
        assert!(s.contains("in the drive list"), "{s}");
    }
}

/// Once swapped, the list marks the row and shows its new key.
#[test]
fn a_saved_swap_marks_the_row() {
    let (mut app, _dir) = capturing("playback.next");
    press(&mut app, "m\n");
    let s = screen(&mut app, 80, 24);
    let row = rows_with(&s, "next track")[0];
    assert!(row.contains("• m"), "{s}");
    assert!(s.contains("m   default n"), "told of with its default:\n{s}");
}

/// Waiting for a key for a changed action says its own key puts it back, and it does.
#[test]
fn its_own_key_puts_it_back() {
    let (mut app, _dir) = app_with(library());
    app.set_keybinds(binds(&[("playback.next", &["]"])]));
    press(&mut app, ":change the keys\nplayback.next\n");
    let asked = screen(&mut app, 80, 24);
    assert!(asked.contains("]   default n") && asked.contains("n puts its default back"), "{asked}");
    press(&mut app, "n");
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("back to its default: n") && s.contains("enter reset"), "{s}");
}

/// An action that came with no key goes back to none by pressing the key it has now: waiting for
/// its key says so, the answer offers it, and once it's kept the file holds nothing for it and its
/// row reads as it came.
#[test]
fn a_keyless_action_goes_back_to_no_key_by_its_own_key() {
    let (mut app, _dir) = capturing("playback.shuffle");
    press(&mut app, "F\n\u{1b}");
    assert_eq!(locallibrary::keybinds::load(&app.store_root).unwrap().keys["playback.shuffle"], ["F"]);
    press(&mut app, ":change the keys\nplayback.shuffle\n");
    let asked = screen(&mut app, 80, 24);
    assert!(asked.contains("F puts it back to no key"), "{asked}");
    press(&mut app, "F");
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("back to no key") && s.contains("enter reset"), "{s}");
    press(&mut app, "\n");
    assert!(!locallibrary::keybinds::load(&app.store_root).unwrap().keys.contains_key("playback.shuffle"));
    let e = crate::tui::keymap::effective::current();
    assert!(e.keys(crate::tui::keymap::Act::Shuffle).is_empty());
    let row = &crate::tui::keybinds::rows(&e, &[], "playback.shuffle")[0];
    assert_eq!((row.keys.as_str(), row.default.as_str(), row.changed), (":", "none", false));
    let listed = screen(&mut app, 80, 24);
    assert!(!rows_with(&listed, "shuffle on or off")[0].contains('•'), "{listed}");
}

/// A key that's always the player's is refused, and enter waits for another.
#[test]
fn a_reserved_key_is_refused() {
    let (mut app, _dir) = capturing("playback.next");
    press(&mut app, "?");
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("? always opens the help") && s.contains("enter try another"), "{s}");
    press(&mut app, "\n");
    assert!(screen(&mut app, 80, 24).contains("Press the new key"));
}

/// An import says how many of theirs it leaves out: actions this version doesn't know, and
/// bindings it couldn't read.
#[test]
fn an_import_says_what_it_leaves_out() {
    let (mut app, _dir) = app_with(library());
    let theirs = binds(&[("playback.next", &["]"]), ("future.action", &["f9"])]);
    app.preview_keys("Keys from @dj.example.com".into(), theirs.clone(), None, 1);
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("Keys from @dj.example.com") && s.contains("n → ]"), "{s}");
    assert!(s.contains("2 left out: this version can't read them"), "{s}");
    app.modal = None;
    app.preview_keys("Keys".into(), binds(&[("playback.next", &["]"])]), None, 0);
    assert!(!screen(&mut app, 80, 24).contains("left out"));
}

/// What's wrong with keybinds.json is said where the keys are kept: a key it can't take, and a
/// file it can't read, which changes don't save over.
#[test]
fn a_file_with_problems_says_so() {
    let (mut app, _dir) = app_with(library());
    let bad = binds(&[("playback.next", &["]"]), ("playback.stop", &["alt-x"]), ("playback.shuffle", &["alt-y"])]);
    locallibrary::keybinds::save(&app.store_root, &bad).unwrap();
    app.load_keybinds();
    press(&mut app, ":change the keys\n");
    let s = screen(&mut app, 160, 45);
    assert!(s.contains("keybinds.json: playback.shuffle: \"alt-y\":") && s.contains("more)"), "{s}");

    let (mut app, _dir) = app_with(library());
    let file = locallibrary::store::player_dir(&app.store_root).unwrap().join(locallibrary::store::KEYBINDS_FILE);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "{ not json").unwrap();
    app.load_keybinds();
    press(&mut app, ":change the keys\n");
    assert!(screen(&mut app, 80, 24).contains("keybinds.json can't be read, so changes last until you quit."));
}

/// A plugin's action with a name too long for its row is cut short, not wrapped into the next.
#[test]
fn a_long_name_is_cut_to_its_row() {
    let r = crate::tui::keybinds::KeysRow {
        act: None,
        id: None,
        name: "a very long plugin action name that goes on and on past any row ".repeat(3),
        keys: "1  2  3  4  5  6  7  8".into(),
        default: "none".into(),
        changed: true,
        fixed: false,
        group: "Plugins",
    };
    let line = super::row_line(&r, 40);
    assert_eq!(line.width(), 40, "{line:?}");
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains('…') && text.starts_with("• 1  2  3  4 "), "{text}");
}

/// Signed in, the keys say whether they're on the account: being published, published, or why
/// they weren't, and whose some of them are.
#[cfg(feature = "account")]
#[test]
fn signed_in_it_says_whether_the_keys_are_published() {
    let (mut app, _dir) = opened();
    app.profile.account.info = Some(signed_in());
    assert!(screen(&mut app, 80, 24).contains("Changes publish to @dj.example.com"));
    app.publishing.due = Some(std::time::Instant::now());
    assert!(screen(&mut app, 80, 24).contains("Publishing to @dj.example.com…"));
    app.publishing.due = None;
    app.on_keys_event(crate::tui::keybinds::KeysEvent::Published(Ok(())));
    assert!(screen(&mut app, 80, 24).contains("Published to @dj.example.com"));
    app.on_keys_event(crate::tui::keybinds::KeysEvent::Published(Err(("pds.example didn't answer".into(), false))));
    assert!(screen(&mut app, 80, 24).contains("Not published: pds.example didn't answer"));
    app.keybinds.from = Some("did:plc:dj".into());
    assert!(screen(&mut app, 160, 45).contains("Some keys from did:plc:dj"));
}

/// Every stage draws at any size without panicking, down to too small to read, and on either side
/// of side by side.
#[test]
fn every_stage_draws_at_any_size() {
    let sizes = [(20, 6), (40, 12), (99, 30), (100, 30), (200, 50)];
    let (mut app, _dir) = capturing("playback.next");
    // Waiting for a key, an answer, a refusal, then a list with nothing left in it.
    for keys in ["", "m", "\u{1b}\n?", "\u{1b}zzqx"] {
        press(&mut app, keys);
        assert!(app.keys.is_some(), "{keys:?}");
        for (w, h) in sizes {
            screen(&mut app, w, h);
        }
    }
}

/// A key nothing has is saved as it is: the screen says what it's for, that enter saves it, and
/// where it'll work.
#[test]
fn a_free_key_offers_to_save_it() {
    let (mut app, _dir) = capturing("playback.next");
    key(&mut app, KeyCode::F(9));
    for size in [(80, 24), (160, 45)] {
        let s = screen(&mut app, size.0, size.1);
        for said in ["f9 for next track", "f9 nothing → next track", "enter save", "esc cancel"] {
            assert!(s.contains(said), "{said}:\n{s}");
        }
        assert!(!s.contains("swap") && !s.contains("reset"), "{s}");
    }
}

/// An action with no key says it's found by name, beside the list and under it.
#[test]
fn an_action_with_no_key_is_found_by_name() {
    let (mut app, _dir) = opened();
    press(&mut app, "next phrase");
    for size in [(80, 24), (160, 45)] {
        let s = screen(&mut app, size.0, size.1);
        assert!(s.contains("no key: find it by name in :"), "{s}");
    }
}

/// A file with one key it can't take says that one, and no more.
#[test]
fn a_file_with_one_problem_says_just_that() {
    let (mut app, _dir) = app_with(library());
    let bad = binds(&[("playback.next", &["]"]), ("playback.stop", &["alt-x"])]);
    locallibrary::keybinds::save(&app.store_root, &bad).unwrap();
    app.load_keybinds();
    press(&mut app, ":change the keys\n");
    let s = screen(&mut app, 160, 45);
    assert!(s.contains("keybinds.json: playback.stop: \"alt-x\": the player doesn't take alt- keys"), "{s}");
    assert!(!s.contains("more)"), "{s}");
}
