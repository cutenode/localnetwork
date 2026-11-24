use hegel::generators as gs;
use locallibrary::keybinds::Keybinds;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

use super::super::conflict::{Place, clashes, places};
use super::super::{Hint, dialogs, help_rows, hints, lookup, menu_letters, view_key};
use super::*;
use crate::tui::app::testing::{app_with, library};
use crate::tui::app::{Focus, View};
use crate::tui::palette::commands;

fn binds(pairs: &[(&str, &[&str])]) -> Keybinds {
    let mut out = Keybinds::default();
    for (id, keys) in pairs {
        out.keys.insert(id.to_string(), keys.iter().map(|k| k.to_string()).collect());
    }
    out
}

fn ch(c: char) -> KeyEvent {
    let shift = if c.is_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
    KeyEvent::new(KeyCode::Char(c), shift)
}

fn f(n: u8) -> KeyEvent {
    KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE)
}

/// What `key` does in each of the views' places, by place.
fn everywhere(key: &KeyEvent) -> Vec<(&'static Place, Option<Act>)> {
    let views = places().iter().filter(|p| !p.scopes[0].dialog());
    views.map(|p| (p, lookup(&p.scopes, key).map(|h| h.act))).collect()
}

/// `playback.next` on `]`: wherever `]` is looked up it skips a track, and `n` does nothing
/// (in Recordings, it still skips the set's tracks, as it came).
#[test]
fn remapping_moves_the_action_and_frees_the_old_key() {
    install(Effective::new(&binds(&[("playback.next", &["]"])]), &[]));
    for (place, does) in everywhere(&ch(']')) {
        assert_eq!(does, Some(Act::Next), "{}", place.name);
    }
    for (place, does) in everywhere(&ch('n')) {
        let set_track = place.scopes.contains(&Scope::Mixes) && does == Some(Act::MixTrack(true));
        assert!(does.is_none() || set_track, "{}: n does {does:?}", place.name);
    }
}

/// A key for an action that had none works wherever the action does: everywhere for the next
/// phrase and a plugin's command, only in the localnetwork view for putting a section in the
/// document.
#[test]
fn a_new_key_works_wherever_the_action_does() {
    crate::tui::app::testing::plugins_once();
    let plugins = crate::tui::keybinds::manifests();
    let set = binds(&[
        ("playback.phrase-next", &["f7"]),
        ("localnetwork.in-document", &["f8"]),
        ("plugin.echo.titles", &["f6"]),
    ]);
    let e = Effective::new(&set, &plugins);
    assert_eq!(e.problems, Vec::<String>::new());
    install(e);
    let views: Vec<_> = places().iter().filter(|p| !p.scopes[0].dialog()).collect();
    assert!(views.len() > 20, "every situation: {}", views.len());
    for p in views {
        let does = |key: KeyEvent| lookup(&p.scopes, &key).map(|h| h.act);
        assert_eq!(does(f(7)), Some(Act::Phrase(true)), "{}", p.name);
        assert_eq!(does(f(6)), Some(Act::Plugin("echo", "titles")), "{}", p.name);
        let profile = p.scopes.contains(&Scope::Profile);
        assert_eq!(does(f(8)), profile.then_some(Act::ProfileInclude), "{}", p.name);
    }
}

/// Keys a key can be given that no action has as it came.
fn free_key(tc: &hegel::TestCase) -> K {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => K::Ch(tc.draw(gs::sampled_from(('A'..='Z').collect::<Vec<_>>()))),
        1 => K::Ctrl(tc.draw(gs::sampled_from(vec!['b', 'e', 'g', 'o', 'r', 't', 'y', 'z']))),
        _ => K::Code(KeyCode::F(tc.draw(gs::sampled_from(vec![1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12])))),
    }
}

/// For any action and any key no action has, given that key it's reached by it in every place
/// it's at home, and nothing's said against it.
#[hegel::test(test_cases = 100)]
fn prop_a_free_key_reaches_its_action_in_every_home_place(tc: hegel::TestCase) {
    let act = tc.draw(gs::sampled_from(ids::bindable(&[])));
    let key = free_key(&tc);
    let id = ids::id(act).unwrap();
    let spelled = key.portable().unwrap().to_string();
    let e = Effective::new(&binds(&[(&id, &[spelled.as_str()])]), &[]);
    assert_eq!(e.problems, Vec::<String>::new(), "{id} on {spelled}");
    let homes: Vec<Scope> = e.rows().iter().filter(|r| r.act == act && r.home).map(|r| r.scope).collect();
    let mut reached = 0;
    for p in places().iter().filter(|p| p.scopes.iter().any(|s| homes.contains(s))) {
        let hit = e.hit(&p.scopes, &key.pressed()).map(|i| e.rows()[i].act);
        assert_eq!(hit, Some(act), "{id} on {spelled} in {}", p.name);
        reached += 1;
    }
    assert!(reached > 0, "{id} is at home somewhere");
}

