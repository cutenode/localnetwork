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

use super::app::testing::{demo_drive, just_now, key, overlay, set, wait};
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
        if !overlay(app) {
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
    // A check's results, a plugin's finding with its fix.
    let mut report = api::check(
        None,
        &api::CheckRequest { drives: vec![root_of(&app)], options: Default::default() },
        &Ctx::silent(),
    )
    .unwrap();
    let fix = locallibrary::plugin::Fix {
        plugin: "echo".into(),
        operation: "playlist".into(),
        params: serde_json::Value::Null,
    };
    let finding = locallibrary::check::Finding::new(locallibrary::check::Level::Warning, "echo.untitled", "1 untitled");
    report.items.push(locallibrary::check::CheckItem { drive: report.drives[0].clone(), finding, fix: Some(fix) });
    app.modal = Some(Modal::Report(super::drives::Report {
        title: "Check · DEMO".into(),
        lines: super::drives::check_lines(&report),
        scroll: 0,
    }));
    check(
        &mut app,
        "a check",
        &["Check · DEMO", "1 untitled", "fix it from the commands (:)", "any other key closes"],
        &[],
    );
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
    // (Said just now, however long the screens below take to draw.)
    app.log.back_mut().unwrap().2 = just_now();
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

/// A plugin's command in the palette, its form and what it answers, and its operation in the
/// actions menu, its form starting with the selected track and its plan to confirm: all from the
/// plugins' manifests.
#[test]
fn plugins() {
    let Fixture { _dir, mut app, .. } = fixture();
    super::app::testing::echo_plugin(_dir.path());
    app.focus = Focus::Tracks;
    for c in ":answer the params".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    check(&mut app, "a plugin's command in the palette", &["answer the params…", "Echo"], &[]);
    key(&mut app, KeyCode::Enter);
    check(
        &mut app,
        "a plugin's form",
        &["Answer the params", "name", "required", "loud", "○ no", "Who to greet", "enter run · esc cancel"],
        &[],
    );
    key(&mut app, KeyCode::Enter);
    check(&mut app, "a plugin's form, short of a param", &["⚠ name is needed"], &[]);
    key(&mut app, KeyCode::Esc);
    let rows = vec![vec![serde_json::json!("Demo Track 1"), serde_json::json!(128)]];
    let view =
        loclib_plugin::View::Table { title: Some("Titles".into()), columns: vec!["Title".into(), "BPM".into()], rows };
    app.show_answer("List the drive's tracks".into(), locallibrary::plugin::CommandOutput::View(view));
    check(
        &mut app,
        "a plugin's table",
        &["Titles", "Title         BPM", "Demo Track 1  128", "any other key closes"],
        &[],
    );
    key(&mut app, KeyCode::Esc);

    key(&mut app, KeyCode::Char('e'));
    check(&mut app, "the actions menu with a plugin's operation", &["make a playlist…", "fetch cover art…"], &[]);
    let Some(Modal::Menu(m)) = &app.modal else { panic!("no menu") };
    let echo = m.items.iter().position(|i| i.label == "make a playlist…").unwrap();
    for _ in 0..echo {
        key(&mut app, KeyCode::Char('j'));
    }
    key(&mut app, KeyCode::Enter);
    let id = app.loaded.as_ref().unwrap().library.tracks[app.visible[0]].id.to_string();
    check(&mut app, "a plugin's operation's form", &["Make a playlist", "tracks", &id, "required"], &[]);
    for c in "Warmup\n".chars() {
        key(&mut app, if c == '\n' { KeyCode::Enter } else { KeyCode::Char(c) });
    }
    wait(&mut app, |a| a.job.is_none());
    check(
        &mut app,
        "a plugin's plan to confirm",
        &["Write to DEMO?", "Echo: make the playlist Warmup", "+ Warmup", "y write", "u takes this back"],
        &[],
    );
}

/// What an analyzer plugin worked out, as a column of the track list sorted by it, and the
/// search box filtering by it: from the plugins' manifests and the store.
#[test]
fn plugins_fields() {
    let Fixture { _dir, mut app, root } = fixture();
    super::app::testing::echo_plugin(_dir.path());
    let lib = app.loaded.as_ref().unwrap().library.clone();
    std::fs::write(root.join(lib.tracks[1].file_path.trim_start_matches('/')), "x".repeat(4000)).unwrap();
    let store = Store::open(&app.store_root).unwrap();
    crate::plugins::host()
        .unwrap()
        .analyze("echo", "size", &root.display().to_string(), None, &store, &Ctx::silent())
        .unwrap();
    app.focus = Focus::Tracks;
    for c in ":show the echo.bytes column".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    app.sort = Some(locallibrary::query::SortKey {
        field: locallibrary::query::SortField::Field(locallibrary::query::FieldName::new("echo.bytes")),
        descending: true,
    });
    wait(&mut app, |a| a.loaded.as_ref().unwrap().values().is_some());
    app.refresh_visible();
    check(&mut app, "a plugin's column, sorted by it", &["echo.bytes↓", "4000", "echo.bytes ↓"], &[]);
    for c in "/echo.bytes:>1000".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    check(&mut app, "the search box filtering by a plugin's field", &["/ echo.bytes:>1000", "1 match", "4000"], &[]);
}

/// What an analyzer says of how its field is shown, in the field's column: its unit, in the
/// width it gives.
#[test]
fn plugins_field_hints() {
    let Fixture { _dir, mut app, root } = fixture();
    super::app::testing::echo_plugin(_dir.path());
    let file = _dir.path().join("echo.toml");
    let hinted = std::fs::read_to_string(&file).unwrap().replace(
        "fields = { bytes = \"integer\" }",
        "fields = { bytes = \"integer\" }\ndisplay.bytes = { unit = \"B\", width = 12 }",
    );
    std::fs::write(&file, hinted).unwrap();
    let lib = app.loaded.as_ref().unwrap().library.clone();
    std::fs::write(root.join(lib.tracks[1].file_path.trim_start_matches('/')), "x".repeat(4000)).unwrap();
    let store = Store::open(&app.store_root).unwrap();
    let plugins = crate::plugins::host().unwrap();
    plugins.analyze("echo", "size", &root.display().to_string(), None, &store, &Ctx::silent()).unwrap();
    app.focus = Focus::Tracks;
    for c in ":show the echo.bytes column".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| a.loaded.as_ref().unwrap().values().is_some());
    check(&mut app, "a plugin's column, as its analyzer says it's shown", &["echo.bytes", "4000 B"], &[]);
}

