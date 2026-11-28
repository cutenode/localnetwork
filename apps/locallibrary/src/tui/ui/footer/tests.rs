use hegel::generators as gs;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;

use super::*;
use crate::tui::app::testing::{app_with, blank, job, key, library, press};

/// The footer `width` wide, as its two rows of text.
fn rows(app: &mut App, width: u16) -> (String, String) {
    let footer = Footer::of(app, width);
    let area = Rect::new(0, 0, width, 2);
    let mut buf = Buffer::empty(area);
    footer.render(area, &mut buf);
    let row = |y| (0..width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>();
    (row(0), row(1))
}

/// In every view and pane, at 80 columns too, the keys row ends with the commands and the help,
/// says no BROWSE, and quitting isn't on it.
#[hegel::test(test_cases = 100)]
fn prop_the_commands_and_the_help_are_always_there(tc: hegel::TestCase) {
    let (mut app, _dir) = app_with(library());
    app.set_view(tc.draw(gs::sampled_from(&View::ALL[..])));
    app.focus =
        tc.draw(gs::sampled_from(&[Focus::Drives, Focus::Playlists, Focus::Tracks, Focus::Details, Focus::Crate][..]));
    app.deck_layout = tc.draw(gs::booleans());
    let width = tc.draw(gs::integers::<u16>().min_value(40).max_value(200));
    let (status, keys) = rows(&mut app, width);
    assert!(keys.trim_end().ends_with(": commands  ? help"), "{keys:?} at {width}");
    assert!(!status.contains("BROWSE") && !keys.contains("BROWSE"), "{status:?} {keys:?}");
    assert!(!keys.contains("quit"), "{keys:?}");
}

/// The keys row starts with what enter does, then the actions, in every view.
#[test]
fn enter_then_the_actions_lead_the_keys() {
    let (mut app, _dir) = app_with(library());
    for view in View::ALL {
        app.set_view(view);
        assert!(matches!(Footer::of(&mut app, 200).keys, Keys::Hints(_)), "{view:?}: no hints");
        let hints = keymap::hints(&app, 200);
        let first: Vec<&str> = hints.iter().take(2).map(|h| h.keys.as_str()).collect();
        assert_eq!(first, ["enter", "e"], "{view:?}: {hints:?}");
    }
}

/// A chip leads the keys row only when keys mean something else, as while searching; no key
/// waits for another, so nothing else puts one there.
#[test]
fn a_chip_says_when_keys_mean_something_else() {
    let (mut app, _dir) = app_with(library());
    assert_eq!(Footer::of(&mut app, 120).mode, None);
    press(&mut app, "q");
    assert_eq!(Footer::of(&mut app, 120).mode, None, "q plays next, at once");
    press(&mut app, "/");
    assert_eq!(Footer::of(&mut app, 120).mode, Some("SEARCH"));
}

/// After one esc with nothing open, the keys row says a second quits.
#[test]
fn one_esc_says_a_second_quits() {
    let (mut app, _dir) = app_with(library());
    key(&mut app, KeyCode::Esc);
    let (_, keys) = rows(&mut app, 80);
    assert_eq!(keys.trim(), "esc again to quit");
}

/// The status row: what's under way on the left with how to stop it, where things are on the
/// right, the space left only where a drive's in view, and the room between them for a message.
#[test]
fn the_status_row_says_what_and_where() {
    let (mut app, _dir) = app_with(library());
    app.job = Some(job(false, false));
    let (status, _) = rows(&mut app, 120);
    assert!(status.starts_with(" ") && status.contains("esc cancel · & jobs"), "{status}");
    let footer = Footer::of(&mut app, 120);
    let room = footer.room(Rect::new(0, 0, 120, 2));
    let doing: usize = footer.doing.iter().map(Span::width).sum();
    assert_eq!(room.x as usize, 1 + doing);
    assert!(room.right() as usize <= 120 - footer.place.iter().map(Span::width).sum::<usize>());
    app.job = None;
    app.set_view(View::Profile);
    let (status, _) = rows(&mut app, 120);
    assert!(!status.contains("free"), "{status}");
    let (mut none, _store) = blank(View::Library);
    let (status, _) = rows(&mut none, 120);
    assert!(status.trim().is_empty(), "no list without a drive: {status:?}");
}

/// A hint's click area is where its keys and label are drawn, and it does what it says.
#[hegel::test(test_cases = 50)]
fn prop_a_hint_is_clicked_where_it_is_drawn(tc: hegel::TestCase) {
    let (mut app, _dir) = app_with(library());
    app.set_view(tc.draw(gs::sampled_from(&View::ALL[..])));
    let width = tc.draw(gs::integers::<u16>().min_value(20).max_value(200));
    let footer = Footer::of(&mut app, width);
    let (_, keys) = rows(&mut app, width);
    let hints = keymap::hints(&app, width as usize);
    for (r, act) in footer.hint_areas(Rect::new(0, 0, width, 2)) {
        assert_eq!(r.y, 1);
        let h = hints.iter().find(|h| h.act == act).unwrap();
        let under: String = keys.chars().skip(r.x as usize).take(r.width as usize).collect();
        assert_eq!(under, format!("{} {}", h.keys, h.label), "{act:?}");
    }
}

/// At 80 columns every view's keys row is short: a handful of keys, then actions, commands and
/// help, with nothing cut.
#[test]
fn at_80_the_keys_row_is_a_handful() {
    let (mut app, _dir) = app_with(library());
    for view in View::ALL {
        app.set_view(view);
        assert!(matches!(Footer::of(&mut app, 80).keys, Keys::Hints(_)), "{view:?}: no hints");
        let hints = keymap::hints(&app, 80);
        assert!(hints.len() <= 8, "{view:?}: {} keys", hints.len());
        let (_, keys) = rows(&mut app, 80);
        assert!(keys.trim_end().ends_with(": commands  ? help"), "{view:?}: {keys:?}");
        assert!(!keys.contains('…'), "{view:?} cut: {keys:?}");
    }
}

/// With no hints the footer still says `? help`, faintly, unless that's turned off too.
#[test]
fn with_no_hints_the_help_stays_unless_turned_off() {
    let (mut app, _dir) = app_with(library());
    app.perform(Act::Hints(Hints::Off), None);
    let (_, keys) = rows(&mut app, 80);
    assert_eq!(keys.trim(), "? help");
    app.perform(Act::HelpHint(false), None);
    let (_, keys) = rows(&mut app, 80);
    assert_eq!(keys.trim(), "");
}
