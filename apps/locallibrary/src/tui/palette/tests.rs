use super::super::app::testing::{LEFT, app_with, blank, key, last_message, library, mouse, point, press, spot};
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

/// In a list any of which may be chosen, space marks the row selected or takes its mark off, and
/// each row shows whether it's marked.
#[hegel::test(test_cases = 30)]
fn prop_a_list_shows_the_marks_on_its_rows(tc: hegel::TestCase) {
    use hegel::generators as gs;
    const ROWS: usize = 5;
    let (mut app, _dir) = app_with(library());
    let items = (0..ROWS).map(|i| Item::new(format!("row {i}"), "")).collect();
    let mut picker = Picker::new("Pick some", items, Then::Act(vec![Act::Quit; ROWS]));
    picker.marks = Some(vec![false; ROWS]);
    app.modal = Some(Modal::Pick(picker));
    let mut marked = [false; ROWS];
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        if tc.draw(gs::booleans()) {
            key(&mut app, KeyCode::Down);
            continue;
        }
        let Some(Modal::Pick(p)) = &app.modal else { panic!("the list closed") };
        marked[p.selected().unwrap()] ^= true;
        press(&mut app, " ");
    }
    let s = super::super::ui::screen(&mut app, 120, 30);
    for (i, marked) in marked.iter().enumerate() {
        let row = s.lines().find(|l| l.contains(&format!("row {i}"))).unwrap_or_else(|| panic!("no row {i}\n{s}"));
        assert_eq!(row.contains(super::super::icons::CHECK.get()), *marked, "row {i}\n{s}");
    }
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
    press(&mut app, "gotod");
    let Some(Modal::Pick(p)) = &app.modal else { panic!() };
    assert_eq!(p.items[p.selected().unwrap()].label, "go to Drives");
    // Keys go to the box, not to the player: Q didn't quit.
    press(&mut app, "Q");
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
    point(&mut app, LEFT, "go to Drives");
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
        // (The Prepare view has no commands of its own.)
        let first = KEYMAP.iter().find(|b| b.act == cmds[0].0).unwrap();
        assert!(view == View::Prepare || first.scope != Scope::Global, "{view:?} starts with its own");
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
        // Scrolled to it first, as a list longer than the menu has rows is before its row's
        // clicked.
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

fn palette(app: &App) -> &Picker {
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no palette") };
    p
}

fn shown_labels(p: &Picker) -> Vec<&str> {
    p.shown.iter().map(|&i| p.items[i].label.as_str()).collect()
}

/// What's been run comes first, newest first, in a palette opened after; a run moves it up.
#[test]
fn commands_run_lately_rank_first() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, ":\u{1b}");
    press(&mut app, ":shuffle\n:repeat\n");
    assert_eq!(app.recent_commands[..2], ["repeat: off, all, one", "shuffle on or off"]);
    press(&mut app, ":");
    assert_eq!(shown_labels(palette(&app))[..2], ["repeat: off, all, one", "shuffle on or off"]);
    let p = palette(&app);
    assert_eq!(p.items[p.selected().unwrap()].label, "repeat: off, all, one", "enter runs the last again");
    press(&mut app, "\u{1b}:shuffle\n");
    assert_eq!(app.recent_commands[0], "shuffle on or off");
    assert_eq!(app.recent_commands.len(), 2, "no name twice");
}

/// Only so many are kept, and what's plugin-made and gone by next time isn't one of them.
#[test]
fn the_recent_list_stays_short() {
    let (mut app, _dir) = app_with(library());
    for i in 0..RECENT + 5 {
        app.used(format!("command {i}"));
    }
    assert_eq!(app.recent_commands.len(), RECENT);
    assert_eq!(app.recent_commands[0], format!("command {}", RECENT + 4));

    let mut p = Picker::new("Commands", vec![Item::new("a suggestion", "")], Then::Act(vec![Act::Review(7)]));
    p.rank(&[]);
    let before = app.recent_commands.clone();
    app.modal = Some(Modal::Pick(p));
    press(&mut app, "\n");
    assert_eq!(app.recent_commands, before);
}

