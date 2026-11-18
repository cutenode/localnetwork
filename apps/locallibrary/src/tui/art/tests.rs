use std::collections::HashMap;

use hegel::generators as gs;
use loclib_waveform::Art;

use super::super::app::testing::{on_deck_art, playing, png, wait};
use super::super::theme::{self, Tier};
use super::*;

fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let vars: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    move |k| vars.get(k).cloned()
}

/// What's known of a terminal says how pictures reach it, and half blocks are the floor.
#[test]
fn a_terminals_graphics_are_worked_out_from_what_it_says() {
    let of = |vars: &[(&str, &str)], sixel| protocol(env(vars), sixel, false);
    assert_eq!(of(&[], false), None);
    assert_eq!(of(&[], true), Some(ProtocolType::Sixel));
    assert_eq!(of(&[("KITTY_WINDOW_ID", "1")], false), Some(ProtocolType::Kitty));
    assert_eq!(of(&[("TERM", "xterm-kitty")], false), Some(ProtocolType::Kitty));
    assert_eq!(of(&[("TERM_PROGRAM", "ghostty")], false), Some(ProtocolType::Kitty));
    assert_eq!(of(&[("TERM_PROGRAM", "iTerm.app")], false), Some(ProtocolType::Iterm2));
    assert_eq!(of(&[("TERM_PROGRAM", "WezTerm")], true), Some(ProtocolType::Iterm2));
    assert_eq!(of(&[("TERM_PROGRAM", "ghostty"), ("TMUX", "/tmp/1")], true), None, "a multiplexer's in the way");
    assert_eq!(of(&[("STY", "1.x")], true), None);
    assert_eq!(of(&[("LOCALLIBRARY_ART", "sixel"), ("TMUX", "/tmp/1")], false), Some(ProtocolType::Sixel), "as told");
    assert_eq!(of(&[("LOCALLIBRARY_ART", "halfblocks"), ("KITTY_WINDOW_ID", "1")], true), None);
    // The setting for half blocks: whatever the terminal says, unless the variable names graphics.
    let blocks = |vars: &[(&str, &str)], sixel| protocol(env(vars), sixel, true);
    assert_eq!(blocks(&[("TERM_PROGRAM", "ghostty")], true), None);
    assert_eq!(blocks(&[("KITTY_WINDOW_ID", "1")], true), None);
    assert_eq!(blocks(&[], true), None);
    assert_eq!(blocks(&[("LOCALLIBRARY_ART", "kitty")], false), Some(ProtocolType::Kitty), "the variable wins");
}

/// The picture for a terminal with graphics is theirs, and it's half blocks without.
#[test]
fn a_cover_is_drawn_in_what_the_terminal_has() {
    let art = Art::decode(&png([200, 40, 90])).unwrap();
    assert!(matches!(picture(&art, (8, 4), None), Picture::Pixels(px) if px.len() == 8 * 8));
    #[expect(deprecated)]
    let mut picker = Picker::from_fontsize((10, 20).into());
    picker.set_protocol_type(ProtocolType::Kitty);
    assert!(matches!(picture(&art, (8, 4), Some(&picker)), Picture::Graphics(p) if matches!(*p, Protocol::Kitty(_))));
}

/// A frame only asks: the cover's made on a worker, for the size asked for, once, and the deck
/// draws it when it's back. A track with no cover has a record.
#[test]
fn a_cover_is_made_off_the_frame() {
    let (mut app, _dir) = on_deck_art();
    assert!(app.cover((8, 4)).is_none(), "not on this frame");
    wait(&mut app, |a| a.covers.cover.is_some());
    assert!(app.cover((8, 4)).is_some());
    assert!(app.cover((10, 5)).is_none(), "another size is made in its turn");
    wait(&mut app, |a| a.covers.cover.as_ref().is_some_and(|c| c.size == (10, 5)));
    let (mut plain, _dir) = playing(Default::default());
    assert!(plain.cover((8, 4)).is_none());
}

/// Art needs colours and more than ASCII, and can be turned off.
#[test]
fn art_gives_way() {
    let (mut app, _dir) = on_deck_art();
    theme::set_test_tier(Some(Tier::Truecolor));
    assert!(app.shows_art());
    app.deck.art = false;
    assert!(!app.shows_art());
    app.deck.art = true;
    theme::set_test_tier(Some(Tier::Colors16));
    assert!(!app.shows_art());
    theme::set_test_tier(None);
}

