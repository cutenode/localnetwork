//! Imports in the player, from a drive's menu or the palette, and a set's export formats in
//! Browse › Sets.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::sets::{Set, SetBook, SetEntry};
use locallibrary::{Device, Store};
use ratatui::crossterm::event::KeyCode;
use serde_json::{Value, json};

use super::super::super::app::testing::{
    app_on, confirming, demo_drive, echo_plugin, key, last_message, mounted, plugged, plugins_once_with, press,
    say_yes, settle, wait,
};
use super::super::super::app::{App, Focus};
use super::super::super::browse::{Category, NodeId};
use super::super::super::edits::Modal;
use super::super::super::mix::typed_path;
use super::super::importers::READ_FROM;
use super::entry;
use crate::plugins::params;

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
    say_yes(&mut app);
    assert_eq!(last_message(&app), "Echo: 1 change written, 0 sessions kept · u to undo");
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
    say_yes(&mut app);
    assert_eq!(last_message(&app), "Echo: 1 file written");
    assert!(out.is_file());
}

/// The echo plugin granted reading sets, with an importer of any file (`given`, answering it as
/// the test plugin reads JSON: in a file of its scratch folder with its `blob` option) and a set's
/// format (`sheet`).
fn given(manifest: String) -> String {
    let more = "\n[[contributes.importers]]\nformat = \"given\"\ntitle = \"Given\"\n\
                provides = [\"tags\", \"sessions\"]\n\
                options = { type = \"object\", properties = { blob = { type = \"string\" } } }\n\n\
                [[contributes.exporters]]\nformat = \"sheet\"\ntitle = \"Set sheet\"\nextension = \"txt\"\n\
                source = \"set\"\n";
    let granted = manifest.replace(
        "capabilities = [\"library:read\", \"library:plan\"]",
        "capabilities = [\"library:read\", \"library:plan\", \"sets:read\"]",
    );
    granted + more
}

/// `app` on the store every case of a property shares, without sets: a plugin's reads are of the
/// store the host started it in, which the first case's is.
fn shared_store(mut app: App) -> App {
    app.store_root = plugins_once_with(given).join("store");
    let _ = std::fs::remove_file(app.store_root.join("sets.json"));
    app
}

/// The palette's entry starting `label` run, and the form it opens.
fn form<'a>(app: &'a mut App, label: &str) -> &'a mut crate::tui::form::Form {
    press(app, &format!(":{label}\n"));
    match &mut app.modal {
        Some(Modal::Form(f)) => f,
        _ => panic!("no form for {label}"),
    }
}

/// For any file typed in the import form (one there, or not given by an absolute path, or not
/// there) and any answer it holds (tracks on the drive or not, with My Tags, and sessions), read
/// inline or from a file: the import is planned in the background, refused when the file isn't
/// one, said to have nothing to change when it hasn't, and otherwise confirmed, finding the
/// tracks on the drive. Declined, nothing's written; on a yes the tracks found have their tags
/// and the sessions are kept in the player's store, as the message after says.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn any_import_is_planned_and_made_as_its_form_says(tc: TestCase) {
    plugins_once_with(given);
    let drive = plugged();
    let mut app = shared_store(app_on(&drive.0));
    let dir = tempfile::tempdir().unwrap();
    let tracks = app.loaded.as_ref().unwrap().library.tracks.clone();
    let mut tagged = Vec::new();
    let entries: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|k| {
            let on = tc.draw(gs::optional(gs::integers::<usize>().max_value(tracks.len() - 1)));
            let path = on.map_or_else(|| format!("/Contents/nowhere/{k}.mp3"), |i| tracks[i].file_path.clone());
            let tag = tc.draw(gs::optional(gs::sampled_from(vec!["Genre/Given", "Situation/Given"])));
            if let (Some(i), Some(tag)) = (on, tag) {
                tagged.push((tracks[i].id, tag));
            }
            json!({ "match": { "path": path }, "my_tags": tag.into_iter().collect::<Vec<_>>() })
        })
        .collect();
    let sessions: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|i| json!({ "name": format!("Night {i}"), "plays": [{ "artist": "A", "title": "B", "started_at": format!("2026-10-0{}T22:00:00Z", i + 1) }] }))
        .collect();
    let file = dir.path().join("read.json");
    std::fs::write(&file, json!({ "tracks": entries, "sessions": sessions }).to_string()).unwrap();
    let typed = match tc.draw(gs::integers::<u8>().max_value(4)) {
        // Where the player runs.
        0 => "nowhere/read.json".into(),
        1 => dir.path().join("nowhere.json").display().to_string(),
        _ => file.display().to_string(),
    };
    let blob = tc.draw(gs::booleans());
    let f = form(&mut app, "import given");
    assert_eq!(f.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), [READ_FROM, "blob"]);
    f.fields[0].text = typed.clone();
    if blob {
        f.fields[1].text = "answer.json".into();
    }
    press(&mut app, "\n");
    wait(&mut app, |a| a.job.is_none());
    if typed != file.display().to_string() {
        // Named as the form passes it on: made absolute from where the player runs, which on
        // Windows spells a relative path with backslashes.
        let passed = params::absolute(&typed_path(&typed).display().to_string()).unwrap();
        assert_eq!(last_message(&app), format!("{} isn't a file", passed.as_str().unwrap()));
        return;
    }
    if tagged.is_empty() && sessions.is_empty() {
        assert!(app.modal.is_none() && last_message(&app).starts_with("nothing to change"));
        return;
    }
    let (operation, summary) = confirming(&mut app);
    assert_eq!(operation, "echo.import.given");
    let found = entries.iter().filter(|e| !e["match"]["path"].as_str().unwrap().contains("/nowhere/")).count();
    let n = entries.len();
    assert!(summary.starts_with(&format!("import {found} of {n} track(s) found on ")), "{summary}");
    let store_root = app.store_root.clone();
    let store = || Store::open(&store_root).ok().map(|s| SetBook::load(&s).unwrap().imported.len());
    let tags = |root: &std::path::Path| {
        let read = locallibrary::tags::read(&Device::open(root).unwrap()).unwrap();
        let on: Vec<(u32, String)> = read
            .categories
            .iter()
            .flat_map(|c| &c.tags)
            .flat_map(|g| g.tracks.iter().map(|t| (t.id, g.path.clone())))
            .collect();
        tagged.iter().filter(|(id, tag)| on.contains(&(*id, tag.to_string()))).count()
    };
    if tc.draw(gs::booleans()) {
        settle(&mut app);
        press(&mut app, "n");
        assert!(app.modal.is_none() && app.job.is_none());
        assert_eq!((tags(&drive.0), store().unwrap_or(0)), (0, 0), "declined, nothing's written");
        return;
    }
    say_yes(&mut app);
    assert_eq!((tags(&drive.0), store().unwrap_or(0)), (tagged.len(), sessions.len()));
    let kept = match sessions.len() {
        1 => "1 session kept".to_string(),
        n => format!("{n} sessions kept"),
    };
    let said = last_message(&app);
    assert!(said.starts_with("Echo: "));
    assert!(said.contains(&kept));
    assert_eq!(said.ends_with(" · u to undo"), !tagged.is_empty(), "undone with u when the drive was written");
}

