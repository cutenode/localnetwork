//! Every view, the help and the dialogs drawn to a test screen at 80x24 and 160x45, from a
//! fixture drive pulled into a store, and checked for the text that should be there: the header
//! and footer, each pane's title and labels, and the fixture's own data. A view that stops
//! drawing, or draws something else, fails here.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use locallibrary::api::{self, DriveStatus, FleetDrive, HistoryRequest, ProfileRequest, PullRequest};
use locallibrary::compare::Relation;
use locallibrary::edit::EditOptions;
use locallibrary::fleet::{DriveLog, Role};
use locallibrary::sets::{EntryOrigin, SetBook, SetEntry};
use locallibrary::{Ctx, Device, Store};
use ratatui::crossterm::event::KeyCode;

use super::app::testing::{demo_drive, key, set, wait};
use super::app::{App, Focus, View};
use super::browse::{Category, NodeId};
use super::edits::{Confirm, Modal, set_rating};
use super::ui::screen;

const SIZES: [(u16, u16); 2] = [(80, 24), (160, 45)];

/// The fixture drive (rekordbox's demo export: two tracks, with placeholder audio)
/// pulled into a store as the library "Demo", shown in the player with every view's data in.
pub(super) struct Fixture {
    pub(super) _dir: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) app: App,
}

pub(super) fn fixture() -> Fixture {
    let (dir, root) = demo_drive();
    // The store, with one snapshot of the drive.
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let pull = PullRequest {
        drive: root.display().to_string(),
        library: Some("Demo".into()),
        message: Some("after Friday's export".into()),
        new_library: true,
        ..Default::default()
    };
    api::pull(&store, &pull, &Ctx::silent()).unwrap();
    shown(dir, root)
}

/// The fixture drive at `root` and its store (`store` in `dir`), shown in the player.
pub(super) fn shown(dir: tempfile::TempDir, root: PathBuf) -> Fixture {
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let store_root = dir.path().join("store");
    let store = Store::open(&store_root).unwrap();
    let history: Vec<_> = (store.libraries().unwrap().into_iter())
        .map(|l| {
            let snaps = api::history(&store, &HistoryRequest { library: l.name.clone(), limit: None }).unwrap();
            (l, snaps)
        })
        .collect();

    let mut app = App::with_library(&root, lib);
    app.store_root = store_root;
    // The drive as the Drives view lists it: its space made up, as the machine's varies.
    let mut info = Device::open(&root).unwrap().info();
    (info.total_space, info.free_space) = (Some(16_000_000_000), Some(12_000_000_000));
    let log = DriveLog { writes: 3, bytes_written: 1_200_000_000, clean_ejects: 2, ..Default::default() };
    app.status = Some(Ok(vec![FleetDrive {
        status: DriveStatus {
            drive: info.clone(),
            library: Some("Demo".into()),
            relation: Some(Relation::Identical),
            head: history[0].1.first().cloned(),
            back_from_set: None,
            settings_drift: 0,
            error: None,
        },
        role: Some(Role::Primary),
        read_only: false,
        low_space: false,
        log: Some(log),
        same_identity_as: Vec::new(),
        exported_over_edits: 0,
    }]));
    app.drives = vec![info];
    app.drive_state.select(Some(0));
    app.history = Some(Ok(history));
    app.history_state.select(Some(0));
    let req = ProfileRequest { drives: vec![root.display().to_string()], no_store: true, ..Default::default() };
    app.profile.view = Some(Ok(api::profile(None, &req, &Ctx::silent()).unwrap()));
    app.mix = super::mix::tests::tab();
    Fixture { _dir: dir, root, app }
}

/// The screen at each size, after `setup`, each checked to hold `needles` (and, at 160
/// columns, `wide` too). Every screen has the header, the tabs and something in the footer.
fn check(app: &mut App, what: &str, needles: &[&str], wide: &[&str]) {
    for (w, h) in SIZES {
        let s = screen(app, w, h);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), h as usize);
        let overlay = app.help || app.show_log || app.modal.is_some();
        if !overlay {
            assert!(lines[0].contains("◉ locallibrary") && lines[0].contains("5 Mixes"), "{what} {w}x{h}: header\n{s}");
            assert!(!lines[h as usize - 1].trim().is_empty(), "{what} {w}x{h}: footer\n{s}");
        }
        let missing: Vec<&&str> =
            needles.iter().chain(if w >= 160 { wide } else { &[] }).filter(|n| !s.contains(**n)).collect();
        assert!(missing.is_empty(), "{what} at {w}x{h} is missing {missing:?}:\n{s}");
        if std::env::var_os("SHOW_SCREENS").is_some() {
            println!("{what} at {w}x{h}:\n{s}");
        }
    }
}

fn root_of(app: &App) -> String {
    app.drives[0].root.display().to_string()
}

