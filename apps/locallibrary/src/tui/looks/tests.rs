use ratatui::crossterm::event::KeyCode;

use super::super::app::View;
use super::super::app::testing::{blank, key, press};
use super::super::screens::{Fixture, fixture};
use super::super::state::Saved;
use super::super::ui::screen;
use super::*;

/// The palette's choice of icons shows from the next frame, stays in what's saved, comes back from
/// it, and goes when the choice is taken back. Saved state's the only way between runs, so it's
/// shared: this and the next test take turns.
#[test]
fn choices_in_the_palette_are_drawn_and_kept() {
    let _turn = TURN.lock().unwrap();
    let Fixture { _dir, mut app, .. } = fixture();
    for c in ":ascii icons".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    assert!(screen(&mut app, 120, 40).contains("ASCII icons"));
    key(&mut app, KeyCode::Enter);
    assert_eq!(icons::chosen(), Some(IconSet::Ascii));
    let drawn = screen(&mut app, 120, 40);
    assert!(drawn.is_ascii() && drawn.contains("drawn in ascii icons"), "{drawn}");
    assert_eq!(app.saved_state().icons.as_deref(), Some("ascii"));

    for c in ":16 colours".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    assert_eq!(theme::chosen_tier(), Some(Tier::Colors16));
    assert_eq!(app.saved_state().color.as_deref(), Some("16"));
    assert!(screen(&mut app, 120, 40).contains("drawn in 16 colours"));

    app.apply_saved(&Saved::default(), false);
    assert_eq!((icons::chosen(), theme::chosen_tier()), (None, None), "nothing saved: nothing chosen");
    app.apply_saved(&Saved { color: Some("256".into()), icons: Some("nerd".into()), ..Saved::default() }, false);
    assert_eq!((icons::chosen(), theme::chosen_tier()), (Some(IconSet::Nerd), Some(Tier::Colors256)));
    app.apply_saved(&Saved { color: Some("lots".into()), icons: Some("emoji".into()), ..Saved::default() }, false);
    assert_eq!((icons::chosen(), theme::chosen_tier()), (None, None), "what isn't known is as good as nothing");
}

/// Any choice in the palette is drawn in from the next frame and kept, and what's kept comes back.
#[hegel::test(test_cases = 12)]
fn prop_any_choice_in_the_palette_is_kept(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let _turn = TURN.lock().unwrap();
    let (mut app, _dir) = blank(View::Library);
    let offered = items();
    let (act, item) = &offered[tc.draw(gs::integers::<usize>().max_value(offered.len() - 1))];
    let (tier, set) = match *act {
        Act::Colours(tier) => (tier, None),
        Act::Icons(set) => (None, set),
        other => panic!("{other:?}"),
    };
    press(&mut app, ":");
    press(&mut app, &item.label);
    press(&mut app, "\n");
    let saved = app.saved_state();
    assert_eq!(saved.color.as_deref(), tier.map(Tier::name), "{}", item.label);
    assert_eq!(saved.icons.as_deref(), set.map(IconSet::name), "{}", item.label);
    assert_eq!((theme::chosen_tier(), icons::chosen()), (tier, set), "{}", item.label);
    if let Some(tier) = tier {
        assert_eq!(theme::tier(), tier);
    }
    if let Some(set) = set {
        assert_eq!(icons::set(), set);
    }
    theme::choose_tier(None);
    icons::choose(None);
    app.apply_saved(&saved, false);
    assert_eq!((theme::chosen_tier(), icons::chosen()), (tier, set), "back from what's saved");
    theme::choose_tier(None);
    icons::choose(None);
}

/// "as the terminal says" and "for this terminal" take the choice back.
#[test]
fn a_choice_can_be_taken_back() {
    let _turn = TURN.lock().unwrap();
    let Fixture { _dir, mut app, .. } = fixture();
    theme::choose_tier(Some(Tier::Colors16));
    icons::choose(Some(IconSet::Nerd));
    app.use_colours(None);
    app.use_icons(None);
    assert_eq!((icons::chosen(), theme::chosen_tier()), (None, None));
    assert_eq!(items().len(), 8);
}

static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
