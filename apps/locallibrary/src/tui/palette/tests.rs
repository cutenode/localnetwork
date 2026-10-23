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

/// Keys are named apart, whatever they are: characters as themselves (space by name), arrows as
/// arrows, other keys by their name, with shift or ctrl said, and a run of digits from one to the
/// other; no two keys share a name.
#[hegel::test(test_cases = 300)]
fn keys_are_named_apart(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let codes = [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::PageDown,
        KeyCode::F(5),
        KeyCode::Tab,
        KeyCode::Enter,
    ];
    let any = |tc: &hegel::TestCase| match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => K::Ch(tc.draw(gs::sampled_from(&[' ', 'a', 'A', '1', ':'][..]))),
        1 => K::Code(tc.draw(gs::sampled_from(&codes[..]))),
        2 => K::Shift(tc.draw(gs::sampled_from(&codes[..]))),
        3 => K::Ctrl(tc.draw(gs::sampled_from(&['c', 'q', 'u'][..]))),
        _ => K::Digits('1', tc.draw(gs::sampled_from(&['5', '8'][..]))),
    };
    let (a, b) = (any(&tc), any(&tc));
    let (na, nb) = (key_name(&a), key_name(&b));
    assert_eq!(na == nb, format!("{a:?}") == format!("{b:?}"), "{a:?} and {b:?} both read {na:?}");
    match a {
        K::Ch(' ') => assert_eq!(na, "space"),
        K::Ch(c) => assert_eq!(na, c.to_string()),
        K::Code(KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right) => assert!("↑↓←→".contains(&na)),
        K::Code(_) => assert!(na.chars().all(|c| !c.is_uppercase()), "{na}"),
        K::Shift(_) => assert!(na.starts_with("shift ")),
        K::Ctrl(c) => assert_eq!(na, format!("ctrl-{c}")),
        K::Digits(x, y) => assert_eq!(na, format!("{x}-{y}")),
    }
}

/// A list of actions does what's chosen (by enter, or a click on its row): an item that can't
/// be done says why and stays; a drive's action is done to that drive while it's plugged in (it
/// says when it isn't any more); a library's to its clones; a click outside closes the list.
#[hegel::test(test_cases = 100)]
fn a_choice_from_a_list_is_done_to_its_drive(tc: hegel::TestCase) {
    use super::super::app::testing::{blank, fleet};
    use super::super::drives::DriveRow;
    use hegel::generators as gs;
    let (mut app, dir) = blank(View::Drives);
    let root = dir.path().join("DNA");
    std::fs::create_dir_all(&root).unwrap();
    let main = uuid::Uuid::from_u128(1);
    let export = tc.draw(gs::booleans());
    let info = locallibrary::DeviceInfo {
        name: Some("DNA".into()),
        has_export: export,
        library_id: Some(main),
        ..locallibrary::Device::open(&root).unwrap().info()
    };
    app.library_names = [(main, "Main".to_string())].into_iter().collect();
    app.drives = vec![info.clone()];
    app.status = Some(Ok(vec![fleet(info, Some("Main"))]));
    let library = tc.draw(gs::booleans());
    app.select_drive_row(if library { DriveRow::Library(main) } else { DriveRow::Drive(0) });
    app.drive_menu();
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no menu: {}", last_message(&app)) };
    let n = p.shown.len();
    let i = tc.draw(gs::integers::<usize>().max_value(n - 1));
    let (label, unavailable) = (p.items[p.shown[i]].label.clone(), p.items[p.shown[i]].unavailable.clone());
    let gone = !library && tc.draw(gs::booleans());
    if gone {
        app.drives.clear();
    }
    if tc.draw(gs::booleans()) {
        let Some(Modal::Pick(p)) = &mut app.modal else { unreachable!() };
        p.state.select(Some(i));
        key(&mut app, KeyCode::Enter);
    } else {
        // Scrolled to it first, as the arrows or the wheel would: a long menu runs past the screen.
        let Some(Modal::Pick(p)) = &mut app.modal else { unreachable!() };
        p.state.select(Some(i));
        super::super::ui::screen(&mut app, 120, 40);
        let r = app.areas.menu;
        if tc.draw(gs::integers::<u8>().max_value(4)) == 0 {
            mouse(&mut app, LEFT, (r.x + r.width + 1, r.y));
            return assert!(app.modal.is_none(), "a click outside closes it");
        }
        let Some(Modal::Pick(p)) = &app.modal else { unreachable!() };
        let row = r.y + 2 + (i - p.state.offset()) as u16;
        mouse(&mut app, LEFT, (r.x + 2, row));
    }
    let said = last_message(&app).to_string();
    match unavailable {
        Some(why) => {
            assert_eq!(said, why, "{label}");
            assert!(matches!(app.modal, Some(Modal::Pick(_))), "it stays");
        }
        None if gone => assert_eq!(said, "DNA isn't plugged in any more", "{label}"),
        None if library && !export => assert_eq!(said, "none of Main's drives is plugged in any more", "{label}"),
        None => {
            // Done: the list is gone, and something is under way, asked, or said.
            let listed = matches!(&app.modal, Some(Modal::Pick(p)) if p.title.starts_with("Drive ·") || p.title.starts_with("Library ·"));
            assert!(!listed, "{label}: the list stayed");
            assert!(app.job.is_some() || app.modal.is_some() || !said.is_empty(), "{label}: nothing done");
        }
    }
}
