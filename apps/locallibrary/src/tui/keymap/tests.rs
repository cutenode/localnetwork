use super::super::app::testing::{app_with, library};
use super::super::app::{DrivePane, Focus};
use super::super::edits::Modal;
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
    assert_eq!(lookup(&[Scope::Deck, Scope::Pads, Scope::Global], &w).map(|b| b.act), Some(Act::NextBands));
    assert!(lookup(&[Scope::Drives, Scope::Global], &w).is_none());
    // Digits are views, unless the pads are on (in player mode, hot cues).
    let three = press(KeyCode::Char('3'));
    assert_eq!(lookup(&[Scope::Library, Scope::Global], &three).map(|b| b.act), Some(Act::ShowView(View::Drives)));
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

/// A key means one thing in each scope (the first row wins, so a second would never run), and no
/// scope takes a key from a wider one it's looked up before (see [`chain`]), except where
/// [`OWN_WAY`] says why.
#[test]
fn no_key_is_bound_twice() {
    let codes = [
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Tab,
        KeyCode::BackTab,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Delete,
        KeyCode::Backspace,
        KeyCode::Insert,
        KeyCode::F(5),
    ];
    let samples: Vec<KeyEvent> = (' '..='~')
        .map(|c| press(KeyCode::Char(c)))
        .chain(codes.map(press))
        .chain(codes.map(|c| KeyEvent::new(c, KeyModifiers::SHIFT)))
        .chain(('a'..='z').map(|c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)))
        .collect();
    let mut scopes: Vec<Scope> = Vec::new();
    for b in KEYMAP {
        if !scopes.contains(&b.scope) {
            scopes.push(b.scope);
        }
    }
    let own = |s: Scope, key: &KeyEvent| KEYMAP.iter().find(|b| b.scope == s && b.keys.iter().any(|k| k.matches(key)));
    let mut taken = Vec::new();
    for key in &samples {
        for &s in &scopes {
            let acts: Vec<Act> = KEYMAP
                .iter()
                .filter(|b| b.scope == s && b.keys.iter().any(|k| k.matches(key)))
                .map(|b| b.act)
                .collect();
            // The actions menu's `w` saves what the view has, and no menu offers both (`menu_act`).
            let apart = s == Scope::Actions && acts == [Act::ProfileWrite, Act::MixReport];
            assert!(acts.len() <= 1 || apart, "{key:?} is bound {} times in {s:?}: {acts:?}", acts.len());
            let Some(mine) = own(s, key) else { continue };
            for wider in chain(s).into_iter().filter(|w| *w != s) {
                let Some(theirs) = own(wider, key) else { continue };
                let pair = (s, theirs.act, mine.act);
                if !OWN_WAY.iter().any(|(s, w, m, _)| (*s, *w, *m) == pair) && !taken.contains(&pair) {
                    taken.push(pair);
                }
            }
        }
    }
    assert!(taken.is_empty(), "keys a scope takes from a wider one (scope, wider act, its act): {taken:?}");
}

/// Where a narrower scope takes a wider one's key on purpose (it's looked up first, so it wins):
/// the scope, what the key does in the wider one, what it does here, and why.
const OWN_WAY: [(Scope, Act, Act, &str); 13] = [
    (Scope::Mixes, Act::PlayPause, Act::MixPlay, "space plays the set open, not the deck"),
    (Scope::Mixes, Act::Next, Act::MixTrack(true), "n skips the set's tracks"),
    (Scope::Mixes, Act::Prev, Act::MixTrack(false), "p goes back in the set's tracks"),
    (Scope::Crate, Act::Remove, Act::Remove, "del takes the track out of the queue, not the playlist"),
    (Scope::Crate, Act::Move(-1), Act::CrateMove(-1), "shift up moves the track up the queue"),
    (Scope::Crate, Act::Move(1), Act::CrateMove(1), "shift down moves the track down the queue"),
    (Scope::Pads, Act::ShowView(View::Library), Act::HotCue, "the digits are hot cues with the pads on"),
    (Scope::Pads, Act::ShowView(View::Prepare), Act::HotCue, "the digits are hot cues with the pads on"),
    (Scope::Pads, Act::ShowView(View::Drives), Act::HotCue, "the digits are hot cues with the pads on"),
    (Scope::Pads, Act::ShowView(View::Mixes), Act::HotCue, "the digits are hot cues with the pads on"),
    (Scope::Pads, Act::ShowView(View::Profile), Act::HotCue, "the digits are hot cues with the pads on"),
    (Scope::Report, Act::Activate, Act::Escape, "results have nothing to do, so enter closes them"),
    (Scope::Messages, Act::Activate, Act::Escape, "the messages have nothing to do, so enter closes them"),
];

/// Every dialog's scopes, as it looks its keys up.
const DIALOGS: [&[Scope]; 17] = [
    &dialogs::ACTIONS,
    &dialogs::FILTERING,
    &dialogs::SUB_MENU,
    &dialogs::CONFIRM,
    &dialogs::ASK,
    &dialogs::REPORT,
    &dialogs::FORM,
    &dialogs::PICKER,
    &dialogs::JOBS,
    &dialogs::HELP,
    &dialogs::MESSAGES,
    &dialogs::OUTPUTS,
    &dialogs::SEARCH,
    &dialogs::PROMPT,
    &dialogs::PICKING,
    &dialogs::KEYS,
    &dialogs::KEYS_ANSWER,
];