/// Whichever set is open (of any few, with any lists marked), a set's format writes out that
/// set, offered a file named after it; with none open it says to open one first.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_sets_format_writes_out_whichever_set_is_open(tc: TestCase) {
    plugins_once_with(given);
    let drive = plugged();
    let mut app = shared_store(app_on(&drive.0));
    let dir = tempfile::tempdir().unwrap();
    let sets: Vec<Set> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
        .map(|i| Set {
            id: format!("3m2set{i}"),
            title: Some(format!("Night {i}")),
            date: chrono::NaiveDate::from_ymd_opt(2026, 10, 1 + i as u32).unwrap(),
            tracks: (0..=i)
                .map(|j| SetEntry { artist: format!("A{i}"), title: format!("T{j}"), ..SetEntry::added("") })
                .collect(),
            ..Set::default()
        })
        .collect();
    SetBook { sets: sets.clone(), ..SetBook::default() }.save(&Store::open_or_init(&app.store_root).unwrap()).unwrap();
    app.list_marks = tc.draw(gs::subsequences(vec!["Warmup".to_string()]));
    let Some(open) = tc.draw(gs::optional(gs::sampled_from(sets.clone()))) else {
        app.plugin_export("echo", "sheet", Some(drive.0.clone()));
        assert_eq!((app.modal.is_none(), last_message(&app)), (true, "open a set first"));
        return;
    };
    app.read_sets();
    #[track_caller]
    fn go(app: &mut App, id: NodeId) {
        wait(app, |a| a.loaded.as_ref().unwrap().tree_position(&id).is_some());
        app.focus = Focus::Playlists;
        app.playlist_state.select(app.loaded.as_ref().unwrap().tree_position(&id));
        key(app, KeyCode::Enter);
    }
    go(&mut app, NodeId::Browse);
    go(&mut app, NodeId::Category(Category::Sets));
    go(&mut app, NodeId::Group(Category::Sets, open.id.clone()));
    assert_eq!(app.open_set(), Some(open.id.clone()));
    let f = form(&mut app, "export the set");
    let named = format!("{} {}.txt", open.date, open.title.as_deref().unwrap());
    assert!(f.fields[0].text.ends_with(&named), "named after the set: {}", f.fields[0].text);
    let out = dir.path().join("out.txt");
    f.fields[0].text = out.display().to_string();
    press(&mut app, "\n");
    let (operation, _) = confirming(&mut app);
    assert_eq!(operation, "echo.export.sheet");
    say_yes(&mut app);
    assert_eq!(last_message(&app), "Echo: 1 file written");
    let lines: String = open.tracks.iter().map(|t| format!("{} - {}\n", t.artist, t.title)).collect();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), lines, "the set open, not another");
}
