use super::super::app::View;
use super::super::app::testing::{blank, press};
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
