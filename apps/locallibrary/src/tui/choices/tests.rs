use std::time::Duration;

use hegel::generators as gs;

use super::super::app::View;
use super::super::app::testing::{app_with, blank, job, last_message, library, press};
use super::super::state::Saved;
use super::super::ui::screen;
use super::*;

/// What's chosen for the deck, as the app holds it.
fn chosen(app: &App) -> (Option<BandMode>, Notation, KeyPalette, u16, bool) {
    let d = &app.deck;
    (d.bands, d.notation, d.key_palette, d.end_warning, d.art)
}

/// Every choice in the palette takes, says so, and comes back from what's saved.
#[test]
fn every_choice_in_the_palette_is_kept() {
    for (act, item) in items() {
        let (mut app, _dir) = blank(View::Library);
        press(&mut app, ":");
        press(&mut app, &item.label);
        press(&mut app, "\n");
        let d = &app.deck;
        match act {
            Act::Bands(mode) => assert_eq!(d.bands, mode, "{}", item.label),
            Act::KeyNotation(n) => assert_eq!(d.notation, n, "{}", item.label),
            Act::KeyPalette(k) => assert_eq!(d.key_palette, k, "{}", item.label),
            Act::EndWarning(secs) => assert_eq!(d.end_warning, secs, "{}", item.label),
            Act::Hints(hints) => assert_eq!(app.hints, hints, "{}", item.label),
            Act::Art(on) => assert_eq!(d.art, on, "{}", item.label),
            other => panic!("{other:?}"),
        }
        let saved = app.saved_state();
        let picked = chosen(&app);
        let (mut fresh, _dir) = blank(View::Library);
        fresh.apply_saved(&saved, false);
        assert_eq!(chosen(&fresh), picked, "{} back from what's saved", item.label);
    }
}

/// What wasn't chosen is the defaults, and what isn't known too.
#[test]
fn the_defaults_are_what_nothing_saved_comes_to() {
    let (mut app, _dir) = blank(View::Library);
    let start = chosen(&app);
    assert_eq!(start, (None, Notation::Camelot, KeyPalette::Theme, 30, true));
    let odd = Saved {
        notation: Some("sol-fa".into()),
        key_palette: Some("mauve".into()),
        bands: Some("sepia".into()),
        ..Saved::default()
    };
    app.apply_saved(&odd, false);
    assert_eq!(chosen(&app), start);
}

/// The setting's in the palette, kept, and read back; what isn't known is the default.
#[test]
fn the_hint_setting_is_kept() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, ":compact hints\n");
    assert_eq!(app.hints, Hints::Compact);
    assert_eq!(app.saved_state().hints.as_deref(), Some("compact"));
    let json = serde_json::to_value(app.saved_state()).unwrap();
    let prefs: locallibrary::settings::transfer::PlayerPrefs = serde_json::from_value(json).unwrap();
    assert_eq!(prefs.hints.as_deref(), Some("compact"), "settings export carries it");

    let (mut next, _dir) = app_with(library());
    next.apply_saved(&app.saved_state(), false);
    assert_eq!(next.hints, Hints::Compact);
    press(&mut next, ":no hints\n");
    assert_eq!(next.hints, Hints::Off);
    assert_eq!(Hints::parse("loud"), None);
    press(&mut next, ":full hints\n");
    assert_eq!(next.saved_state().hints, None, "the default isn't written");
}

/// Off takes the footer's hints away; errors and a job's progress stay.
#[test]
fn errors_and_progress_show_with_the_hints_off() {
    let (mut app, _dir) = app_with(library());
    app.hints = Hints::Off;
    assert!(!screen(&mut app, 120, 40).contains("? help"));
    app.toast("it went wrong", Level::Error);
    app.job = Some(job(true, false));
    app.on_progress(99, "copying · a.mp3".into(), false);
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("it went wrong") && s.contains("copying · a.mp3"), "{s}");
}

/// An error stays until it's put away, past any info, which goes by itself; both stay in the log.
#[test]
fn errors_outlast_info_and_the_log_keeps_them() {
    let (mut app, _dir) = app_with(library());
    app.toast("it went wrong", Level::Error);
    app.toast = app.toast.take().map(|(m, l, at)| (m, l, at - Duration::from_secs(600)));
    app.tick();
    assert_eq!(last_message(&app), "it went wrong", "long past when info would go");
    app.toast("and this is news", Level::Info);
    app.toast = app.toast.take().map(|(m, l, at)| (m, l, at - Duration::from_secs(600)));
    app.tick();
    assert!(app.toast.is_none());
    assert_eq!(app.held.as_deref(), Some("it went wrong"), "still up");
    assert!(screen(&mut app, 80, 24).contains("it went wrong"));
    press(&mut app, "\u{1b}");
    assert!(app.held.is_none());
    assert!(app.log.iter().any(|(m, l, _)| m == "it went wrong" && *l == Level::Error));
}

/// Whatever's chosen, in any order, from the palette, with `w` going round the waveforms' colours
/// and the played tracks forgotten between: each says what it did, the last of each kind is what
/// the deck has, and that comes back from what's saved.
#[hegel::test(test_cases = 30)]
fn prop_the_last_choice_of_each_kind_holds(tc: hegel::TestCase) {
    // The hint setting has a test of its own, and isn't part of the deck's choices.
    let choices: Vec<_> = items().into_iter().filter(|(act, _)| !matches!(act, Act::Hints(_))).collect();
    let (mut app, _dir) = blank(View::Library);
    let mut want = chosen(&app);
    let steps = tc.draw(gs::integers::<usize>().min_value(1).max_value(8));
    for _ in 0..steps {
        let said = match tc.draw(gs::integers::<usize>().max_value(choices.len() + 1)) {
            i if i < choices.len() => {
                let (act, item) = &choices[i];
                press(&mut app, ":");
                press(&mut app, &item.label);
                press(&mut app, "\n");
                match *act {
                    Act::Bands(mode) => want.0 = mode,
                    Act::KeyNotation(n) => want.1 = n,
                    Act::KeyPalette(k) => want.2 = k,
                    Act::EndWarning(secs) => want.3 = secs,
                    Act::Art(on) => want.4 = on,
                    other => panic!("{other:?}"),
                }
                item.label.clone()
            }
            i if i == choices.len() => {
                app.perform(Act::NextBands, None);
                want.0 = match want.0 {
                    None => Some(BandMode::Blue),
                    Some(BandMode::Blue) => Some(BandMode::Rgb),
                    Some(BandMode::Rgb) => Some(BandMode::ThreeBand),
                    Some(BandMode::ThreeBand) => None,
                };
                bands_label(want.0)
            }
            _ => {
                app.played_tracks.insert("/Contents/a.mp3".into());
                app.perform(Act::ResetPlayed, None);
                assert!(app.played_tracks.is_empty(), "forgotten");
                "nothing counts as played now".into()
            }
        };
        assert_eq!(chosen(&app), want, "after {said}");
        assert_eq!(last_message(&app), said);
    }
    let (mut fresh, _dir) = blank(View::Library);
    fresh.apply_saved(&app.saved_state(), false);
    assert_eq!(chosen(&fresh), want, "back from what's saved");
}
