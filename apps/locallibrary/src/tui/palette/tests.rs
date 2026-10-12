use super::super::app::testing::{LEFT, app_with, key, last_message, library, mouse, point, press, spot};
use super::*;

#[test]
fn fuzzy_matching_prefers_word_starts_and_runs() {
    assert!(fuzzy("", "anything").is_some());
    assert!(fuzzy("xyz", "next track").is_none());
    assert!(fuzzy("nt", "next track").is_some());
    // Starts of words beat letters in the middle.
    assert!(fuzzy("nt", "next track") > fuzzy("nt", "print"));
    // A run beats the same letters spread out.
    assert!(fuzzy("loop", "loop here") > fuzzy("loop", "l o o p"));
    assert_eq!(fuzzy("NEXT", "next track"), fuzzy("next", "Next Track"), "case doesn't matter");
    assert!(fuzzy("next track", "next track").is_some(), "spaces in the query are ignored");
}

#[test]
fn the_palette_lists_the_views_commands_first_and_runs_one() {
    let cmds = commands(View::Profile);
    assert_eq!(cmds[0].0, Act::ProfileInclude, "the view's own first");
    assert!(cmds.iter().any(|(a, _)| *a == Act::Quit));
    assert!(!cmds.iter().any(|(a, _)| *a == Act::CrateSave), "not the library's in the profile");

    let (mut app, _dir) = app_with(library());
    press(&mut app, ":");
    assert!(matches!(app.modal, Some(Modal::Pick(_))));
    press(&mut app, "drives");
    let Some(Modal::Pick(p)) = &app.modal else { panic!() };
    assert_eq!(p.items[p.selected().unwrap()].label, "the drives");
    // Keys go to the box, not to the player: q didn't quit.
    press(&mut app, "q");
    assert!(!app.quit);
    key(&mut app, KeyCode::Backspace);
    press(&mut app, "\n");
    assert!(app.modal.is_none());
    assert_eq!(app.view, View::Drives);
    // Nothing matching: enter says so and stays.
    press(&mut app, ":zzzz\n");
    assert!(matches!(app.modal, Some(Modal::Pick(_))));
    assert_eq!(last_message(&app), "no command has those letters");
    press(&mut app, "\u{1b}");
    assert!(app.modal.is_none());
}

#[test]
fn a_command_is_found_by_the_name_beside_it() {
    let (acts, items): (Vec<Act>, Vec<Item>) = commands(View::Drives).into_iter().unzip();
    let mut p = Picker::new("Commands", items, Then::Act(acts.clone()));
    for (typed, act) in [("pull", Act::Pull), ("set prep", Act::SetPrep), ("restore", Act::RestorePlaylist)] {
        p.query = typed.into();
        p.filter();
        assert_eq!(acts[p.selected().unwrap()], act, "{typed}");
    }
}

/// The list moves with the arrows and pages, and with the wheel, stopping at either end; a
/// click on an item chooses it, and a click outside closes the list.
#[test]
fn the_palette_moves_and_is_clicked() {
    let (mut app, _dir) = app_with(library());
    let selected = |app: &App| match &app.modal {
        Some(Modal::Pick(p)) => p.state.selected(),
        _ => panic!("closed"),
    };
    press(&mut app, ":");
    key(&mut app, KeyCode::PageDown);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Up);
    assert_eq!(selected(&app), Some(10));
    key(&mut app, KeyCode::PageUp);
    key(&mut app, KeyCode::Up);
    assert_eq!(selected(&app), Some(0), "stops at the top");
    let title = spot(&mut app, "Commands");
    mouse(&mut app, MouseEventKind::ScrollDown, title);
    mouse(&mut app, MouseEventKind::Moved, title);
    assert_eq!(selected(&app), Some(3));
    mouse(&mut app, MouseEventKind::ScrollUp, title);
    assert_eq!(selected(&app), Some(0));
    // A click on the title (or the line being typed) chooses nothing.
    mouse(&mut app, LEFT, title);
    assert_eq!(selected(&app), Some(0));
    press(&mut app, "drive");
    point(&mut app, LEFT, "the drives");
    assert!(app.modal.is_none());
    assert_eq!(app.view, View::Drives);
    // Nothing matching: there's nothing to move to.
    press(&mut app, ":zzzz");
    key(&mut app, KeyCode::Down);
    assert_eq!(selected(&app), None);

    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), (0, 0));
    assert!(app.modal.is_none(), "closed by a click outside");
}

/// An item that can't be chosen now says why, and the list stays.
#[test]
fn an_item_that_cant_be_chosen_says_why() {
    let (mut app, _dir) = app_with(library());
    let item = Item { unavailable: Some("not while it plays".into()), ..Item::new("quit", "q") };
    app.modal = Some(Modal::Pick(Picker::new("Commands", vec![item], Then::Act(vec![Act::Quit]))));
    press(&mut app, "\n");
    assert!(!app.quit && matches!(app.modal, Some(Modal::Pick(_))));
    assert_eq!(last_message(&app), "not while it plays");
}

/// Every view's palette lists its own commands first and each command once, with the key
/// that does it written as the help writes it.
#[test]
fn every_views_commands_name_their_keys_as_the_help_does() {
    for view in View::ALL {
        let cmds = commands(view);
        let mut acts: Vec<Act> = cmds.iter().map(|(a, _)| *a).collect();
        let n = acts.len();
        acts.sort_by_key(|a| format!("{a:?}"));
        acts.dedup();
        assert_eq!(acts.len(), n, "{view:?} lists a command twice");
        // (The Snapshots view has no commands of its own.)
        let first = KEYMAP.iter().find(|b| b.act == cmds[0].0).unwrap();
        assert!(view == View::Snapshots || first.scope != Scope::Global, "{view:?} starts with its own");
    }
    // Each command in the palette, beside the key the help shows for it.
    for b in KEYMAP.iter().filter(|b| !b.name.is_empty() && !b.shown.is_empty()) {
        let Some(k) = b.keys.first() else { continue };
        assert!(b.shown.contains(&key_name(k)), "{:?}: {} isn't in {:?}", b.act, key_name(k), b.shown);
    }
}

/// A query matches just when its letters (spaces aside, any case) are in the text in order.
/// Neither case nor the query's spaces change the score, and a letter the query doesn't have,
/// put anywhere in the text, never makes it match better: it can only split a run, push a
/// letter further in or stop one starting a word.
#[hegel::test(test_cases = 300)]
fn fuzzy_matches_letters_in_order(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let text: String = tc.draw(gs::text().alphabet("abcAB -").max_size(12));
    let query: String = tc.draw(gs::text().alphabet("abcAB ").max_size(5));
    let lower = |s: &str| s.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect::<Vec<_>>();
    let (q, t) = (lower(&query), text.chars().flat_map(char::to_lowercase).collect::<Vec<_>>());
    let mut rest = t.iter();
    let in_order = q.iter().all(|c| rest.any(|x| x == c));
    let score = fuzzy(&query, &text);
    assert_eq!(score.is_some(), in_order, "{query:?} in {text:?}");
    assert_eq!(fuzzy(&query.to_uppercase(), &text.to_lowercase()), score, "{query:?} in {text:?}");
    assert_eq!(fuzzy(&query.replace(' ', ""), &text), score, "{query:?} in {text:?}");
    for at in 0..=text.len() {
        let padded = format!("{}x{}", &text[..at], &text[at..]);
        assert!(fuzzy(&query, &padded) <= score, "{query:?} in {padded:?} beats {text:?}");
    }
}
