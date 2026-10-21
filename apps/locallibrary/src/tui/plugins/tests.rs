use locallibrary::query::{FieldName, SortField};
use loclib_plugin::{ListItem, Pair};
use ratatui::crossterm::event::KeyCode;
use serde_json::json;

use super::super::app::Msg;
use super::super::app::testing::{
    app_on, blank, confirming, demo_drive, echo_plugin, key, last_message, mounted, press, say_yes, wait,
};
use super::super::palette::Picker;
use super::super::ui::screen;
use super::exporters::WRITE_TO;
use super::*;

/// The list open now.
fn picker(app: &App) -> &Picker {
    match &app.modal {
        Some(Modal::Pick(p)) => p,
        _ => panic!("no list open: {}", last_message(app)),
    }
}

/// The item labelled `label` in the list open, and why it can't be chosen.
fn entry<'a>(app: &'a App, label: &str) -> (&'a Item, Option<&'a str>) {
    let item = picker(app).items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label:?}"));
    (item, item.unavailable.as_deref())
}

/// The panel's lines, as one text.
fn panel(app: &App) -> String {
    match &app.modal {
        Some(Modal::Report(r)) => r.lines.iter().map(|(_, l)| l.as_str()).collect(),
        _ => panic!("no panel: {}", last_message(app)),
    }
}

/// Every plugin's commands are in the palette, by title with the plugin's name, after the
/// player's own; those of a plugin reading a drive say why they can't run without one, or on a
/// drive opened as a folder (which a plugin can't name).
#[test]
fn plugins_commands_are_in_the_palette() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let (mut app, _store) = blank(Screen::Library);
    press(&mut app, ":");
    let (item, why) = entry(&app, "answer the params…");
    assert_eq!((item.detail.as_str(), why), ("Echo", Some("open a drive first")));
    assert_eq!(entry(&app, "fetch cover art…").0.detail, "Cover art");
    let p = picker(&app);
    let echo = p.items.iter().position(|i| i.label == "answer the params…").unwrap();
    assert!(p.items[..echo].iter().any(|i| i.label == "quit"), "the player's own first");

    let mut app = app_on(&root);
    press(&mut app, ":");
    assert_eq!(entry(&app, "answer the params…").1, None);
    press(&mut app, "\u{1b}");
    app.extra_roots.push(root.clone());
    press(&mut app, ":");
    assert_eq!(entry(&app, "answer the params…").1, Some("only on a plugged-in drive"));
}

/// The drive menu ends with plugins' commands, operations, analyzers and export formats, which
/// run on that drive.
#[test]
fn plugins_commands_are_in_the_drive_menu() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    let info = locallibrary::api::drive_info(&locallibrary::api::DriveRef { drive: root.display().to_string() });
    app.drives = vec![info.unwrap()];
    app.drive_state.select(Some(0));
    app.view = Screen::Drives;
    press(&mut app, "e");
    let p = picker(&app);
    assert!(p.title.starts_with("Drive · "), "{}", p.title);
    assert_eq!(p.items.last().map(|i| i.label.as_str()), Some("export for echo-m3u…"));
    assert!(p.items.iter().any(|i| i.label == "analyze the tracks: file size"));
    assert!(p.items.iter().any(|i| i.label == "make a playlist…"));
    assert_eq!(entry(&app, "answer the params…").1, None);
    press(&mut app, "wait until\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Wait until cancelled"));
    press(&mut app, "\u{1b}");
    assert!(app.job.is_none());
}

/// A command with params asks for them first: one that's needed, one too short and one that
/// isn't of its type are each said on their field, and nothing runs until they do. What it
/// answers shows in a panel.
#[test]
fn a_command_asks_for_its_params_and_answers_in_a_panel() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    press(&mut app, ":answer the params\n");
    let s = screen(&mut app, 120, 40);
    for needle in ["Answer the params", "name", "required", "times", "loud", "○ no", "mood", "‹ none ›", "Who to greet"]
    {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    let form = |app: &App| match &app.modal {
        Some(Modal::Form(f)) => (f.selected, f.error.clone()),
        _ => panic!("no form: {}", last_message(app)),
    };
    press(&mut app, "\n");
    assert_eq!(form(&app), (0, Some("name is needed".into())));
    assert!(screen(&mut app, 120, 40).contains("⚠ name is needed"));
    press(&mut app, "A\n");
    assert_eq!(form(&app), (0, Some("name: \"A\" is shorter than 2 characters".into())));
    press(&mut app, "da\t3x\n");
    assert_eq!(form(&app), (1, Some("times takes a whole number, not \"3x\"".into())));
    key(&mut app, KeyCode::Backspace);
    press(&mut app, "\t ");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Right);
    press(&mut app, "\t\tdeep, house\n");
    assert!(app.job.is_some(), "running");
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Report(r)) = &app.modal else { panic!("no panel: {}", last_message(&app)) };
    assert_eq!(r.title, "Answer the params");
    let s = screen(&mut app, 120, 40);
    for needle in ["\"name\": \"Ada\"", "\"times\": 3", "\"loud\": true", "\"mood\": \"wild\"", "\"deep\","] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    press(&mut app, "x");
    assert!(app.modal.is_none(), "any other key closes it");
    // Esc closes the form, running nothing.
    press(&mut app, ":answer the params\n\u{1b}");
    assert!(app.modal.is_none() && app.job.is_none());
}

/// A plugin's hook gets where the user is: the drive, by its id or name, and the tracks marked.
#[test]
fn a_command_runs_where_the_user_is() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    let mut ids: Vec<u32> = app.loaded.as_ref().unwrap().library.tracks.iter().map(|t| t.id).collect();
    press(&mut app, "mjm:answer the params\nAda");
    for _ in 0..7 {
        key(&mut app, KeyCode::Down);
    }
    press(&mut app, " \n");
    wait(&mut app, |a| a.job.is_none());
    let text = panel(&app);
    let context: Value = serde_json::from_str(&text).unwrap();
    let mut tracks: Vec<u32> = serde_json::from_value(context["tracks"].clone()).unwrap();
    ids.sort();
    tracks.sort();
    assert_eq!(tracks, ids, "{text}");
    assert!(context["drive"].as_str().is_some_and(|d| !d.is_empty() && !d.contains('/')), "{text}");
}

/// A param from the selection starts as the drive the host would name to the plugin (its id), as
/// on the command line and in the sidecar, not as the drive list shows it.
#[test]
fn a_drive_from_the_selection_is_named_as_the_host_names_it() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let file = dir.path().join("echo.toml");
    let on =
        r#"properties = { name = { type = "string" }, on = { type = "string", x-from = "selection.drive" }, tracks"#;
    let manifest =
        std::fs::read_to_string(&file).unwrap().replacen(r#"properties = { name = { type = "string" }, tracks"#, on, 1);
    std::fs::write(&file, manifest).unwrap();
    let mut device = locallibrary::Device::open_export(&root).unwrap();
    let id = device.adopt(Some("Named")).unwrap().id.to_string();
    let mut app = app_on(&root);
    app.drives = vec![
        locallibrary::api::drive_info(&locallibrary::api::DriveRef { drive: root.display().to_string() }).unwrap(),
    ];
    assert_eq!(app.drive_label(&root), "Named");
    assert_eq!(app.plugin_drive(&root).as_deref(), Some(id.as_str()));
    press(&mut app, ":make a playlist…\n");
    let Some(Modal::Form(f)) = &app.modal else { panic!("no form: {}", last_message(&app)) };
    let on = f.fields.iter().find(|x| x.name == "on").map(|x| x.text.as_str());
    assert_eq!((f.title.as_str(), on), ("Make a playlist", Some(id.as_str())));
}

/// A param from the selection emptied in the form reaches the plugin left out, though tracks are
/// marked, and one typed in as given: what lets coverart's `fetch` take a playlist typed in over
/// the tracks selected (its own tests show it does, given that).
#[test]
fn a_selection_emptied_in_the_form_stays_empty() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let file = dir.path().join("echo.toml");
    let from = r#"context = { type = "boolean" }, tracks = { type = "array", items = { type = "integer" }, x-from = "selection.tracks" }, playlist = { type = "string", x-from = "selection.playlist" } } }"#;
    let manifest = std::fs::read_to_string(&file).unwrap().replacen(r#"context = { type = "boolean" } } }"#, from, 1);
    std::fs::write(&file, manifest).unwrap();
    let mut app = app_on(&root);
    press(&mut app, "mjm:answer the params\nAda");
    let field = |app: &App, name: &str| match &app.modal {
        Some(Modal::Form(f)) => f.fields.iter().position(|x| x.name == name).map(|i| (i, f.fields[i].text.clone())),
        _ => panic!("no form: {}", last_message(app)),
    };
    let (at, marked) = field(&app, "tracks").unwrap();
    assert!(!marked.is_empty(), "the marked tracks, to start with");
    for _ in 0..at {
        key(&mut app, KeyCode::Down);
    }
    while field(&app, "tracks").is_some_and(|(_, text)| !text.is_empty()) {
        key(&mut app, KeyCode::Backspace);
    }
    key(&mut app, KeyCode::Down);
    press(&mut app, "Sets/Friday\n");
    wait(&mut app, |a| a.job.is_none());
    let answered: Value = serde_json::from_str(&panel(&app)).unwrap();
    assert_eq!((answered.get("tracks"), &answered["playlist"]), (None, &json!("Sets/Friday")), "{answered}");
}

/// A command without params runs at once, in the background, with esc to stop it; the plugin's
/// log goes to the messages, never on the screen.
#[test]
fn a_command_runs_in_the_background_and_logs_to_the_messages() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    crate::plugins::log_to(Some(app.plugin_log()));
    press(&mut app, ":wait until\n");
    assert!(app.modal.is_none());
    assert!(screen(&mut app, 120, 30).contains("Wait until cancelled"), "the header says what's running");
    press(&mut app, "\u{1b}");
    assert_eq!((app.job.is_none(), last_message(&app)), (true, "cancelled"));

    press(&mut app, ":answer the params\nAda");
    for _ in 0..6 {
        key(&mut app, KeyCode::Down);
    }
    press(&mut app, "hello from echo\n");
    wait(&mut app, |a| a.log.iter().any(|(m, ..)| m == "echo: hello from echo"));
    assert_ne!(last_message(&app), "echo: hello from echo", "not in the footer");
    crate::plugins::log_to(None);
}

/// What a command answers: nothing or a line as a message, more as a panel, a view titled as it
/// says.
#[test]
fn answers_are_messages_or_panels() {
    let (mut app, _dir) = blank(Screen::Library);
    app.show_answer("Tidy".into(), CommandOutput::Headless(Value::Null));
    assert_eq!(last_message(&app), "Tidy: done");
    app.show_answer("Tidy".into(), CommandOutput::Headless(json!("3 files tidied")));
    assert_eq!(last_message(&app), "3 files tidied");
    assert!(app.modal.is_none());
    app.show_answer("Tidy".into(), CommandOutput::Headless(json!("one\ntwo")));
    let Some(Modal::Report(r)) = &app.modal else { panic!() };
    assert_eq!((r.title.as_str(), r.lines.len()), ("Tidy", 2));
    let view = View::Kv { title: Some("Counts".into()), pairs: vec![Pair { label: "tracks".into(), value: json!(2) }] };
    app.show_answer("Tidy".into(), CommandOutput::View(view));
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("Counts") && s.contains("tracks  2"), "{s}");
}