/// Where `s`'s keys are looked up, most specific first: a dialog's as it looks them up, and the
/// keys every dialog shares (`List`, `Dialog`) as the last of those.
fn chain(s: Scope) -> Vec<Scope> {
    match s {
        _ if s.dialog() => DIALOGS.iter().find(|d| d[0] == s).map_or(vec![s, Scope::Dialog], |d| d.to_vec()),
        Scope::DriveList
        | Scope::Steps
        | Scope::Tree
        | Scope::Tracks
        | Scope::Details
        | Scope::Crate
        | Scope::Replay => {
            vec![s, Scope::Library, Scope::Global]
        }
        Scope::Deck => vec![s, Scope::Pads, Scope::Global],
        _ => vec![s, Scope::Global],
    }
}

/// The keys a hint writes out (`↑↓`, `n p`, `1-8`), as keys pressed.
fn written(shown: &str) -> Vec<KeyEvent> {
    let named = |t: &str| match t {
        "↑↓" | "↑" => Some(KeyCode::Up),
        "←→" => Some(KeyCode::Right),
        "enter" => Some(KeyCode::Enter),
        "esc" => Some(KeyCode::Esc),
        "tab" => Some(KeyCode::Tab),
        "space" => Some(KeyCode::Char(' ')),
        "del" => Some(KeyCode::Delete),
        "1-8" => Some(KeyCode::Char('1')),
        t if t.chars().count() == 1 => t.chars().next().map(KeyCode::Char),
        _ => None,
    };
    let tokens: Vec<&str> = shown.split(' ').collect();
    match tokens[..] {
        ["shift", arrows] => named(arrows).map(|c| KeyEvent::new(c, KeyModifiers::SHIFT)).into_iter().collect(),
        _ => tokens.iter().filter_map(|t| named(t)).map(press).collect(),
    }
}

/// Every hint is named and names what its key does there: its own key, the key looked up for
/// its act, or (written out, as `n p`) one of the keys it writes.
#[test]
fn every_hint_names_what_its_key_does() {
    let e = Effective::defaults();
    for (i, b) in e.rows().iter().enumerate().filter_map(|(i, r)| r.b.filter(|b| b.hint.is_some()).map(|b| (i, b))) {
        let scopes = chain(b.scope);
        let h = hint_of(&e, i, &scopes).unwrap_or_else(|| panic!("{:?} in {:?} has no key there", b.act, b.scope));
        assert!(!h.label.is_empty(), "{:?} in {:?} has no label", b.act, b.scope);
        let keys = if b.keys.is_empty() && !b.shown.is_empty() {
            written(b.shown)
        } else {
            vec![key_for(&scopes, b.act).or(b.keys.first().copied()).unwrap().pressed()]
        };
        let does = |key: &KeyEvent| lookup(&scopes, key).map(|found| found.act);
        assert!(
            keys.iter().any(|k| does(k) == Some(b.act)),
            "{:?} in {:?}: {:?} does {:?}",
            b.act,
            b.scope,
            h.keys,
            keys.iter().map(does).collect::<Vec<_>>()
        );
    }
}

/// Every dialog closes on esc, and no dialog takes q or any key but its own to close.
#[test]
fn every_dialog_closes_on_esc_and_only_its_keys() {
    use super::dialogs::*;
    let esc = press(KeyCode::Esc);
    for scopes in [
        &ACTIONS[..],
        &FILTERING,
        &SUB_MENU,
        &CONFIRM,
        &ASK,
        &REPORT,
        &FORM,
        &PICKER,
        &JOBS,
        &HELP,
        &MESSAGES,
        &OUTPUTS,
        &SEARCH,
        &PROMPT,
        &PICKING,
    ] {
        assert_eq!(dialog_act(scopes, &esc), Some(Act::Escape), "{scopes:?}");
        assert_ne!(dialog_act(scopes, &press(KeyCode::Char('q'))), Some(Act::Escape), "{scopes:?}");
        // The hints end with esc, as esc; a list moves as the arrows.
        let hints = dialog_hints(scopes);
        let last = hints.last().map(|h| (h.act, h.keys.as_str()));
        assert_eq!(last, Some((Act::Escape, "esc")), "{scopes:?}");
        assert!(hints.iter().all(|h| !matches!(h.act, Act::Move(_)) || h.keys == "↑↓"), "{hints:?}");
    }
    // Typed into, j and k are letters; in a list, they move.
    assert_eq!(dialog_act(&PICKER, &press(KeyCode::Char('j'))), None);
    assert_eq!(dialog_act(&REPORT, &press(KeyCode::Char('j'))), Some(Act::Move(1)));
    assert_eq!(dialog_act(&FILTERING, &press(KeyCode::Char('e'))), None, "filtering, e is typed");
    assert_eq!(dialog_act(&ACTIONS, &press(KeyCode::Char('e'))), Some(Act::Escape), "the key that opened it closes it");
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
    assert_eq!(titles[..3], ["Browsing", "Marks and the queue", "Changing the drive"]);
    assert!(titles.contains(&"Playback") && titles.contains(&"Deck"));
    let (own, _) = help(View::Profile);
    assert_eq!(own[0].0, "localnetwork");
}

