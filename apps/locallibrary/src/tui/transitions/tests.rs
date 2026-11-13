use hegel::generators as gs;
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

/// For any run of frames: a fade starts only when what's shown changes, in 24-bit colour without
/// less motion asked for, and runs its length from the last change. It only ever changes colours
/// in its own area, never a symbol, and leaves the cells as they were drawn once it's over.
#[hegel::test(test_cases = 100)]
fn prop_a_fade_follows_a_change_for_its_length(tc: hegel::TestCase) {
    let tier = tc.draw(gs::sampled_from(vec![Tier::Truecolor, Tier::Colors256, Tier::Colors16]));
    let (enabled, less_motion) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let fades = enabled && !less_motion && tier == Tier::Truecolor;
    theme::set_test_tier(Some(tier));
    theme::set_test_reduced_motion(less_motion);
    let page = Rect { width: AREA.width + 4, height: AREA.height + 2, ..AREA };
    let lit = || {
        let mut buf = Buffer::empty(page);
        for (i, cell) in buf.content.iter_mut().enumerate() {
            cell.set_symbol(if i % 3 == 0 { "█" } else { "a" }).set_fg(Color::Rgb(250, 240, 230));
        }
        buf
    };
    let mut t = Transitions { enabled, ..Default::default() };
    let (mut was, mut ran): (Option<Shown>, Option<u32>) = (None, None);
    let steps = tc.draw(gs::integers::<usize>().min_value(1).max_value(12));
    for _ in 0..steps {
        if tc.draw(gs::booleans()) {
            let shown = Shown {
                deck: tc.draw(gs::booleans()),
                details: tc.draw(gs::booleans()),
                crate_panel: false,
                help: tc.draw(gs::booleans()),
                track: tc.draw(gs::optional(gs::integers::<u64>().max_value(2))),
            };
            t.follow(shown, AREA);
            if fades && was.is_some_and(|w| w != shown) {
                ran = Some(0);
            }
            was = Some(shown);
        } else {
            let ms = tc.draw(gs::integers::<u32>().max_value(120));
            let mut buf = lit();
            t.step(Duration::from_millis(ms.into()), &mut buf);
            ran = ran.map(|r| r + ms).filter(|&r| r < FADE_MS);
            let (fresh, outside) = (lit(), |x, y| !AREA.contains(ratatui::layout::Position::new(x, y)));
            for (i, (cell, before)) in buf.content.iter().zip(&fresh.content).enumerate() {
                let (x, y) = buf.pos_of(i);
                assert_eq!(cell.symbol(), before.symbol(), "({x}, {y})");
                assert!(!outside(x, y) || cell == before, "({x}, {y}) is outside the fade");
            }
            if ran.is_none() {
                let mut after = lit();
                t.draw(&mut after);
                assert_eq!(after, fresh, "nothing's touched once it's over");
            }
        }
        assert_eq!(t.active(), ran.is_some(), "{tier:?}, enabled {enabled}, less motion {less_motion}");
    }
    theme::set_test_reduced_motion(false);
    theme::set_test_tier(None);
}

fn lit_wash() -> Buffer {
    let mut buf = Buffer::empty(AREA);
    for cell in &mut buf.content {
        cell.set_symbol("▆").set_fg(Color::Rgb(40, 120, 220));
    }
    buf
}

/// The warning of a track's end washes the overview twice as it comes on, all the way into the
/// warning colour at the height of each pulse, then stops. Staying on starts nothing more; going
/// off first (a seek back) stops it, and coming on again starts another.
#[test]
fn the_end_warning_washes_once_a_crossing() {
    let mut t = on();
    let warn = theme::rgb3(theme::current().critical);
    t.follow_warning(false, AREA);
    t.follow_warning(true, AREA);
    assert!(t.active(), "it came on");
    let mut buf = lit_wash();
    t.step_wash(Duration::from_millis(180), &mut buf);
    assert!(buf.content.iter().all(|c| c.fg == warn), "{buf:?}");
    let mut buf = lit_wash();
    t.step_wash(Duration::from_millis(180), &mut buf);
    assert_eq!(buf, lit_wash(), "back between the pulses");
    t.step_wash(Duration::from_millis(400), &mut lit_wash());
    assert!(!t.active(), "two pulses, then over");
    t.follow_warning(true, AREA);
    assert!(!t.active(), "still on: nothing more");
    t.follow_warning(false, AREA);
    t.follow_warning(true, AREA);
    assert!(t.active(), "on again after a seek back");
    t.follow_warning(false, AREA);
    assert!(!t.active(), "a seek back stops it");
}

