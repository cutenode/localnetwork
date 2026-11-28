use std::path::PathBuf;
use std::time::Duration;

use hegel::generators as gs;
use locallibrary::Playlist;
use locallibrary::anlz::TrackAnalysis;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;
use ratatui::style::{Color, Modifier};

use crate::tui::app::testing::*;
use crate::tui::app::{App, DrivePane, Focus, Level, ReplayOffer, View};
use crate::tui::deck::Outputs;
use crate::tui::drives::{Report, Tone};
use crate::tui::edits::{Modal, Sub};
use crate::tui::keymap::{self, Act};
use crate::tui::theme::{self, Mode, Tier};
use crate::tui::ui::{draw, screen};

use super::*;

/// A frame as a terminal holds it.
fn drawn(app: &mut App, w: u16, h: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn rows(s: &str) -> Vec<&str> {
    s.lines().collect()
}

/// Those rows of a frame as text.
fn text_of(buf: &Buffer, area: Rect) -> Vec<String> {
    let row = |y| (area.x..area.right()).map(|x| buf[(x, y)].symbol()).collect();
    (area.y..area.bottom()).map(row).collect()
}

/// Tab to the library's `pane`, as a person moves between panes.
fn tab_to(app: &mut App, pane: Focus) {
    for _ in 0..5 {
        if app.focus == pane {
            return;
        }
        press(app, "\t");
    }
    assert_eq!(app.focus, pane, "tab reaches it");
}

fn any_view(tc: &hegel::TestCase) -> View {
    tc.draw(gs::sampled_from(&View::ALL[..]))
}

/// A size, any terminal's.
fn any_size(tc: &hegel::TestCase) -> (u16, u16) {
    (
        tc.draw(gs::integers::<u16>().min_value(1).max_value(200)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(60)),
    )
}

#[test]
fn a_terminal_gets_the_first_density_it_fits_both_ways() {
    let sizes = [
        (200, 60, Density::Full),
        (120, 34, Density::Full),
        (119, 34, Density::Collapsed),
        (120, 33, Density::Collapsed),
        (100, 28, Density::Collapsed),
        (99, 28, Density::NoDetails),
        (100, 27, Density::NoDetails),
        (80, 24, Density::NoDetails),
        (79, 24, Density::Compact),
        (80, 23, Density::Compact),
        (60, 20, Density::Compact),
        (59, 20, Density::DeckOnly),
        (60, 19, Density::DeckOnly),
        (1, 1, Density::DeckOnly),
    ];
    for (w, h, want) in sizes {
        assert_eq!(Density::of(w, h), want, "{w}x{h}");
    }
}

/// At any size and in any view nothing panics and the deck is there. With room for the views the
/// tabs, the status line and the footer are too; with none, the screen names the size it needs.
#[hegel::test(test_cases = 300)]
fn prop_every_size_has_the_deck(tc: hegel::TestCase) {
    let (mut app, _dir) = playing(TrackAnalysis { preview: Some(wave(400)), ..Default::default() });
    app.view = any_view(&tc);
    let (w, h) = any_size(&tc);
    let s = screen(&mut app, w, h);
    let shell = Shell::of(Rect::new(0, 0, w, h));
    let lines = rows(&s);
    let deck = lines[shell.strip.y as usize].to_string();
    if w >= 30 {
        assert!(deck.contains("Around"), "the deck at {w}x{h}:\n{s}");
    }
    let screen_rect = Rect::new(0, 0, w, h);
    assert!(screen_rect.intersection(app.areas.overview) == app.areas.overview, "{:?} at {w}x{h}", app.areas.overview);
    if Density::of(w, h) == Density::DeckOnly {
        assert!(app.areas.tabs.is_empty(), "tabs to click at {w}x{h}:\n{s}");
        if w >= 5 && h >= 2 {
            assert!(s.contains("60x20"), "the size it needs at {w}x{h}:\n{s}");
        }
        return;
    }
    assert!(w >= 60 && h >= 20, "more than the deck at {w}x{h}");
    assert_eq!(app.areas.overview.y, shell.strip.y + 1, "{w}x{h}");
    assert!(lines[0].contains("Library") && lines[0].contains("localnetwork"), "the tabs at {w}x{h}:\n{s}");
    // Each tab is clicked where it's drawn, and localnetwork's ends the row.
    let header: Vec<char> = lines[0].chars().collect();
    for (v, r) in &app.areas.tabs {
        let drawn: String = header[r.x as usize..r.right() as usize].iter().collect();
        assert!(drawn.trim().ends_with(v.title()), "{v:?} is clicked on {drawn:?} at {w}x{h}:\n{s}");
    }
    assert_eq!(app.areas.tabs.len(), View::ALL.len(), "{w}x{h}");
    assert!(app.areas.tabs.iter().any(|(v, r)| *v == View::Profile && r.right() == w), "{w}x{h}:\n{s}");
    assert!(lines[h as usize - 1].trim_end().ends_with("? help"), "the keys at {w}x{h}:\n{s}");
}

/// What's said under the deck fits a narrow screen too.
#[test]
fn the_size_needed_is_said_in_what_fits() {
    let (mut app, _dir) = blank(View::Library);
    assert!(screen(&mut app, 40, 10).contains("Needs a 60x20 terminal"));
    let tiny = screen(&mut app, 6, 3);
    assert!(tiny.contains("60x20") && !tiny.contains("Needs"), "{tiny}");
}

/// Whatever is open over the view, at any size, the deck strip's rows stay as they were.
#[hegel::test(test_cases = 200)]
fn prop_nothing_covers_the_deck(tc: hegel::TestCase) {
    let (mut app, _dir) = playing(TrackAnalysis { preview: Some(wave(400)), ..Default::default() });
    app.track_state.select(Some(0));
    let (w, h) = any_size(&tc);
    let bare = drawn(&mut app, w, h);
    match tc.draw(gs::integers::<u8>().max_value(7)) {
        0 => press(&mut app, "?"),
        1 => press(&mut app, "!"),
        2 => press(&mut app, ":"),
        3 => press(&mut app, "e"),
        4 => app.modal = Some(Modal::Ask("Sure?".repeat(40), Box::new(|_: &mut App| {}))),
        5 => {
            let lines = vec![(Tone::Text, "a line".to_string()); 80];
            app.modal = Some(Modal::Report(Report { title: "Long".into(), lines, scroll: 0 }));
        }
        6 => app.deck.outputs = Some(Outputs { names: vec!["Phones".into(); 40], ..Default::default() }),
        _ => press(&mut app, "t"),
    }
    let over = drawn(&mut app, w, h);
    let strip = Shell::of(Rect::new(0, 0, w, h)).strip;
    assert_eq!(text_of(&over, strip), text_of(&bare, strip), "at {w}x{h}");
}

/// What's open, top first, each what a single esc closes.
fn layers(app: &App) -> Vec<&'static str> {
    let mut open = Vec::new();
    let mut add = |on: bool, what| {
        if on {
            open.push(what);
        }
    };
    add(app.help, "help");
    add(app.show_log, "messages");
    add(app.deck.outputs.is_some(), "outputs");
    add(matches!(&app.modal, Some(Modal::Menu(m)) if m.sub.is_some()), "a sub-menu");
    add(app.modal.is_some(), "a dialog");
    add(app.searching, "the search box");
    add(app.picking.is_some(), "picking a playlist");
    add(app.mix.input.is_some(), "typing a path");
    add(app.deck_layout, "full screen");
    let error = app.held.is_some() || app.toast.as_ref().is_some_and(|t| t.1 == Level::Error);
    add(error, "an error");
    add(app.replay_offer.is_some(), "the offer to replay");
    add(!app.query.is_empty() && !app.searching, "the search");
    open
}

