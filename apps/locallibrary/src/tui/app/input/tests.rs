use std::time::Instant;

use ratatui::crossterm::event::KeyCode;

use super::super::testing::*;
use super::super::*;
use super::QUIT_AGAIN;
use crate::tui::edits::Modal;
use crate::tui::keymap;
use crate::tui::mix::Pane;
use crate::tui::ui::screen;

/// One esc with nothing open never quits: the footer says a second does, and a second long after
/// is a first again.
#[test]
fn one_esc_at_the_top_never_quits() {
    let (mut app, _dir) = app_with(library());
    key(&mut app, KeyCode::Esc);
    assert!(!app.quit && app.quit_waiting());
    assert!(screen(&mut app, 100, 30).contains("esc again to quit"));
    app.quit_armed = Some(Instant::now() - QUIT_AGAIN);
    key(&mut app, KeyCode::Esc);
    assert!(!app.quit, "too late for a second: a first again");
    assert!(app.quit_waiting());
    key(&mut app, KeyCode::Esc);
    assert!(app.quit, "a second soon after quits");
}

/// Only esc after esc quits: any other key between them starts over.
#[test]
fn a_key_between_two_escs_keeps_the_player() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "\u{1b}j\u{1b}");
    assert!(!app.quit && app.quit_waiting());
    assert!(!screen(&mut app, 100, 30).contains("Q quit"), "Q isn't quitting any more");
    press(&mut app, "Q");
    assert!(!app.quit);
}

/// The dialogs a key opens, and those only `:` opens (by the action it runs).
const DIALOGS: [(&str, Option<Act>); 6] = [
    ("?", None),
    (":", None),
    ("&", None),
    ("e", None),
    ("messages", Some(Act::Messages)),
    ("outputs", Some(Act::Outputs)),
];

/// Opens one of [`DIALOGS`].
fn open_dialog(app: &mut App, (open, act): (&str, Option<Act>)) {
    match act {
        Some(act) => app.perform(act, None),
        None => press(app, open),
    }
}

/// Esc closes a dialog first, and doing so never counts toward quitting.
#[test]
fn esc_in_a_dialog_closes_it_without_arming_quitting() {
    for (open, act) in DIALOGS {
        let (mut app, _dir) = app_with(library());
        open_dialog(&mut app, (open, act));
        assert!(overlay(&app), "{open} opens a dialog");
        key(&mut app, KeyCode::Esc);
        assert!(!overlay(&app), "esc closes {open}'s dialog");
        assert!(!app.quit_waiting(), "closing {open}'s dialog didn't arm quitting");
        key(&mut app, KeyCode::Esc);
        assert!(!app.quit && app.quit_waiting(), "{open}");
    }
}

/// Dialogs close on esc (or the key that opened them; the messages, as results do, on enter
/// too), never on q or any other key.
#[test]
fn dialogs_close_on_their_own_keys_only() {
    for (open, act) in DIALOGS.into_iter().filter(|(open, _)| *open != ":") {
        let (mut app, _dir) = app_with(library());
        open_dialog(&mut app, (open, act));
        // The help is typed into: q there is a letter of what's looked for. The library's
        // actions menu has q as everywhere does, playing next: v is none of its letters.
        match open {
            "?" => {}
            "e" => press(&mut app, "v"),
            _ => press(&mut app, "q"),
        }
        assert!(overlay(&app), "{open}: q leaves it open");
        let again = match open {
            "messages" => "\n",
            "outputs" => "\u{1b}",
            key => key,
        };
        press(&mut app, again);
        assert!(!overlay(&app), "{again:?} closes {open}");
    }
}

/// Esc closes the pane with focus (the details, the queue, a Drives pane) before it counts toward
/// quitting.
#[test]
fn esc_closes_the_pane_with_focus() {
    let (mut app, _dir) = app_with(library());
    app.show_details = true;
    app.focus = Focus::Details;
    key(&mut app, KeyCode::Esc);
    assert!(!app.show_details && app.focus == Focus::Tracks && !app.quit_waiting());
    press(&mut app, "t");
    app.focus = Focus::Crate;
    key(&mut app, KeyCode::Esc);
    assert!(!app.show_crate && app.focus == Focus::Tracks && !app.quit_waiting());
    app.set_view(View::Drives);
    app.show_drive_pane(DrivePane::Snapshots);
    key(&mut app, KeyCode::Esc);
    assert!(app.drive_pane == DrivePane::List && !app.quit_waiting());
}

