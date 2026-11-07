//! Real screens drawn at each colour tier, cell by cell: what a terminal of that tier is sent has to
//! be something it can draw, and the screens have to hold up without the colours they've lost.

use locallibrary::anlz::TrackAnalysis;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use super::super::app::testing::{cue, grid, key, playing, wave};
use super::super::app::{App, View};
use super::super::screens::{Fixture, fixture};
use super::super::terminal::Colours;
use super::super::ui;
use super::*;
use ratatui::crossterm::event::KeyCode;

const SIZES: [(u16, u16); 3] = [(80, 24), (120, 40), (160, 45)];
const MODES: [Mode; 2] = [Mode::Dark, Mode::Light];

fn drawn(app: &mut App, (w, h): (u16, u16)) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    terminal.backend().buffer().clone()
}

/// Whether a terminal of `tier` can draw `c`.
fn in_range(c: Color, tier: Tier) -> bool {
    match (tier, c) {
        (_, Color::Reset) => true,
        (Tier::Truecolor, Color::Rgb(..)) => true,
        (Tier::Colors256, Color::Indexed(i)) => i >= 16,
        (Tier::Colors16, Color::Indexed(i)) => i < 16,
        _ => false,
    }
}

/// A track with everything drawn for it, playing.
fn playing_app() -> (App, tempfile::TempDir) {
    let analysis = TrackAnalysis {
        beats: grid(0, 128.0, 400),
        cues: vec![cue(Some(0), 4_000, None), cue(Some(1), 30_000, Some(34_000)), cue(None, 60_000, None)],
        preview: Some(wave(400)),
        detail: Some(wave(4_000)),
        ..Default::default()
    };
    playing(analysis)
}

fn check(app: &mut App, what: &str) {
    for tier in Tier::ALL {
        set_test_tier(Some(tier));
        for mode in MODES {
            set_test_mode(Some(mode));
            for size in SIZES {
                let buf = drawn(app, size);
                let (mut foreground, mut background) = (0, 0);
                for (i, cell) in buf.content.iter().enumerate() {
                    let at = (i % usize::from(size.0), i / usize::from(size.0));
                    assert!(in_range(cell.fg, tier), "{what} {tier:?} {mode:?} {size:?}: {:?} at {at:?}", cell.fg);
                    assert!(in_range(cell.bg, tier), "{what} {tier:?} {mode:?} {size:?}: {:?} at {at:?}", cell.bg);
                    foreground += usize::from(cell.fg != Color::Reset);
                    background += usize::from(cell.bg != Color::Reset);
                }
                assert!(foreground > 0, "{what} {tier:?} {mode:?}: not a colour on it");
                if tier != Tier::Colors16 && what != "help" {
                    assert!(background > 0, "{what} {tier:?} {mode:?}: nothing behind anything");
                }
            }
        }
    }
    set_test_tier(None);
    set_test_mode(None);
}

#[test]
fn every_view_stays_in_its_tiers_colours() {
    let Fixture { _dir, mut app, .. } = fixture();
    check(&mut app, "library");
    for view in [View::Prepare, View::Drives, View::Mixes, View::Profile] {
        app.view = view;
        check(&mut app, view.title());
    }
    app.view = View::Library;
    key(&mut app, KeyCode::Char('?'));
    check(&mut app, "help");
}

#[test]
fn a_playing_track_stays_in_its_tiers_colours() {
    let (mut app, _dir) = playing_app();
    check(&mut app, "playing");
    app.fullscreen = true;
    check(&mut app, "the deck");
}

/// 16 colours put nothing behind the selection or a chip: they're reverse video, and an idle pane's
/// selection and a marked row are left to their symbols.
#[test]
fn sixteen_colours_say_it_with_reverse_video() {
    let Fixture { _dir, mut app, .. } = fixture();
    for mode in MODES {
        set_test_tier(Some(Tier::Colors16));
        set_test_mode(Some(mode));
        let buf = drawn(&mut app, (120, 40));
        let reversed = buf.content.iter().filter(|c| c.modifier.contains(Modifier::REVERSED)).count();
        assert!(reversed > 20, "the selected row and the active tab: {reversed} cells");
        assert_eq!(selected().bg, None);
        assert_eq!(behind(current().marked), Style::new());
        assert_eq!(chip(current().info).bg, None);
        assert!(chip(current().info).add_modifier.contains(Modifier::REVERSED));
        assert_eq!(dim().fg, None, "quieter text is the terminal's own, dim");
        assert!(faint().add_modifier.contains(Modifier::DIM));
        assert_eq!(border_type(true), BorderType::Thick, "the focused pane");
        assert_eq!(colour_dot(TrackColor::Aqua).content, "6", "track colours by number");
    }
    set_test_tier(None);
    set_test_mode(None);
}