/// However many things are open, in any view, esc closes one, the one on top.
#[hegel::test(test_cases = 300)]
fn prop_esc_closes_one_layer(tc: hegel::TestCase) {
    let (mut app, _dir) = app_with(library());
    let (w, h) = (100, 30);
    app.track_state.select(Some(0));
    let on = || tc.draw(gs::booleans());
    if on() {
        app.view = View::Mixes;
        app.perform(Act::MixOpen, None);
    } else if on() {
        app.deck_layout = true;
    }
    if on() {
        app.toast("it went wrong", Level::Error);
    }
    if on() {
        app.replay_offer = Some(ReplayOffer { root: PathBuf::from("/d"), drive: "D".into(), edits: 2 });
    }
    if on() {
        app.query = "daft".into();
    }
    if on() {
        let target = app.target().unwrap();
        app.picking = Some(crate::tui::edits::Pick { target, focus: Focus::Tracks });
    }
    app.searching = on() && app.view == View::Library;
    if on() {
        app.open_menu(None);
        if on() {
            let Some(Modal::Menu(m)) = &mut app.modal else { panic!("no menu") };
            m.sub = Some(Sub::Rating);
        }
    }
    if on() {
        app.deck.outputs = Some(Outputs::default());
    }
    app.show_log = on();
    app.help = on();
    while let [top, rest @ ..] = &layers(&app)[..] {
        let (top, rest) = (*top, rest.to_vec());
        screen(&mut app, w, h);
        press(&mut app, "\u{1b}");
        assert_eq!(layers(&app), rest, "esc over {top}");
    }
}