/// A view command's rows (and items) sent in parts fill a panel as they come, from the job still
/// running only, and the view it answers then takes the panel's place; another panel open is left
/// as it is.
#[test]
fn a_views_rows_fill_its_panel_as_they_come() {
    let (mut app, _dir) = blank(Screen::Library);
    let item = ListItem { label: "Friday".into(), detail: Some("12 tracks".into()) };
    let part = |rows: Vec<Vec<Value>>, items: Vec<ListItem>| Partial { id: json!(1), rows, items };
    app.show_partial("Titles".into(), part(vec![vec![json!("a"), json!(1)]], Vec::new()));
    app.show_partial("Titles".into(), part(vec![vec![json!("b"), Value::Null]], vec![item]));
    let Some(Modal::Report(r)) = &app.modal else { panic!("a panel") };
    let lines: Vec<String> = r.lines.iter().map(|(t, l)| format!("{t:?} {l}")).collect();
    assert_eq!(
        (r.title.as_str(), &lines[..]),
        ("Titles", &["Row a  1", "Row b  -", "Text Friday", "Faint   12 tracks"].map(String::from)[..])
    );
    // Another panel is left as it is.
    app.show_partial("Other".into(), part(vec![vec![json!("c")]], Vec::new()));
    let Some(Modal::Report(r)) = &app.modal else { panic!("a panel") };
    assert_eq!((r.title.as_str(), r.lines.len()), ("Titles", 4));
    let view =
        View::Table { title: None, columns: vec!["Title".into()], rows: vec![vec![json!("a")], vec![json!("b")]] };
    app.show_answer("Titles".into(), CommandOutput::View(view));
    let Some(Modal::Report(r)) = &app.modal else { panic!("a panel") };
    assert_eq!(r.lines.len(), 3, "the view, headings and all");

    // Only the job still running shows what it hands over.
    app.modal = None;
    app.task_showing("Listing".into(), |_, _| Ok(Box::new(|_: &mut App| {})));
    let job = app.job.as_ref().unwrap().id;
    let shown = |title: &str| -> super::super::edits::Done {
        let title = title.to_string();
        Box::new(move |app: &mut App| app.toast(title, Level::Info))
    };
    app.handle_msg(Msg::Shown { job: job + 1, update: shown("stale") });
    app.handle_msg(Msg::Shown { job, update: shown("current") });
    assert_eq!(last_message(&app), "current");
}

/// A secret setting is never filled in: typed in, it's kept in the keychain (a file, here), not
/// with the settings; left empty, it's kept as it is.
#[test]
fn a_secret_typed_in_the_form_is_kept_apart() {
    let (dir, _root) = demo_drive();
    echo_plugin(dir.path());
    let file = dir.path().join("echo.toml");
    let token = "\n[[contributes.settings]]\nkey = \"token\"\ntitle = \"Token\"\ntype = \"secret\"\n";
    std::fs::write(&file, std::fs::read_to_string(&file).unwrap() + token).unwrap();
    // SAFETY: as in `echo_plugin`, before the host reads its options.
    unsafe { std::env::set_var("LOCALLIBRARY_SESSION_STORE", "file") };
    let (mut app, _store) = blank(Screen::Library);
    let texts = |f: &super::super::form::Form| f.fields.iter().map(|x| x.text.clone()).collect::<Vec<_>>();
    assert_eq!(texts(settings_form(&mut app)).last().map(String::as_str), Some(""));
    press(&mut app, "\t\t\t\t");
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("Token (empty keeps it)"), "{s}");
    press(&mut app, "hunter2\n");
    wait(&mut app, |a| a.job.is_none());
    let plugins = crate::plugins::host().unwrap();
    let store = locallibrary::Store::open(&app.store_root).unwrap();
    // Not echoed on failure: CodeQL takes the app's toasts (a sign-in's names the account) for something secret.
    assert!(plugins.secrets("echo", Some(&store)).unwrap()["token"]);
    let settings = std::fs::read_to_string(app.store_root.join("plugins/echo/settings.json")).unwrap_or_default();
    assert!(!settings.contains("hunter2"), "{settings}");
    // Open again: not filled in; saved empty, kept.
    assert_eq!(texts(settings_form(&mut app)).last().map(String::as_str), Some(""));
    press(&mut app, "\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "nothing changed");
    assert!(plugins.secrets("echo", Some(&store)).unwrap()["token"]);
}

/// Each kind of view as a panel's lines: a list's details under their labels, a table aligned
/// under its headings (and said to be empty when it is), pairs aligned, text by its lines.
#[test]
fn views_are_drawn_generically() {
    let item = |label: &str, detail: Option<&str>| ListItem { label: label.into(), detail: detail.map(String::from) };
    let list = View::List { title: None, items: vec![item("Friday", Some("12 tracks")), item("Warmup", None)] };
    let lines = |v: &View| view_lines(v).into_iter().map(|(t, l)| format!("{t:?} {l}")).collect::<Vec<_>>();
    assert_eq!(lines(&list), ["Text Friday", "Faint   12 tracks", "Text Warmup"]);
    let rows = vec![vec![json!("Around the World"), json!(121.0)], vec![json!("Pick Up"), Value::Null]];
    let table = View::Table { title: None, columns: vec!["Title".into(), "BPM".into()], rows };
    assert_eq!(
        lines(&table),
        ["Columns Title             BPM", "Row Around the World  121.0", "Row Pick Up           -"]
    );
    let empty = View::Table { title: None, columns: vec!["Title".into()], rows: Vec::new() };
    assert_eq!(lines(&empty), ["Columns Title", "Faint nothing to show"]);
    let pairs = vec![Pair { label: "a".into(), value: json!(1) }, Pair { label: "bpm".into(), value: json!([1, 2]) }];
    assert_eq!(lines(&View::Kv { title: None, pairs }), ["Text a    1", "Text bpm  1, 2"]);
    assert_eq!(lines(&View::Text { title: None, text: "one\ntwo".into() }), ["Text one", "Text two"]);
    assert_eq!(lines(&View::Text { title: None, text: String::new() }), ["Faint nothing to show"]);

    // A table wider than the panel is cut at its edge, not wrapped, so its columns stay aligned.
    let (mut app, _dir) = blank(Screen::Library);
    let wide = vec![vec![json!("x".repeat(60)), json!("y".repeat(60)), json!("z".repeat(60))]];
    let view =
        View::Table { title: Some("Wide".into()), columns: vec!["A".into(), "B".into(), "C".into()], rows: wide };
    app.show_answer("T".into(), CommandOutput::View(view));
    let s = screen(&mut app, 80, 24);
    assert_eq!(s.lines().filter(|l| l.contains("xxxx") || l.contains("zzzz")).count(), 1, "{s}");
}

/// The actions menu (`e`) open now, as its items' labels and why each can't be done.
fn menu(app: &App) -> Vec<(String, Option<String>)> {
    match &app.modal {
        Some(Modal::Menu(m)) => m.items.iter().map(|i| (i.label.clone(), i.unavailable.clone())).collect(),
        _ => panic!("no menu: {}", last_message(app)),
    }
}

/// Operations change a drive, so they need one plugged in, whatever their plugin reads: in the
/// palette, and at the end of the actions menu (`e`), where an unplugged drive says so too. Those
/// that ask for params first end in "…".
#[test]
fn operations_are_offered_where_they_can_run() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    // An echo plugin that reads nothing: its commands run without a drive.
    let file = dir.path().join("echo.toml");
    let manifest = std::fs::read_to_string(&file).unwrap();
    // Nor provides anything, which would read the tracks.
    let provider =
        "[[contributes.providers]]\nname = \"tags\"\ntitle = \"The tags as they are\"\nkind = \"metadata\"\n";
    assert!(manifest.contains(provider));
    let manifest = manifest
        .replace(r#"capabilities = ["library:read", "library:plan"]"#, r#"capabilities = ["library:plan"]"#)
        .replace(provider, "");
    std::fs::write(&file, manifest).unwrap();
    let (mut app, _store) = blank(Screen::Library);
    press(&mut app, ":");
    assert_eq!(entry(&app, "answer the params…").1, None);
    assert_eq!(entry(&app, "make a playlist…").1, Some("open a drive first"));
    assert_eq!(entry(&app, "make a playlist…").0.detail, "Echo");
    assert_eq!(entry(&app, "fetch cover art…").1, Some("open a drive first"));

    let mut app = app_on(&root);
    press(&mut app, ":");
    assert_eq!(entry(&app, "make a playlist…").1, None);
    press(&mut app, "\u{1b}e");
    let items = menu(&app);
    let echo = items.iter().position(|(l, _)| l == "make a playlist…").expect("in the menu");
    assert!(items[..echo].iter().any(|(l, _)| l == "clear the crate"), "after the player's own");
    assert!(items.iter().all(|(l, _)| l != "answer the params…"), "commands change nothing: not here");
    assert_eq!(items[echo].1, None);
    press(&mut app, "\u{1b}");
    app.loaded.as_mut().unwrap().stale = true;
    press(&mut app, "e");
    assert_eq!(menu(&app)[echo].1.as_deref(), Some("the drive was unplugged"));
    press(&mut app, "\u{1b}");
    app.loaded.as_mut().unwrap().stale = false;
    app.extra_roots.push(root.clone());
    press(&mut app, "e");
    assert_eq!(menu(&app)[echo].1.as_deref(), Some("only on a plugged-in drive"));
}

/// An operation from the actions menu: its form starts with the marked tracks, it's planned in
/// the background, its plan (the plugin's summary first) is confirmed as any edit is, applied
/// with a backup, the drive read again, and `u` takes it back.
#[test]
fn an_operation_is_planned_confirmed_applied_and_undone() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    let ids: Vec<String> = app.loaded.as_ref().unwrap().library.tracks.iter().map(|t| t.id.to_string()).collect();
    press(&mut app, "mjme");
    let echo = menu(&app).iter().position(|(l, _)| l == "make a playlist…").unwrap();
    for _ in 0..echo {
        press(&mut app, "j");
    }
    press(&mut app, "\n");
    let Some(Modal::Form(f)) = &app.modal else { panic!("no form: {}", last_message(&app)) };
    assert_eq!(f.fields[1].text, ids.join(", "), "the marked tracks");
    press(&mut app, "Plugged\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
    assert_eq!(confirming(&mut app), ("echo.playlist".into(), "make the playlist Plugged".into()));
    let s = screen(&mut app, 120, 40);
    for needle in ["Write to ", "Echo: make the playlist Plugged", "+ Plugged", "y write", "u takes this back"] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    let said = say_yes(&mut app);
    // Not echoed on failure: CodeQL takes a toast written after an apply for something secret.
    assert!(said.contains("u to undo"));
    let has = |app: &App| app.loaded.as_ref().unwrap().library.playlists.iter().any(|p| p.name == "Plugged");
    assert!(has(&app), "read again");
    press(&mut app, "u");
    say_yes(&mut app);
    assert!(!has(&app), "undone: {}", last_message(&app));
}

