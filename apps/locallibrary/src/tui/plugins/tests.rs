use loclib_plugin::{ListItem, Pair};
use ratatui::crossterm::event::KeyCode;
use serde_json::json;

use super::super::app::testing::{app_on, blank, demo_drive, echo_plugin, key, last_message, press, wait};
use super::super::palette::Picker;
use super::super::ui::screen;
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
    let (item, why) = entry(&app, "Answer the params");
    assert_eq!((item.detail.as_str(), why), ("Echo", Some("open a drive first")));
    assert_eq!(entry(&app, "Find a tracklist's tracks").0.detail, "Tracklist import");
    let p = picker(&app);
    let echo = p.items.iter().position(|i| i.label == "Answer the params").unwrap();
    assert!(p.items[..echo].iter().any(|i| i.label == "quit"), "the player's own first");

    let mut app = app_on(&root);
    press(&mut app, ":");
    assert_eq!(entry(&app, "Answer the params").1, None);
    press(&mut app, "\u{1b}");
    app.extra_roots.push(root.clone());
    press(&mut app, ":");
    assert_eq!(entry(&app, "Answer the params").1, Some("only on a plugged-in drive"));
}

/// The drive menu ends with plugins' commands, which run on that drive.
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
    assert_eq!(p.items.last().map(|i| i.label.as_str()), Some("Wait until cancelled"));
    assert_eq!(entry(&app, "Answer the params").1, None);
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