/// The actions menu's letters run their item, `/` starts a filter that esc clears before it
/// closes the menu, and filtering, letters are typed.
#[test]
fn the_actions_menu_runs_letters_and_filters_after_slash() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "e/col");
    let Some(Modal::Menu(m)) = &app.modal else { panic!("the menu is open") };
    let shown = m.visible();
    assert_eq!(m.items[shown[0]].label, "set the colour…", "{:?}", m.query);
    key(&mut app, KeyCode::Esc);
    let Some(Modal::Menu(m)) = &app.modal else { panic!("esc cleared the filter, not the menu") };
    assert!(m.query.is_none());
    key(&mut app, KeyCode::Esc);
    assert!(app.modal.is_none());
    // In another view, the same: letters, then `/`.
    app.set_view(View::Prepare);
    press(&mut app, "e/eje");
    let Some(Modal::Pick(p)) = &app.modal else { panic!("Prepare's actions are open") };
    assert_eq!(p.items[p.selected().unwrap()].label, "eject the drive");
    press(&mut app, "\u{1b}");
    let Some(Modal::Pick(p)) = &app.modal else { panic!("esc cleared the filter, not the menu") };
    assert!(p.query.is_empty() && !p.typing());
    press(&mut app, "r");
    assert!(app.modal.is_none(), "r ran refresh from the menu");
}

/// A footer hint does what it says when clicked: `q play next` queues the track, never `n`'s
/// next track.
#[test]
fn a_clicked_hint_does_what_it_says() {
    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Tracks;
    point(&mut app, LEFT, "play next");
    let up: Vec<&str> = app.crate_list.up_next().map(|t| t.track.title.as_str()).collect();
    assert_eq!(up.len(), 1, "the selected track is up next: {up:?}");
    assert!(app.now.is_none(), "nothing skipped to play");
}

/// Recordings' own playback keys are rows of their own; with nothing to do there, they do what
/// they do everywhere.
#[test]
fn recordings_keys_fall_back_to_everywheres() {
    let (mut app, _dir) = app_with(library());
    app.set_view(View::Mixes);
    for (k, act) in [("n", Act::MixTrack(true)), ("p", Act::MixTrack(false)), (" ", Act::MixPlay)] {
        let b = keymap::lookup(
            &keymap::scopes(&app),
            &ratatui::crossterm::event::KeyEvent::from(KeyCode::Char(k.chars().next().unwrap())),
        );
        assert_eq!(b.map(|b| b.act), Some(act), "{k:?}");
    }
    press(&mut app, "n");
    assert!(app.now.is_none() && !app.quit);
}

/// Opening a recording by path (from `:` or the actions menu) shows the list the path is typed in, however narrow.
#[test]
fn opening_a_recording_by_path_shows_where_it_is_typed() {
    let (mut app, _dir) = app_with(library());
    app.set_view(View::Mixes);
    app.mix.pane = Pane::Tracks;
    app.perform(Act::MixOpen, None);
    assert!(app.mix.pane == Pane::Mixes && app.mix.input.is_some());
}

/// Lists in dialogs stop at their ends, a form's fields too.
#[test]
fn a_dialog_list_stops_at_its_ends() {
    let mut state = ratatui::widgets::ListState::default().with_selected(Some(0));
    step_list(&mut state, 3, Act::Move(-1));
    assert_eq!(state.selected(), Some(0));
    step_list(&mut state, 3, Act::End(1));
    assert_eq!(state.selected(), Some(2));
    step_list(&mut state, 3, Act::Page(1));
    assert_eq!(state.selected(), Some(2));
    step_list(&mut state, 0, Act::Move(1));
    assert_eq!(scrolled(5, Act::End(-1)), 0);
    assert_eq!(scrolled(5, Act::Page(-1)), 0);
    assert_eq!(scrolled(5, Act::Activate), 5);
}

/// What only a dialog does (yes, no, a field's next choice, cancelling a job, where the search
/// looks) does nothing when it reaches a view: no key there is bound to it.
#[test]
fn a_dialogs_own_acts_do_nothing_in_a_view() {
    let (mut app, _dir) = app_with(library());
    let before = screen(&mut app, 100, 30);
    for act in [Act::None, Act::Answer(true), Act::Answer(false), Act::Change(1), Act::CancelJob, Act::SearchWhere] {
        app.perform(act, None);
    }
    assert_eq!(screen(&mut app, 100, 30), before);
    assert!(app.modal.is_none() && !app.quit && !app.searching);
}