/// Planning an operation is stopped with esc, and nothing shows; planned again, it's confirmed.
/// A plan that changes nothing says so, and one the plugin can't make says why, as an error.
#[test]
fn planning_an_operation_is_stopped_with_esc() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    press(&mut app, ":make a playlist…\nGone\n");
    assert!(app.job.is_some());
    press(&mut app, "\u{1b}");
    assert_eq!((app.job.is_none(), last_message(&app)), (true, "cancelled"));
    press(&mut app, ":make a playlist…\nKept\n");
    assert_eq!(confirming(&mut app).1, "make the playlist Kept");
    assert!(app.waiting.is_empty());
    say_yes(&mut app);
    press(&mut app, ":make a playlist…\nKept\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!((app.modal.is_none(), last_message(&app)), (true, "nothing to change"));
    // A file that isn't there, named as the platform names it.
    let list = dir.path().join("nowhere").join("list.m3u");
    press(&mut app, &format!(":make a playlist from a tracklist\n{}\tFriday\n", list.display()));
    wait(&mut app, |a| a.job.is_none());
    assert!(app.modal.is_none());
    let (said, level) = app.toast.clone().map(|t| (t.0, t.1)).unwrap();
    assert!(level == Level::Error && said.contains(&list.display().to_string()), "{said}");
}

/// A plan is a plugin's when its operation is one of a plugin's operations or exports, by the
/// plugin's id and its name (or `export.` and the format).
#[test]
fn plans_are_known_by_their_plugins() {
    let (dir, _root) = demo_drive();
    echo_plugin(dir.path());
    assert_eq!(planned_by("echo.playlist", false), Some("Echo"));
    assert_eq!(planned_by("tracklist.import", false), Some("Tracklist import"));
    assert_eq!(planned_by("echo.export.echo-m3u", true), Some("Echo"));
    assert_eq!(planned_by("interop.export.traktor", true), Some("Other DJ software"));
    assert_eq!(planned_by("echo.export.echo-m3u", false), None, "not an operation");
    assert_eq!(planned_by("echo.playlist", true), None, "not an export");
    assert_eq!(planned_by("echo.export.nope", true), None);
    assert_eq!(planned_by("echo.echo-m3u", true), None, "an export is `export.` and its format");
    // Planned by the host from what plugins found or worked out.
    assert_eq!(planned_by("plugins.provide", false), Some("Plugins"));
    assert_eq!(planned_by("plugins.apply_fields", false), Some("Plugins"));
    assert_eq!(planned_by("plugins.provide", true), None, "not an export");
    for not in ["echo.echo", "echo.nope", "echo", "edit", "edits.replay", "echoplaylist"] {
        assert_eq!(planned_by(not, false), None, "{not}");
    }
}

/// Titles are named as the player's own commands are, a name in capitals kept.
#[test]
fn titles_are_named_as_the_players_own() {
    assert_eq!(named("Fetch cover art"), "fetch cover art");
    assert_eq!(named("M3U playlist"), "M3U playlist");
    assert_eq!(named("A"), "A");
    assert_eq!(named("A playlist"), "a playlist");
    assert_eq!(named("3D view"), "3D view");
    assert_eq!(named("Éclair"), "éclair");
    assert_eq!(named(""), "");
}

/// The confirmation has what the plugin says about its plan, its warnings too, besides the edits.
#[test]
fn plugin_warnings_show_in_the_confirmation() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    let plugins = crate::plugins::host().unwrap();
    // The test plugin plans the edits it's given, saying "as given" and warning "given".
    let params = json!({ "name": "x", "edits": [{ "op": "create_playlist", "path": "Given" }] });
    let context = Context { drive: Some(root.display().to_string()), ..Context::default() };
    let plan = plugins
        .plan_op("echo", "playlist", params, context, None, &locallibrary::Ctx::silent())
        .unwrap()
        .plan()
        .unwrap();
    app.show(Modal::Confirm(super::super::edits::Confirm {
        drive: "DEMO".into(),
        root,
        plan: Box::new(plan),
        scroll: 0,
        shown: std::time::Instant::now(),
        after: None,
    }));
    let s = screen(&mut app, 120, 40);
    for needle in ["Echo: as given", "+ Given", "⚠ given"] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
}

/// An operation is planned on a drive: without one, it says so.
#[test]
fn an_operation_without_a_drive_isnt_planned() {
    let (dir, _root) = demo_drive();
    echo_plugin(dir.path());
    let (mut app, _store) = blank(Screen::Library);
    let manifest = crate::plugins::host().unwrap().manifest("echo").unwrap();
    let hook = Cli::Operation(&manifest.contributes.operations[0]);
    app.run_plugin(Run { plugin: "echo", hook, root: None, context: Context::default() }, json!({ "name": "x" }));
    assert_eq!((app.job.is_none(), last_message(&app)), (true, "open a drive first"));
}

/// The demo drive open with the echo plugin, its second track's audio file made bigger than the
/// first's (the echo plugin's analyzer measures `echo.bytes`, a file's size), not analysed yet.
fn sized() -> (tempfile::TempDir, App) {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let app = app_on(&root);
    let second = &app.loaded.as_ref().unwrap().library.tracks[1];
    std::fs::write(root.join(second.file_path.trim_start_matches('/')), "x".repeat(4000)).unwrap();
    (dir, app)
}

/// The tracks shown, by id.
fn shown(app: &App) -> Vec<u32> {
    let l = app.loaded.as_ref().unwrap();
    app.visible.iter().map(|&i| l.library.tracks[i].id).collect()
}

/// Analysing from the palette: on a drive, in the background with esc to stop it, saying how it
/// went; without one it says so. What it worked out shows in the column asked for (from the
/// palette, saved for the next run), right away.
#[test]
fn analyzers_run_from_the_palette_and_fill_their_columns() {
    let (_dir, _root) = demo_drive();
    let (mut app, _store) = blank(Screen::Library);
    echo_plugin(_dir.path());
    press(&mut app, ":");
    assert_eq!(entry(&app, "analyze the tracks: file size").1, Some("open a drive first"));
    press(&mut app, "\u{1b}");

    let (_dir, mut app) = sized();
    let ids: Vec<u32> = app.loaded.as_ref().unwrap().library.tracks.iter().map(|t| t.id).collect();
    press(&mut app, ":show the echo.bytes column\n");
    assert_eq!(app.field_columns, [FieldName::new("echo.bytes")]);
    wait(&mut app, |a| a.loaded.as_ref().unwrap().values().is_some());
    let s = screen(&mut app, 120, 30);
    assert!(s.contains("echo.bytes") && !s.contains("4000"), "not analysed yet:\n{s}");
    press(&mut app, ":analyze the tracks: file\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Analyzing"));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "file size: 2 tracks analyzed, 0 already");
    wait(&mut app, |a| a.loaded.as_ref().unwrap().values().is_some_and(|v| v.tracks.len() == 2));
    let s = screen(&mut app, 120, 30);
    assert!(s.contains("4000") && s.contains("500"), "{s}");
    press(&mut app, ":analyze the tracks: file\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "file size: 0 tracks analyzed, 2 already");
    assert_eq!(app.saved_state().field_columns, ["echo.bytes"]);
    press(&mut app, ":hide the echo.bytes column\n");
    assert!(app.field_columns.is_empty() && !screen(&mut app, 120, 30).contains("echo.bytes"));
    assert_eq!(ids.len(), 2);
}

/// The search box filters by a plugin's field as `tracks --filter` does: a track without a value
/// isn't matched; a typo says which field it meant.
#[test]
fn the_search_box_filters_by_plugins_fields() {
    let (_dir, mut app) = sized();
    let second = app.loaded.as_ref().unwrap().library.tracks[1].id;
    let reading = |a: &App| matches!(a.loaded.as_ref().unwrap().fields, super::super::app::Fields::Reading);
    press(&mut app, "/echo.bytes:>1000");
    assert_eq!(app.filter_note.as_deref(), Some("reading what analyzers worked out…"));
    wait(&mut app, |a| !reading(a));
    assert!(app.visible.is_empty(), "nothing analysed: nothing matched");
    press(&mut app, "\u{1b}");
    press(&mut app, ":analyze the tracks: file\n");
    wait(&mut app, |a| a.job.is_none());
    press(&mut app, "/echo.bytes:>1000");
    wait(&mut app, |a| !reading(a));
    assert_eq!(shown(&app), [second]);
    assert_eq!(app.filter_error, None);
    press(&mut app, "\u{1b}/echo.byte:1");
    let said = app.filter_error.clone().unwrap_or_default();
    assert!(said.contains("did you mean echo.bytes?"), "{said}");
}

/// A plugin's column sorts by its values when its header's clicked, tracks without one last
/// either way; a playlist can't be sorted on the drive by it.
#[test]
fn plugins_columns_sort_by_their_values() {
    let (_dir, mut app) = sized();
    press(&mut app, ":analyze the tracks: file\n");
    wait(&mut app, |a| a.job.is_none());
    let lib = app.loaded.as_ref().unwrap().library.clone();
    let (first, second) = (lib.tracks[0].id, lib.tracks[1].id);
    // The first's file changes: it has no value now.
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    std::fs::write(root.join(lib.tracks[0].file_path.trim_start_matches('/')), "y").unwrap();
    press(&mut app, ":show the echo.bytes column\n");
    wait(&mut app, |a| a.loaded.as_ref().unwrap().values().is_some());
    let header = super::super::app::testing::spot(&mut app, "echo.bytes");
    super::super::app::testing::mouse(&mut app, super::super::app::testing::LEFT, header);
    let field = SortField::Field(FieldName::new("echo.bytes"));
    assert_eq!(app.sort.map(|k| (k.field, k.descending)), Some((field, false)));
    assert_eq!(shown(&app), [second, first]);
    assert!(screen(&mut app, 120, 30).contains("echo.bytes↑"));
    super::super::app::testing::mouse(&mut app, super::super::app::testing::LEFT, header);
    assert_eq!(shown(&app), [second, first], "the one without a value stays last");
    // Shown a playlist, it can't be sorted on the drive by it.
    let mut t = app.target().unwrap();
    t.list = Some("Sets/Friday".into());
    let items = super::super::edits::menu_items(&t);
    let sort = items.iter().find(|i| i.action == super::super::edits::Action::SortList).unwrap();
    assert_eq!(sort.unavailable.as_deref(), Some("a plugin's column can't sort the drive's playlists"));
}

/// The settings of the plugin open in a form, from the palette.
fn settings_form(app: &mut App) -> &super::super::form::Form {
    press(app, ":echo plugin settings\n");
    match &app.modal {
        Some(Modal::Form(f)) => f,
        _ => panic!("no form: {}", last_message(app)),
    }
}