/// Being recent settles near ties and doesn't lift a scattered match over a clean one.
#[test]
fn recent_is_an_edge_with_something_typed() {
    let items = vec![Item::new("snapshots", ""), Item::new("set now a panel", "")];
    let mut p = Picker::new("Commands", items, Then::Act(vec![Act::Quit, Act::Help]));
    p.rank(&["set now a panel".to_string()]);
    assert_eq!(shown_labels(&p), ["set now a panel", "snapshots"], "nothing typed: recent first");
    p.query = "snap".into();
    p.filter();
    assert_eq!(shown_labels(&p), ["snapshots", "set now a panel"]);
    let items = vec![Item::new("sync the drives", ""), Item::new("sync the sets", "")];
    let mut p = Picker::new("Commands", items, Then::Act(vec![Act::Quit, Act::Help]));
    p.rank(&["sync the sets".to_string()]);
    p.query = "sync".into();
    p.filter();
    assert_eq!(shown_labels(&p), ["sync the sets", "sync the drives"]);
}

/// A label typed out in full is the one chosen, even with a longer one holding it used lately.
#[test]
fn a_label_typed_in_full_outranks_a_recent_one() {
    let items = vec![Item::new("cover art", ""), Item::new("no cover art", "")];
    let mut p = Picker::new("Commands", items, Then::Act(vec![Act::Art(true), Act::Art(false)]));
    p.rank(&["no cover art".to_string()]);
    p.query = "cover art".into();
    p.filter();
    assert_eq!(shown_labels(&p), ["cover art", "no cover art"]);
    p.query = "cover".into();
    p.filter();
    assert_eq!(shown_labels(&p), ["no cover art", "cover art"], "a part typed still leans to the recent one");
}

/// They're kept between runs, in what's saved and what `settings export` takes.
#[test]
fn recent_commands_outlive_a_run() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, ":shuffle\n");
    let saved = app.saved_state();
    let (mut next, _dir) = app_with(library());
    next.apply_saved(&saved, false);
    press(&mut next, ":");
    assert_eq!(shown_labels(palette(&next))[0], "shuffle on or off");

    let json = serde_json::to_value(&saved).unwrap();
    let prefs: locallibrary::settings::transfer::PlayerPrefs = serde_json::from_value(json).unwrap();
    assert_eq!(prefs.recent_commands.unwrap(), ["shuffle on or off"]);
}

/// A command shows its key; one with no key here says where it lives.
#[test]
fn every_command_shows_its_key_or_its_home() {
    let detail = |view, act| commands(view).into_iter().find(|(a, _)| *a == act).unwrap().1.detail;
    assert_eq!(detail(View::Library, Act::Quit), "Q");
    assert_eq!(detail(View::Library, Act::QueueNext), "q n");
    assert_eq!(detail(View::Drives, Act::Pull), "W · pull");
    assert_eq!(detail(View::Prepare, Act::Eject), "E · eject");
    assert_eq!(detail(View::Library, Act::Pull), "Prepare · pull");
    assert_eq!(detail(View::Library, Act::Jobs), "&");
    assert_eq!(detail(View::Library, Act::ShowView(View::Drives)), "g d");
    for view in View::ALL {
        for (act, item) in commands(view) {
            assert!(!item.detail.is_empty(), "{act:?} in {view:?}");
        }
    }
}