#[test]
fn the_footer_drops_the_least_important_hints_to_fit() {
    let (mut app, _dir) = app_with(library());
    app.focus = Focus::Tracks;
    let wide = hints(&app, 300);
    let keys: Vec<&str> = wide.iter().map(|h| h.keys.as_str()).collect();
    assert_eq!(keys[..3], ["enter", "e", "/"], "what enter does, the actions, then the track list's own: {keys:?}");
    assert!(!keys.contains(&"Q") && !keys.contains(&"ctrl-c"), "quitting isn't hinted: {keys:?}");
    for width in [40, 60, 80, 100] {
        let h = hints(&app, width);
        let used: usize = 1 + h.iter().map(|h| h.keys.width() + h.label.width() + 3).sum::<usize>();
        assert!(used <= width, "{width}: {h:?}");
        assert!(h.iter().any(|h| h.keys == "enter"), "the most important stays: {h:?}");
        let last: Vec<&str> = h.iter().rev().take(2).map(|h| h.keys.as_str()).collect();
        assert_eq!(last, ["?", ":"], "the commands and the help end it at {width}: {h:?}");
    }
    app.deck_layout = true;
    let deck = hints(&app, 300);
    assert_eq!((deck[0].keys.as_str(), deck[0].label), ("1-8", "hot cue"));
    assert_eq!(deck.last().map(|h| h.act), Some(Act::Help), "player mode keeps the help: {deck:?}");
}

/// Each view's help starts with its own keys under its own heading; the keys for everywhere
/// follow. The commands with no key are the palette's alone.
#[test]
fn each_views_help_starts_with_its_own_keys() {
    for (view, title) in [(View::Prepare, "Prepare"), (View::Drives, "Drives"), (View::Mixes, "Recordings")] {
        let (own, global) = help(view);
        assert_eq!(own[0].0, title);
        assert!(global.iter().all(|(t, _)| *t != title), "{view:?}");
        assert!(own.iter().chain(&global).all(|(t, _)| !t.starts_with("Commands")), "{view:?}");
    }
}

/// The footer's hints fit the width they're given, in any view and pane, and a wider footer
/// keeps every hint a narrower one shows.
#[hegel::test(test_cases = 200)]
fn prop_hints_fit_the_footer_and_widen_with_it(tc: hegel::TestCase) {
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
    let used: usize = shown.iter().map(|h| h.keys.width() + h.label.width() + 3).sum();
    // Each takes its keys, a space, its label and two spaces, after the footer's one.
    assert!(shown.is_empty() || used < width, "{shown:?} in {width}");
    let more = hints(&app, wider);
    assert!(shown.iter().all(|h| more.contains(h)), "{shown:?} at {width}, {more:?} at {wider}");
}

/// Every view's help lists its own keys and then the ones for everywhere, under headings of
/// their own, none empty; its commands (the view's own, then everywhere's) are each named once,
/// and for any letters typed the palette shows just the commands they're in, best first.
#[hegel::test(test_cases = 50)]
fn prop_every_view_has_its_help_and_its_commands(tc: hegel::TestCase) {
    use super::super::palette::{Picker, Then, commands};
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
        let score = |i: usize| p.items[i].score(&typed);
        let matching = (0..p.items.len()).filter(|&i| score(i).is_some()).count();
        assert_eq!(p.shown.len(), matching, "{v:?}: {typed:?}");
        assert!(p.shown.windows(2).all(|w| score(w[0]) >= score(w[1])), "best first");
    }
}

/// The table's rows are built at compile time, so its constructors run here: rebuilt from its
/// own fields, a row drawn from it is the same row, and a row with no keys is a help line (a
/// note) or a footer hint (a tip), as the constructors make them.
#[hegel::test(test_cases = 100)]
fn prop_every_row_is_what_its_constructor_makes(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let b = tc.draw(gs::sampled_from(KEYMAP));
    let rebuilt = key(b.scope, b.keys, b.shown, b.act, b.name, b.help, b.group).label(b.label).words(b.words);
    let rebuilt = match b.hint {
        Some(prio) => rebuilt.hint(prio),
        None => rebuilt,
    };
    assert_eq!(format!("{rebuilt:?}"), format!("{b:?}"));
    if b.keys.is_empty() && b.act == Act::None {
        assert_eq!(format!("{:?}", note(b.scope, b.shown, b.help, b.group)), format!("{b:?}"));
    }
    if is_tip(&b) {
        let prio = b.hint.expect("a tip is a hint");
        assert_eq!(format!("{:?}", tip(b.scope, b.shown, b.act, prio).label(b.label)), format!("{b:?}"));
    }
    // A tip of any priority is a hint and nothing else.
    let prio = tc.draw(gs::integers::<u8>());
    let t = tip(b.scope, b.shown, b.act, prio);
    assert!(is_tip(&t) && t.label.is_empty());
    assert_eq!(t.hint, Some(prio));
}