/// A plugin's settings are in the palette, with or without a drive, and open in a form as they're
/// set now; saved, only what changed is kept, and one left empty goes back to its default.
#[test]
fn plugins_settings_are_a_form_kept_in_the_store() {
    let (dir, _root) = demo_drive();
    echo_plugin(dir.path());
    let (mut app, _store) = blank(Screen::Library);
    press(&mut app, ":");
    assert_eq!(entry(&app, "Echo plugin settings…").1, None);
    assert_eq!(entry(&app, "Cover art plugin settings…").0.detail, "Cover art");
    assert!(picker(&app).items.iter().all(|i| i.label != "Sound analysis plugin settings…"), "it has none");
    press(&mut app, "\u{1b}");
    let (_drive, root) = demo_drive();
    let mut on = app_on(&root);
    on.drives = vec![
        locallibrary::api::drive_info(&locallibrary::api::DriveRef { drive: root.display().to_string() }).unwrap(),
    ];
    on.drive_state.select(Some(0));
    on.view = Screen::Drives;
    press(&mut on, "e");
    assert!(picker(&on).items.iter().all(|i| !i.label.ends_with("plugin settings…")), "not a drive's");
    press(&mut app, "\u{1b}");
    let texts = |f: &super::super::form::Form| f.fields.iter().map(|x| x.text.clone()).collect::<Vec<_>>();
    let f = settings_form(&mut app);
    assert_eq!(
        (f.title.as_str(), texts(f)),
        ("Echo settings", vec!["hello".into(), "1.5".into(), "false".into(), "calm".into()])
    );
    let s = screen(&mut app, 120, 40);
    for needle in ["Echo settings", "greeting", "Greeting (empty for hello)", "enter save · esc cancel"] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    // Changed: the greeting, the switch and the mood.
    for _ in 0.."hello".len() {
        key(&mut app, KeyCode::Backspace);
    }
    press(&mut app, "hi\t\t \t ");
    press(&mut app, "\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Saving settings"));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "Echo settings saved");
    let plugins = crate::plugins::host().unwrap();
    let store_root = app.store_root.clone();
    let kept = || {
        let store = locallibrary::Store::open(&store_root).unwrap();
        plugins.settings("echo", Some(&store)).unwrap()
    };
    assert_eq!(
        serde_json::to_value(kept()).unwrap(),
        json!({ "greeting": "hi", "speed": 1.5, "loud": true, "mood": "wild" })
    );
    let file = store_root.join("plugins/echo/settings.json");
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(written, json!({ "greeting": "hi", "loud": true, "mood": "wild" }), "only what changed");
    // Open again: as set now. Nothing changed: nothing written.
    assert_eq!(texts(settings_form(&mut app)), ["hi", "1.5", "true", "wild"]);
    press(&mut app, "\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "nothing changed");
    // The greeting left empty: back to its default.
    settings_form(&mut app);
    key(&mut app, KeyCode::Backspace);
    key(&mut app, KeyCode::Backspace);
    press(&mut app, "\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(kept()["greeting"], "hello");
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(written.get("greeting").is_none(), "{written}");
}

/// A value its setting can't take is said on its field, and nothing is saved.
#[test]
fn a_setting_of_the_wrong_kind_isnt_saved() {
    let (dir, _root) = demo_drive();
    echo_plugin(dir.path());
    let (mut app, _store) = blank(Screen::Library);
    settings_form(&mut app);
    press(&mut app, "\tfast\n");
    let Some(Modal::Form(f)) = &app.modal else { panic!("closed") };
    assert_eq!((f.selected, f.error.as_deref()), (1, Some("speed takes a number, not \"1.5fast\"")));
    assert!(app.job.is_none() && !app.store_root.join("plugins").exists());
}

/// Each kind of setting as the form's schema has it, with its title and default said.
#[test]
fn settings_are_a_schema_of_their_kinds() {
    let setting = |key: &str, kind, default: Option<Value>, choices: &[&str]| Setting {
        key: key.into(),
        title: key.to_uppercase(),
        kind,
        default,
        choices: choices.iter().map(|c| c.to_string()).collect(),
    };
    let schema = settings_schema(&[
        setting("a", SettingType::String, Some(json!("x")), &[]),
        setting("b", SettingType::Number, None, &[]),
        setting("c", SettingType::Boolean, Some(json!(true)), &[]),
        setting("d", SettingType::Enum, None, &["p", "q"]),
        setting("e", SettingType::File, None, &[]),
        setting("f", SettingType::Directory, None, &[]),
    ]);
    assert_eq!(
        schema["properties"],
        json!({
            "a": { "type": "string", "description": "A (empty for x)" },
            "b": { "type": "number", "description": "B" },
            "c": { "type": "boolean", "description": "C (empty for true)" },
            "d": { "type": "string", "enum": ["p", "q"], "description": "D" },
            "e": { "type": "string", "x-path": true, "description": "E" },
            "f": { "type": "string", "x-path": true, "description": "F" },
        })
    );
}

/// The export form open, its "write to" replaced with `to`.
fn export_to(app: &mut App, to: &std::path::Path) {
    press(app, ":export for echo\n");
    let Some(Modal::Form(f)) = &mut app.modal else { panic!("no form: {}", last_message(app)) };
    f.fields[0].text = to.display().to_string();
    press(app, "\n");
}

/// Exporting from the palette: on a drive plugged in, a form asks where (named after the drive)
/// and for the format's options; the plugin says what it would write, writing nothing, shown as
/// the command line's dry run is, and a yes writes it. Files there already are asked about as
/// overwriting, and a no leaves them be.
#[test]
fn an_export_is_planned_confirmed_and_written() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let (mut app, _store) = blank(Screen::Library);
    press(&mut app, ":");
    assert_eq!(entry(&app, "export for echo-m3u…").1, Some("open a drive first"));
    assert_eq!(entry(&app, "export for traktor…").0.detail, "Other DJ software");
    press(&mut app, "\u{1b}");
    app.plugin_export("echo", "echo-m3u", None);
    assert_eq!((app.modal.is_none(), last_message(&app)), (true, "open a drive first"));

    mounted(dir.path());
    let mut app = app_on(&root);
    press(&mut app, ":export for echo\n");
    let Some(Modal::Form(f)) = &app.modal else { panic!("no form: {}", last_message(&app)) };
    let name = app.drive_label(&root);
    assert_eq!((f.title.as_str(), f.fields[0].text.clone()), ("M3U playlist", format!("~/Music/{name}.m3u")));
    press(&mut app, "\u{1b}");
    let out = dir.path().join("list.m3u");
    export_to(&mut app, &out);
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
    let (operation, summary) = confirming(&mut app);
    assert_eq!(operation, "echo.export.echo-m3u");
    assert!(
        summary.starts_with("write 1 file(s) (") && summary.ends_with(&format!("to {}", out.display())),
        "{summary}"
    );
    assert!(!out.exists(), "a dry run writes nothing");
    let s = screen(&mut app, 120, 40);
    for needle in ["Write 1 file?", "2 tracks", "list.m3u  playlist,", "y write"] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    assert_eq!(say_yes(&mut app), "Echo: 1 file written");
    assert!(std::fs::read_to_string(&out).unwrap().starts_with("#EXTM3U"));

    // There already: asked about as overwriting, and left be on a no.
    std::fs::write(&out, "mine").unwrap();
    export_to(&mut app, &out);
    confirming(&mut app);
    let s = screen(&mut app, 120, 40);
    for needle in ["Overwrite 1 file?", "(already there)", "This replaces files already there.", "y overwrite"] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    super::super::app::testing::settle(&mut app);
    press(&mut app, "n");
    assert_eq!((last_message(&app), std::fs::read_to_string(&out).unwrap().as_str()), ("nothing written", "mine"));
}

/// An export is never written onto a drive: planning it says so, as an error, and writes nothing.
#[test]
fn an_export_onto_the_drive_is_refused() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    mounted(dir.path());
    let mut app = app_on(&root);
    export_to(&mut app, &root.join("list.m3u"));
    wait(&mut app, |a| a.job.is_none());
    let (said, level) = app.toast.clone().map(|t| (t.0, t.1)).unwrap();
    assert!(level == Level::Error && said.contains("choose a folder on this computer"), "{said}");
    assert!(app.modal.is_none() && !root.join("list.m3u").exists());
}

/// A format's options are fields of the export's form, after where to write it, which a folder
/// format names after the drive and the format.
#[test]
fn an_exports_form_has_the_formats_options() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    press(&mut app, ":export for traktor\n");
    let Some(Modal::Form(f)) = &app.modal else { panic!("no form: {}", last_message(&app)) };
    let names: Vec<&str> = f.fields.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["write_to", "location_root", "skip_analysis"]);
    let name = app.drive_label(&root);
    assert_eq!(f.fields[0].text, format!("~/Music/{name} for traktor"));
    assert!(f.fields[0].required && f.fields[2].is_switch());
}

/// Playlists marked in the tree are what's exported, as with rekordbox XML: the plugin gets
/// exactly those.
#[test]
fn an_export_takes_the_marked_playlists() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    mounted(dir.path());
    let mut app = app_on(&root);
    // (The test plugin takes any names: it only says which it got.)
    let names = ["Sets/Friday".to_string(), "Warmup".to_string()];
    app.list_marks = names.to_vec();
    export_to(&mut app, &dir.path().join("marked.m3u"));
    confirming(&mut app);
    let Some(Modal::Confirm(c)) = &app.modal else { panic!() };
    assert_eq!(c.export().unwrap().plan.notes, [format!("playlists: {}", names.join(", "))]);
    assert!(screen(&mut app, 120, 40).contains(&format!("playlists: {}", names[0])));
    press(&mut app, "\u{1b}");
    app.list_marks.clear();
    export_to(&mut app, &dir.path().join("all.m3u"));
    confirming(&mut app);
    let Some(Modal::Confirm(c)) = &app.modal else { panic!() };
    assert!(c.export().unwrap().plan.notes.is_empty(), "none marked: the whole drive");
}

/// Plugins' commands and operations are in the menus their manifests say: the actions menu has
/// those for tracks or a playlist, each needing what it works on selected, and a drive's menu
/// those for a drive.
#[test]
fn menus_offer_what_plugins_say_where() {
    use super::super::edits::{Action, Target, menu_items};
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    // Its operation for a playlist alone, and its view command for tracks and a drive.
    let file = dir.path().join("echo.toml");
    let manifest = std::fs::read_to_string(&file)
        .unwrap()
        .replace(r#"where = ["track", "playlist", "drive"]"#, r#"where = ["playlist"]"#)
        .replacen("mode = \"view\"\n", "mode = \"view\"\nwhere = [\"track\", \"drive\"]\n", 1);
    std::fs::write(&file, manifest).unwrap();
    let offered = |tracks: &[&str], node: Option<(&str, bool)>| {
        let t = Target {
            root: root.clone(),
            tracks: tracks.iter().map(|t| t.to_string()).collect(),
            node: node.map(|(path, folder)| (path.to_string(), folder)),
            ..Target::default()
        };
        let plugins = menu_items(&t).into_iter().filter(|i| matches!(i.action, Action::Plugin(..)));
        plugins.map(|i| (i.label, i.unavailable)).collect::<Vec<_>>()
    };
    let said = |label: &str, why: Option<&str>| (label.to_string(), why.map(String::from));
    let track = Some("no track selected");
    let playlist = Some("select a playlist");
    assert_eq!(
        offered(&["/Contents/a.mp3"], None),
        [
            said(
                "make a playlist from a tracklist: M3U, CSV (Spotify, TIDAL, Beatport... exports), rekordbox text or XML, or \"Artist - Title\" lines…",
                playlist
            ),
            said("list the drive's tracks", None),
            said("make a playlist…", playlist),
        ]
    );
    let on_a_list: Vec<_> = offered(&[], Some(("Sets/Friday", false))).into_iter().map(|(_, why)| why).collect();
    assert_eq!(on_a_list, [None, track.map(String::from), None]);
    let on_a_folder: Vec<_> = offered(&[], Some(("Sets", true))).into_iter().map(|(_, why)| why).collect();
    assert_eq!(on_a_folder, [playlist, track, playlist].map(|w| w.map(String::from)));

    let app = app_on(&root);
    let drive: Vec<String> = app.plugin_items(Some(&root), &[Where::Drive]).into_iter().map(|(_, i)| i.label).collect();
    assert!(drive.iter().any(|l| l == "list the drive's tracks") && drive.iter().any(|l| l == "answer the params…"));
    assert!(!drive.iter().any(|l| l == "make a playlist…"), "for a playlist alone: {drive:?}");
    let everything = app.plugin_items(Some(&root), &EVERYWHERE);
    assert!(everything.iter().any(|(_, i)| i.label == "make a playlist…"), "the palette has it");
}

/// A secret setting is typed as a password is: the form shows a dot for each character.
#[test]
fn a_secret_is_typed_unseen() {
    let secret = Setting {
        key: "token".into(),
        title: "Token".into(),
        kind: SettingType::Secret,
        default: None,
        choices: Vec::new(),
    };
    let mut form = Form::with("Keys".into(), Some(settings_schema(&[secret])), &Map::new(), For::Settings("echo"));
    form.fields[0].text = "hunter2".into();
    let (mut app, _store) = blank(Screen::Library);
    app.modal = Some(Modal::Form(form));
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("•••••••") && !s.contains("hunter2"), "{s}");
}

