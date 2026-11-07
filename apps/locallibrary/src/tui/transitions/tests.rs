use ratatui::style::Color;

use super::super::app::View;
use super::super::app::testing::{blank, playing_wavs, press, wait};
use super::super::ui::screen;
use super::*;

const AREA: Rect = Rect { x: 0, y: 0, width: 10, height: 4 };

fn on() -> Transitions {
    theme::set_test_tier(Some(Tier::Truecolor));
    Transitions { enabled: true, ..Default::default() }
}

fn shown() -> Shown {
    Shown { track: Some(1), ..Default::default() }
}

/// The first frame shows what there is; a change after it fades in, and when the fade's over it
/// stops asking for frames and leaves the cells as they were drawn.
#[test]
fn a_change_fades_and_stops() {
    let mut t = on();
    t.follow(shown(), AREA);
    assert!(!t.active(), "nothing to fade from");
    t.follow(shown(), AREA);
    assert!(!t.active(), "nothing changed");
    t.follow(Shown { deck: true, ..shown() }, AREA);
    assert!(t.active());

    let lit = || {
        let mut buf = Buffer::empty(AREA);
        for cell in &mut buf.content {
            cell.set_symbol("█").set_fg(Color::Rgb(250, 240, 230));
        }
        buf
    };
    let mut part_way = lit();
    t.step(Duration::from_millis(40), &mut part_way);
    assert!(t.active(), "still going");
    assert_ne!(part_way, lit(), "on its way in");
    let mut done = lit();
    t.step(Duration::from_millis(FADE_MS.into()), &mut done);
    assert!(!t.active(), "over: back to the slow tick");
    assert_eq!(done, lit());
    t.draw(&mut done);
    assert_eq!(done, lit(), "and nothing's touched after");
}

/// With less motion, in fewer colours, or for a test's app, nothing fades.
#[test]
fn nothing_fades_without_what_it_takes() {
    let change = |t: &mut Transitions| {
        t.follow(shown(), AREA);
        t.follow(Shown { help: true, ..shown() }, AREA);
        t.active()
    };
    let mut t = on();
    theme::set_test_reduced_motion(true);
    assert!(!change(&mut t), "less motion");
    theme::set_test_reduced_motion(false);
    for tier in [Tier::Colors256, Tier::Colors16] {
        theme::set_test_tier(Some(tier));
        let mut t = Transitions { enabled: true, ..Default::default() };
        assert!(!change(&mut t), "{tier:?}");
    }
    theme::set_test_tier(None);
    let mut off = Transitions::default();
    assert!(!change(&mut off));
}

/// Paused on the deck, nothing moves once the visualisers have settled, so the player goes back to
/// its slow tick.
#[test]
fn a_paused_deck_goes_idle() {
    let (_dir, mut app, _out) = playing_wavs(&[8_000]);
    wait(&mut app, |a| a.now.as_ref().is_some_and(|n| !n.loading && !n.opening));
    app.deck_layout = true;
    screen(&mut app, 160, 45);
    assert!(app.is_playing() && app.animating());
    app.audio.as_ref().unwrap().toggle_pause();
    let counts: Vec<usize> = app.areas.spectra.iter().map(|&(w, m)| loclib_waveform::bar_layout(w, m).0).collect();
    for _ in 0..40 {
        let tap = app.audio.as_ref().unwrap().tap.clone();
        app.analyzer.update(&tap, false, &counts, 0.25);
        screen(&mut app, 160, 45);
    }
    assert!(!app.animating(), "nothing's moving");
}

/// A track loading, the deck, details, the crate and the help each fade in; a fade keeps the frame
/// rate up only while it runs, and a player that's still goes idle.
#[test]
fn the_player_goes_idle_after_a_fade() {
    let (mut app, _dir) = blank(View::Library);
    theme::set_test_tier(Some(Tier::Truecolor));
    app.transitions.enabled = true;
    screen(&mut app, 120, 40);
    assert!(!app.animating(), "nothing's moving");
    press(&mut app, "f");
    screen(&mut app, 120, 40);
    assert!(app.transitions.active() && app.animating());
    let mut buf = Buffer::empty(AREA);
    app.transitions.step(Duration::from_secs(1), &mut buf);
    assert!(!app.animating(), "over");
    screen(&mut app, 120, 40);
    assert!(!app.animating(), "and it stays over");
    theme::set_test_tier(None);
}