/// A plugin's theme, chosen in the palette: drawn in from then on (the default's colours gone),
/// and kept for next time; the default again the same way.
#[test]
fn plugins_themes() {
    let Fixture { _dir, mut app, .. } = fixture();
    super::app::testing::echo_plugin(_dir.path());
    app.focus = Focus::Tracks;
    for c in ":theme".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    check(&mut app, "the themes in the palette", &["Night theme", "Echo", "default theme"], &[]);
    let colours = |app: &mut App| {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 45)).unwrap();
        terminal.draw(|f| super::ui::draw(f, app)).unwrap();
        terminal.backend().buffer().content.iter().map(|c| c.fg).collect::<Vec<_>>()
    };
    let (night, accent) =
        (super::theme::rgb(255, 136, 0), super::theme::rgb3(locallibrary::theme::Theme::DEFAULT.accent));
    key(&mut app, KeyCode::Esc);
    for c in ":night theme".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    assert_eq!(super::theme::chosen(), Some("echo.night"));
    check(&mut app, "drawn in a plugin's theme", &["drawn in the Night theme"], &[]);
    let drawn = colours(&mut app);
    assert!(drawn.contains(&night) && !drawn.contains(&accent), "the theme's accent, not the default's");
    assert_eq!(app.saved_state().theme.as_deref(), Some("echo.night"));
    for c in ":default theme".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    let drawn = colours(&mut app);
    assert!(drawn.contains(&accent) && !drawn.contains(&night));
    assert_eq!((super::theme::chosen(), app.saved_state().theme), (None, None));
}

/// A plugin's settings in a form, as set now, from the palette.
#[test]
fn plugins_settings() {
    let Fixture { _dir, mut app, .. } = fixture();
    super::app::testing::echo_plugin(_dir.path());
    for c in ":echo plugin settings".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    check(&mut app, "a plugin's settings in the palette", &["Echo plugin settings…", "Echo"], &[]);
    key(&mut app, KeyCode::Enter);
    check(
        &mut app,
        "a plugin's settings",
        &["Echo settings", "greeting", "hello", "speed", "1.5", "loud", "○ no", "‹ calm ›", "enter save"],
        &[],
    );
}

/// An export in a plugin's format: its form, with the format's options, and its plan to confirm,
/// a file there already to overwrite.
#[test]
fn plugins_exports() {
    let Fixture { _dir, mut app, root } = fixture();
    super::app::testing::echo_plugin(_dir.path());
    super::app::testing::mounted(root.parent().unwrap());
    for c in ":export for traktor".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    check(
        &mut app,
        "an export's form",
        &["Traktor Pro: collection.nml", "write to", "~/Music/", "location root", "skip analysis", "○ no", "enter run"],
        &[],
    );
    key(&mut app, KeyCode::Esc);
    let out = _dir.path().join("Friday.m3u");
    std::fs::write(&out, "mine").unwrap();
    let source = loclib_plugin::ExportSource { drive: root.display().to_string(), ..Default::default() };
    let plugins = crate::plugins::host().unwrap();
    let plan =
        plugins.export_plan("echo", "echo-m3u", source, &out, serde_json::Value::Null, None, &Ctx::silent()).unwrap();
    app.modal = Some(Modal::Confirm(Confirm {
        root: root.clone(),
        drive: app.drive_label(&root),
        plan: Box::new(plan),
        scroll: 0,
        shown: Instant::now() - Duration::from_secs(1),
        after: None,
    }));
    check(
        &mut app,
        "an export's plan to confirm",
        &[
            "Overwrite 1 file?",
            "write 1 file(s)",
            "2 tracks",
            "Friday.m3u  playlist,",
            "(already there)",
            "y overwrite",
        ],
        &[],
    );
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
    // A guard said something about it, and stops it.
    let mut plan = Box::new(api::edit_plan(&req, &Ctx::silent()).unwrap());
    plan.warnings.push("On a Friday: it's a Friday".into());
    plan.blocked = Some("echo.friday".into());
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
        "a guarded confirmation",
        &["⚠ On a Friday: it's a Friday", "✗ echo.friday stops these changes"],
        &[],
    );
    // A plan of another kind that edits the library (a contact's playlist): stopped all the same.
    let contact =
        api::SetContact { drive: root.display().to_string(), contact: "dj@example.com".into(), playlist: true };
    let mut plan = Box::new(api::drive_contact_plan(&contact, &Ctx::silent()).unwrap());
    plan.blocked = Some("echo.friday".into());
    app.modal = Some(Modal::Confirm(Confirm {
        root: root.clone(),
        drive,
        plan,
        scroll: 0,
        shown: Instant::now() - Duration::from_secs(1),
        after: None,
    }));
    check(&mut app, "a guarded contact", &["IF FOUND", "✗ echo.friday stops these changes"], &[]);
}