/// Every view's palette says how to report a bug; the help, which lists keys, doesn't.
#[test]
fn every_views_palette_says_how_to_report_a_bug() {
    use super::super::palette::commands;
    for view in View::ALL {
        assert!(commands(view).iter().any(|(act, _)| *act == Act::Copy("bug")), "{view:?}");
        let (own, global) = help(view);
        let mut lines = own.iter().chain(&global).flat_map(|(_, lines)| lines);
        assert!(!lines.any(|(_, help)| help.contains("reports a bug")), "{view:?}");
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
        Act::HotCue => LeftOut("takes the digit pressed"),
        Act::Palette | Act::Actions | Act::PaneActions => LeftOut("opens a list from the view's own key"),
        Act::Move(_) | Act::Page(_) | Act::End(_) | Act::Focus(_) | Act::Activate | Act::Escape => {
            LeftOut("moving about means nothing from a list over the view")
        }
        Act::Remove | Act::DriveActions | Act::MixForget => LeftOut("acts on what the pane selected"),
        Act::Answer(_) | Act::Change(_) | Act::CancelJob | Act::SearchWhere => LeftOut("a dialog's own"),
        Act::Browse => LeftOut("only player mode's"),
        Act::Bands(_)
        | Act::KeyNotation(_)
        | Act::KeyPalette(_)
        | Act::EndWarning(_)
        | Act::Art(_)
        | Act::ArtHalfblocks(_)
        | Act::Theme(_)
        | Act::Colours(_)
        | Act::Icons(_)
        | Act::Hints(_)
        | Act::HelpHint(_)
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
        | Act::Fetch(_)
        | Act::Edit(_) => Listed,
        Act::Quit
        | Act::QueueNext
        | Act::Fill
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
        | Act::Run(_)
        | Act::Copy(_)
        | Act::OpenFolder
        | Act::Snapshots
        | Act::SignIn
        | Act::ProfileInclude
        | Act::ProfileHideArtist
        | Act::ProfileDocument
        | Act::ProfileWrite
        | Act::ProfileRounding
        | Act::ProfileAccount
        | Act::MixOpen
        | Act::MixZoom
        | Act::MixExport
        | Act::MixReport
        | Act::MixPlay
        | Act::MixTrack(_)
        | Act::MixGoTo
        | Act::NextBands
        | Act::Keys
        | Act::KeysImport
        | Act::KeysReset
        | Act::KeysPreset(_) => Named,
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
    for (keyed, letter) in
        [(Act::Check, 'c'), (Act::Pull, 's'), (Act::SyncGroup, 'g'), (Act::Pack, 'p'), (Act::Eject, 'd')]
    {
        assert_eq!(lookup(&[Scope::Actions], &press(KeyCode::Char(letter))).map(|b| b.act), Some(keyed));
        for scope in [Scope::Prepare, Scope::Drives] {
            assert!(key_for(&[scope], keyed).is_none(), "{keyed:?} has a key of its own in {scope:?}");
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
    assert!(compact.iter().any(|h| h.keys == "enter"), "the most important stays: {compact:?}");
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
    assert_eq!(first(&app, "previous cue"), Some("c  h"), "each word typed");
    assert_eq!(first(&app, "smart rule"), None, "a command with no key is the palette's");
    assert!(help_rows(&app, "zzzz").is_empty());
}

/// Enter can run a row that's an action; a row that needs its key, a note, or a dialog's key (the
/// list's space marks in a list, not the library's track) can't.
#[test]
fn only_what_stands_alone_runs_from_the_help() {
    let run = |act: Act| KEYMAP.iter().filter(|b| b.act == act && !b.scope.dialog()).any(runs);
    assert!(run(Act::Pull) && run(Act::Quit) && run(Act::Jobs));
    for act in [Act::None, Act::HotCue, Act::Move(1), Act::Activate] {
        assert!(!run(act), "{act:?}");
    }
    let marks = KEYMAP.iter().find(|b| b.scope == Scope::Picker && b.act == Act::Mark);
    assert!(marks.is_some_and(|b| !runs(b)), "a list's space");
    assert!(KEYMAP.iter().filter(|b| b.scope.dialog()).all(|b| !runs(b)));
}

/// Typing into the help narrows it, one row selected; enter runs that row and closes the help. A row
/// that needs its key leaves the help open; with no row, enter searches the palette instead.
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
    app.on_key(press(KeyCode::Backspace));
    assert_eq!(app.help_query, "zzz");
    // Enter searches the palette for what no key has.
    typing(&mut app, "\n");
    assert!(!app.help);
    assert!(matches!(&app.modal, Some(Modal::Pick(p)) if p.query == "zzz"));
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
        let b = row.binding;
        let text = format!("{} {} {} {}", b.shown, b.help, b.name, b.words).to_lowercase();
        assert!(typed.split_whitespace().all(|w| text.contains(&w.to_lowercase())), "{text}");
    }
}

/// Keys are named apart, whatever they are: characters as themselves (space by name), arrows as
/// arrows, other keys by their name, with shift or ctrl said, and a run of digits from one to the
/// other; no two keys share a name.
#[hegel::test(test_cases = 300)]
fn prop_keys_are_named_apart(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let codes = [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::PageDown,
        KeyCode::F(5),
        KeyCode::Tab,
        KeyCode::BackTab,
        KeyCode::Enter,
        KeyCode::Delete,
        KeyCode::PageUp,
    ];
    let any = |tc: &hegel::TestCase| match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => K::Ch(tc.draw(gs::sampled_from(&[' ', 'a', 'A', '1', ':'][..]))),
        1 => K::Code(tc.draw(gs::sampled_from(&codes[..]))),
        2 => K::Shift(tc.draw(gs::sampled_from(&codes[..]))),
        3 => K::Ctrl(tc.draw(gs::sampled_from(&['c', 'q', 'u'][..]))),
        _ => K::Digits('1', tc.draw(gs::sampled_from(&['5', '8'][..]))),
    };
    let (a, b) = (any(&tc), any(&tc));
    let (na, nb) = (a.name(), b.name());
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
    // Pressed, a key is what it matches.
    assert!(a.matches(&a.pressed()), "{a:?}");
}

/// The scopes a dialog looks its keys up in: its own, its list's (typed into or not), then every
/// dialog's.
#[test]
fn a_dialog_looks_in_its_own_scope_its_lists_then_every_dialogs() {
    assert_eq!(dialog_scopes(Scope::Help, true), [Scope::Help, Scope::Typed, Scope::Dialog]);
    assert_eq!(dialog_scopes(Scope::Report, false), [Scope::Report, Scope::List, Scope::Dialog]);
}