/// A changed key shows wherever keys are shown: the footer, the help, the palette, the header's
/// tabs, the actions menus, and a pair written together.
#[test]
fn changed_keys_show_everywhere_keys_are_shown() {
    let set = binds(&[
        ("library.search", &["F"]),
        ("view.drives", &["d"]),
        ("drive.check", &["K"]),
        ("playback.next", &["]"]),
    ]);
    install(Effective::new(&set, &[]));
    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Tracks;
    let footer: Vec<Hint> = hints(&app, 300);
    assert!(footer.iter().any(|h| h.keys == "F" && h.act == Act::Search), "{footer:?}");
    assert!(!footer.iter().any(|h| h.keys == "/"), "{footer:?}");
    let help = help_rows(&app, "");
    let keys_of = |act: Act| help.iter().find(|r| r.binding.act == act).map(|r| r.keys.clone());
    assert_eq!(keys_of(Act::Search).as_deref(), Some("F"));
    assert_eq!(keys_of(Act::Next).as_deref(), Some("]  p"), "the pair, its first changed");
    assert_eq!(keys_of(Act::ShowView(View::Drives)).as_deref(), Some("d"));
    let palette = commands(View::Library);
    let detail = |act: Act| palette.iter().find(|(a, _)| *a == act).map(|(_, i)| i.detail.clone());
    assert_eq!(detail(Act::Search).as_deref(), Some("F"));
    assert_eq!(detail(Act::Next).as_deref(), Some("]"));
    assert_eq!(view_key(View::Drives), "d");
    let header = crate::tui::ui::screen(&mut app, 160, 40);
    assert!(header.lines().next().is_some_and(|l| l.contains("d Drives")), "{header}");
    assert_eq!(menu_letters(&[Act::Pull, Act::Check]), ["s", "K"]);
    assert_eq!(super::super::dialog_act(&dialogs::ACTIONS, &ch('K')), None, "a letter is the menu's, not a lookup's");
}

/// A file that gives one key to two actions keeps it for the first, and says so of the other; a
/// key it can't read is said too, and an action it doesn't know is left alone.
#[test]
fn a_doubled_file_keeps_the_first_and_says_so() {
    let set = binds(&[
        ("library.search", &["x"]),
        ("playback.next", &["x", "]"]),
        ("playback.stop", &["alt-s"]),
        ("someday.new-thing", &["y"]),
    ]);
    let e = Effective::new(&set, &[]);
    assert_eq!(
        e.problems,
        ["playback.stop: \"alt-s\": the player doesn't take alt- keys", "playback.next: x is search's already"]
    );
    assert_eq!(e.keys(Act::Search), [K::Ch('x')]);
    assert_eq!(e.keys(Act::Next), [K::Ch(']')]);
    assert_eq!(e.keys(Act::Stop), Vec::<K>::new(), "kept as it came: none");
    assert!(clashes(&e).is_empty());
}

/// The keys as they came clash nowhere: what a view takes from everywhere is its own way.
#[test]
fn the_keys_as_they_came_clash_nowhere() {
    assert_eq!(clashes(&Effective::defaults()), Vec::new());
}

/// A key set for an action that another came with stays the other's, and the file is told it
/// can't have it.
#[test]
fn a_key_another_action_came_with_stays_its() {
    let e = Effective::new(&binds(&[("library.mark", &["n"])]), &[]);
    assert_eq!(e.problems, ["library.mark: n is next track's already"]);
    assert_eq!(e.keys(Act::Next), [K::Ch('n')]);
    assert!(!e.keys(Act::Mark).contains(&K::Ch('n')), "{:?}", e.keys(Act::Mark));
    assert!(clashes(&e).is_empty());
}

/// A key set for one action that another's key also reaches (↑ takes shift ↑ too) is left out of
/// it, as one the other came with is: each is said once, and nothing ends up bound twice.
#[test]
fn a_key_reached_by_another_actions_key_is_left_out_once() {
    let e = Effective::new(&binds(&[("playback.activate", &["shift-up"]), ("library.search", &["up"])]), &[]);
    assert_eq!(e.problems, ["library.search: ↑ is move's already", "playback.activate: shift ↑ is move's already"]);
    assert!(!e.keys(Act::Search).contains(&K::Code(KeyCode::Up)), "{:?}", e.keys(Act::Search));
    assert!(!e.keys(Act::Activate).contains(&K::Shift(KeyCode::Up)), "{:?}", e.keys(Act::Activate));
    assert_eq!(clashes(&e), Vec::new());
}