/// For any terminal: what's asked for by name wins, the setting for half blocks or a multiplexer
/// gets half blocks, then the terminal's own name says, then kitty's variables, and sixel only
/// where it listed them.
#[hegel::test(test_cases = 300)]
fn prop_a_terminal_gets_the_graphics_it_says_it_has(tc: hegel::TestCase) {
    let pick = |tc: &hegel::TestCase, values: Vec<&'static str>| tc.draw(gs::optional(gs::sampled_from(values)));
    let named = pick(&tc, vec!["kitty", " iterm2 ", "sixel", "halfblocks", "ascii"]);
    let program = pick(&tc, vec!["ghostty", "iTerm.app", "WezTerm", "Apple_Terminal"]);
    let term = pick(&tc, vec!["xterm-kitty", "xterm-256color"]);
    let mut vars = vec![];
    vars.extend(named.map(|v| ("LOCALLIBRARY_ART", v)));
    vars.extend(program.map(|v| ("TERM_PROGRAM", v)));
    vars.extend(term.map(|v| ("TERM", v)));
    for var in ["TMUX", "STY", "ZELLIJ", "KITTY_WINDOW_ID"] {
        if tc.draw(gs::booleans()) {
            vars.push((var, "1"));
        }
    }
    let (sixel, halfblocks) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let has = |k: &str| vars.iter().any(|(var, _)| *var == k);
    let multiplexed = has("TMUX") || has("STY") || has("ZELLIJ");
    let want = match (named.map(str::trim), program) {
        (Some("kitty"), _) => Some(ProtocolType::Kitty),
        (Some("iterm2"), _) => Some(ProtocolType::Iterm2),
        (Some("sixel"), _) => Some(ProtocolType::Sixel),
        (Some("halfblocks"), _) => None,
        _ if halfblocks || multiplexed => None,
        (_, Some("ghostty")) => Some(ProtocolType::Kitty),
        (_, Some("iTerm.app" | "WezTerm")) => Some(ProtocolType::Iterm2),
        _ if has("KITTY_WINDOW_ID") || term == Some("xterm-kitty") => Some(ProtocolType::Kitty),
        _ => sixel.then_some(ProtocolType::Sixel),
    };
    assert_eq!(protocol(env(&vars), sixel, halfblocks), want, "{vars:?}, sixel {sixel}, half blocks {halfblocks}");
}

/// A cover's made for any size it's asked for, once, and it's only drawn at that size: half
/// blocks for every cell, two pixels high.
#[hegel::test(test_cases = 20)]
fn prop_a_cover_is_drawn_at_the_size_asked(tc: hegel::TestCase) {
    let (mut app, _dir) = on_deck_art();
    let side = |most| gs::integers::<u16>().min_value(1).max_value(most);
    let mut sizes = tc.draw(gs::vecs(gs::tuples!(side(40), side(20))).min_size(1).max_size(3));
    sizes.dedup();
    let track = app.now.as_ref().unwrap().id;
    for size in sizes {
        let made = app.covers.cover.as_ref().map(|c| c.size);
        assert_eq!(app.cover(size).is_some(), made == Some(size), "{size:?} only once it's made");
        assert_eq!(app.covers.asked, Some((track, size)));
        wait(&mut app, |a| a.covers.cover.as_ref().is_some_and(|c| c.size == size));
        match app.cover(size) {
            Some(Picture::Pixels(px)) => {
                assert_eq!(px.len(), usize::from(size.0) * usize::from(size.1) * 2, "{size:?}")
            }
            Some(Picture::Graphics(_)) => {}
            None => panic!("made at {size:?}, so drawn"),
        }
    }
}

/// Choosing half blocks in the palette, or the terminal's graphics again, makes the cover again
/// with the choice, and the choice is kept for the next run.
#[hegel::test(test_cases = 10)]
fn prop_choosing_half_blocks_makes_the_cover_again_and_is_kept(tc: hegel::TestCase) {
    let (mut app, _dir) = on_deck_art();
    let size = (8, 4);
    app.cover(size);
    wait(&mut app, |a| a.covers.cover.is_some());
    let on = tc.draw(gs::booleans());
    app.perform(super::super::keymap::Act::ArtHalfblocks(on), None);
    assert!(app.covers.cover.is_none() && app.covers.picker.is_none(), "made again");
    app.cover(size);
    wait(&mut app, |a| a.covers.cover.is_some());
    assert_eq!(app.saved_state().art_halfblocks, on);
    let mut next = App::blank(app.store_root.clone(), None);
    next.apply_saved(&app.saved_state(), false);
    assert_eq!(next.deck.art_halfblocks, on);
}