/// A digit goes to each view, and the view's tab shows it.
#[test]
fn a_digit_goes_to_a_view() {
    for (digit, view) in
        [("1", View::Library), ("2", View::Prepare), ("3", View::Drives), ("4", View::Mixes), ("5", View::Profile)]
    {
        let (mut app, _dir) = blank(View::Library);
        press(&mut app, digit);
        assert_eq!(app.view, view);
        let s = screen(&mut app, 120, 40);
        assert!(rows(&s)[0].contains(&format!("{digit} {}", view.title())), "{s}");
        assert_eq!(keymap::view_key(view), digit);
    }
}

/// Picking a playlist stays in the library: a digit doesn't leave it.
#[test]
fn picking_stays_in_the_library() {
    let (mut app, _dir) = app_with(library());
    app.track_state.select(Some(0));
    let target = app.target().unwrap();
    app.picking = Some(crate::tui::edits::Pick { target, focus: Focus::Tracks });
    app.focus = Focus::Playlists;
    press(&mut app, "3");
    assert_eq!(app.view, View::Library);
    assert!(app.picking.is_some());
}

/// Digits are hot cues while the pads are on, in any view, and the deck says they are.
#[hegel::test(test_cases = 20)]
fn prop_digits_are_hot_cues_with_the_pads_on(tc: hegel::TestCase) {
    let cues = vec![cue(Some(0), 1_000, None)];
    let (mut app, _dir) = playing(TrackAnalysis { cues, ..Default::default() });
    app.view = any_view(&tc);
    app.deck.pads = true;
    press(&mut app, "1");
    assert_eq!(app.pending_seek, Some(Duration::from_secs(1)));
    assert_eq!(app.focus, Focus::Tracks);
    let s = screen(&mut app, 120, 40);
    assert!(s.contains(" pads on ") && s.contains("1 A") && s.contains(" PADS "), "{s}");
}