/// An analyzer with a key and a BPM writes them to the tracks from the palette: a form asks which
/// (both, to start with) and whether over what the tracks have, then the plan comes back to
/// confirm, as any edit's does, and is applied.
#[test]
fn an_analyzers_key_and_bpm_are_written_from_the_palette() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let file = dir.path().join("echo.toml");
    let heard = "\n[[contributes.analyzers]]\nname = \"heard\"\ntitle = \"Heard\"\nversion = \"1\"\n\
                 fields = { tone = \"string\", tempo = \"number\" }\nroles = { key = \"tone\", bpm = \"tempo\" }\n";
    std::fs::write(&file, std::fs::read_to_string(&file).unwrap() + heard).unwrap();
    let mut app = app_on(&root);
    let before = app.loaded.as_ref().unwrap().library.tracks.clone();
    // What the test plugin's `heard` answers: what's in the audio file.
    for t in &before {
        let key = locallibrary::harmony::Camelot::parse(&t.key).unwrap().parallel().code();
        let answer = json!({ "tone": key, "tempo": t.bpm * 2.0 });
        std::fs::write(root.join(t.file_path.trim_start_matches('/')), answer.to_string()).unwrap();
    }
    press(&mut app, ":analyze the tracks: heard\n");
    wait(&mut app, |a| a.job.is_none());
    press(&mut app, ":write keys and BPMs to the tracks: heard\n");
    let s = screen(&mut app, 120, 30);
    for needle in ["Write from heard", "fields", "key, bpm", "overwrite", "min confidence"] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    // Over what the tracks have.
    press(&mut app, "\t \n");
    assert_eq!(
        confirming(&mut app),
        ("plugins.apply_fields".into(), "write key and bpm from Heard to 2 track(s)".into())
    );
    // Said first in the confirmation, as a plugin's own operation's summary is.
    let s = screen(&mut app, 120, 30);
    assert!(s.contains("Plugins: write key and bpm from Heard to 2 track(s)"), "{s}");
    say_yes(&mut app);
    let after = locallibrary::Device::open_export(&root).unwrap().library().unwrap().tracks;
    for (b, a) in before.iter().zip(&after) {
        let key = locallibrary::harmony::Camelot::parse(&b.key).unwrap().parallel().note();
        assert_eq!((&a.key, a.bpm), (&key, b.bpm * 2.0));
    }
    // Without a drive open, it says so.
    let (mut app, _store) = blank(Screen::Library);
    press(&mut app, ":");
    assert_eq!(entry(&app, "write keys and BPMs to the tracks: heard…").1, Some("open a drive first"));
}

// -------------------------------------------------------------------------------------------------
// Properties: the plugins' entries driven with keys, drawn values, at drawn sizes.
// -------------------------------------------------------------------------------------------------

use hegel::TestCase;
use hegel::generators as gs;

use super::super::app::testing::{Plugged, plugged, plugins_once, settle};
use super::super::palette::Then;

/// Where a case runs plugins: with no drive open, on one plugged in, or on a folder opened by
/// path (which a plugin can't name).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Opened {
    Nowhere,
    Plugged,
    Folder,
}

/// The library shown with a drive as `at` says, its store in a folder of its own, and the drive
/// (taken out when it's dropped).
#[hegel::test_helper]
fn somewhere(tc: &TestCase) -> (Opened, App, Option<Plugged>, tempfile::TempDir) {
    plugins_once();
    let at = tc.draw(gs::sampled_from(&[Opened::Nowhere, Opened::Plugged, Opened::Plugged, Opened::Folder][..]));
    let (mut app, store) = blank(Screen::Library);
    if at == Opened::Nowhere {
        return (at, app, None, store);
    }
    let drive = plugged();
    let store_root = app.store_root.clone();
    app = app_on(&drive.0);
    app.store_root = store_root;
    if at == Opened::Folder {
        app.extra_roots.push(drive.0.clone());
    }
    (at, app, Some(drive), store)
}

/// Whether the palette's entry for `act` is a plugin's.
fn plugins_own(act: &Act) -> bool {
    matches!(
        act,
        Act::Plugin(..)
            | Act::Analyze(..)
            | Act::ApplyFields(..)
            | Act::Export(..)
            | Act::FieldColumn(_)
            | Act::PluginSettings(_)
            | Act::ProviderSettings
            | Act::Fetch(_)
    )
}

/// The palette or the drive's menu open now, as its items and what each does.
fn palette(app: &App) -> (Vec<Item>, Vec<Act>) {
    let p = picker(app);
    let (Then::Act(acts) | Then::Drive { acts, .. }) = &p.then else { panic!("not the palette: {}", p.title) };
    (p.items.clone(), acts.clone())
}

/// The list open moved (with the arrows) to its `i`th item.
fn move_to(app: &mut App, i: usize) {
    for _ in 0..200 {
        let at = picker(app).state.selected().unwrap_or(0);
        match at.cmp(&i) {
            std::cmp::Ordering::Equal => return,
            std::cmp::Ordering::Less => key(app, KeyCode::Down),
            std::cmp::Ordering::Greater => key(app, KeyCode::Up),
        }
    }
    panic!("the arrows didn't get to {i}");
}

/// The command or operation `name` of the plugin `plugin`, as its manifest has it: its title and
/// the names of its params, in order.
fn hook(plugin: &str, name: &str) -> (String, Vec<String>) {
    let c = &crate::plugins::host().unwrap().manifest(plugin).unwrap().contributes;
    let command = c.commands.iter().find(|x| x.name == name).map(|x| (&x.title, x.params.as_ref()));
    let (title, params) = command
        .or_else(|| c.operations.iter().find(|x| x.name == name).map(|x| (&x.title, x.params.as_ref())))
        .unwrap();
    (title.clone(), properties(params).map(|(k, _)| k.clone()).collect())
}

/// The form open now: its title and its fields' names.
fn form_open(app: &App) -> (String, Vec<String>) {
    match &app.modal {
        Some(Modal::Form(f)) => (f.title.clone(), f.fields.iter().map(|x| x.name.clone()).collect()),
        _ => panic!("no form: {}", last_message(app)),
    }
}

/// The plugin entries of the palette, and of the drive's menu (in the Drives view), come after the
/// player's own commands and never take one's name, so typing a command's whole name still finds
/// one of the player's first; the drive's menu has no settings, columns or fetching. Each entry
/// that can't run where the player is says why when it's chosen and does nothing (nor does its
/// analyzer or export asked for anyway); one that can does what it is (a form for its params, its
/// settings or which provider to fetch from, a job, a panel, a column) and esc backs out of it to
/// the view it started in.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_palettes_plugin_entries_do_what_they_say(tc: TestCase) {
    let (at, mut app, drive, _store) = somewhere(&tc);
    // Every plugin here reads the drive: its hooks need one plugged in, its settings and columns
    // don't.
    let why = match at {
        Opened::Nowhere => Some("open a drive first"),
        Opened::Folder => Some("only on a plugged-in drive"),
        Opened::Plugged => None,
    };
    let in_menu = drive.is_some() && tc.draw(gs::booleans());
    if let Some(d) = drive.as_ref().filter(|_| in_menu) {
        let info = locallibrary::api::drive_info(&locallibrary::api::DriveRef { drive: d.0.display().to_string() });
        app.drives = vec![info.unwrap()];
        app.drive_state.select(Some(0));
        app.view = Screen::Drives;
    }
    let view = app.view;
    let open = if in_menu { "e" } else { ":" };
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        press(&mut app, open);
        let (items, acts) = palette(&app);
        let first = acts.iter().position(plugins_own).expect("plugins' entries");
        assert!(acts[first..].iter().all(plugins_own), "the player's own first");
        let own: Vec<String> = items[..first].iter().map(|i| i.label.clone()).collect();
        for i in &items[first..] {
            assert!(!own.contains(&i.label), "{:?} takes a command's name", i.label);
        }
        for (i, act) in items.iter().zip(&acts).skip(first) {
            let needs = matches!(
                act,
                Act::Plugin(..) | Act::Analyze(..) | Act::ApplyFields(..) | Act::Export(..) | Act::Fetch(_)
            );
            assert_eq!(i.unavailable.as_deref(), why.filter(|_| needs), "{}", i.label);
            assert!(needs || !in_menu, "{:?} isn't a drive's", i.label);
        }
        let name = tc.draw(gs::sampled_from(own));
        press(&mut app, &name);
        let p = picker(&app);
        let first_shown = p.shown.first().map(|&i| &p.items[i].label);
        assert!(p.shown.first().is_some_and(|&i| i < first), "{name:?} typed: {first_shown:?} first");
        press(&mut app, "\u{1b}");
        press(&mut app, open);

        // Only what runs here and now: the echo plugin's hooks and analyzer; the others' hooks
        // (each asks for params first, nothing run until they're given), writing an analyzer's
        // values (a form first too), exports, settings and columns.
        let safe: Vec<usize> = (first..acts.len())
            .filter(|&i| match acts[i] {
                Act::Plugin(p, n) => p == "echo" || !hook(p, n).1.is_empty(),
                Act::Analyze(p, _) => p == "echo",
                _ => true,
            })
            .collect();
        let i = tc.draw(gs::sampled_from(safe));
        move_to(&mut app, i);
        let columns = app.field_columns.clone();
        key(&mut app, KeyCode::Enter);
        if let Some(why) = &items[i].unavailable {
            assert_eq!(picker(&app).selected(), Some(i), "still open on it");
            assert_eq!((last_message(&app), app.job.is_none()), (why.as_str(), true));
            press(&mut app, "\u{1b}");
            assert!(app.modal.is_none() && app.view == view);
            // Asked for anyway (a key, a stale list), an analyzer or an export says the same.
            if !in_menu && matches!(acts[i], Act::Analyze(..) | Act::ApplyFields(..) | Act::Export(..)) {
                app.toast = None;
                app.perform(acts[i], None);
                assert_eq!((last_message(&app), app.job.is_none(), app.modal.is_none()), (why.as_str(), true, true));
            }
            continue;
        }
        let w = tc.draw(gs::integers::<u16>().min_value(1).max_value(160));
        let h = tc.draw(gs::integers::<u16>().min_value(1).max_value(50));
        assert_eq!(screen(&mut app, w, h).lines().count(), h as usize);
        chosen(&mut app, acts[i], &columns, drive.as_ref());
        assert!(app.modal.is_none() && app.job.is_none() && app.view == view, "back where it started");
    }
}