#[test]
fn library() {
    let Fixture { _dir, mut app, .. } = fixture();
    let lib = app.loaded.as_ref().unwrap().library.clone();
    let first = lib.tracks[0].title.clone();
    app.focus = Focus::Tracks;
    check(
        &mut app,
        "the library",
        &["Drives", "Playlists", "All tracks", "Title", "BPM", "Key", &first, "Now playing", "enter play"],
        &["Album", "Pick a track and press enter."],
    );
    // With the details and the crate open, and focus in the tree.
    key(&mut app, KeyCode::Char('a'));
    key(&mut app, KeyCode::Char('i'));
    key(&mut app, KeyCode::Char('t'));
    check(&mut app, "the library's side panels", &["Crate", "Track", "Loopmasters"], &["mixes into", "Playlists"]);
    app.focus = Focus::Playlists;
    app.toast = None;
    check(&mut app, "the playlist tree", &["Playlists", "▸ Browse", "enter open"], &[]);
}

/// Browse › Sets, and a set: its tracks on the drive, and how many of it aren't.
#[test]
fn sets() {
    let Fixture { _dir, mut app, .. } = fixture();
    let lib = app.loaded.as_ref().unwrap().library.clone();
    let entry = |i: usize| SetEntry::from_track(&lib.tracks[i], EntryOrigin::Session, None);
    let tracks = vec![entry(1), SetEntry::added("Floorplan - Never Grow Old"), entry(0)];
    let friday = set("3m2friday2222", "2026-10-03", Some("Friday at the Warehouse"), None, tracks);
    SetBook { sets: vec![friday], ..Default::default() }.save(&Store::open(&app.store_root).unwrap()).unwrap();
    app.read_sets();
    let open = |app: &mut App, id: NodeId| {
        wait(app, |a| a.loaded.as_ref().unwrap().tree_position(&id).is_some());
        app.focus = Focus::Playlists;
        app.playlist_state.select(app.loaded.as_ref().unwrap().tree_position(&id));
        key(app, KeyCode::Enter);
    };
    open(&mut app, NodeId::Browse);
    open(&mut app, NodeId::Category(Category::Sets));
    check(&mut app, "the sets", &["▾ Sets"], &["· 2026-10-03 Friday at"]);
    open(&mut app, NodeId::Group(Category::Sets, "3m2friday2222".into()));
    check(
        &mut app,
        "a set",
        &["Sets › 2026-10-03 Friday", &lib.tracks[0].title],
        &["Sets › 2026-10-03 Friday at the Warehouse 2 · 1 not on this drive"],
    );
}

#[test]
fn drives() {
    let Fixture { _dir, mut app, .. } = fixture();
    key(&mut app, KeyCode::Char('2'));
    assert_eq!(app.view, View::Drives);
    let name = app.drives[0].volume_name.clone();
    check(
        &mut app,
        "the drives",
        &[&name, "LIBRARY", "tracks      2", "enter browse", "e actions"],
        &["SPACE", "25% used", "STORE", "up to date", "Demo", "LOGBOOK", "primary", "3 times, 1.2 GB", "ACTIONS"],
    );
    // What can be done to it.
    key(&mut app, KeyCode::Char('e'));
    check(&mut app, "the drive's actions", &["Drive · ", "save a snapshot of the drive", "eject the drive"], &[]);
    key(&mut app, KeyCode::Esc);
    // A check's results.
    app.modal = Some(Modal::Report(super::drives::Report {
        title: "Check · DEMO".into(),
        lines: super::drives::check_lines(
            &api::check(
                None,
                &api::CheckRequest { drives: vec![root_of(&app)], options: Default::default() },
                &Ctx::silent(),
            )
            .unwrap(),
        ),
        scroll: 0,
    }));
    check(&mut app, "a check", &["Check · DEMO", "any other key closes"], &[]);
}

#[test]
fn snapshots() {
    let Fixture { _dir, mut app, .. } = fixture();
    key(&mut app, KeyCode::Char('3'));
    assert_eq!(app.view, View::Snapshots);
    let Some(Ok(history)) = &app.history else { panic!("no history") };
    let short = history[0].1[0].short_id.clone();
    check(
        &mut app,
        "the snapshots",
        &["Snapshots", "Library", "Demo", &short[..8], "R refresh", "enter bring back a playlist"],
        &["after Friday's export"],
    );
    // Enter: a playlist from the selected snapshot, onto the drive open in the library.
    key(&mut app, KeyCode::Enter);
    check(&mut app, "bringing back from a snapshot", &[&format!("Bring back from {} the playlist", &short[..8])], &[]);
}

#[test]
fn profile() {
    let Fixture { _dir, mut app, .. } = fixture();
    key(&mut app, KeyCode::Char('4'));
    assert_eq!(app.view, View::Profile);
    let first = locallibrary::profile::Section::ALL[0].title();
    check(&mut app, "the profile", &[first, "i in/out of document"], &[]);
}

#[test]
fn mixes() {
    let Fixture { _dir, mut app, .. } = fixture();
    key(&mut app, KeyCode::Char('5'));
    assert_eq!(app.view, View::Mixes);
    check(
        &mut app,
        "a set",
        &["Sunday Warm-up", "Signal Path", "Night Bus", "O open a file"],
        &["Halogen", "30s blend"],
    );
}

