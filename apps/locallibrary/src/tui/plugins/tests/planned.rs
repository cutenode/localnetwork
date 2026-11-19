//! A plugin's command planned first, about rekordbox's collection, in the player.

use locallibrary::plugin::PluginNotice;
use loclib_plugin::{CommandAction, HostAction, Suggestion};

use super::super::super::app::testing::{blank, confirming, echo_plugin_with, key, press, settle, wait};
use super::*;

/// `keep`: the test plugin's command planned first, about the collection alone.
const KEEP: &str = "
[[contributes.commands]]
name = \"keep\"
title = \"Keep a copy\"
mode = \"headless\"
where = [\"collection\"]
plan = true
long_running = true
params = { type = \"object\" }
";

/// A command about the collection is in the palette with no drive open, and in no drive's menu.
/// Planned first, it's confirmed as any plan is, saying who asks, and once confirmed it runs, its
/// answer in a panel.
#[test]
fn a_planned_command_about_the_collection_is_confirmed_then_runs() {
    let dir = tempfile::tempdir().unwrap();
    echo_plugin_with(dir.path(), KEEP);
    let (mut app, _store) = blank(Screen::Library);
    press(&mut app, ":");
    let (item, why) = entry(&app, "keep a copy");
    assert_eq!((item.detail.as_str(), why), ("Echo", None), "no drive needed");
    press(&mut app, "\u{1b}");
    assert!(offered(true, &[Where::Drive]).iter().all(|o| o.name != "keep"), "no drive's menu has it");
    press(&mut app, ":keep a copy\n");
    let (operation, _) = confirming(&mut app);
    assert_eq!(operation, "echo.command.keep");
    let s = screen(&mut app, 120, 40);
    for needle in ["Keep a copy?", "Echo: keep a copy", "y run"] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    settle(&mut app);
    press(&mut app, "y");
    wait(&mut app, |a| a.job.is_none());
    let answer = panel(&app);
    assert!(answer.contains("\"request\"") && !answer.contains("\"dry_run\": true"), "{answer}");
}

/// Echo suggesting its command `command` with `params`, saying `message`.
fn suggests_command(message: &str, command: &str, params: serde_json::Value) -> PluginNotice {
    let serde_json::Value::Object(params) = params else { panic!("params are an object") };
    let action = HostAction::Command(CommandAction { command: command.into(), params });
    PluginNotice { plugin: "echo".into(), message: message.into(), suggests: Some(Suggestion::Action(action)) }
}

/// A command a plugin suggests is reviewed from the palette as the user would run it, with the
/// params suggested: planned first, the question its dry run asks is a list to choose from, and
/// once chosen it's planned again with the answer, confirmed, then run. One the plugin doesn't
/// have starts nothing.
#[test]
fn a_suggested_planned_command_asks_then_is_confirmed_and_runs() {
    let dir = tempfile::tempdir().unwrap();
    echo_plugin_with(dir.path(), KEEP);
    let (mut app, _store) = blank(Screen::Library);
    let item = json!({ "label": "Monday", "value": "monday" });
    let question = json!({ "title": "Which backup?", "param": "backup", "items": [item] });
    let told = app.plugin_notices();
    told(suggests_command("keep a copy", "keep", json!({ "plan": { "choose": question } })));
    told(suggests_command("back up", "backup", json!({})));
    wait(&mut app, |a| a.suggested.len() == 2);
    press(&mut app, ":review: keep a copy\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(picker(&app).title, "Which backup?");
    key(&mut app, KeyCode::Enter);
    let (operation, _) = confirming(&mut app);
    assert_eq!(operation, "echo.command.keep");
    settle(&mut app);
    press(&mut app, "y");
    wait(&mut app, |a| a.job.is_none());
    let answer = panel(&app);
    assert!(answer.contains("\"backup\": \"monday\""), "{answer}");
    app.modal = None;
    press(&mut app, ":review: back up\n");
    wait(&mut app, |a| a.job.is_none());
    assert!(app.modal.is_none(), "nothing to run");
}