/// What choosing the plugin's entry for `act` does, and esc backing out of it (`columns` the
/// fields' columns shown before, the drive the one open).
fn chosen(app: &mut App, act: Act, columns: &[FieldName], drive: Option<&Plugged>) {
    match act {
        Act::Plugin(p, n) => {
            let (title, params) = hook(p, n);
            if !params.is_empty() {
                assert_eq!(form_open(app), (title, params));
                press(app, "\u{1b}");
            } else if n == "wait" {
                assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some(title.as_str()));
                press(app, "\u{1b}");
                assert_eq!(last_message(app), "cancelled");
            } else {
                wait(app, |a| a.job.is_none());
                match n {
                    // The drive's tracks, as a table titled with the drive.
                    "titles" => {
                        let root = &drive.unwrap().0;
                        let Some(Modal::Report(r)) = &app.modal else { panic!("no panel: {}", last_message(app)) };
                        let lib = locallibrary::Device::open_export(root).unwrap().library().unwrap();
                        let shown: Vec<&str> = r.lines.iter().skip(1).map(|(_, l)| l.trim_end()).collect();
                        let titles: Vec<&str> = lib.tracks.iter().map(|t| t.title.as_str()).collect();
                        assert_eq!((r.title.clone(), shown), (app.drive_label(root), titles));
                        press(app, "x");
                    }
                    _ => assert_eq!(last_message(app), format!("{title}: done")),
                }
            }
        }
        Act::Analyze(..) => {
            assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Analyzing"));
            wait(app, |a| a.job.is_none());
            // The demo drive's two tracks: analysed now, or by an entry chosen before.
            let fresh = ["file size: 2 tracks analyzed, 0 already", "file size: 0 tracks analyzed, 2 already"];
            assert!(fresh.contains(&last_message(app)));
        }
        Act::ApplyFields(p, a) => {
            let m = crate::plugins::host().unwrap().manifest(p).unwrap();
            let a = m.contributes.analyzers.iter().find(|x| x.name == a).unwrap();
            let fields = ["fields", "overwrite", "min_confidence"].map(String::from).to_vec();
            assert_eq!(form_open(app), (format!("Write from {}", named(&a.title)), fields));
            press(app, "\u{1b}");
        }
        Act::Export(p, f) => {
            let m = crate::plugins::host().unwrap().manifest(p).unwrap();
            let e = m.contributes.exporters.iter().find(|e| e.format == f).unwrap();
            let (title, fields) = form_open(app);
            assert_eq!((title, fields[0].as_str()), (e.title.clone(), WRITE_TO));
            press(app, "\u{1b}");
        }
        Act::PluginSettings(p) => {
            let m = crate::plugins::host().unwrap().manifest(p).unwrap();
            let keys: Vec<String> = m.contributes.settings.iter().map(|s| s.key.clone()).collect();
            assert_eq!(form_open(app), (format!("{} settings", m.name), keys));
            press(app, "\u{1b}");
        }
        Act::ProviderSettings => {
            assert_eq!(form_open(app), ("Provider settings".into(), vec!["auto_pick".into()]));
            press(app, "\u{1b}");
        }
        // A form asking which provider and whether over what tracks have, for the track selected.
        Act::Fetch(kind) => match &app.modal {
            Some(Modal::Form(_)) => {
                let title = match kind {
                    loclib_plugin::ProviderKind::Artwork => "Fetch cover art",
                    loclib_plugin::ProviderKind::Metadata => "Fetch track details",
                };
                assert_eq!(form_open(app), (title.to_string(), vec!["from".to_string(), "replace".into()]));
                press(app, "\u{1b}");
            }
            _ => assert_eq!(last_message(app), "no track selected"),
        },
        Act::FieldColumn(name) => {
            assert!(app.modal.is_none());
            assert_ne!(app.field_columns.contains(&name), columns.contains(&name), "{name:?} toggled");
            assert_eq!(app.field_columns.len().abs_diff(columns.len()), 1);
        }
        other => panic!("{other:?} isn't a plugin's"),
    }
}

/// The kinds of step `0..n` in an order drawn for the case.
#[hegel::test_helper]
fn turns(tc: &TestCase, n: usize) -> Vec<usize> {
    let mut kinds: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        kinds.swap(i, tc.draw(gs::integers::<usize>().max_value(i)));
    }
    kinds
}

/// The form open now.
fn the_form(app: &App) -> &super::super::form::Form {
    match &app.modal {
        Some(Modal::Form(f)) => f,
        _ => panic!("no form: {}", last_message(app)),
    }
}

/// The form open moved to its field `to`, with the arrows or tab, forwards or back, or a click on
/// it (a right click first, which does nothing).
#[hegel::test_helper]
fn to_field(tc: &TestCase, app: &mut App, to: usize) {
    use super::super::app::testing::{LEFT, mouse};
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let f = the_form(app);
    let (n, at) = (f.fields.len(), f.selected);
    let (code, times) = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => (tc.draw(gs::sampled_from(&[KeyCode::Down, KeyCode::Tab][..])), (to + n - at) % n),
        1 => (tc.draw(gs::sampled_from(&[KeyCode::Up, KeyCode::BackTab][..])), (at + n - to) % n),
        _ => {
            screen(app, 120, 40);
            let r = app.areas.menu;
            mouse(app, MouseEventKind::Down(MouseButton::Right), (r.x + 2, r.y + 1));
            mouse(app, LEFT, (r.x + 2, r.y + 1 + to as u16));
            (KeyCode::Null, 0)
        }
    };
    for _ in 0..times {
        key(app, code);
    }
    assert_eq!(the_form(app).selected, to);
}

/// What's typed in the echo command's form, field by field (see `echo_plugin`): a name, a number
/// of times, the switch, the mood (as many places right from none), a file, tags, a line to log
/// and whether to answer the context instead.
#[derive(Debug)]
struct Typed {
    name: &'static str,
    times: &'static str,
    loud: bool,
    mood: usize,
    file: &'static str,
    tags: &'static str,
    log: &'static str,
    context: bool,
}

/// What the form should send for `t`, or the field it should stop at and why, as the schema
/// asks: the name needed and at least two characters, the times a whole number (written as an
/// id may be, `#7`), files made absolute (`~` the home folder), tags split at commas.
fn expected(t: &Typed) -> Result<Value, (usize, String)> {
    let name = t.name.trim();
    if name.is_empty() {
        return Err((0, "name is needed".into()));
    }
    let mut given = serde_json::Map::new();
    given.insert("name".into(), json!(name));
    let times = t.times.trim();
    if !times.is_empty() {
        let n = times.strip_prefix('#').unwrap_or(times).parse::<i64>();
        let n = n.map_err(|_| (1, format!("times takes a whole number, not {times:?}")))?;
        given.insert("times".into(), json!(n));
    }
    given.insert("loud".into(), json!(t.loud));
    if t.mood > 0 {
        given.insert("mood".into(), json!(["calm", "wild"][t.mood - 1]));
    }
    let file = t.file.trim();
    if !file.is_empty() {
        let home = file.strip_prefix("~/").map(|rest| std::env::home_dir().unwrap().join(rest));
        let path = std::path::absolute(home.unwrap_or_else(|| file.into())).unwrap();
        given.insert("file".into(), json!(path.display().to_string()));
    }
    if !t.tags.trim().is_empty() {
        let tags: Vec<&str> = t.tags.split(',').map(str::trim).filter(|x| !x.is_empty()).collect();
        given.insert("tags".into(), json!(tags));
    }
    if !t.log.is_empty() {
        given.insert("log".into(), json!(t.log));
    }
    given.insert("context".into(), json!(t.context));
    if name.chars().count() < 2 {
        return Err((0, format!("name: {name:?} is shorter than 2 characters")));
    }
    Ok(Value::Object(given))
}

/// A command's params filled in with keys, in any order, moving between fields every way there
/// is: the form refuses what doesn't fit the schema (on the field it's about, saying why, running
/// nothing) and sends exactly what was typed otherwise, as the plugin's answer shows; its context
/// is the drive and the tracks marked (else the one selected). What the plugin logs goes to the
/// messages, not the footer. Esc closes a form refused, back to the library.
#[hegel::test(test_cases = 25, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_commands_form_sends_what_fits_its_schema(tc: TestCase) {
    plugins_once();
    let drive = plugged();
    let mut app = app_on(&drive.0);
    crate::plugins::log_to(Some(app.plugin_log()));
    let lib = app.loaded.as_ref().unwrap().library.clone();
    let marked: Vec<bool> = lib.tracks.iter().map(|_| tc.draw(gs::booleans())).collect();
    app.marks = lib.tracks.iter().zip(&marked).filter(|(_, m)| **m).map(|(t, _)| t.file_path.clone()).collect();
    let t = Typed {
        name: tc.draw(gs::sampled_from(&["", "  ", "A", "😹", "Ada", " Bo ", "日本"][..])),
        times: tc.draw(gs::sampled_from(&["", "3", "-2", "#7", " 12 ", "3x", "1.5"][..])),
        loud: tc.draw(gs::booleans()),
        mood: tc.draw(gs::integers::<usize>().max_value(2)),
        file: tc.draw(gs::sampled_from(&["", "list.m3u", "/tmp/x.m3u", "~/Music/x.m3u", "a b/c"][..])),
        tags: tc.draw(gs::sampled_from(&["", "deep", "deep, house", " , x ,", "日本, 😹"][..])),
        log: tc.draw(gs::sampled_from(&["", "hello from echo", "日本 😹"][..])),
        context: tc.draw(gs::booleans()),
    };
    press(&mut app, ":answer the params\n");
    assert_eq!(the_form(&app).fields.len(), 8);
    for field in turns(&tc, 8) {
        to_field(&tc, &mut app, field);
        let text = |s: &str| (!s.is_empty()).then(|| s.to_string());
        let typed = match field {
            0 => text(t.name),
            1 => text(t.times),
            4 => text(t.file),
            5 => text(t.tags),
            6 => text(t.log),
            _ => None,
        };
        if let Some(typed) = typed {
            // Now and then a slip, taken back.
            if tc.draw(gs::booleans()) {
                press(&mut app, "x");
                key(&mut app, KeyCode::Backspace);
            }
            press(&mut app, &typed);
        }
        let flip = |tc: &TestCase| tc.draw(gs::sampled_from(&[KeyCode::Char(' '), KeyCode::Right, KeyCode::Left][..]));
        match field {
            2 if t.loud => key(&mut app, flip(&tc)),
            7 if t.context => key(&mut app, flip(&tc)),
            3 if tc.draw(gs::booleans()) => (0..t.mood).for_each(|_| key(&mut app, KeyCode::Right)),
            3 => (0..(3 - t.mood) % 3).for_each(|_| key(&mut app, KeyCode::Left)),
            _ => {}
        }
    }
    let (w, h) = (
        tc.draw(gs::integers::<u16>().min_value(1).max_value(140)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(40)),
    );
    assert_eq!(screen(&mut app, w, h).lines().count(), h as usize);
    press(&mut app, "\n");
    match expected(&t) {
        Err((i, why)) => {
            let f = the_form(&app);
            assert_eq!((f.selected, f.error.as_deref(), app.job.is_none()), (i, Some(why.as_str()), true), "{t:?}");
            assert!(unspaced(&screen(&mut app, 120, 40)).contains(&unspaced(&format!("⚠ {why}"))));
            press(&mut app, "\u{1b}");
            assert!(app.modal.is_none() && app.job.is_none() && app.view == Screen::Library);
        }
        Ok(sent) => {
            assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Answer the params"), "{t:?}");
            wait(&mut app, |a| a.job.is_none());
            let answer: Value = serde_json::from_str(&panel(&app)).unwrap();
            if t.context {
                let device = locallibrary::Device::open(&drive.0).unwrap();
                let named = device.id().map_or_else(|| device.name(), |id| id.to_string());
                let mut ids: Vec<u32> = serde_json::from_value(answer["tracks"].clone()).unwrap();
                ids.sort();
                let mut want: Vec<u32> =
                    lib.tracks.iter().zip(&marked).filter(|(_, m)| **m).map(|(t, _)| t.id).collect();
                if want.is_empty() {
                    want.push(lib.tracks[app.selected_track().unwrap()].id);
                }
                want.sort();
                assert_eq!((answer["drive"].as_str(), ids), (Some(named.as_str()), want));
            } else {
                assert_eq!(answer, sent);
            }
            if !t.log.is_empty() {
                let line = format!("echo: {}", t.log);
                wait(&mut app, |a| a.log.iter().any(|(m, ..)| *m == line));
                assert_ne!(last_message(&app), line, "not in the footer");
            }
        }
    }
    crate::plugins::log_to(None);
}