#[test]
fn help_and_messages() {
    let Fixture { _dir, mut app, .. } = fixture();
    key(&mut app, KeyCode::Char('?'));
    assert!(app.help);
    // The library's own keys first, then those for everywhere (beside them when there's room).
    check(
        &mut app,
        "the help",
        &["Keys · Library", "BROWSING", "any other key closes"],
        &["MARKS AND THE CRATE", "PLAYBACK"],
    );
    key(&mut app, KeyCode::Char('x'));
    app.toast("added 2 to the crate", super::app::Level::Info);
    key(&mut app, KeyCode::Char('!'));
    check(&mut app, "the messages", &["Messages", "added 2 to the crate", "just now"], &[]);
}

/// Run the app's background work until nothing's under way.
fn settle(app: &mut App) {
    wait(app, |a| a.job.is_none() && a.loading.is_none());
}

#[test]
fn my_tags() {
    let Fixture { _dir, mut app, .. } = fixture();
    app.focus = Focus::Tracks;
    key(&mut app, KeyCode::Char('#'));
    check(&mut app, "the My Tags column", &["My Tags", "Title"], &[]);
    key(&mut app, KeyCode::Char('T'));
    settle(&mut app);
    check(&mut app, "the tags to give", &["My Tag for 1 track", "a new tag…", "Category/Tag"], &[]);
}

/// Undoing an edit, as the confirmation shows it; the offer to replay edits; bringing back a
/// playlist, from a snapshot picked.
#[test]
fn undo_replay_and_restore() {
    let Fixture { _dir, mut app, root } = fixture();
    app.focus = Focus::Tracks;
    let l = app.loaded.as_ref().unwrap();
    let path = l.library.tracks[0].file_path.clone();
    let req = api::EditRequest {
        drive: root.display().to_string(),
        edits: vec![set_rating(&[path], 4)],
        options: EditOptions::default(),
    };
    api::edit_apply(&req, &Ctx::silent()).unwrap();
    key(&mut app, KeyCode::Char('u'));
    settle(&mut app);
    if let Some(Modal::Confirm(c)) = &mut app.modal {
        c.shown -= Duration::from_secs(1);
    }
    check(
        &mut app,
        "undoing",
        &["Undo the last change on DEMO?", "put back the backup (before editing)", "1 track as they were", "y undo"],
        &["comes off the journal"],
    );
    key(&mut app, KeyCode::Char('n'));

    app.replay_offer = Some(super::app::ReplayOffer { root: root.clone(), drive: "DEMO".into(), edits: 3 });
    app.toast = None;
    check(
        &mut app,
        "the offer to replay",
        &["rekordbox exported over 3 edits on DEMO. Replay?", "y replay edits"],
        &[],
    );
    app.replay_offer = None;

    app.perform(super::keymap::Act::RestorePlaylist, None);
    check(&mut app, "naming the playlist to bring back", &["Bring back the playlist or folder:"], &[]);
    for c in "Friday\n".chars() {
        key(&mut app, if c == '\n' { KeyCode::Enter } else { KeyCode::Char(c) });
    }
    settle(&mut app);
    check(
        &mut app,
        "the snapshots to bring it back from",
        &["Bring back Friday from Demo", "the newest snapshot that has it"],
        &[],
    );
}

#[test]
fn command_palette() {
    let Fixture { _dir, mut app, .. } = fixture();
    app.focus = Focus::Tracks;
    key(&mut app, KeyCode::Char(':'));
    check(
        &mut app,
        "the palette",
        &["Commands", "›", "search or filter the tracks", "↑↓ choose"],
        &["sort by the next column"],
    );
    for c in "loop".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    check(&mut app, "the palette narrowed", &["loop here, or exit the loop", "l "], &[]);
    // In the footer, among the hints: how to open it.
    key(&mut app, KeyCode::Esc);
    check(&mut app, "the library", &[": commands"], &[]);
}

#[test]
fn edit_menu_and_confirmation() {
    let Fixture { _dir, mut app, root } = fixture();
    app.focus = Focus::Tracks;
    key(&mut app, KeyCode::Char('e'));
    check(&mut app, "the edit menu", &["Edit ·", "set the rating…", "every change to the drive asks first"], &[]);
    key(&mut app, KeyCode::Esc);

    // The plan for a rating, as the player gets it back from planning.
    let l = app.loaded.as_ref().unwrap();
    let path = l.library.tracks[0].file_path.clone();
    let req = api::EditRequest {
        drive: root.display().to_string(),
        edits: vec![set_rating(&[path], 4)],
        options: EditOptions::default(),
    };
    let plan = Box::new(api::edit_plan(&req, &Ctx::silent()).unwrap());
    let drive = app.drive_label(&root);
    app.modal = Some(Modal::Confirm(Confirm {
        root: root.clone(),
        drive: drive.clone(),
        plan,
        scroll: 0,
        shown: Instant::now() - Duration::from_secs(1),
        after: None,
    }));
    check(
        &mut app,
        "the confirmation",
        &["Write to DEMO?", "export.pdb", "1 track changed", "y write", "n cancel", "backed up first"],
        &[],
    );
}