/// Any tier and mode, on any terminal's colours: a screen stays in the tier's range, and reverse
/// video stands for what 16 colours can't put behind a row.
#[hegel::test(test_cases = 12)]
fn prop_any_tier_mode_and_background_draw_in_range(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let tier = tc.draw(gs::sampled_from(&Tier::ALL[..]));
    let mode = tc.draw(gs::sampled_from(&MODES[..]));
    let channel = || tc.draw(gs::arrays::<_, _, 3>(gs::integers::<u8>()));
    let said = tc.draw(gs::booleans()).then(|| Colours { background: channel(), foreground: channel() });
    let (mut app, _dir) = playing_app();
    app.fullscreen = tc.draw(gs::booleans());
    set_test_tier(Some(tier));
    set_test_mode(Some(mode));
    set_test_terminal(said);
    let buf = drawn(&mut app, (120, 40));
    for cell in &buf.content {
        assert!(in_range(cell.fg, tier) && in_range(cell.bg, tier), "{tier:?}: {:?} on {:?}", cell.fg, cell.bg);
    }
    assert_eq!(selected().bg.is_none(), tier == Tier::Colors16, "{tier:?}");
    assert_eq!(chip(current().info).add_modifier.contains(Modifier::REVERSED), tier == Tier::Colors16);
    set_test_terminal(None);
    set_test_mode(None);
    set_test_tier(None);
}

/// A real background shows through what fades, and a chip's ink follows the paper.
#[test]
fn colours_fade_towards_the_real_background() {
    set_test_tier(Some(Tier::Truecolor));
    set_test_terminal(Some(Colours { background: [200, 100, 0], foreground: [0, 0, 0] }));
    assert_eq!(fade([0, 0, 0], 0.5), [100, 50, 0]);
    assert_eq!(palette().background, [200, 100, 0]);
    set_test_terminal(None);
    set_test_mode(Some(Mode::Light));
    assert_eq!(page(), [255, 255, 255]);
    assert_eq!(palette().background, [255, 255, 255]);
    assert_eq!(ink_on([20, 20, 20]), current().ink, "white on a deep colour");
    assert_eq!(ink_on([250, 250, 250]), current().text, "dark on a pale one");
    set_test_mode(Some(Mode::Dark));
    assert_eq!(page(), [9, 9, 11]);
    assert_eq!(ink_on([250, 250, 250]), current().ink);
    set_test_mode(None);
    set_test_tier(None);
}

/// A drive's colour isn't the theme's to choose: on a light terminal a yellow is taken down until it
/// can be seen, on an almost black one it's left alone, and with 16 colours the scheme decides.
#[test]
fn a_drives_colours_are_kept_readable() {
    let yellow = [240, 220, 40];
    set_test_tier(Some(Tier::Truecolor));
    set_test_mode(Some(Mode::Light));
    assert!(contrast(marker(yellow), page()) >= 3.0, "{:?}", marker(yellow));
    assert_eq!(marker([30, 30, 30]), [30, 30, 30], "reads as it is");
    set_test_mode(Some(Mode::Dark));
    assert_eq!(marker([20, 20, 60]), [20, 20, 60], "made for this background");
    set_test_terminal(Some(Colours { background: [120, 120, 120], foreground: [255, 255, 255] }));
    assert_ne!(marker([120, 130, 120]), [120, 130, 120], "a mid-grey terminal");
    set_test_terminal(None);
    set_test_tier(Some(Tier::Colors16));
    set_test_mode(Some(Mode::Light));
    assert_eq!(marker(yellow), yellow);
    set_test_mode(None);
    set_test_tier(None);
}
