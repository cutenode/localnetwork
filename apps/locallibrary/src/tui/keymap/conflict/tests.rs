use hegel::generators as gs;
use ratatui::crossterm::event::KeyCode;

use super::*;

/// The next track on `m`, which marks in the Library: swapped, mark gets next track's `n` (and
/// keeps insert), and the next track has `m` everywhere.
#[test]
fn a_taken_key_offers_a_swap() {
    let e = Effective::defaults();
    let a = answer(&e, Act::Next, K::Ch('m'));
    assert_eq!(a.verdict, Verdict::Swap { with: Act::Mark, gets: vec![K::Code(KeyCode::Insert), K::Ch('n')] });
    let did = |before: Option<Act>| a.effects.iter().find(|x| x.before == before && x.after == Some(Act::Next));
    assert!(did(Some(Act::Mark)).is_some_and(|x| x.places.contains(&"Library")), "{:?}", a.effects);
    assert!(did(None).is_some_and(|x| x.places.contains(&"Drives")), "{:?}", a.effects);
    let mut binds = Keybinds::default();
    apply(&mut binds, &a);
    assert_eq!(binds.keys["playback.next"], ["m"]);
    assert_eq!(binds.keys["library.mark"], ["insert", "n"]);
    let after = Effective::new(&binds, &[]);
    assert_eq!(after.problems, Vec::<String>::new());
    assert!(clashes(&after).is_empty());
}

/// Space plays and pauses, and plays the set in Recordings: two things, so it's refused for stop.
#[test]
fn two_conflicts_are_refused() {
    let a = answer(&Effective::defaults(), Act::Stop, K::Ch(' '));
    assert_eq!(a.verdict, Verdict::Refused("space does 2 things here: pause, play. Pick another key".into()));
}

/// Playing the set on `l`, which loops: the loop would get space, which pauses, so it's refused.
#[test]
fn a_swap_that_makes_a_new_conflict_is_refused() {
    let a = answer(&Effective::defaults(), Act::MixPlay, K::Ch('l'));
    let Verdict::Refused(why) = &a.verdict else { panic!("{:?}", a.verdict) };
    assert_eq!(why, "l is loop's, which can't take space in its place: pick another key");
}

/// The keys that always do the same, and those terminals send as others, can't be given; nor can
/// a character to what a dialog typed into does.
#[test]
fn esc_ctrl_c_and_the_help_keys_cant_be_taken() {
    let e = Effective::defaults();
    let why = |act: Act, key: K| match answer(&e, act, key).verdict {
        Verdict::Refused(why) => why,
        other => panic!("{key:?} for {act:?}: {other:?}"),
    };
    assert_eq!(why(Act::Next, K::Code(KeyCode::Esc)), "esc always closes");
    assert_eq!(why(Act::Next, K::Ctrl('c')), "ctrl-c always quits");
    assert_eq!(why(Act::Next, K::Ch('?')), "? always opens the help");
    assert_eq!(why(Act::Next, K::Ch(':')), ": always opens the commands");
    assert_eq!(why(Act::Next, K::Ctrl('h')), "terminals send ctrl-h as backspace");
    assert_eq!(why(Act::SearchWhere, K::Ch('a')), "characters type in the search");
    // Its own keys back are fine, and so is a key that types nothing.
    assert_eq!(answer(&e, Act::Change(1), K::Ch(' ')).verdict, Verdict::Reset);
    assert_eq!(answer(&e, Act::SearchWhere, K::Code(KeyCode::F(9))).verdict, Verdict::Save);
}

/// The keys a person might try.
fn any_key(tc: &hegel::TestCase) -> K {
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => {
            K::Ch(tc.draw(gs::sampled_from(vec!['a', 'e', 'j', 'm', 'n', 'p', 'q', 'u', 'x', ' ', '1', '6', '-', '&'])))
        }
        1 => K::Ch(tc.draw(gs::sampled_from(vec!['A', 'N', 'Z']))),
        2 => K::Ctrl(tc.draw(gs::sampled_from(vec!['q', 'z', 'c']))),
        3 => K::Code(tc.draw(gs::sampled_from(vec![
            KeyCode::F(5),
            KeyCode::F(6),
            KeyCode::Up,
            KeyCode::Tab,
            KeyCode::Enter,
        ]))),
        _ => K::Shift(tc.draw(gs::sampled_from(vec![KeyCode::Up, KeyCode::Down]))),
    }
}

/// However many keys are pressed for whichever actions, each answer kept as it says, no key ends
/// up with two actions where they're at home, and the file reads back with nothing to say.
#[hegel::test(test_cases = 60)]
fn prop_no_answer_doubles_a_key(tc: hegel::TestCase) {
    let acts = ids::bindable(&[]);
    let mut binds = Keybinds::default();
    let mut e = Effective::defaults();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        let act = tc.draw(gs::sampled_from(acts.clone()));
        let a = answer(&e, act, any_key(&tc));
        apply(&mut binds, &a);
        e = Effective::new(&binds, &[]);
        assert_eq!(e.problems, Vec::<String>::new(), "{a:?}");
        assert_eq!(clashes(&e), Vec::new(), "{a:?}");
        if let Verdict::Save | Verdict::Swap { .. } = a.verdict {
            assert!(e.keys(act).contains(&a.key), "{a:?}");
        }
    }
}

/// Tab moves through a form's fields, where a form's next choice is at home too: it's refused,
/// not offered as a swap, since moving there is the dialog's own.
#[test]
fn a_key_only_a_dialog_has_is_refused_without_a_swap() {
    let a = answer(&Effective::defaults(), Act::Change(1), K::Code(KeyCode::Tab));
    assert_eq!(a.verdict, Verdict::Refused("tab is move down in a form: pick another key".into()));
}

/// A run of digits stands for the hot cues' keys as they came, and has no spelling in
/// keybinds.json: it can't be given to an action.
#[test]
fn a_run_of_digits_cant_be_kept() {
    assert_eq!(K::Digits('1', '8').portable(), None);
    let a = answer(&Effective::defaults(), Act::Next, K::Digits('1', '8'));
    assert_eq!(a.verdict, Verdict::Refused("1-8 can't be kept as a key".into()));
}