/// The focused pane says so without colour, with a marker in one title only, and a selection in a
/// pane without focus is still there, dimmed.
#[test]
fn focus_reads_without_colour() {
    theme::set_test_mode(Some(Mode::Plain));
    let (mut app, _dir) = app_with(library());
    app.track_state.select(Some(0));
    let titles = [(Focus::Drives, "▸ Drives"), (Focus::Playlists, "▸ Playlists"), (Focus::Tracks, "▸ All tracks")];
    for (pane, title) in titles {
        tab_to(&mut app, pane);
        let s = screen(&mut app, 160, 45);
        assert!(s.contains(title), "{title}: {s}");
        assert_eq!(titles.iter().filter(|(_, t)| s.contains(t)).count(), 1, "{s}");
    }
    let buf = drawn(&mut app, 160, 45);
    let row = (2..45).find(|&y| text_of(&buf, Rect::new(0, y, 30, 1))[0].contains("All tracks")).unwrap();
    let style = buf[(8, row)].style();
    theme::set_test_mode(None);
    assert!(style.add_modifier.contains(Modifier::REVERSED | Modifier::DIM), "{style:?}");
}

/// With colours, what's behind a selection without focus is the theme's idle colour, in every list.
#[test]
fn a_selection_without_focus_is_dimmed_by_the_theme() {
    theme::set_test_mode(Some(Mode::Dark));
    theme::set_test_tier(Some(Tier::Truecolor));
    let (mut app, _dir) = app_with(library());
    let idle = {
        let [r, g, b] = theme::current().idle;
        Color::Rgb(r, g, b)
    };
    let buf = drawn(&mut app, 160, 45);
    let row = (2..45).find(|&y| text_of(&buf, Rect::new(0, y, 30, 1))[0].contains("All tracks")).unwrap();
    let bg = buf[(8, row)].bg;
    theme::set_test_tier(None);
    theme::set_test_mode(None);
    assert_eq!(bg, idle);
}

/// The header says where things stand: nothing before a drive is open, then the drive, the list,
/// the search and the marks, at any height, the panes right under it.
#[test]
fn the_header_says_where_things_stand() {
    let (mut app, _dir) = blank(View::Library);
    let s = screen(&mut app, 120, 40);
    assert!(!rows(&s)[0].contains('›'), "{s}");

    let (mut app, _dir) = app_with(library());
    app.query = "daft".into();
    app.search_all = true;
    app.refresh_visible();
    app.marks.insert("a track".into());
    let drive = {
        let root = app.loaded.as_ref().unwrap().root().to_path_buf();
        app.drive_label(&root)
    };
    for h in [45, 29] {
        let s = screen(&mut app, 160, h);
        for said in [drive.as_str(), "› All tracks", "/ daft whole drive", "marked"] {
            assert!(rows(&s)[0].contains(said), "{said} at 160x{h}: {s}");
        }
        assert!(rows(&s)[1].contains(" Drives ─"), "the panes start under the header: {s}");
    }
}

/// The status row: what's going on, where the selection is and the drive's space; the keys row's
/// chip, only when keys mean something else.
#[test]
fn the_status_row_says_what_and_where() {
    let (mut app, _dir) = app_with(library());
    app.track_state.select(Some(1));
    let n = app.visible.len();
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let mut info = locallibrary::Device::open(&root).unwrap().info();
    info.free_space = Some(12_000_000_000);
    app.drives = vec![info];
    let row = |app: &mut App, y: usize| {
        let s = screen(app, 160, 40);
        rows(&s)[y].to_string()
    };
    let line = row(&mut app, 38);
    assert!(!line.contains("BROWSE") && line.contains(&format!("2/{n}")), "{line}");
    assert!(line.contains("12.0 GB free") || line.contains("11.2 GB free"), "{line}");

    app.searching = true;
    assert!(row(&mut app, 39).contains(" SEARCH "));
    app.searching = false;
    app.picking = Some(crate::tui::edits::Pick { target: app.target().unwrap(), focus: Focus::Tracks });
    assert!(row(&mut app, 39).contains(" PICK "));
    app.picking = None;
    app.deck_layout = true;
    assert!(!row(&mut app, 39).contains("PLAYER"), "player mode is plain to see");
    app.deck_layout = false;
    app.deck.pads = true;
    assert!(row(&mut app, 39).contains(" PADS "));
    app.deck.pads = false;
    app.view = View::Mixes;
    app.perform(Act::MixOpen, None);
    assert!(row(&mut app, 39).contains(" TYPING "));
    press(&mut app, "\u{1b}");
    app.view = View::Library;

    app.job = Some(job(false, false));
    app.job.as_mut().unwrap().detail = "half way".into();
    let line = row(&mut app, 38);
    assert!(line.contains("half way") && line.contains("esc cancel"), "{line}");
    app.job = None;
    app.loading = Some("Reading TEST".into());
    assert!(row(&mut app, 38).contains("Reading TEST"));
    app.loading = None;
}