/// Whatever a file gives whichever actions, the keys in effect double none, and each thing it
/// couldn't do is said of one of its actions.
#[hegel::test(test_cases = 100)]
fn prop_no_file_doubles_a_key(tc: hegel::TestCase) {
    let acts = ids::bindable(&[]);
    let spellings = vec![
        "n", "m", "x", "]", "1", "é", "F", "space", "enter", "tab", "up", "shift-up", "f5", "f9", "ctrl-q", "ctrl-z",
        "alt-x", "nope",
    ];
    let mut set = Keybinds::default();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        let act = tc.draw(gs::sampled_from(acts.clone()));
        let n = tc.draw(gs::integers::<usize>().max_value(3));
        let keys: Vec<String> = (0..n).map(|_| tc.draw(gs::sampled_from(spellings.clone())).to_string()).collect();
        set.keys.insert(ids::id(act).unwrap().into_owned(), keys);
    }
    let e = Effective::new(&set, &[]);
    assert_eq!(clashes(&e), Vec::new(), "{set:?}");
    for p in &e.problems {
        assert!(set.keys.keys().any(|id| p.starts_with(&format!("{id}: "))), "{p}");
    }
}

/// Recordings' next and previous track are hinted as a pair: once one has changed, the footer
/// writes the pair as it is now.
#[test]
fn a_changed_pair_is_hinted_as_it_is_now() {
    install(Effective::new(&binds(&[("recordings.next-track", &["]"])]), &[]));
    let (mut app, _dir) = app_with(library());
    app.view = View::Mixes;
    let footer = hints(&app, 300);
    assert!(footer.iter().any(|h| h.act == Act::MixTrack(true) && h.keys == "] p"), "{footer:?}");
    let s = crate::tui::ui::screen(&mut app, 160, 40);
    assert!(s.lines().last().is_some_and(|l| l.contains("] p")), "{s}");
}

/// However the hot cues' keys are set, the pads' hint and the help write the first and the last
/// (the one, when there's one).
#[hegel::test(test_cases = 50)]
fn prop_the_hot_cues_are_written_first_to_last(tc: hegel::TestCase) {
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(8));
    let mut keys: Vec<K> = Vec::new();
    while keys.len() < n {
        let k = free_key(&tc);
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    let spelled: Vec<String> = keys.iter().map(|k| k.portable().unwrap().to_string()).collect();
    let spelled: Vec<&str> = spelled.iter().map(String::as_str).collect();
    let e = Effective::new(&binds(&[("deck.hot-cue", &spelled)]), &[]);
    assert_eq!(e.problems, Vec::<String>::new());
    install(e);
    let (mut app, _dir) = app_with(library());
    app.deck.pads = true;
    let (first, last) = (keys[0].name(), keys[n - 1].name());
    let written = if n == 1 { first } else { format!("{first}…{last}") };
    let footer = hints(&app, 300).into_iter().find(|h| h.act == Act::HotCue).map(|h| h.keys);
    assert_eq!(footer.as_deref(), Some(written.as_str()));
    let help = help_rows(&app, "").into_iter().find(|r| r.binding.act == Act::HotCue).map(|r| r.keys);
    assert_eq!(help.as_deref(), Some(written.as_str()));
}

/// In player mode the hot cues' hint is written from the keys that play them there, as it is
/// with the pads on.
#[test]
fn player_mode_hints_the_hot_cues_as_they_are() {
    install(Effective::new(&binds(&[("deck.hot-cue", &["F", "G"])]), &[]));
    let (mut app, _dir) = app_with(library());
    app.deck_layout = true;
    assert!(app.deck_showing());
    let footer = hints(&app, 300);
    assert_eq!(footer.first().map(|h| (h.keys.as_str(), h.label)), Some(("F…G", "hot cue")), "{footer:?}");
    let s = crate::tui::ui::screen(&mut app, 160, 40);
    assert!(s.lines().last().is_some_and(|l| l.contains("F…G hot cue")), "{s}");
}

/// Hot cues given no keys stay under their heading in the help, written `none`: they can't be
/// found by name in `:`, so they're not listed as if they could.
#[test]
fn hot_cues_with_no_keys_are_listed_as_none() {
    install(Effective::new(&binds(&[("deck.hot-cue", &[])]), &[]));
    let (mut app, _dir) = app_with(library());
    let help = help_rows(&app, "");
    let row = help.iter().find(|r| r.binding.act == Act::HotCue).unwrap();
    assert_eq!((row.title, row.keys.as_str()), (row.binding.group.title(), "none"));
    crate::tui::app::testing::press(&mut app, "?hot cue");
    let s = crate::tui::ui::screen(&mut app, 160, 45).to_lowercase();
    assert!(s.contains("none") && !s.contains("by name"), "{s}");
}
