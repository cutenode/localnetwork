use super::super::app::testing::{app_with, library};
use super::*;

fn press(c: KeyCode) -> KeyEvent {
    let shift = matches!(c, KeyCode::Char(ch) if ch.is_ascii_uppercase());
    KeyEvent::new(c, if shift { KeyModifiers::SHIFT } else { KeyModifiers::NONE })
}

#[test]
fn the_most_specific_scope_wins() {
    let del = press(KeyCode::Delete);
    assert_eq!(lookup(&[Scope::Crate, Scope::Library, Scope::Global], &del).map(|b| b.scope), Some(Scope::Crate));
    assert_eq!(lookup(&[Scope::Tracks, Scope::Library, Scope::Global], &del).map(|b| b.scope), Some(Scope::Library));
    let w = press(KeyCode::Char('w'));
    assert_eq!(lookup(&[Scope::Profile, Scope::Global], &w).map(|b| b.act), Some(Act::ProfileWrite));
    assert_eq!(lookup(&[Scope::Mixes, Scope::Global], &w).map(|b| b.act), Some(Act::MixReport));
    assert!(lookup(&[Scope::Drives, Scope::Global], &w).is_none());
    // Digits are views, unless the pads are on.
    let three = press(KeyCode::Char('3'));
    assert_eq!(lookup(&[Scope::Library, Scope::Global], &three).map(|b| b.act), Some(Act::ViewKey));
    assert_eq!(lookup(&[Scope::Pads, Scope::Library, Scope::Global], &three).map(|b| b.act), Some(Act::HotCue));
    let nine = press(KeyCode::Char('9'));
    assert!(lookup(&[Scope::Pads, Scope::Global], &nine).is_none());
    // Shift-up moves in the crate; plain up moves the selection.
    let shift_up = KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT);
    assert_eq!(lookup(&[Scope::Crate, Scope::Global], &shift_up).map(|b| b.act), Some(Act::CrateMove(-1)));
    assert_eq!(lookup(&[Scope::Tracks, Scope::Global], &shift_up).map(|b| b.act), Some(Act::Move(-1)));
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(lookup(&[Scope::Global], &ctrl_c).map(|b| b.act), Some(Act::Quit));
}

/// A key means one thing in each scope (the first row wins, so a second would never run),
/// and no view takes a key that works everywhere, except to do the same thing its way.
#[test]
fn no_key_is_bound_twice() {
    let samples: Vec<KeyEvent> = (' '..='~')
        .map(|c| press(KeyCode::Char(c)))
        .chain(
            [
                KeyCode::Enter,
                KeyCode::Esc,
                KeyCode::Tab,
                KeyCode::BackTab,
                KeyCode::Up,
                KeyCode::Down,
                KeyCode::Left,
                KeyCode::Right,
                KeyCode::Delete,
                KeyCode::Backspace,
                KeyCode::Insert,
                KeyCode::F(5),
            ]
            .map(press),
        )
        .collect();
    let scopes = [
        Scope::Global,
        Scope::Library,
        Scope::DriveList,
        Scope::Tree,
        Scope::Tracks,
        Scope::Details,
        Scope::Crate,
        Scope::Pads,
        Scope::Drives,
        Scope::Snapshots,
        Scope::Profile,
        Scope::Mixes,
        Scope::Replay,
    ];
    for key in &samples {
        for s in scopes {
            let n = KEYMAP.iter().filter(|b| b.scope == s && b.keys.iter().any(|k| k.matches(key))).count();
            assert!(n <= 1, "{key:?} is bound {n} times in {s:?}");
        }
        // Keys every view has: a view may only take one to do the same with its own things.
        let global = lookup(&[Scope::Global], key).map(|b| b.act);
        for s in [Scope::Drives, Scope::Snapshots, Scope::Profile, Scope::Mixes] {
            let own = KEYMAP.iter().find(|b| b.scope == s && b.keys.iter().any(|k| k.matches(key)));
            if let (Some(own), Some(g)) = (own, global) {
                panic!("{key:?} is {g:?} everywhere but {:?} in {s:?}", own.act);
            }
        }
    }
}

#[test]
fn every_key_is_in_the_help_or_the_palette() {
    for b in KEYMAP.iter().filter(|b| !b.keys.is_empty()) {
        // The second of a pair is told of on the first's line.
        let second = b.shown.is_empty();
        assert!(!b.help.is_empty() || !b.name.is_empty() || second, "{:?} says nothing about itself", b.act);
    }
    let (own, global) = help(View::Library);
    let titles: Vec<&str> = own.iter().chain(&global).map(|(t, _)| *t).collect();
    assert_eq!(titles[..3], ["Browsing", "Marks and the crate", "Changing the drive"]);
    assert!(titles.contains(&"Playback") && titles.contains(&"Deck"));
    let (own, _) = help(View::Profile);
    assert_eq!(own[0].0, "Profile");
}

#[test]
fn the_footer_drops_the_least_important_hints_to_fit() {
    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Tracks;
    let wide = hints(&app, 300);
    let keys: Vec<&str> = wide.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys[..3], ["enter", "space", "/"], "the track list's own first: {keys:?}");
    assert!(keys.contains(&"q") && keys.contains(&"space"));
    for width in [40, 60, 80, 100] {
        let h = hints(&app, width);
        let used: usize = 1 + h.iter().map(|(k, l)| k.width() + l.width() + 3).sum::<usize>();
        assert!(used <= width, "{width}: {h:?}");
        assert!(h.iter().any(|(k, _)| k == "enter"), "the most important stays: {h:?}");
    }
    assert!(!hints(&app, 80).iter().any(|(k, _)| k == "q"), "quitting goes first");
    app.fullscreen = true;
    assert_eq!(hints(&app, 300)[0], ("1-8".to_string(), "hot cue"));
}

