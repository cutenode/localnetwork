use loclib_plugin::{ListItem, Pair};
use ratatui::crossterm::event::KeyCode;
use serde_json::json;

use super::super::app::testing::{
    app_on, blank, confirming, demo_drive, echo_plugin, key, last_message, mounted, press, say_yes, wait,
};
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
    let (item, why) = entry(&app, "answer the params…");
    assert_eq!((item.detail.as_str(), why), ("Echo", Some("open a drive first")));
    assert_eq!(entry(&app, "find a tracklist's tracks…").0.detail, "Tracklist import");
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
    let manifest =
        manifest.replace(r#"capabilities = ["library:read", "library:plan"]"#, r#"capabilities = ["library:plan"]"#);
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
    say_yes(&mut app);
    assert!(last_message(&app).contains("u to undo"), "{}", last_message(&app));
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
    let plan = plugins.plan_op("echo", "playlist", params, context, None, &locallibrary::Ctx::silent()).unwrap();
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
    say_yes(&mut app);
    assert_eq!(last_message(&app), "Echo: 1 file written");
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