/// Keys as the footer and the help write them.
#[test]
fn keys_are_written_as_the_help_writes_them() {
    let names = [
        (K::Code(KeyCode::PageUp), "pgup"),
        (K::Code(KeyCode::PageDown), "pgdn"),
        (K::Code(KeyCode::Delete), "del"),
        (K::Code(KeyCode::BackTab), "shift-tab"),
        (K::Shift(KeyCode::Up), "shift ↑"),
        (K::Ch(' '), "space"),
    ];
    for (k, name) in names {
        assert_eq!(k.name(), name, "{k:?}");
    }
}

/// No key is a capital letter, as bound or as written, here or in the library's actions menu.
#[test]
fn no_key_is_a_capital_letter() {
    let capital = |c: char| c.is_ascii_uppercase();
    for b in KEYMAP.iter() {
        for k in b.keys {
            if let K::Ch(c) | K::Ctrl(c) | K::Shift(KeyCode::Char(c)) | K::Code(KeyCode::Char(c)) = *k {
                assert!(!capital(c), "{:?} in {:?} is bound to {c}", b.act, b.scope);
            }
            assert!(
                !matches!(k, K::Shift(KeyCode::Char(_))),
                "{:?} in {:?}: shift and a letter is its capital",
                b.act,
                b.scope
            );
        }
        let written = b.shown.split_whitespace().any(|t| t.chars().count() == 1 && t.chars().all(capital));
        assert!(!written, "{:?} in {:?} is written {:?}", b.act, b.scope, b.shown);
    }
    let (mut app, _dir) = app_with(library());
    app.track_state.select(Some(0));
    let target = app.target().unwrap();
    for item in crate::tui::edits::menu_items(&target) {
        assert!(!item.key.chars().any(capital), "{} is on {}", item.label, item.key);
    }
}

