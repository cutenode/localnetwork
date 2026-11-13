//! A plugin's command planned first, about rekordbox's collection, in the player.

use super::super::super::app::testing::{blank, confirming, echo_plugin_with, press, settle, wait};
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