/// What can't run now stays listed, greyed with the reason; enter says the reason and stays open.
#[test]
fn what_cant_run_is_greyed_with_the_reason() {
    let (mut app, _dir) = blank(View::Library);
    press(&mut app, ":eject");
    let s = super::super::ui::screen(&mut app, 120, 40);
    assert!(s.contains("eject the drive · open a drive first"), "{s}");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| super::super::ui::draw(f, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y: u16| (0..120).map(|x| buffer[(x, y)].symbol()).collect::<String>();
    let y = (0..40).find(|y| row(*y).contains("eject the drive")).unwrap();
    let x = (0..120).find(|x| buffer[(*x, y)].symbol() == "j").unwrap();
    assert_eq!(buffer[(x, y)].fg, super::super::theme::faint().fg.unwrap());
    press(&mut app, "\n");
    assert_eq!(last_message(&app), "open a drive first");
    assert!(matches!(app.modal, Some(Modal::Pick(_))), "it stays");
}

/// With something selected or playing, those reasons go, and a job running is a reason of its own.
#[test]
fn the_reasons_follow_what_the_player_is_doing() {
    let (mut app, _dir) = app_with(library());
    assert_eq!(app.unavailable(Act::Pull), None);
    assert_eq!(app.unavailable(Act::Stop).as_deref(), Some("nothing's playing"));
    assert_eq!(app.unavailable(Act::SmartNew).as_deref(), Some("search first (/)"));
    assert_eq!(app.unavailable(Act::SortReverse).as_deref(), Some("sort the list first (b)"));
    assert_eq!(app.unavailable(Act::CrateClear).as_deref(), Some("the crate is empty"));
    app.job = Some(super::super::app::testing::job(false, false));
    assert_eq!(app.unavailable(Act::Pull).as_deref(), Some("wait for planning to finish"));
    app.job = None;
    app.query = "bpm:120".into();
    assert_eq!(app.unavailable(Act::SmartNew), None);

    app.view = View::Drives;
    app.drive_header = Some(super::super::drives::DriveRow::Library(uuid::Uuid::new_v4()));
    assert_eq!(app.unavailable(Act::Eject), None);
    assert_eq!(app.unavailable(Act::Pull).as_deref(), Some("select one of the library's drives"));
}

/// `e` in a pane with no menu of its own: enter's own action first, so enter runs it, then its keys.
#[test]
fn e_opens_a_panes_actions() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "gp\u{1b}e");
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no menu") };
    assert_eq!(p.title, "Actions · Prepare");
    assert_eq!((p.items[0].label.as_str(), p.items[0].detail.as_str()), ("run", "enter"));
    assert!(p.items.iter().any(|i| i.label == "eject the drive" && i.detail == "E"));
    press(&mut app, "\n");
    assert!(app.job.is_some(), "the first job selected, a check, runs");

    for (view, first) in
        [(View::Mixes, "open · play from"), (View::Profile, "put the section in the document, or take it out")]
    {
        let (mut app, _dir) = blank(view);
        press(&mut app, "e");
        let Some(Modal::Pick(p)) = &app.modal else { panic!("no menu in {view:?}") };
        assert_eq!(p.items[0].label, first);
    }
}

/// Whatever's been run, the list of recent commands holds each name once, the last run first, and
/// never more than it keeps.
#[hegel::test]
fn prop_recent_commands_stay_short_and_unique(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir) = app_with(library());
    let names = tc.draw(gs::vecs(gs::integers::<u8>().max_value(30)).max_size(60));
    for n in &names {
        app.used(format!("command {n}"));
    }
    let kept = &app.recent_commands;
    assert!(kept.len() <= RECENT);
    assert_eq!(names.last().map(|n| format!("command {n}")).as_ref(), kept.first());
    let mut unique = kept.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), kept.len());
}

/// However they rank, every item that matches shows exactly once, and a drawn palette's selected
/// row is on screen.
#[hegel::test]
fn prop_ranking_keeps_every_match(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(120));
    let items: Vec<Item> = (0..n).map(|i| Item::new(format!("command {i}"), "")).collect();
    let mut p = Picker::new("Commands", items, Then::Act(vec![Act::Quit; n]));
    let recent: Vec<String> = tc
        .draw(gs::vecs(gs::integers::<usize>().max_value(130)).max_size(8))
        .into_iter()
        .map(|i| format!("command {i}"))
        .collect();
    p.rank(&recent);
    p.query = tc.draw(gs::sampled_from(vec!["", "1", "co", "9 9", "zz"])).to_string();
    p.filter();
    let matching = (0..n).filter(|&i| fuzzy(&p.query, &p.items[i].label).is_some()).count();
    let mut shown = p.shown.clone();
    shown.sort_unstable();
    shown.dedup();
    assert_eq!(shown.len(), matching);
    let (mut app, _dir) = app_with(library());
    let down = tc.draw(gs::integers::<usize>().max_value(130));
    (0..down).for_each(|_| p.step(1));
    let selected = p.selected();
    app.modal = Some(Modal::Pick(p));
    let s = super::super::ui::screen(&mut app, 80, 24);
    if let Some(i) = selected {
        assert!(s.contains(&format!("command {i}")), "{i}: {s}");
    }
}
