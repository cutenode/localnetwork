use hegel::generators as gs;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;

use super::*;
use crate::tui::app::testing::{app_with, blank, library};

/// The header `width` wide, as text, and the buffer it's drawn in.
fn drawn(header: &Header, width: u16) -> (String, Buffer) {
    let area = Rect::new(0, 0, width, 1);
    let mut buf = Buffer::empty(area);
    header.render(area, &mut buf);
    let text = (0..width).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    (text, buf)
}

/// The tabs name the views, each with the digit that goes there, the open one picked out; the
/// player's name is left to the terminal's title.
#[test]
fn the_tabs_name_the_views_with_their_digits() {
    let (app, _dir) = app_with(library());
    for width in [80, 160] {
        let (text, buf) = drawn(&Header::of(&app), width);
        for v in View::ALL {
            assert!(text.contains(&format!("{} {}", keymap::view_key(v), v.title())), "{v:?} at {width}: {text}");
        }
        assert!(!text.contains("locallibrary"), "{text}");
        let at = Header::of(&app).tab_areas(Rect::new(0, 0, width, 1));
        let (_, open) = at.iter().find(|(v, _)| *v == View::Library).unwrap();
        assert!(buf[(open.x + 2, 0)].modifier.contains(Modifier::BOLD), "the open tab stands out");
    }
}

/// Each view says what it's about: the library its drive and list, the search and the marks
/// (which stay at 80 columns, the list's name giving way), Prepare its drive, localnetwork who's
/// signed in.
#[test]
fn each_view_says_what_its_about() {
    let (mut app, _dir) = app_with(library());
    let (text, _) = drawn(&Header::of(&app), 160);
    assert!(text.contains("›") && text.contains("All tracks"), "{text}");
    app.query = "flo".into();
    app.marks.insert("/Contents/1.mp3".into());
    let (text, _) = drawn(&Header::of(&app), 80);
    assert!(text.contains("/ flo") && text.contains("1 marked"), "{text}");
    assert!(!text.contains("All tracks"), "the list's name gave way: {text}");
    app.set_view(View::Prepare);
    assert!(drawn(&Header::of(&app), 120).0.contains("on "));
    app.set_view(View::Profile);
    assert!(drawn(&Header::of(&app), 120).0.contains("not signed in"));
    let (mut none, _store) = blank(View::Prepare);
    assert!(drawn(&Header::of(&none), 120).0.contains("no drive open"));
    none.view = View::Library;
    assert!(Header::of(&none).context.is_empty(), "nothing until a drive is open");
}

/// At any width the header keeps to its row, and where a tab is said to be (for a click) is
/// where its name is drawn.
#[hegel::test(test_cases = 100)]
fn prop_a_tabs_click_is_where_its_name_is(tc: hegel::TestCase) {
    let (mut app, _dir) = app_with(library());
    app.view = tc.draw(gs::sampled_from(&View::ALL[..]));
    let width = tc.draw(gs::integers::<u16>().max_value(240));
    let header = Header::of(&app);
    let (text, _) = drawn(&header, width);
    assert_eq!(text.chars().count(), width as usize);
    for (v, r) in header.tab_areas(Rect::new(0, 0, width, 1)) {
        assert!(r.right() <= width, "{v:?} at {r:?} past {width}");
        let under: String = text.chars().skip(r.x as usize).take(r.width as usize).collect();
        assert!(under.contains(v.title()), "{v:?} at {r:?}: {under:?} in {text:?}");
    }
}

/// The context cut to its room: the list's name first, then from the end.
#[test]
fn the_context_gives_way_from_the_list_name() {
    let context = vec![
        Span::styled("DEMO", bold(theme::current().text)),
        Span::styled(" › ", faint()),
        Span::styled("A long playlist name", dim()),
        Span::styled("  2 marked", bold(theme::current().accent)),
    ];
    let w = |s: &[Span]| s.iter().map(Span::width).sum::<usize>();
    let all = w(&context);
    let fit = |room| fitted(&context, Some(2), room);
    assert_eq!(fit(all), context);
    let cut = fit(all - 10);
    assert_eq!(w(&cut), all - 10);
    assert!(cut[3].content.contains("marked") && cut[2].content.ends_with('…'));
    // Shorter still, the list goes, then the drive's name gives way, before the marks.
    let cut = fit(w(&context[..1]) + w(&context[3..]));
    assert_eq!(cut, [context[0].clone(), context[3].clone()]);
    let cut = fit(3 + w(&context[3..]));
    assert!(cut.last().is_some_and(|s| s.content.contains("marked")), "{cut:?}");
    assert!(cut[0].content.ends_with('…'));
    assert!(w(&fit(5)) <= 5);
    assert!(fit(0).is_empty());
}

/// With no list's name, nothing goes for being dim: on a 16-colour terminal every dim span looks
/// the same, and Drives' space left is dim. The drive's name is cut, and the space stays.
#[test]
fn only_the_list_gives_way_first() {
    let context = vec![
        Span::styled("A DRIVE WITH A VERY LONG NAME", bold(theme::current().text)),
        Span::styled(" · 12.0 GB free", dim()),
    ];
    let room = 24;
    let cut = fitted(&context, None, room);
    assert!(cut[0].content.ends_with('…') && cut[1].content.contains("free"), "{cut:?}");
}
