use super::super::app::testing::{app_with, library};
use super::super::ui::screen;
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
    // Digits are panes, unless the pads are on.
    let three = press(KeyCode::Char('3'));
    assert_eq!(lookup(&[Scope::Library, Scope::Global], &three).map(|b| b.act), Some(Act::PaneKey));
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
    assert_eq!(own[0].0, "Publish");
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
    app.deck_layout = true;
    assert_eq!(hints(&app, 300)[0], ("1-8".to_string(), "hot cue"));
}

/// Each view's help starts with its own keys under its own heading; the keys for everywhere
/// follow, the commands with no key last.
#[test]
fn each_views_help_starts_with_its_own_keys() {
    for (view, title) in [(View::Prepare, "Prepare"), (View::Drives, "Drives"), (View::Mixes, "Recordings")] {
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
    app.view = tc.draw(gs::sampled_from(&View::ALL[..]));
    app.focus =
        tc.draw(gs::sampled_from(&[Focus::Drives, Focus::Playlists, Focus::Tracks, Focus::Details, Focus::Crate][..]));
    app.deck_layout = tc.draw(gs::booleans());
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
    let b = tc.draw(gs::sampled_from(KEYMAP));
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

/// Every view's help says how to report a bug.
#[test]
fn every_views_help_says_how_to_report_a_bug() {
    for view in View::ALL {
        let (_, global) = help(view);
        let mut lines = global.iter().flat_map(|(_, lines)| lines);
        assert!(lines.any(|(_, help)| help.contains("locallibrary bug")), "{view:?}");
    }
}

/// How a player gets to an action, beside any key it has.
#[derive(Debug, PartialEq)]
enum Reached {
    /// By name in the palette, from its row of the table.
    Named,
    /// In the palette, from a list made as it opens (a choice, or what a plugin brings).
    Listed,
    /// Deliberately not in the palette, and why.
    LeftOut(&'static str),
}

/// Where every action goes. No `_` arm: a new action doesn't compile until it's decided here.
fn reached(act: Act) -> Reached {
    use Reached::*;
    match act {
        Act::None => LeftOut("a row that only explains"),
        Act::PaneKey | Act::HotCue => LeftOut("takes the digit pressed"),
        Act::Goto => LeftOut("waits for the key after it"),
        Act::Palette | Act::Actions | Act::PaneActions => LeftOut("opens a list from the view's own key"),
        Act::Move(_) | Act::Page(_) | Act::End(_) | Act::Focus(_) | Act::Activate | Act::Escape => {
            LeftOut("moving about means nothing from a list over the view")
        }
        Act::Remove | Act::DriveActions => LeftOut("acts on what the pane selected"),
        Act::Browse | Act::NextBands => LeftOut("only the Deck layout's; the palette has the choices"),
        Act::Bands(_)
        | Act::KeyNotation(_)
        | Act::KeyPalette(_)
        | Act::EndWarning(_)
        | Act::Art(_)
        | Act::Theme(_)
        | Act::Colours(_)
        | Act::Icons(_)
        | Act::Hints(_)
        | Act::Plugin(..)
        | Act::Analyze(..)
        | Act::ApplyFields(..)
        | Act::FieldColumn(_)
        | Act::PluginSettings(_)
        | Act::AccountSignIn(..)
        | Act::AccountSignOut(_)
        | Act::PluginAccess(_)
        | Act::Review(_)
        | Act::Dismiss(_)
        | Act::ProviderSettings
        | Act::CheckSettings
        | Act::Fix(_)
        | Act::Export(..)
        | Act::Import(..)
        | Act::Source(..)
        | Act::Fetch(_) => Listed,
        Act::Quit
        | Act::Help
        | Act::Messages
        | Act::Jobs
        | Act::ShowView(_)
        | Act::Refresh
        | Act::PlayPause
        | Act::Seek(_)
        | Act::Next
        | Act::Prev
        | Act::Stop
        | Act::Cue(_)
        | Act::Phrase(_)
        | Act::Volume(_)
        | Act::Shuffle
        | Act::Repeat
        | Act::GoTo
        | Act::Pads
        | Act::BeatJump(_)
        | Act::Quantize
        | Act::Single
        | Act::AutoCue
        | Act::Outputs
        | Act::Loop
        | Act::LoopSize(_)
        | Act::Visualiser
        | Act::DeckLayout
        | Act::ResetPlayed
        | Act::Zoom(_)
        | Act::Search
        | Act::Sort
        | Act::SortReverse
        | Act::Ratings
        | Act::TagsColumn
        | Act::Tag
        | Act::Details
        | Act::CratePanel
        | Act::Mark
        | Act::MarkAll
        | Act::CrateAdd
        | Act::CrateMove(_)
        | Act::CrateSave
        | Act::CrateClear
        | Act::Undo
        | Act::Pull
        | Act::Check
        | Act::SetPrep
        | Act::SyncGroup
        | Act::Pack
        | Act::RestorePlaylist
        | Act::Replay
        | Act::Eject
        | Act::SmartList
        | Act::SmartApply
        | Act::SmartNew
        | Act::ExportList
        | Act::OpenFolder
        | Act::Snapshots
        | Act::SignIn
        | Act::ProfileInclude
        | Act::ProfilePin
        | Act::ProfileHide
        | Act::ProfileHideArtist
        | Act::ProfileDocument
        | Act::ProfileWrite
        | Act::ProfileScope
        | Act::ProfileRounding
        | Act::ProfileAccount
        | Act::MixOpen
        | Act::MixZoom
        | Act::MixExport
        | Act::MixReport => Named,
    }
}

/// Every action has a palette name or is left out for a stated reason, and the table agrees.
#[test]
fn every_action_has_a_palette_name_or_is_deliberately_left_out() {
    for b in KEYMAP.iter().filter(|b| b.act != Act::None) {
        match reached(b.act) {
            Reached::Named => assert!(!name(b.act).is_empty(), "{:?} has no name in the palette", b.act),
            Reached::LeftOut(why) => {
                let exceptions = [Act::Actions, Act::DriveActions];
                assert!(b.name.is_empty() || exceptions.contains(&b.act), "{:?} is named, though {why}", b.act);
            }
            Reached::Listed => assert!(b.name.is_empty(), "{:?} comes from a list", b.act),
        }
    }
}

/// The twelve jobs that lived only in the palette have a home in Prepare or Drives, and the ones
/// with a key there show it there.
#[test]
fn the_palette_only_jobs_have_homes() {
    use super::super::prepare;
    let jobs = [
        Act::Pull,
        Act::Check,
        Act::SetPrep,
        Act::SyncGroup,
        Act::Pack,
        Act::RestorePlaylist,
        Act::Replay,
        Act::SmartList,
        Act::SmartApply,
        Act::SmartNew,
        Act::ExportList,
        Act::Eject,
    ];
    for act in jobs {
        assert!(prepare::holds(act), "{act:?}");
    }
    for (keyed, shown) in
        [(Act::Check, "V"), (Act::Pull, "W"), (Act::SyncGroup, "Y"), (Act::Pack, "K"), (Act::Eject, "E")]
    {
        for scope in [Scope::Prepare, Scope::Drives] {
            let b = KEYMAP.iter().find(|b| b.scope == scope && b.act == keyed).unwrap();
            assert_eq!(b.shown, shown, "{keyed:?} in {scope:?}");
            assert_eq!(
                lookup(&[scope], &press(KeyCode::Char(shown.chars().next().unwrap()))).map(|b| b.act),
                Some(keyed)
            );
        }
    }
}

/// Compact holds the few that matter most and off holds none; full is as many as fit.
#[test]
fn the_hint_setting_decides_what_the_footer_holds() {
    let (mut app, _dir) = app_with(library());
    let full = hints(&app, 400).len();
    assert!(full > COMPACT, "{full}");
    app.hints = Hints::Compact;
    let compact = hints(&app, 400);
    assert_eq!(compact.len(), COMPACT);
    assert!(compact.iter().any(|(k, _)| k == "enter"), "the most important stays: {compact:?}");
    assert!(hints(&app, 20).len() < COMPACT, "and it still has to fit");
    app.hints = Hints::Off;
    assert!(hints(&app, 400).is_empty());
}

/// The help opens on the pane with focus, and typing narrows it, a row for the keys typed first.
#[test]
fn the_help_starts_at_the_pane_with_focus_and_narrows() {
    let (mut app, _dir) = app_with(library());
    let first = |app: &App, q: &str| help_rows(app, q).first().map(|r| r.binding.shown);
    app.focus = Focus::Crate;
    assert_eq!(help_rows(&app, "")[0].binding.scope, Scope::Crate);
    app.focus = Focus::Tracks;
    assert_eq!(help_rows(&app, "")[0].binding.scope, Scope::Library);
    app.view = View::Drives;
    app.drive_pane = DrivePane::History;
    assert_eq!(help_rows(&app, "")[0].binding.scope, Scope::History);
    app.view = View::Library;

    let n = help_rows(&app, "").len();
    let cue = help_rows(&app, "CUE");
    assert!(!cue.is_empty() && cue.len() < n, "{}", cue.len());
    assert!(cue.iter().all(|r| format!("{} {}", r.binding.help, r.binding.name).to_lowercase().contains("cue")));
    assert_eq!(first(&app, "e"), Some("e"), "the keys typed come first");
    assert_eq!(first(&app, "smart rule"), Some("smart"), "each word typed");
    assert!(help_rows(&app, "zzzz").is_empty());
}

/// Enter can run a row that's an action; a row that needs its key, or a note, can't.
#[test]
fn only_what_stands_alone_runs_from_the_help() {
    assert!(runs(Act::Pull) && runs(Act::Quit) && runs(Act::Jobs));
    for act in [Act::None, Act::HotCue, Act::PaneKey, Act::Goto, Act::Move(1), Act::Activate] {
        assert!(!runs(act), "{act:?}");
    }
}

/// Typing into the help narrows it, one row selected; enter runs that row and closes the help. A row
/// that needs its key leaves the help open.
#[test]
fn the_help_runs_the_row_selected() {
    let (mut app, _dir) = app_with(library());
    typing(&mut app, "?shuffle");
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("› shuffle") && s.contains("shuffle / repeat") && !s.contains("BROWSING"), "{s}");
    typing(&mut app, "\n");
    assert!(!app.help && app.shuffle, "run");

    typing(&mut app, "?hot cue a");
    assert!(screen(&mut app, 120, 40).contains("hot cue A"));
    typing(&mut app, "\n");
    assert!(app.help, "a digit does that");

    typing(&mut app, "\u{1b}?zzzz");
    assert!(screen(&mut app, 80, 24).contains("no key has those words"));
    typing(&mut app, "\n");
    assert!(app.help);
    app.on_key(press(KeyCode::Backspace));
    assert_eq!(app.help_query, "zzz");
}

/// Opening it starts over: nothing typed last time stays.
#[test]
fn the_help_opens_clean() {
    let (mut app, _dir) = app_with(library());
    typing(&mut app, "?cue\u{1b}?");
    assert!(app.help_query.is_empty());
    assert_eq!(app.help_row, 0);
}

fn typing(app: &mut App, keys: &str) {
    super::super::app::testing::press(app, keys);
}

/// However it's narrowed, the help keeps its rows in one place each and only holds what was typed.
#[hegel::test]
fn prop_narrowing_the_help_only_takes_rows_away(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir) = app_with(library());
    app.view = tc.draw(gs::sampled_from(View::ALL.to_vec()));
    let typed = tc.draw(gs::sampled_from(vec!["", "e", "cue", "the drive", "PLAY", "zz", " ", "?"]));
    let all = help_rows(&app, "");
    let some = help_rows(&app, typed);
    assert!(some.len() <= all.len());
    for row in &some {
        assert!(all.iter().any(|r| std::ptr::eq(r.binding, row.binding)));
        let text = format!("{} {} {}", row.binding.shown, row.binding.help, row.binding.name).to_lowercase();
        assert!(typed.split_whitespace().all(|w| text.contains(&w.to_lowercase())), "{text}");
    }
}