/// Who's signed in is localnetwork's to say, in the header; the status row never says it.
#[cfg(feature = "account")]
#[test]
fn the_header_names_who_is_signed_in() {
    let (mut app, _dir) = app_with(library());
    app.track_state.select(Some(0));
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let mut info = locallibrary::Device::open(&root).unwrap().info();
    info.free_space = Some(12_000_000_000);
    app.drives = vec![info];
    let account = serde_json::json!({
        "did": "did:plc:abc",
        "handle": "a.rather.long.handle.example.com",
        "pds": "https://pds.example",
        "auth_server": "https://pds.example",
        "scopes": ["atproto"],
        "signed_in_at": "2026-01-05T12:00:00Z",
        "sign_in_again_by": "2026-02-05T12:00:00Z",
        "access_expires_at": null,
        "client": { "kind": "hosted", "client_id": "https://example.com/client.json" },
        "storage": "file"
    });
    app.profile.account.info = Some(serde_json::from_value(account).unwrap());
    let s = screen(&mut app, 60, 20);
    let line = rows(&s)[18];
    assert!(!line.contains('@') && line.contains("free") && line.contains("1/"), "{line}");
    app.view = View::Profile;
    let s = screen(&mut app, 160, 40);
    assert!(rows(&s)[0].contains("@a.rather.long.handle.example.com"), "{s}");
    assert!(!rows(&s)[38].contains('@'), "{s}");
}

/// The latest message goes on the status line. One too long, or more than one, goes over the
/// view just above the deck, and a dialog keeps it to the status line.
#[test]
fn messages_stay_off_the_deck() {
    let (mut app, _dir) = playing(TrackAnalysis { preview: Some(wave(400)), ..Default::default() });
    let shell = Shell::of(Rect::new(0, 0, 80, 24));
    app.toast("saved", Level::Info);
    let buf = drawn(&mut app, 80, 24);
    assert!(text_of(&buf, shell.status())[0].contains("saved"));

    app.toast("word ".repeat(30), Level::Info);
    let buf = drawn(&mut app, 80, 24);
    let text = text_of(&buf, Rect::new(0, 0, 80, shell.strip.y)).join("\n");
    assert!(text.contains("word word"), "{text}");
    assert!(!text_of(&buf, shell.strip).join("").contains("word"));

    app.modal = Some(Modal::Ask("Sure?".into(), Box::new(|_: &mut App| {})));
    let buf = drawn(&mut app, 80, 24);
    let line = text_of(&buf, shell.status())[0].clone();
    assert!(line.contains("word") && !text_of(&buf, shell.strip).join("").contains("word"), "{line}");
}

