//! Imports in the player, from a drive's menu or the palette, and a set's export formats in
//! Browse › Sets.

use locallibrary::Store;
use locallibrary::sets::{Set, SetBook};
use ratatui::crossterm::event::KeyCode;

use super::super::super::app::testing::{
    app_on, confirming, demo_drive, echo_plugin, key, last_message, mounted, press, say_yes, wait,
};
use super::super::super::app::{App, Focus};
use super::super::super::browse::{Category, NodeId};
use super::super::super::edits::Modal;
use super::super::importers::READ_FROM;
use super::entry;

/// The import form open, its file replaced with `file`.
fn import(app: &mut App, file: &std::path::Path) {
    press(app, ":import audio\n");
    let Some(Modal::Form(f)) = &mut app.modal else { panic!("no form") };
    assert_eq!((f.title.as_str(), f.fields[0].name.as_str()), ("Audio files, a line each", READ_FROM));
    f.fields[0].text = file.display().to_string();
    press(app, "\n");
}

/// Importing from the palette: a form asks for the file, the plan (what it adds to the tracks it
/// finds) is confirmed as any edit is, and it's written on a yes, undone with `u`. Nothing to add
/// is said so.
#[test]
fn an_import_is_planned_confirmed_and_made() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    mounted(dir.path());
    let mut app = app_on(&root);
    let first = app.loaded.as_ref().unwrap().library.tracks[0].file_path.clone();
    let list = dir.path().join("list.txt");
    std::fs::write(&list, format!("{first}\n")).unwrap();
    import(&mut app, &list);
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
    let (operation, summary) = confirming(&mut app);
    assert_eq!(operation, "echo.import.lines");
    assert!(summary.starts_with("import 1 of 1 track(s) found on"), "{summary}");
    assert_eq!(say_yes(&mut app), "Echo: 1 change written, 0 sessions kept · u to undo");
    import(&mut app, &list);
    wait(&mut app, |a| a.job.is_none());
    assert!(last_message(&app).contains("nothing to change"));
    app.toast = None;
    app.plugin_import("echo", "lines", None);
    assert_eq!((app.modal.is_none(), last_message(&app)), (true, "open a drive first"));
}

/// A set's format is offered while a set is open, writing out that set (not the lists marked);
/// elsewhere it says to open one first.
#[test]
fn a_sets_format_writes_out_the_set_open() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let file = dir.path().join("echo.toml");
    let manifest = std::fs::read_to_string(&file).unwrap().replace(
        "capabilities = [\"library:read\", \"library:plan\"]",
        "capabilities = [\"library:read\", \"library:plan\", \"sets:read\"]",
    );
    let sheet = "\n[[contributes.exporters]]\nformat = \"sheet\"\ntitle = \"Set sheet\"\nextension = \"txt\"\n\
                 source = \"set\"\n";
    std::fs::write(&file, manifest + sheet).unwrap();
    mounted(dir.path());
    // The store the plugin reads the set from: the player's.
    let mut app = app_on(&root);
    app.store_root = dir.path().join("store");
    let friday = Set { id: "3m2friday2222".into(), title: Some("Friday".into()), ..Set::default() };
    SetBook { sets: vec![friday], ..SetBook::default() }.save(&Store::open_or_init(&app.store_root).unwrap()).unwrap();
    press(&mut app, ":");
    assert_eq!(entry(&app, "export the set for sheet…").1, Some("open a set first"));
    press(&mut app, "\u{1b}");
    app.read_sets();
    let open = |app: &mut App, id: NodeId| {
        wait(app, |a| a.loaded.as_ref().unwrap().tree_position(&id).is_some());
        app.focus = Focus::Playlists;
        app.playlist_state.select(app.loaded.as_ref().unwrap().tree_position(&id));
        key(app, KeyCode::Enter);
    };
    open(&mut app, NodeId::Browse);
    open(&mut app, NodeId::Category(Category::Sets));
    open(&mut app, NodeId::Group(Category::Sets, "3m2friday2222".into()));
    assert_eq!(app.open_set().as_deref(), Some("3m2friday2222"));
    // A list marked in the tree is the drive's, not the set's.
    app.list_marks = vec!["Warmup".into()];
    press(&mut app, ":export the set\n");
    let Some(Modal::Form(f)) = &mut app.modal else { panic!("no form") };
    assert!(f.fields[0].text.ends_with("Friday.txt"), "named after the set: {}", f.fields[0].text);
    let out = dir.path().join("friday.txt");
    f.fields[0].text = out.display().to_string();
    press(&mut app, "\n");
    let (operation, _) = confirming(&mut app);
    assert_eq!(operation, "echo.export.sheet");
    assert_eq!(say_yes(&mut app), "Echo: 1 file written");
    assert!(out.is_file());
}