/// A click outside a plan to confirm puts it away, writing nothing.
#[test]
fn a_click_outside_a_plan_writes_nothing() {
    use crate::tui::edits::Confirm;
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let plan = Plan { operation: "edit", summary: vec!["a line".into()], ..Default::default() };
    app.modal = Some(Modal::Confirm(Confirm {
        root,
        drive: "TEST".into(),
        plan: Box::new(plan),
        scroll: 0,
        shown: Instant::now(),
        after: None,
    }));
    screen(&mut app, 100, 30);
    mouse(&mut app, LEFT, (0, 0));
    assert!(app.modal.is_none());
    assert_eq!(last_message(&app), "nothing written");
}

/// A click ends what a key would: a click between two escs means the second doesn't quit.
#[test]
fn a_click_ends_esc_s_wait() {
    let (mut app, _dir) = app_with(library());
    screen(&mut app, 100, 30);
    key(&mut app, KeyCode::Esc);
    mouse(&mut app, LEFT, (0, 0));
    key(&mut app, KeyCode::Esc);
    assert!(!app.quit, "a click came between the escs");
}

/// Too small for a footer, the deck's line says what a second esc does.
#[test]
fn with_only_the_deck_the_first_esc_still_says_so() {
    let (mut app, _dir) = app_with(library());
    screen(&mut app, 50, 15);
    key(&mut app, KeyCode::Esc);
    let s = screen(&mut app, 50, 15);
    assert!(s.contains("esc again to quit") && s.contains("Nothing playing"), "the deck strip stays:\n{s}");
    assert!(!app.quit);
}

/// Esc turns the pads off before it asks for a second to quit.
#[test]
fn esc_leaves_the_pads_first() {
    let (mut app, _dir) = app_with(library());
    app.deck.pads = true;
    key(&mut app, KeyCode::Esc);
    assert!(!app.deck.pads && !app.quit_waiting());
    key(&mut app, KeyCode::Esc);
    assert!(app.quit_waiting());
}

/// Every dialog stays open on `q` and the other keys it doesn't use, and closes on esc without
/// arming quit: a result, a question, a plan, the job list, and a name being typed.
#[test]
fn every_dialog_waits_for_esc() {
    use crate::tui::drives::{Report, Tone};
    use crate::tui::edits::Confirm;
    use ratatui::widgets::ListState;
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    // How each is opened, on the drive at the path given.
    type Opens = fn(&mut App, &std::path::Path);
    let open: [(&str, Opens); 5] = [
        ("a result", |app, _| {
            app.modal = Some(Modal::Report(Report {
                title: "Done".into(),
                lines: vec![(Tone::Text, "a line".into())],
                scroll: 0,
            }));
        }),
        ("a question", |app, _| app.modal = Some(Modal::Ask("Sure?".into(), Box::new(|_: &mut App| {})))),
        ("a plan", |app, root| {
            let plan = Plan { operation: "edit", summary: vec!["a line".into()], ..Default::default() };
            let confirm = Confirm {
                root: root.to_path_buf(),
                drive: "TEST".into(),
                plan: Box::new(plan),
                scroll: 0,
                shown: Instant::now(),
                after: None,
            };
            app.modal = Some(Modal::Confirm(confirm));
        }),
        ("the jobs", |app, _| app.modal = Some(Modal::Jobs(ListState::default()))),
        ("a name being typed", |app, _| {
            app.track_state.select(Some(0));
            let target = app.target().unwrap();
            app.run_action(crate::tui::edits::Action::NewPlaylistWith, target);
        }),
    ];
    for (what, open) in open {
        open(&mut app, &root);
        assert!(app.modal.is_some(), "{what} is open");
        if what != "a name being typed" {
            press(&mut app, "qx");
            assert!(app.modal.is_some(), "{what} stays open on q and x");
        }
        // Long enough for a plan to take keys.
        if let Some(Modal::Confirm(c)) = &mut app.modal {
            c.shown = Instant::now() - std::time::Duration::from_secs(5);
        }
        key(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none(), "esc closes {what}");
        assert!(!app.quit_waiting() && !app.quit, "closing {what} doesn't arm quit");
    }
}