/// The library as the room runs out: a pane collapses to its title and comes back with focus, the
/// details go, then only the pane with focus has the room.
#[test]
fn panes_give_way_in_order() {
    let (mut app, _dir) = app_with(library());
    app.track_state.select(Some(0));
    press(&mut app, "ti");

    screen(&mut app, 120, 34);
    assert!(app.areas.drives.height > 1 && app.areas.crate_panel.height > 1 && app.areas.details.height > 1);

    let s = screen(&mut app, 100, 28);
    assert_eq!(app.areas.drives.height, 1, "{s}");
    assert_eq!(app.areas.crate_panel.height, 1, "{s}");
    assert!(app.areas.playlists.height > 1 && app.areas.details.height > 1, "{s}");
    assert!(s.contains(" Drives ─") && s.contains(" Queue "), "{s}");
    tab_to(&mut app, Focus::Drives);
    screen(&mut app, 100, 28);
    assert!(app.areas.drives.height > 1 && app.areas.playlists.height == 1);
    tab_to(&mut app, Focus::Crate);
    screen(&mut app, 100, 28);
    assert!(app.areas.crate_panel.height > 1 && app.areas.details.height == 1);

    // A click on a collapsed title opens it.
    tab_to(&mut app, Focus::Tracks);
    screen(&mut app, 100, 28);
    let r = app.areas.drives;
    mouse(&mut app, LEFT, (r.x + 2, r.y));
    assert_eq!(app.focus, Focus::Drives);

    tab_to(&mut app, Focus::Tracks);
    let s = screen(&mut app, 80, 24);
    assert!(app.areas.details.is_empty() && !s.contains(" Track ─"), "{s}");
    press(&mut app, "i");
    assert_eq!(app.focus, Focus::Details, "i brings it back");
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("▸ Track ─"), "{s}");
    press(&mut app, "i");
    assert!(!app.show_details && app.focus == Focus::Tracks, "and closes it");
    press(&mut app, "i");
    assert!(app.show_details && app.focus == Focus::Details, "and opens it");
    tab_to(&mut app, Focus::Tracks);

    let s = screen(&mut app, 60, 20);
    assert_eq!((app.areas.drives.height, app.areas.playlists.height, app.areas.crate_panel.height), (1, 1, 1), "{s}");
    assert!(app.areas.tracks.height > 1 && app.areas.tracks.width == 60, "{s}");
    assert!(app.areas.details.is_empty(), "{s}");
    tab_to(&mut app, Focus::Playlists);
    screen(&mut app, 60, 20);
    assert!(app.areas.playlists.height > 1 && app.areas.tracks.height == 1);
}

/// The Drives view at its smallest: the pane with focus has the room, the other its title.
#[test]
fn the_drives_panes_stack_at_the_smallest() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "3");
    let s = screen(&mut app, 60, 20);
    assert!(app.areas.drive_list.height > 1 && app.areas.drive_side.height == 1, "{s}");
    assert!(s.contains(" Drive  Snapshots  History "), "{s}");
    press(&mut app, "\t");
    let s = screen(&mut app, 60, 20);
    assert!(app.areas.drive_list.height == 1 && app.areas.drive_side.height > 1, "{s}");
    assert!(s.contains(" Drives ─"), "{s}");
}

/// Snapshots and history sit beside the list as tabs, reached by tab or a click, with the
/// drive's details scrolled when they have focus.
#[test]
fn the_drives_view_folds_in_snapshots_and_history() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "3");
    assert_eq!(app.drive_tab, DrivePane::Info);
    press(&mut app, "\t");
    assert_eq!(app.drive_pane, DrivePane::Info);
    press(&mut app, "\t");
    assert_eq!((app.drive_pane, app.drive_tab), (DrivePane::Snapshots, DrivePane::Snapshots));
    wait(&mut app, |a| a.history.is_some());
    press(&mut app, "\t\t");
    assert_eq!(app.drive_pane, DrivePane::List);
    assert_eq!(app.drive_tab, DrivePane::History);
    app.on_key(ratatui::crossterm::event::KeyEvent::new(
        KeyCode::BackTab,
        ratatui::crossterm::event::KeyModifiers::SHIFT,
    ));
    assert_eq!(app.drive_pane, DrivePane::History);

    screen(&mut app, 120, 40);
    let tabs = app.areas.drive_tabs.clone();
    for (pane, r) in tabs {
        mouse(&mut app, LEFT, (r.x + 2, r.y));
        assert_eq!(app.drive_pane, pane);
    }
    assert!(screen(&mut app, 120, 40).contains("▸ History"));
    let r = app.areas.drive_list;
    mouse(&mut app, LEFT, (r.x + 2, r.y));
    assert_eq!(app.drive_pane, DrivePane::List);
}

