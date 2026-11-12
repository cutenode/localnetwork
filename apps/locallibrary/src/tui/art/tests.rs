use std::collections::HashMap;

use hegel::generators as gs;
use loclib_waveform::Art;

use super::super::app::testing::{app_with, library, on_deck_art, playing, png, wait};
use super::super::theme::{self, Tier};
use super::*;

fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let vars: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    move |k| vars.get(k).cloned()
}

/// What's known of a terminal says how pictures reach it, and half blocks are the floor.
#[test]
fn a_terminals_graphics_are_worked_out_from_what_it_says() {
    let of = |vars: &[(&str, &str)], sixel| protocol(env(vars), sixel);
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

/// Chips come for the rows asked about, a few at a time, and the frame never waits for one. A
/// library's size doesn't matter: what's not asked about is never read.
#[test]
fn chips_are_read_for_rows_on_screen() {
    let dir = tempfile::tempdir().unwrap();
    let cover = dir.path().join("PIONEER/Artwork/1.png");
    std::fs::create_dir_all(cover.parent().unwrap()).unwrap();
    std::fs::write(&cover, png([20, 200, 90])).unwrap();
    let (mut app, _keep) = app_with(library());
    let root = dir.path();
    let asked = |app: &mut App| app.covers.chip(root, 1, "/PIONEER/Artwork/1.png");
    assert_eq!(asked(&mut app), None, "not read yet");
    assert_eq!(asked(&mut app), None);
    assert_eq!(app.covers.wanted.len(), 2);
    app.covers.new_frame();
    assert!(!app.covers.busy(), "scrolled off before it was read");
    app.want_chips();
    assert!(app.covers.chips.is_empty(), "so it never is");

    asked(&mut app);
    asked(&mut app);
    app.want_chips();
    assert_eq!(app.covers.reading, 1, "asked twice, read once");
    assert_eq!(app.covers.chip(root, 1, "/x"), None, "being read");
    app.covers.new_frame();
    wait(&mut app, |a| a.covers.reading == 0);
    assert_eq!(app.covers.chip(root, 1, "/PIONEER/Artwork/1.png"), Some([[20, 200, 90]; 2]));
    assert_eq!(app.covers.chip(Path::new("/elsewhere"), 1, "/x"), None, "ids belong to a drive");
}

/// A chip read for a drive that's since been left isn't kept for the drive now open: the same id
/// there is another cover.
#[test]
fn a_chip_from_another_drive_is_dropped() {
    let (mut app, _dir) = app_with(library());
    let (here, there) = (Path::new("/here"), Path::new("/there"));
    app.covers.chip(here, 7, "/a.png");
    app.want_chips();
    app.covers.chip(there, 7, "/b.png");
    app.chip_read(here, 7, Some([[1, 2, 3]; 2]));
    assert_eq!(app.covers.chip(there, 7, "/b.png"), None, "not this drive's");
    assert_eq!(app.covers.reading, 0, "and it's no longer being read");
}

/// However many rows want one, only a few covers are read at once, and what's kept has a limit.
#[test]
fn chips_are_bounded() {
    let (mut app, _dir) = app_with(library());
    let root = Path::new("/nowhere");
    for id in 0..40 {
        app.covers.chip(root, id, "/none.png");
    }
    app.want_chips();
    assert_eq!(app.covers.reading, CHIPS_AT_ONCE);
    wait(&mut app, |a| a.covers.reading == 0);
    app.covers.new_frame();
    app.covers.chips.extend((0..CHIPS_KEPT as u32).map(|id| (id, None)));
    app.want_chips();
    assert!(app.covers.chips.len() < CHIPS_KEPT);
}

/// For any terminal: what's asked for by name wins, a multiplexer gets half blocks, then the
/// terminal's own name says, then kitty's variables, and sixel only where it listed them.
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
    let sixel = tc.draw(gs::booleans());
    let has = |k: &str| vars.iter().any(|(var, _)| *var == k);
    let multiplexed = has("TMUX") || has("STY") || has("ZELLIJ");
    let want = match (named.map(str::trim), program) {
        (Some("kitty"), _) => Some(ProtocolType::Kitty),
        (Some("iterm2"), _) => Some(ProtocolType::Iterm2),
        (Some("sixel"), _) => Some(ProtocolType::Sixel),
        (Some("halfblocks"), _) => None,
        _ if multiplexed => None,
        (_, Some("ghostty")) => Some(ProtocolType::Kitty),
        (_, Some("iTerm.app" | "WezTerm")) => Some(ProtocolType::Iterm2),
        _ if has("KITTY_WINDOW_ID") || term == Some("xterm-kitty") => Some(ProtocolType::Kitty),
        _ => sixel.then_some(ProtocolType::Sixel),
    };
    assert_eq!(protocol(env(&vars), sixel), want, "{vars:?}, sixel {sixel}");
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

/// For any rows scrolling by over two drives: a chip is only ever the colours of the cover on the
/// drive open, no more than a few are read at once, and every cover a row still shows is read in
/// the end, while one that's missing stays without a chip.
#[hegel::test(test_cases = 40)]
fn prop_chips_are_a_drives_own_covers(tc: hegel::TestCase) {
    const IDS: u32 = 6;
    let colour = |id: u32| [id as u8 * 40, 200, 90];
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("PIONEER/Artwork")).unwrap();
    let has: Vec<bool> = (0..IDS).map(|_| tc.draw(gs::booleans())).collect();
    for id in (0..IDS).filter(|&id| has[id as usize]) {
        std::fs::write(dir.path().join(format!("PIONEER/Artwork/{id}.png")), png(colour(id))).unwrap();
    }
    let (drive, elsewhere) = (dir.path(), Path::new("/nowhere"));
    let path = |id: u32| format!("/PIONEER/Artwork/{id}.png");
    let (mut app, _keep) = app_with(library());
    let steps = tc.draw(gs::integers::<usize>().max_value(20));
    for _ in 0..steps {
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 | 1 => {
                let id = tc.draw(gs::integers::<u32>().max_value(IDS - 1));
                let root = if tc.draw(gs::weighted_booleans(0.8)) { drive } else { elsewhere };
                let chip = app.covers.chip(root, id, &path(id));
                assert!(chip.is_none() || (root == drive && has[id as usize]), "{id} on {root:?}: {chip:?}");
                assert!(chip.is_none_or(|c| c == [colour(id); 2]), "{id}: {chip:?}");
            }
            2 => app.covers.new_frame(),
            3 => app.want_chips(),
            _ => wait(&mut app, |a| a.covers.reading == 0),
        }
        assert!(app.covers.reading <= CHIPS_AT_ONCE, "{} at once", app.covers.reading);
    }
    wait(&mut app, |a| a.covers.reading == 0);
    for id in 0..IDS {
        let mut chip = app.covers.chip(drive, id, &path(id));
        for _ in 0..3 {
            if chip.is_some() {
                break;
            }
            app.want_chips();
            wait(&mut app, |a| a.covers.reading == 0);
            chip = app.covers.chip(drive, id, &path(id));
        }
        assert_eq!(chip, has[id as usize].then_some([colour(id); 2]), "{id}");
    }
}