/// A `w` x `h` picture whose every pixel is its own colour.
fn checks(w: u32, h: u32) -> RgbImage {
    RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 7) as u8, (y * 7) as u8, ((x + y) % 2 * 200) as u8]))
}

fn encoded(image: &RgbImage) -> Vec<u8> {
    let mut png = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    png
}

/// A small cover fills its space one way or the other in the terminal's own graphics: it's grown
/// to it, not drawn at its own size in a corner.
#[test]
fn a_small_cover_fills_its_space() {
    #[expect(deprecated)]
    let mut picker = Picker::from_fontsize((10, 20).into());
    picker.set_protocol_type(ProtocolType::Kitty);
    for (w, h, size) in [(8, 8, (18, 9)), (7, 7, (18, 9)), (6, 4, (30, 9)), (3, 7, (40, 6)), (100, 100, (18, 9))] {
        let art = Art::decode(&encoded(&checks(w, h))).unwrap();
        let Picture::Graphics(p) = picture(&art, size, Some(&picker)) else { panic!("graphics") };
        let got = p.size();
        assert!(got.width <= size.0 && got.height <= size.1, "{w}x{h} in {size:?}: {got:?} is too big");
        assert!(got.width == size.0 || got.height == size.1, "{w}x{h} in {size:?}: {got:?} doesn't fill it");
    }
}

/// Grown a whole number of times, each of the cover's pixels is a square of identical screen
/// pixels, none blended with its neighbours, and what's left to the edge of a cell is see-through.
#[test]
fn a_small_cover_grows_in_whole_pixels() {
    let image = checks(6, 6);
    let (grown, resize) = fitted(&image, (10, 20), (18, 9));
    assert!(matches!(resize, Resize::Fit(None)), "drawn as it is");
    let grown = grown.to_rgba8();
    assert_eq!(grown.dimensions(), (180, 180), "30 times, 18 cells by 9");
    for (x, y, px) in grown.enumerate_pixels() {
        let [r, g, b] = image.get_pixel(x / 30, y / 30).0;
        assert_eq!(px.0, [r, g, b, 255], "({x}, {y})");
    }
    let (padded, _) = fitted(&checks(7, 7), (10, 20), (18, 9));
    let padded = padded.to_rgba8();
    assert_eq!(padded.dimensions(), (180, 180), "175 pixels, out to the cell");
    assert_eq!(padded.get_pixel(174, 174).0[3], 255);
    assert_eq!(padded.get_pixel(179, 179).0[3], 0);
}

/// Where no whole number of times fills it, a cover still grows to fill its space, each screen
/// pixel the nearest of its own; and a big one shrinks smoothly.
#[test]
fn a_cover_is_grown_or_shrunk_to_suit() {
    let image = checks(100, 100);
    let (_, resize) = fitted(&image, (10, 20), (18, 9));
    assert!(matches!(resize, Resize::Scale(Some(FilterType::Nearest))), "1.8 times");
    let font = ratatui_image::FontSize::new(10, 20);
    let grown = resize.resize(&DynamicImage::ImageRgb8(image.clone()), font, Size::new(18, 9), None).to_rgb8();
    let colours: std::collections::HashSet<_> = image.pixels().map(|p| p.0).collect();
    assert!(grown.pixels().all(|p| colours.contains(&p.0)), "every pixel is one of the cover's");
    let (_, resize) = fitted(&checks(600, 600), (10, 20), (18, 9));
    assert!(matches!(resize, Resize::Fit(Some(FilterType::Triangle))));
}

/// In half blocks a small cover grows the same way, its pixels never blended.
#[test]
fn a_small_cover_in_half_blocks_is_crisp() {
    let image = checks(4, 4);
    let art = Art::decode(&encoded(&image)).unwrap();
    let px = art.resized(12, 6);
    assert_eq!(px.len(), 12 * 12);
    for (i, p) in px.iter().enumerate() {
        let (x, y) = (i as u32 % 12, i as u32 / 12);
        assert_eq!(*p, Some(image.get_pixel(x / 3, y / 3).0), "({x}, {y})");
    }
}