/// What players recorded lists the drive's lists, opens one in the library, and says when there's
/// nothing to list.
#[test]
fn the_history_pane_opens_a_list() {
    let (mut app, _dir) = blank(View::Drives);
    app.drive_tab = DrivePane::History;
    assert!(screen(&mut app, 120, 40).contains("No drive open."));

    let mut lib = library();
    lib.history.clear();
    let (mut app, _dir) = app_with(lib);
    press(&mut app, "3\t\t\t");
    assert!(screen(&mut app, 120, 40).contains("Nothing recorded on this drive yet."));
    let played = |id, name: &str, track_ids| Playlist {
        id,
        parent_id: None,
        name: name.into(),
        is_folder: false,
        sort_order: id,
        track_ids,
    };
    let mut lib = library();
    lib.history = vec![
        Playlist { is_folder: true, ..played(1, "2026", Vec::new()) },
        played(2, "HISTORY 001", vec![lib.tracks[0].id]),
        played(3, "HISTORY 002", Vec::new()),
    ];
    let (mut app, _dir) = app_with(lib);
    press(&mut app, "3\t\t\t");
    let s = screen(&mut app, 120, 40);
    assert!(
        s.contains("HISTORY 001") && s.contains("1 track") && s.contains("HISTORY 002") && !s.contains("2026"),
        "{s}"
    );
    let r = app.areas.played;
    mouse(&mut app, LEFT, (r.x + 4, r.y + 2));
    assert_eq!(app.played.selected(), Some(1));
    press(&mut app, "k\n");
    assert_eq!((app.view, app.focus), (View::Library, Focus::Tracks));
    assert_eq!(app.visible.len(), 1);
    assert!(screen(&mut app, 120, 40).contains("HISTORY 001"));
}

/// Keys that move in the Drives view's details scroll them, and stop at their end.
#[test]
fn the_drives_details_scroll() {
    let (mut app, _dir) = app_with(library());
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    app.drives = vec![locallibrary::Device::open(&root).unwrap().info()];
    app.drive_state.select(Some(0));
    press(&mut app, "3\t");
    let first = screen(&mut app, 120, 28);
    press(&mut app, "jj");
    assert_eq!(app.drive_scroll, 2);
    assert_ne!(screen(&mut app, 120, 28), first);
    press(&mut app, "kkk");
    assert_eq!(app.drive_scroll, 0);
    // Another drive's details start from their top.
    press(&mut app, "jj");
    key(&mut app, KeyCode::BackTab);
    press(&mut app, "j");
    assert_eq!(app.drive_scroll, 0);
    press(&mut app, "\t");
    key(&mut app, KeyCode::End);
    screen(&mut app, 120, 28);
    assert!(app.drive_scroll < 100, "{}", app.drive_scroll);
}

/// The tabs and the deck answer clicks in the views with their own mouse handling too, and with
/// only the deck the panes that were there don't.
#[test]
fn the_deck_and_tabs_answer_in_every_view() {
    let (mut app, _dir) = playing(TrackAnalysis { preview: Some(wave(400)), ..Default::default() });
    for view in View::ALL {
        app.view = view;
        screen(&mut app, 120, 40);
        let r = app.areas.overview;
        assert!(!r.is_empty(), "{view:?}");
        app.pending_seek = None;
        mouse(&mut app, LEFT, (r.x + r.width / 2, r.y + 1));
        assert_eq!(app.pending_seek, Some(Duration::from_millis(151_250)), "{view:?}");
    }
    app.view = View::Mixes;
    screen(&mut app, 120, 40);
    app.mix.pane = crate::tui::mix::Pane::Tracks;
    screen(&mut app, 40, 10);
    mouse(&mut app, LEFT, (2, 6));
    assert_eq!(app.mix.pane, crate::tui::mix::Pane::Tracks, "that pane isn't on screen");
}