/// A place keys are tried, by name, and how the player gets there.
type Setup = (&'static str, fn(&mut App));

/// Where the keys are tried: each view, the library's panes, player mode and the pads.
fn setups() -> Vec<Setup> {
    vec![
        ("the track list", |app| app.focus = Focus::Tracks),
        ("the playlists", |app| app.focus = Focus::Playlists),
        ("the queue", |app| {
            app.show_crate = true;
            app.focus = Focus::Crate;
        }),
        ("pads on", |app| app.deck.pads = true),
        ("player mode", |app| app.deck_layout = true),
        ("Prepare", |app| app.view = View::Prepare),
        ("Drives", |app| app.view = View::Drives),
        ("Snapshots", |app| {
            app.view = View::Drives;
            app.drive_pane = DrivePane::Snapshots;
        }),
        ("localnetwork", |app| app.view = View::Profile),
        ("Recordings", |app| app.view = View::Mixes),
    ]
}

/// No key waits for another: in each view, pane, player mode and with the pads on, after any key
/// bound there that doesn't open something or go elsewhere, the next key (`j`, or `k` where it
/// left the last row selected) does what it does on its own.
#[test]
fn no_key_waits_for_another() {
    let mut tried = 0;
    for (place, setup) in setups() {
        let (mut app, _dir) = app_with(library());
        setup(&mut app);
        let scopes = scopes(&app);
        let keys = KEYMAP.iter().filter(|b| scopes.contains(&b.scope)).flat_map(|b| b.keys.iter());
        let typed: Vec<KeyEvent> = keys.filter(|k| matches!(k, K::Ch(_) | K::Code(_))).map(|k| k.pressed()).collect();
        for key in typed {
            let (mut app, _dir) = app_with(library());
            setup(&mut app);
            let (view, focus, deck) = (app.view, app.focus, app.deck_showing());
            app.list_selection().0.replace(0);
            app.on_key(key);
            let elsewhere = app.input_taken() || app.view != view || app.focus != focus || app.deck_showing() != deck;
            let (at, len) = {
                let (sel, len) = app.list_selection();
                (sel.unwrap_or(0), len)
            };
            if elsewhere || app.quit_waiting() || len < 2 {
                continue;
            }
            let (next, then) = if at == 0 { ('j', at + 1) } else { ('k', at - 1) };
            app.on_key(press(KeyCode::Char(next)));
            assert_eq!(app.list_selection().0.unwrap_or(0), then, "{place}: after {key:?}, {next} didn't move");
            tried += 1;
        }
    }
    assert!(tried > 60, "the keys were tried: {tried}");
}

/// A DJ finds an action by the words they'd use for it (and the footer's names for it) in the
/// palette as it opens, with the looks, the choices, the actions menu's and the plugins', first.
#[test]
fn the_palette_finds_actions_by_the_words_a_dj_uses() {
    use super::super::palette::{Picker, Then};
    let (mut app, _dir) = app_with(library());
    app.track_state.select(Some(0));
    app.playlist_state.select(Some(1));
    app.open_palette();
    let Some(Modal::Pick(opened)) = app.modal.take() else { panic!("no palette") };
    let Then::Act(acts) = opened.then else { panic!("not the palette") };
    let items = opened.items;
    for (typed, act) in [
        ("info", Act::Details),
        ("skip", Act::Next),
        ("rewind", Act::Seek(-1)),
        ("full screen", Act::DeckLayout),
        ("fullscreen", Act::DeckLayout),
        ("visualizer", Act::Visualiser),
        ("crate", Act::CrateAdd),
        ("profile", Act::ShowView(View::Profile)),
        ("delete", Act::Edit(crate::tui::edits::Action::Delete)),
        ("delete playlist", Act::Edit(crate::tui::edits::Action::Delete)),
        ("tag list", Act::CrateAdd),
        ("cancel", Act::Jobs),
        ("publish", Act::ShowView(View::Profile)),
        ("previous cue", Act::Cue(false)),
        ("next phrase", Act::Phrase(true)),
        ("zoom in", Act::Zoom(true)),
        ("shuffle", Act::Shuffle),
        ("stop", Act::Stop),
        ("quantize", Act::Quantize),
        ("headphones", Act::Outputs),
        ("select all", Act::MarkAll),
        ("my tag", Act::Tag),
        ("clear the queue", Act::CrateClear),
        ("waveform colours", Act::NextBands),
    ] {
        let mut p = Picker::new("Commands", items.clone(), Then::Act(acts.clone()));
        p.query = typed.into();
        p.filter();
        let first = p.shown.first().map(|&i| acts[i]);
        assert_eq!(first, Some(act), "{typed:?} found {:?} first", first.map(|_| &items[p.shown[0]].label));
    }
}

/// Every key is somewhere a person reads it: what `?` shows in each view, pane, player mode and
/// with the pads on lists every row with a key that works there.
#[test]
fn every_key_is_in_the_help() {
    for (place, setup) in setups() {
        let (mut app, _dir) = app_with(library());
        setup(&mut app);
        let listed: Vec<String> = help_rows(&app, "").into_iter().map(|r| r.keys).collect();
        let here = scopes(&app);
        for b in KEYMAP.iter().filter(|b| !b.keys.is_empty() && here.contains(&b.scope) && !b.shown.is_empty()) {
            assert!(
                listed.iter().any(|k| k == b.shown),
                "{} ({:?} in {:?}) isn't in the help in {place}",
                b.shown,
                b.act,
                b.scope
            );
        }
    }
}

/// A lookup of [`key_for`]'s: where, for what, and what it found.
type Lookup = (Vec<Scope>, Act, Option<K>);

thread_local! {
    /// The keys [`key_for`] looked up on this thread while a test watches.
    static LOOKED_UP: std::cell::RefCell<Option<Vec<Lookup>>> = const { std::cell::RefCell::new(None) };
}

/// Notes a lookup of [`key_for`]'s, while a test watches.
pub(super) fn looked_up(scopes: &[Scope], act: Act, found: Option<K>) {
    LOOKED_UP.with(|l| l.borrow_mut().as_mut().map(|v| v.push((scopes.to_vec(), act, found))));
}

/// The keys [`key_for`] looks up while `f` runs.
fn watching(f: impl FnOnce()) -> Vec<Lookup> {
    LOOKED_UP.with(|l| *l.borrow_mut() = Some(Vec::new()));
    f();
    LOOKED_UP.with(|l| l.borrow_mut().take()).unwrap_or_default()
}

/// Every key a view draws beside what it does is found where it looks it up: a lookup of an act
/// the scopes it searches don't bind, though a view binds it, draws a blank key. An act no view
/// gives a key (a Prepare job found by name, replaying edits, whose `y` is the offer's) is blank
/// on purpose.
#[test]
fn every_key_a_view_draws_is_found() {
    let mut places = setups();
    places.push(("a drive in Drives", |app| {
        app.view = View::Drives;
        let root = app.loaded.as_ref().unwrap().root().to_path_buf();
        app.drives = vec![locallibrary::Device::open(&root).unwrap().info()];
        app.drive_state.select(Some(0));
    }));
    places.push(("History", |app| {
        app.view = View::Drives;
        app.drive_pane = DrivePane::History;
    }));
    let mut drawn = 0;
    for (place, setup) in places {
        let (mut app, _dir) = app_with(library());
        setup(&mut app);
        // What an earlier place's frame kept would hide what this one looks up.
        effective::forget();
        let lookups = watching(|| {
            screen(&mut app, 160, 50);
        });
        drawn += lookups.len();
        let blank: Vec<_> =
            lookups.into_iter().filter(|(_, act, found)| found.is_none() && bound_in_a_view(*act)).collect();
        assert!(blank.is_empty(), "{place}: no key where it's looked up: {blank:?}");
    }
    assert!(drawn > 0, "the views looked keys up");
}

/// Whether a view, or a pane of one, gives `act` a key as the keys came.
fn bound_in_a_view(act: Act) -> bool {
    let in_a_view = |s: Scope| View::ALL.iter().any(|v| view_scopes(*v).contains(&s));
    KEYMAP.iter().any(|b| b.act == act && !b.keys.is_empty() && in_a_view(b.scope))
}

/// The keys every view has, from where it is: esc, `?` help, `:` commands, `e` the view's actions,
/// the arrows, enter, tab and the views on 1-5, looked up as the player looks them up. `/` search
/// is the library's alone for now (LOC-837 brings it to the other views), so it isn't one of them.
#[test]
fn every_view_has_the_standard_keys() {
    let places: [(Setup, Act); 9] = [
        (("Library", |app| app.focus = Focus::Tracks), Act::Actions),
        (("the playlists", |app| app.focus = Focus::Playlists), Act::Actions),
        (
            ("the queue", |app| {
                app.show_crate = true;
                app.focus = Focus::Crate;
            }),
            Act::Actions,
        ),
        (("Prepare", |app| app.view = View::Prepare), Act::PaneActions),
        (("Drives", |app| app.view = View::Drives), Act::DriveActions),
        (
            ("Snapshots", |app| {
                app.view = View::Drives;
                app.drive_pane = DrivePane::Snapshots;
            }),
            Act::PaneActions,
        ),
        (
            ("History", |app| {
                app.view = View::Drives;
                app.drive_pane = DrivePane::History;
            }),
            Act::PaneActions,
        ),
        (("Recordings", |app| app.view = View::Mixes), Act::PaneActions),
        (("localnetwork", |app| app.view = View::Profile), Act::PaneActions),
    ];
    for ((place, setup), actions) in places {
        let (mut app, _dir) = app_with(library());
        setup(&mut app);
        let scopes = scopes(&app);
        let mut standard = vec![
            (press(KeyCode::Esc), Act::Escape),
            (press(KeyCode::Char('?')), Act::Help),
            (press(KeyCode::Char(':')), Act::Palette),
            (press(KeyCode::Char('e')), actions),
            (press(KeyCode::Up), Act::Move(-1)),
            (press(KeyCode::Down), Act::Move(1)),
            (press(KeyCode::Left), Act::Seek(-1)),
            (press(KeyCode::Right), Act::Seek(1)),
            (press(KeyCode::Enter), Act::Activate),
            (press(KeyCode::Tab), Act::Focus(false)),
        ];
        standard.extend(View::ALL.into_iter().zip('1'..='5').map(|(v, d)| (press(KeyCode::Char(d)), Act::ShowView(v))));
        for (key, act) in standard {
            assert_eq!(lookup(&scopes, &key).map(|h| h.act), Some(act), "{place}: {key:?}");
        }
    }
}

/// The keys shown are the ones that do it there: with the pads on the tabs don't show the digits,
/// and in Recordings the palette doesn't say n skips the track (it skips the set's).
#[test]
fn a_key_is_shown_only_where_it_does_what_it_says() {
    use super::super::palette::commands_in;
    let (mut app, _dir) = app_with(library());
    let tabs = |app: &mut App| super::super::ui::screen(app, 160, 40).lines().next().unwrap_or_default().to_string();
    assert!(tabs(&mut app).contains("1 Library"), "{}", tabs(&mut app));
    app.deck.pads = true;
    assert!(!tabs(&mut app).contains("1 Library"), "{}", tabs(&mut app));
    app.deck.pads = false;
    app.view = View::Mixes;
    let palette = commands_in(View::Mixes, &scopes(&app));
    let detail = |act: Act| palette.iter().find(|(a, _)| *a == act).map(|(_, i)| i.detail.clone()).unwrap_or_default();
    assert_ne!(detail(Act::Next), "n");
    assert_ne!(detail(Act::PlayPause), "space");
    assert_eq!(detail(Act::MixPlay), "space");
}

/// The help leads with the keys, and what has no key comes after, found by name in `:`.
#[test]
fn the_help_leads_with_the_keys() {
    let (app, _dir) = app_with(library());
    let rows = help_rows(&app, "");
    let by_name = |r: &HelpRow| r.binding.keys.is_empty() && super::written(r.binding) == ":";
    let first = rows.iter().position(by_name).expect("some are found by name");
    assert!(first > 10, "the keys come first: {first}");
    for r in &rows[first..] {
        assert!(
            by_name(r) && r.title == "By name, in :",
            "{:?} {:?} under {:?}",
            r.binding.act,
            r.binding.shown,
            r.title
        );
    }
    assert!(
        rows[..first]
            .iter()
            .all(|r| !r.binding.keys.is_empty() || r.binding.act == Act::None || !r.binding.shown.is_empty())
    );
}

/// Typed in the help, the words a DJ uses find a row as they do in the palette.
#[test]
fn the_help_finds_the_palettes_words() {
    let (app, _dir) = app_with(library());
    let found = |typed: &str| help_rows(&app, typed).into_iter().map(|r| r.binding.act).collect::<Vec<_>>();
    assert!(found("random").contains(&Act::Shuffle));
    assert!(found("sound card").contains(&Act::Outputs));
}

/// Each line of `ids.txt`: an id and the act it names, or `retired`.
fn issued() -> Vec<(&'static str, &'static str)> {
    let lines = include_str!("ids.txt").lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty());
    lines.map(|l| l.split_once(' ').unwrap_or((l, ""))).collect()
}