/// Each view's help starts with its own keys under its own heading; the keys for everywhere
/// follow, the commands with no key last.
#[test]
fn each_views_help_starts_with_its_own_keys() {
    for (view, title) in [(View::Drives, "Drives"), (View::Snapshots, "Snapshots"), (View::Mixes, "Mixes")] {
        let (own, global) = help(view);
        assert_eq!(own[0].0, title);
        assert!(global.iter().all(|(t, _)| *t != title), "{view:?}");
        assert!(global.last().is_some_and(|(t, _)| t.starts_with("Commands")), "{view:?}");
    }
}

/// The footer's hints fit the width they're given, in any view and pane, and a wider footer
/// keeps every hint a narrower one shows.
#[hegel::test(test_cases = 200)]
fn hints_fit_the_footer_and_widen_with_it(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir) = app_with(library());
    app.view = View::ALL[tc.draw(gs::integers::<usize>().max_value(4))];
    app.focus = [Focus::Drives, Focus::Playlists, Focus::Tracks, Focus::Details, Focus::Crate]
        [tc.draw(gs::integers::<usize>().max_value(4))];
    app.fullscreen = tc.draw(gs::booleans());
    app.deck.pads = tc.draw(gs::booleans());
    let width = tc.draw(gs::integers::<usize>().max_value(200));
    let wider = width + tc.draw(gs::integers::<usize>().max_value(100));
    let shown = hints(&app, width);
    let used: usize = shown.iter().map(|(k, l)| k.width() + l.width() + 3).sum();
    // Each takes its keys, a space, its label and two spaces, after the footer's one.
    assert!(shown.is_empty() || used < width, "{shown:?} in {width}");
    let more = hints(&app, wider);
    assert!(shown.iter().all(|h| more.contains(h)), "{shown:?} at {width}, {more:?} at {wider}");
}

/// Every view's help lists its own keys and then the ones for everywhere, under headings of
/// their own, none empty; its commands (the view's own, then everywhere's) are each named once,
/// and for any letters typed the palette shows just the commands they're in, best first.
#[hegel::test(test_cases = 50)]
fn every_view_has_its_help_and_its_commands(tc: hegel::TestCase) {
    use super::super::palette::{Picker, Then, commands, fuzzy};
    use hegel::generators as gs;
    let typed: String = tc.draw(gs::text().alphabet("aeioustr ").max_size(4));
    for v in View::ALL {
        let (own, global) = help(v);
        for sections in [&own, &global] {
            let titles: Vec<&str> = sections.iter().map(|(t, _)| *t).collect();
            let mut unique = titles.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), titles.len(), "{v:?}: a heading twice");
            assert!(sections.iter().all(|(_, lines)| !lines.is_empty()), "{v:?}: an empty heading");
        }
        let cmds = commands(v);
        let mut names: Vec<&str> = cmds.iter().map(|(_, i)| i.label.as_str()).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n, "{v:?}: a command named twice");
        // The palette shows just the commands those letters are in (by name or key), best first.
        let (acts, items): (Vec<_>, Vec<_>) = cmds.into_iter().unzip();
        let mut p = Picker::new("Commands", items, Then::Act(acts));
        p.query = typed.clone();
        p.filter();
        let score = |i: usize| fuzzy(&typed, &p.items[i].label).max(fuzzy(&typed, &p.items[i].detail));
        let matching = (0..p.items.len()).filter(|&i| score(i).is_some()).count();
        assert_eq!(p.shown.len(), matching, "{v:?}: {typed:?}");
        assert!(p.shown.windows(2).all(|w| score(w[0]) >= score(w[1])), "best first");
    }
}

/// The table's rows are built at compile time, so its constructors run here: rebuilt from its
/// own fields, a row drawn from it is the same row, and a row with no keys is a help line (a
/// note) or a footer hint (a tip), as the constructors make them.
#[hegel::test(test_cases = 100)]
fn every_row_is_what_its_constructor_makes(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let b = KEYMAP[tc.draw(gs::integers::<usize>().max_value(KEYMAP.len() - 1))];
    let rebuilt = key(b.scope, b.keys, b.shown, b.act, b.name, b.help, b.group);
    let rebuilt = match b.hint {
        Some((label, prio)) => rebuilt.hint(label, prio),
        None => rebuilt,
    };
    assert_eq!(format!("{rebuilt:?}"), format!("{b:?}"));
    if b.keys.is_empty() && b.act == Act::None {
        let as_note = format!("{:?}", note(b.scope, b.shown, b.help, b.group));
        let as_tip = b.hint.map(|(label, prio)| format!("{:?}", tip(b.scope, b.shown, label, prio)));
        assert!(as_note == format!("{b:?}") || as_tip == Some(format!("{b:?}")), "{b:?}");
    }
    // A tip of any label and priority is a footer hint and nothing else.
    let label: &'static str = Box::leak(tc.draw(gs::text().max_size(12)).into_boxed_str());
    let prio = tc.draw(gs::integers::<u8>());
    let t = tip(b.scope, b.shown, label, prio);
    assert!(t.keys.is_empty() && t.act == Act::None && t.name.is_empty() && t.help.is_empty());
    assert_eq!(t.hint, Some((label, prio)));
}
