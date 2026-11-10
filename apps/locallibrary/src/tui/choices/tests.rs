use hegel::generators as gs;

use super::super::app::View;
use super::super::app::testing::{blank, last_message, press};
use super::super::state::Saved;
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

/// Whatever's chosen, in any order, from the palette, with `w` going round the waveforms' colours
/// and the played tracks forgotten between: each says what it did, the last of each kind is what
/// the deck has, and that comes back from what's saved.
#[hegel::test(test_cases = 30)]
fn prop_the_last_choice_of_each_kind_holds(tc: hegel::TestCase) {
    let choices = items();
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