/// In 16 colours the wash snaps to the warning colour at the height of a pulse and back; with less
/// motion, without colours, or for a test's app, there's none.
#[test]
fn the_wash_snaps_in_16_colours_and_needs_motion() {
    let mut t = on();
    theme::set_test_tier(Some(Tier::Colors16));
    t.follow_warning(true, AREA);
    let named = || {
        let mut buf = Buffer::empty(AREA);
        buf.content.iter_mut().for_each(|c| {
            c.set_symbol("▆").set_fg(Color::Blue);
        });
        buf
    };
    let mut early = named();
    t.step_wash(Duration::from_millis(40), &mut early);
    assert_eq!(early, named(), "not yet");
    let mut peak = named();
    t.step_wash(Duration::from_millis(140), &mut peak);
    assert!(peak.content.iter().all(|c| c.fg == theme::rgb3(theme::current().critical)), "{peak:?}");
    theme::set_test_tier(None);

    let starts = |t: &mut Transitions| {
        t.follow_warning(false, AREA);
        t.follow_warning(true, AREA);
        t.active()
    };
    theme::set_test_reduced_motion(true);
    assert!(!starts(&mut on()), "less motion");
    theme::set_test_reduced_motion(false);
    assert!(!starts(&mut Transitions::default()), "a test's app");
}

/// For any run of frames: a wash starts only as the warning comes on (with motion), stops when it
/// goes off or two pulses on, and only ever changes colours in the overview, never a symbol.
#[hegel::test(test_cases = 100)]
fn prop_the_end_warning_washes_from_each_crossing(tc: hegel::TestCase) {
    let less_motion = tc.draw(gs::booleans());
    theme::set_test_reduced_motion(less_motion);
    let mut t = on();
    let page = Rect { width: AREA.width + 4, height: AREA.height + 2, ..AREA };
    let lit = || {
        let mut buf = Buffer::empty(page);
        for (i, cell) in buf.content.iter_mut().enumerate() {
            cell.set_symbol(if i % 2 == 0 { "▆" } else { "a" }).set_fg(Color::Rgb(40, 120, 220));
        }
        buf
    };
    let (mut warned, mut ran): (bool, Option<Duration>) = (false, None);
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(14)) {
        if tc.draw(gs::booleans()) {
            let warning = tc.draw(gs::booleans());
            t.follow_warning(warning, AREA);
            if !warning {
                ran = None;
            } else if !warned && !less_motion {
                ran = Some(Duration::ZERO);
            }
            warned = warning;
        } else {
            let by = Duration::from_millis(tc.draw(gs::integers::<u64>().max_value(300)));
            let mut buf = lit();
            t.step_wash(by, &mut buf);
            ran = ran.map(|r| r + by).filter(|&r| r < 2 * PULSE);
            for (i, (cell, before)) in buf.content.iter().zip(&lit().content).enumerate() {
                let (x, y) = buf.pos_of(i);
                assert_eq!(cell.symbol(), before.symbol(), "({x}, {y})");
                let inside = AREA.contains(ratatui::layout::Position::new(x, y));
                assert!(inside || cell == before, "({x}, {y}) is outside the overview");
            }
        }
        assert_eq!(t.active(), ran.is_some(), "less motion {less_motion}");
    }
    theme::set_test_reduced_motion(false);
    theme::set_test_tier(None);
}