/// An id, once released, names the same act for good, and one retired never comes back: every
/// act a key can be set for has its line in `ids.txt`, with the id it has here, and no id is
/// there twice.
#[test]
fn ids_never_change_or_return() {
    let acts = ids::bindable(&[]);
    let lines = issued();
    for (id, act) in &lines {
        if *act == "retired" {
            continue;
        }
        let found = acts.iter().find(|a| format!("{a:?}") == *act).unwrap_or_else(|| panic!("{id}: no act {act}"));
        assert_eq!(ids::id(*found).as_deref(), Some(*id), "{act} has another id now");
    }
    for (i, (id, _)) in lines.iter().enumerate() {
        assert!(!lines[..i].iter().any(|(earlier, _)| earlier == id), "{id} is issued twice");
    }
    for act in acts {
        let id = ids::id(act).unwrap();
        assert!(lines.iter().any(|(issued, _)| *issued == id), "{id} ({act:?}) isn't in ids.txt");
        assert_eq!(ids::act(&id, &[]), Some(act));
    }
}

/// The keybinds Lexicon knows every id ever issued, retired ones too.
#[cfg(feature = "account")]
#[test]
fn every_lexicon_id_is_in_ids_txt() {
    use locallibrary::atmosphere::lexicon::{KEYBINDS, document};
    let doc = document(KEYBINDS).unwrap();
    let known = doc["defs"]["binding"]["properties"]["action"]["knownValues"].as_array().unwrap();
    let known: Vec<&str> = known.iter().filter_map(|v| v.as_str()).collect();
    let issued: Vec<&str> = issued().into_iter().map(|(id, _)| id).collect();
    assert_eq!(known, issued);
}