/// Words a drawn view holds: wide characters and emoji, nothing, and a line long enough to wrap.
const WORDS: &[&str] = &[
    "Friday",
    "日本語のタイトル",
    "😹",
    "Around the World",
    "",
    "a much longer line of words, long enough to wrap in a narrow panel and then some more",
];

#[hegel::test_helper]
fn any_word(tc: &TestCase) -> String {
    tc.draw(gs::sampled_from(WORDS)).to_string()
}

/// A value a drawn view's cell holds: text, numbers, nothing, a switch, a list.
#[hegel::test_helper]
fn any_value(tc: &TestCase) -> Value {
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => json!(any_word(tc).chars().take(20).collect::<String>()),
        1 => json!(121.5),
        2 => json!(-3),
        3 => Value::Null,
        4 => json!(true),
        _ => json!(["deep", "日本"]),
    }
}

/// A view of any kind, with up to so many items, rows or pairs (a table's cells short enough to
/// fit its columns), titled or not.
#[hegel::test_helper]
fn any_view(tc: &TestCase) -> View {
    let title = tc.draw(gs::booleans()).then(|| any_word(tc));
    let n = tc.draw(gs::integers::<usize>().max_value(12));
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => View::List {
            title,
            items: (0..n)
                .map(|_| ListItem { label: any_word(tc), detail: tc.draw(gs::booleans()).then(|| any_word(tc)) })
                .collect(),
        },
        1 => {
            let columns: Vec<String> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)))
                .map(|_| tc.draw(gs::sampled_from(&["Title", "BPM", "日本", "😹"][..])).to_string())
                .collect();
            let rows = (0..n).map(|_| columns.iter().map(|_| any_value(tc)).collect()).collect();
            View::Table { title, columns, rows }
        }
        2 => View::Kv { title, pairs: (0..n).map(|_| Pair { label: any_word(tc), value: any_value(tc) }).collect() },
        _ => View::Text { title, text: (0..n).map(|_| any_word(tc)).collect::<Vec<_>>().join("\n") },
    }
}

/// Text without its spaces: wrapping takes some off, and a wide character's second column is
/// one on the screen.
fn unspaced(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// What a command's view shows: every item's label and detail, every heading and cell, every
/// label and value, every line, in order.
fn cells(v: &View) -> Vec<String> {
    match v {
        View::List { items, .. } => {
            items.iter().flat_map(|i| [Some(i.label.clone()), i.detail.clone()]).flatten().collect()
        }
        View::Table { columns, rows, .. } => {
            columns.iter().cloned().chain(rows.iter().flatten().map(crate::plugins::params::cell)).collect()
        }
        View::Kv { pairs, .. } => pairs.iter().flat_map(|p| [p.label.clone(), cell(&p.value)]).collect(),
        View::Text { text, .. } => text.lines().map(String::from).collect(),
        View::Choose(_) => unreachable!("never drawn"),
    }
}

/// A command's answer, a view of any kind or plain JSON, is a message when it's nothing or a line
/// and otherwise a panel titled as the view says (else as the command is), holding every cell of
/// the view. Drawn at any size it fills the screen without a panic, and scrolled with the keys
/// from top to bottom it shows each of its lines (a table's cut at the panel's edge, the rest
/// wrapped); any other key closes it.
#[hegel::test(test_cases = 60)]
fn a_commands_answer_shows_whole_at_any_size(tc: TestCase) {
    let (mut app, _dir) = blank(Screen::Library);
    let answer = match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => CommandOutput::Headless(Value::Null),
        1 => CommandOutput::Headless(json!(any_word(&tc))),
        2 => CommandOutput::Headless(json!((0..3).map(|_| any_word(&tc)).collect::<Vec<_>>().join("\n"))),
        3 => CommandOutput::Headless(json!({ "tracks": [1, 2], "said": any_word(&tc) })),
        _ => CommandOutput::View(any_view(&tc)),
    };
    let (title, view) = match &answer {
        CommandOutput::View(v) => (v.title().unwrap_or("Tidy").to_string(), Some(v.clone())),
        _ => ("Tidy".to_string(), None),
    };
    let message = match &answer {
        CommandOutput::Headless(Value::Null) => Some("Tidy: done".to_string()),
        CommandOutput::Headless(Value::String(s)) if !s.contains('\n') => Some(s.clone()),
        _ => None,
    };
    app.show_answer("Tidy".into(), answer);
    if let Some(said) = message {
        assert_eq!((app.modal.is_none(), last_message(&app)), (true, said.as_str()));
        return;
    }
    let lines = match &app.modal {
        Some(Modal::Report(r)) => {
            assert_eq!(r.title, title);
            r.lines.clone()
        }
        _ => panic!("no panel"),
    };
    if let Some(v) = &view {
        let all = unspaced(&lines.iter().map(|(_, l)| l.as_str()).collect::<String>());
        let mut rest = all.as_str();
        for c in cells(v) {
            let c = unspaced(&c);
            let at = rest.find(&c).unwrap_or_else(|| panic!("{c:?} isn't in {all:?}"));
            rest = &rest[at + c.len()..];
        }
    }
    let (w, h) = (
        tc.draw(gs::integers::<u16>().min_value(1).max_value(160)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(50)),
    );
    assert_eq!(screen(&mut app, w, h).lines().count(), h as usize);
    if w >= 8 && h >= 4 {
        // The panel's rows, as scrolling one at a time shows them; each row's text by where it
        // is in the whole.
        let (pw, inner_w) = (w.min(100), w.min(100) as usize - 4);
        let mut rows: Vec<Option<String>> = Vec::new();
        let mut last = None;
        loop {
            let s = screen(&mut app, w, h);
            let Some(Modal::Report(r)) = &app.modal else { panic!("closed") };
            // Drawn at the end already: scrolled no further.
            if last == Some(r.scroll) {
                break;
            }
            last = Some(r.scroll);
            for (k, row) in panel_rows(&s, w, h, pw).into_iter().enumerate() {
                let at = r.scroll as usize + k;
                if rows.len() <= at {
                    rows.resize(at + 1, None);
                }
                rows[at] = Some(row);
            }
            key(&mut app, KeyCode::Down);
        }
        let shown: String = rows.iter().map(|r| r.clone().expect("a row never shown")).collect();
        let want: String = lines
            .iter()
            .map(|(tone, l)| match tone {
                Tone::Columns | Tone::Row => super::super::ui::clip(l, inner_w),
                _ => l.clone(),
            })
            .collect();
        assert_eq!(unspaced(&shown), unspaced(&want), "{w}x{h}");
    }
    press(&mut app, "x");
    assert!(app.modal.is_none(), "any other key closes it");
}

/// The text inside the borders and padding of the panel `pw` columns wide, centred on the screen
/// `s` (`w` by `h`), row by row. (A wide character takes two columns, the second drawn as a
/// space.)
fn panel_rows(s: &str, w: u16, h: u16, pw: u16) -> Vec<String> {
    let rows: Vec<Vec<char>> = s.lines().map(|l| l.chars().collect()).collect();
    let (left, right) = (((w - pw) / 2) as usize, ((w - pw) / 2 + pw - 1) as usize);
    let edge = |r: &[char], a: char, b: char| r.get(left) == Some(&a) && r.get(right) == Some(&b);
    for top in (0..rows.len()).filter(|&t| edge(&rows[t], '╭', '╮')) {
        let sides = rows[top + 1..].iter().take_while(|r| edge(r, '│', '│')).count();
        let centred = top == (h as usize - (sides + 2)) / 2;
        if centred && rows.get(top + 1 + sides).is_some_and(|r| edge(r, '╰', '╯')) {
            return rows[top + 1..=top + sides].iter().map(|r| r[left + 2..right - 1].iter().collect()).collect();
        }
    }
    panic!("no panel {pw} wide in\n{s}");
}

/// The playlists on the drive at `root`, as it reads from its databases: each by its path, with
/// its tracks.
fn on_drive(root: &std::path::Path) -> std::collections::BTreeSet<(String, Vec<u32>)> {
    let lib = locallibrary::Device::open_export(root).unwrap().library().unwrap();
    let path = |p: &locallibrary::Playlist| {
        let (mut p, mut parts) = (p.parent_id, vec![p.name.clone()]);
        while let Some(parent) = p.and_then(|id| lib.playlists.iter().find(|q| q.id == id)) {
            parts.insert(0, parent.name.clone());
            p = parent.parent_id;
        }
        parts.join("/")
    };
    lib.playlists.iter().map(|p| (path(p), p.track_ids.clone())).collect()
}

/// The actions menu open moved (with the arrows or j and k) to the item labelled `label`.
fn menu_to(app: &mut App, label: &str) {
    let at = menu(app).iter().position(|(l, _)| l == label).unwrap_or_else(|| panic!("no {label:?}"));
    for _ in 0..100 {
        let Some(Modal::Menu(m)) = &app.modal else { panic!("no menu") };
        let now = m.state.selected().unwrap_or(0);
        match now.cmp(&at) {
            std::cmp::Ordering::Equal => return,
            std::cmp::Ordering::Less => press(app, "j"),
            std::cmp::Ordering::Greater => key(app, KeyCode::Up),
        }
    }
    panic!("the arrows didn't get to {label:?}");
}

