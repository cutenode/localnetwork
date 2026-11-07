use std::collections::HashMap;

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