/// A key pressed for an action is kept in keybinds.json as it's spelled, and read back it answers
/// to the same press; the keys the player keeps for itself are refused as they're read. One it
/// can't take (alt held, a key with no name) is none.
#[hegel::test(test_cases = 300)]
fn prop_a_key_pressed_reads_back_as_the_same_press(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::keybinds::Key;
    let named = [
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Insert,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
    ];
    let unnamed =
        [KeyCode::CapsLock, KeyCode::Null, KeyCode::NumLock, KeyCode::PrintScreen, KeyCode::Pause, KeyCode::Menu];
    let (code, modifiers) = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => {
            let typed = vec![' ', 'a', 'z', 'é', '&', ']', '1', '-', '?', ':'];
            (KeyCode::Char(tc.draw(gs::sampled_from(typed))), KeyModifiers::NONE)
        }
        1 => (KeyCode::Char(tc.draw(gs::sampled_from(('A'..='Z').collect::<Vec<_>>()))), KeyModifiers::SHIFT),
        2 => (KeyCode::Char(tc.draw(gs::sampled_from(('a'..='z').collect::<Vec<_>>()))), KeyModifiers::CONTROL),
        3 => {
            let shift = tc.draw(gs::booleans());
            match tc.draw(gs::sampled_from(named.to_vec())) {
                // Terminals send shift-tab as a key of its own.
                KeyCode::Tab if shift => (KeyCode::BackTab, KeyModifiers::SHIFT),
                code => (code, if shift { KeyModifiers::SHIFT } else { KeyModifiers::NONE }),
            }
        }
        4 => (KeyCode::F(tc.draw(gs::integers::<u8>().min_value(1).max_value(12))), KeyModifiers::NONE),
        _ => (tc.draw(gs::sampled_from(unnamed.to_vec())), KeyModifiers::NONE),
    };
    let alt = tc.draw(gs::booleans());
    let pressed = KeyEvent::new(code, if alt { modifiers | KeyModifiers::ALT } else { modifiers });
    let takes = !alt && !unnamed.contains(&code);
    let Some(k) = K::of(&pressed) else {
        assert!(!takes, "{pressed:?} is a key the player takes");
        return;
    };
    assert!(takes, "{pressed:?} gave {k:?}");
    assert!(k.matches(&pressed), "{k:?} for {pressed:?}");
    let spelled = k.portable().unwrap_or_else(|| panic!("{k:?} has no spelling")).to_string();
    match Key::parse(&spelled) {
        Ok(read) => assert!(K::from(read).matches(&pressed), "{spelled:?} for {pressed:?}"),
        Err(why) => assert!(matches!(k, K::Ch('?' | ':') | K::Ctrl('c' | 'h' | 'i' | 'm')), "{spelled:?}: {why}"),
    }
}

/// An act has an id exactly when a key can be set for it, and its id names it: a value the
/// keymap never uses, a library action with nothing of its own to bind, a command line it
/// doesn't copy, or what's found at run time has none.
#[hegel::test(test_cases = 300)]
fn prop_an_act_has_an_id_only_when_a_key_can_be_set_for_it(tc: hegel::TestCase) {
    use super::super::edits::Action;
    use hegel::generators as gs;
    let bindable = ids::bindable(&[]);
    let copied: Vec<Act> = KEYMAP.iter().map(|b| b.act).filter(|a| matches!(a, Act::Copy(_))).collect();
    let act = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => tc.draw(gs::sampled_from(KEYMAP.iter().map(|b| b.act).collect::<Vec<_>>())),
        1 => {
            let n = tc.draw(gs::integers::<i8>());
            let steps = vec![
                Act::Seek(n),
                Act::Volume(n),
                Act::BeatJump(n),
                Act::Move(n),
                Act::Page(n),
                Act::End(n),
                Act::CrateMove(n),
                Act::Change(n),
            ];
            tc.draw(gs::sampled_from(steps))
        }
        2 => Act::Edit(Action::Rate(tc.draw(gs::integers::<u8>()))),
        3 => {
            let own = vec![Action::RatingMenu, Action::Delete, Action::CrateAdd, Action::Tag, Action::PlayNext];
            Act::Edit(tc.draw(gs::sampled_from(own)))
        }
        4 => {
            let not_copied = ["store", "store y", "drive x", "bug report", "x"].map(Act::Copy);
            tc.draw(gs::sampled_from(copied.iter().copied().chain(not_copied).collect::<Vec<_>>()))
        }
        _ => tc.draw(gs::sampled_from(vec![Act::ProviderSettings, Act::CheckSettings, Act::None, Act::Help])),
    };
    let id = ids::id(act);
    assert_eq!(id.is_some(), bindable.contains(&act), "{act:?}: {id:?}");
    if let Some(id) = id {
        assert_eq!(ids::act(&id, &[]), Some(act), "{id}");
    }
}