/// A key, with ctrl held.
fn ctrl(app: &mut App, c: char) {
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
    app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

/// A plugin's operation, from the palette or the actions menu, with a playlist's name drawn: esc
/// while it's planned leaves the drive as it was; a name there already changes nothing, and says
/// so; otherwise the confirmation has the plugin's summary, a no writes nothing, a yes writes
/// the playlist (the drive read again shows it) and u takes the drive back to how it was.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_operation_is_planned_applied_and_taken_back(tc: TestCase) {
    plugins_once();
    let drive = plugged();
    let mut app = app_on(&drive.0);
    let before = on_drive(&drive.0);
    let name = tc.draw(gs::sampled_from(&["Plugged", "日本 😹", "Friday", "Sets/New", "Sets/Friday"][..]));
    if tc.draw(gs::booleans()) {
        press(&mut app, ":make a playlist…\n");
    } else {
        press(&mut app, "e");
        menu_to(&mut app, "make a playlist…");
        key(&mut app, KeyCode::Enter);
    }
    assert_eq!(form_open(&app).0, "Make a playlist");
    press(&mut app, name);
    press(&mut app, "\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
    if tc.draw(gs::booleans()) {
        press(&mut app, "\u{1b}");
        assert_eq!((app.job.is_none(), app.modal.is_none(), last_message(&app)), (true, true, "cancelled"));
        assert_eq!(on_drive(&drive.0), before);
        return;
    }
    wait(&mut app, |a| a.job.is_none());
    if before.iter().any(|(p, _)| p == name) {
        assert_eq!((app.modal.is_none(), last_message(&app)), (true, "nothing to change"));
        return;
    }
    assert_eq!(confirming(&mut app), ("echo.playlist".into(), format!("make the playlist {name}")));
    let (w, h) = (
        tc.draw(gs::integers::<u16>().min_value(1).max_value(160)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(50)),
    );
    assert_eq!(screen(&mut app, w, h).lines().count(), h as usize);
    let s = unspaced(&screen(&mut app, 120, 40));
    assert!(s.contains(&unspaced(&format!("Echo: make the playlist {name}"))), "{s}");
    if tc.draw(gs::booleans()) {
        settle(&mut app);
        press(&mut app, tc.draw(gs::sampled_from(&["n", "\u{1b}", "q"][..])));
        assert_eq!((app.modal.is_none(), last_message(&app)), (true, "nothing written"));
        assert_eq!(on_drive(&drive.0), before);
        return;
    }
    say_yes(&mut app);
    // The playlist, empty, and the folders it's in, if they weren't there.
    let parts: Vec<&str> = name.split('/').collect();
    let made: std::collections::BTreeSet<(String, Vec<u32>)> = (1..=parts.len())
        .map(|n| (parts[..n].join("/"), Vec::new()))
        .filter(|(p, _)| before.iter().all(|(q, _)| q != p))
        .collect();
    let now = on_drive(&drive.0);
    assert_eq!(now.difference(&before).cloned().collect::<std::collections::BTreeSet<_>>(), made);
    assert!(before.is_subset(&now), "nothing else changed");
    let read = &app.loaded.as_ref().unwrap().library;
    assert!(read.playlists.iter().any(|p| name.ends_with(&p.name)), "read again");
    press(&mut app, "u");
    say_yes(&mut app);
    assert_eq!(on_drive(&drive.0), before, "taken back: {}", last_message(&app));
}

/// An export from the palette or the drive's menu, written where the form's typed to, with
/// playlists marked in the tree or none: planning writes nothing (esc stops it); the confirmation
/// asks to write the file, or to overwrite it when it's there, naming the playlists marked; a yes
/// writes every track's file into it, anything else leaves what was there. Onto the drive itself,
/// it's refused.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_export_is_planned_then_written_or_left_be(tc: TestCase) {
    plugins_once();
    let drive = plugged();
    let mut app = app_on(&drive.0);
    let out_dir = tempfile::tempdir().unwrap();
    let onto_drive = tc.draw(gs::integers::<u8>().max_value(5)) == 0;
    let out = if onto_drive { drive.0.join("list.m3u") } else { out_dir.path().join("list.m3u") };
    let there = tc.draw(gs::booleans()) && !onto_drive;
    if there {
        std::fs::write(&out, "mine").unwrap();
    }
    // (The test plugin takes any names: it only says which it got.)
    let lists = ["Sets/Friday", "Warmup", "日本 😹"];
    app.list_marks = lists.iter().filter(|_| tc.draw(gs::booleans())).map(|l| l.to_string()).collect();
    if tc.draw(gs::booleans()) {
        press(&mut app, ":export for echo\n");
    } else {
        let info = locallibrary::api::drive_info(&locallibrary::api::DriveRef { drive: drive.0.display().to_string() });
        app.drives = vec![info.unwrap()];
        app.drive_state.select(Some(0));
        app.view = Screen::Drives;
        press(&mut app, "e");
        let at = picker(&app).items.iter().position(|i| i.label == "export for echo-m3u…").unwrap();
        move_to(&mut app, at);
        key(&mut app, KeyCode::Enter);
    }
    assert_eq!(form_open(&app).1[0], WRITE_TO);
    ctrl(&mut app, 'u');
    press(&mut app, &out.display().to_string());
    press(&mut app, "\n");
    let was = |out: &std::path::Path| std::fs::read_to_string(out).ok();
    let left = if there { Some("mine".to_string()) } else { None };
    if !onto_drive && tc.draw(gs::booleans()) {
        press(&mut app, "\u{1b}");
        assert_eq!((app.job.is_none(), last_message(&app), was(&out)), (true, "cancelled", left));
        return;
    }
    wait(&mut app, |a| a.job.is_none());
    if onto_drive {
        let (said, level) = app.toast.clone().map(|t| (t.0, t.1)).unwrap();
        assert!(level == Level::Error && said.contains("choose a folder on this computer"), "{said}");
        assert!(app.modal.is_none() && !out.exists());
        return;
    }
    let (operation, summary) = confirming(&mut app);
    assert_eq!(operation, "echo.export.echo-m3u");
    assert!(summary.ends_with(&format!("to {}", out.display())), "{summary}");
    assert_eq!(was(&out), left, "a dry run writes nothing");
    let Some(Modal::Confirm(c)) = &app.modal else { panic!() };
    let notes = c.export().unwrap().plan.notes.clone();
    let marked = (!app.list_marks.is_empty()).then(|| format!("playlists: {}", app.list_marks.join(", ")));
    assert_eq!(notes, Vec::from_iter(marked));
    let s = screen(&mut app, 120, 50);
    let ask = if there { "Overwrite 1 file?" } else { "Write 1 file?" };
    assert!(s.contains(ask), "{ask:?} isn't in\n{s}");
    settle(&mut app);
    if tc.draw(gs::booleans()) {
        press(&mut app, tc.draw(gs::sampled_from(&["n", "\u{1b}", "q"][..])));
        assert_eq!((last_message(&app), was(&out)), ("nothing written", left));
        return;
    }
    assert_eq!(say_yes(&mut app), "Echo: 1 file written");
    let written = was(&out).unwrap();
    assert!(written.starts_with("#EXTM3U\n"), "{written}");
    for t in &app.loaded.as_ref().unwrap().library.tracks {
        assert!(written.lines().any(|l| l == t.file_path), "{} isn't in {written}", t.file_path);
    }
}

/// The echo plugin's settings form filled in with keys, a value of each kind drawn: one its
/// setting can't take is refused on its field, keeping nothing. Otherwise each is kept as typed
/// (left empty, its default), only those set apart from their defaults are written, the form
/// opens again as it was saved, and saving that again changes nothing. Esc keeps nothing.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn settings_are_kept_as_the_form_has_them(tc: TestCase) {
    plugins_once();
    let (mut app, _store) = blank(Screen::Library);
    let greeting = tc.draw(gs::sampled_from(&["", "hi", "日本 😹", "hello"][..]));
    let speed = tc.draw(gs::sampled_from(&["", "2", "0.5", "1.5", "-1", "fast"][..]));
    let loud = tc.draw(gs::booleans());
    let mood = tc.draw(gs::integers::<usize>().max_value(2));
    press(&mut app, ":echo plugin settings\n");
    assert_eq!(form_open(&app).1, ["greeting", "speed", "loud", "mood"]);
    for field in turns(&tc, 4) {
        to_field(&tc, &mut app, field);
        match field {
            0 | 1 => {
                if tc.draw(gs::booleans()) {
                    ctrl(&mut app, 'u');
                } else {
                    (0..5).for_each(|_| key(&mut app, KeyCode::Backspace));
                }
                press(&mut app, [greeting, speed][field]);
            }
            2 if loud => press(&mut app, " "),
            // From calm, its default.
            3 => (0..(mood + 2) % 3).for_each(|_| key(&mut app, KeyCode::Right)),
            _ => {}
        }
    }
    let file = app.store_root.join("plugins/echo/settings.json");
    if tc.draw(gs::integers::<u8>().max_value(4)) == 0 {
        press(&mut app, "\u{1b}");
        assert!(app.modal.is_none() && app.job.is_none() && !file.exists());
        return;
    }
    press(&mut app, "\n");
    // A whole number is kept whole (2, not 2.0).
    let number = speed.parse::<i64>().map(|n| json!(n)).or_else(|_| speed.parse::<f64>().map(|f| json!(f))).ok();
    if !speed.is_empty() && number.is_none() {
        let f = the_form(&app);
        let why = format!("speed takes a number, not {speed:?}");
        assert_eq!((f.selected, f.error.as_deref(), app.job.is_none()), (1, Some(why.as_str()), true));
        press(&mut app, "\u{1b}");
        assert!(!file.exists());
        return;
    }
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "Echo settings saved");
    let defaults = json!({ "greeting": "hello", "speed": 1.5, "mood": "calm" });
    let mut given = serde_json::Map::new();
    if !greeting.is_empty() {
        given.insert("greeting".into(), json!(greeting));
    }
    if let Some(n) = number {
        given.insert("speed".into(), n);
    }
    given.insert("loud".into(), json!(loud));
    if mood > 0 {
        given.insert("mood".into(), json!(["calm", "wild"][mood - 1]));
    }
    let mut kept = defaults.as_object().unwrap().clone();
    kept.extend(given.clone());
    let plugins = crate::plugins::host().unwrap();
    let store = locallibrary::Store::open(&app.store_root).unwrap();
    assert_eq!(
        serde_json::to_value(plugins.settings("echo", Some(&store)).unwrap()).unwrap(),
        Value::Object(kept.clone())
    );
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    given.retain(|k, v| defaults.get(k) != Some(v));
    assert_eq!(written, Value::Object(given), "only what's set apart from its default");
    press(&mut app, ":echo plugin settings\n");
    let texts: Vec<String> = the_form(&app).fields.iter().map(|f| f.text.clone()).collect();
    let keys = ["greeting", "speed", "loud", "mood"];
    assert_eq!(texts, keys.map(|k| cell(&kept[k])));
    press(&mut app, "\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "nothing changed");
}

/// Whatever playlists a plugin's operation plans (the test plugin plans the edits it's given,
/// saying "as given" and warning "given"), the confirmation has its summary first, each playlist
/// it makes, and its warning once; a yes makes them all on the drive and u takes them all back.
#[hegel::test(test_cases = 5, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_plugins_plan_is_confirmed_and_applied_as_it_says(tc: TestCase) {
    plugins_once();
    let drive = plugged();
    let mut app = app_on(&drive.0);
    let before = on_drive(&drive.0);
    let names = ["Given", "日本 😹", "Warmup", "Late night"];
    let mut made: Vec<&str> = names.into_iter().filter(|_| tc.draw(gs::booleans())).collect();
    if made.is_empty() {
        made.push(names[tc.draw(gs::integers::<usize>().max_value(names.len() - 1))]);
    }
    let edits: Vec<Value> = made.iter().map(|p| json!({ "op": "create_playlist", "path": p })).collect();
    let manifest = crate::plugins::host().unwrap().manifest("echo").unwrap();
    let hook = Cli::Operation(&manifest.contributes.operations[0]);
    let run = Run { plugin: "echo", hook, root: Some(drive.0.clone()), context: Context::default() };
    app.run_plugin(run, json!({ "name": "x", "edits": edits }));
    assert_eq!(confirming(&mut app), ("echo.playlist".into(), "as given".into()));
    let s = unspaced(&screen(&mut app, 120, 50));
    assert!(s.contains(&unspaced("Echo: as given")), "{s}");
    assert_eq!(s.matches("⚠given").count(), 1, "{s}");
    for p in &made {
        assert!(s.contains(&unspaced(&format!("+ {p}"))), "{p:?} isn't in {s}");
    }
    say_yes(&mut app);
    let now = on_drive(&drive.0);
    let new: std::collections::BTreeSet<String> = now.difference(&before).map(|(p, _)| p.clone()).collect();
    assert_eq!(new, made.iter().map(|p| p.to_string()).collect());
    press(&mut app, "u");
    say_yes(&mut app);
    assert_eq!(on_drive(&drive.0), before, "taken back: {}", last_message(&app));
}

mod asking;
mod events;