/// Esc with a job under way asks before stopping it, however often it's pressed, and never quits;
/// yes stops it.
#[test]
fn esc_with_a_job_asks_before_stopping_it_and_never_quits() {
    let (mut app, _dir) = app_with(library());
    app.job = Some(job(false, false));
    for _ in 0..3 {
        key(&mut app, KeyCode::Esc);
        assert!(
            matches!(&app.modal, Some(Modal::Ask(q, _)) if q == "Planning is under way: stop it?"),
            "{:?}",
            app.toast
        );
        key(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none() && !app.quit && !app.quit_waiting());
    }
    assert!(app.job.as_ref().is_some_and(|j| !j.cancel.is_cancelled()), "still going");
    key(&mut app, KeyCode::Esc);
    press(&mut app, "y");
    assert!(app.job.is_none() || app.job.as_ref().is_some_and(|j| j.cancel.is_cancelled()), "stopped");
    assert!(!app.quit);
}

/// Quitting asks while anything the jobs list shows is under way, not only a drive's job.
#[test]
fn quitting_asks_while_anything_is_under_way() {
    let (mut app, _dir) = app_with(library());
    app.status_loading = true;
    app.on_key(ratatui::crossterm::event::KeyEvent::new(
        KeyCode::Char('c'),
        ratatui::crossterm::event::KeyModifiers::CONTROL,
    ));
    let Some(Modal::Ask(q, _)) = &app.modal else { panic!("quit without asking") };
    assert!(q.ends_with("is under way: quit anyway?"), "{q}");
    assert!(!app.quit);
}

/// With more than one thing under way, quitting says how many.
#[test]
fn quitting_counts_what_is_under_way() {
    let (mut app, _dir) = app_with(library());
    app.job = Some(job(false, false));
    app.status_loading = true;
    app.on_key(ratatui::crossterm::event::KeyEvent::new(
        KeyCode::Char('c'),
        ratatui::crossterm::event::KeyModifiers::CONTROL,
    ));
    let Some(Modal::Ask(q, _)) = &app.modal else { panic!("quit without asking") };
    assert_eq!(q, "2 jobs are under way: quit anyway?");
    assert!(!app.quit);
}

/// Filtering the actions menu shows what's typed in its title, and the footer the filter's keys.
#[test]
fn the_actions_menu_shows_its_filter() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "e/col");
    let s = screen(&mut app, 120, 30);
    assert!(s.contains("Actions · / col"), "{s}");
    assert!(s.contains("esc clear"), "{s}");
}

/// An edit given a key of its own does it to what's selected, and with no drive open says to open
/// one; `:` runs it too.
#[test]
fn an_edit_on_a_key_wants_a_drive() {
    let (mut app, _dir) = blank(View::Library);
    let mut keys = locallibrary::keybinds::Keybinds::default();
    keys.keys.insert("edit.rating-menu".into(), vec!["]".into()]);
    locallibrary::keybinds::save(&app.store_root, &keys).unwrap();
    app.load_keybinds();
    press(&mut app, "]");
    assert_eq!(last_message(&app), "open a drive first");
    assert!(app.modal.is_none());

    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Tracks;
    press(&mut app, ":set the colour\n");
    let Some(Modal::Menu(m)) = &app.modal else { panic!("no menu: {}", last_message(&app)) };
    assert_eq!(m.sub, Some(crate::tui::edits::Sub::Colour));
    let s = screen(&mut app, 120, 30);
    assert!(s.contains("Colour · "), "{s}");
}

/// Importing someone's keys asks whose, again when nothing's typed; a name that isn't a handle is
/// refused before anything goes online.
#[cfg(feature = "account")]
#[test]
fn importing_keys_asks_whose() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, ":import someone's keys\n");
    let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt: {}", last_message(&app)) };
    assert_eq!(p.label, "Import the keys of (a handle or DID)");
    press(&mut app, "\n");
    assert_eq!(last_message(&app), "type their handle (alice.bsky.social) or DID");
    assert!(matches!(app.modal, Some(Modal::Prompt(_))), "still asking");
    press(&mut app, "no one\n");
    assert_eq!(last_message(&app), "reading no one's keys…");
    let refused = |a: &App| a.log.iter().any(|(m, ..)| m.contains("the Atmosphere account \"no one\" (not a handle"));
    wait(&mut app, refused);
}

/// The mouse moving over the Keys screen leaves it as it is.
#[test]
fn the_keys_screen_ignores_the_mouse_moving() {
    use ratatui::crossterm::event::MouseEventKind;
    let (mut app, _dir) = app_with(library());
    press(&mut app, ":change the keys\n");
    mouse(&mut app, MouseEventKind::ScrollDown, (10, 10));
    mouse(&mut app, MouseEventKind::Moved, (10, 10));
    assert_eq!(app.keys.as_ref().map(|k| k.row), Some(3));
}
