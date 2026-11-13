//! A plugin's command planned first, about rekordbox's collection, on the command line and in the
//! sidecar: planned, shown and asked about, then run as it was planned.

use super::*;

/// `keep`: the test plugin's command planned first, about the collection alone, running long.
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

/// An [`Env`] with the echo plugin and `keep`.
fn keeping() -> Env {
    let mut env = Env::new();
    env.plugins(&[&format!("{}{KEEP}", echo())]);
    env
}

/// `keep`'s params, planning to write `file`.
fn writing(file: &std::path::Path) -> String {
    json!({ "plan": { "files": [{ "path": file, "what": "a copy", "bytes": 7 }] } }).to_string()
}

/// `--dry-run` shows the plan and stops; without `--yes` and a terminal to ask on, nothing runs
/// (exit 3); `--yes` runs it as it was planned; a plan that fails runs nothing.
#[test]
fn a_planned_command_is_shown_asked_about_then_run() {
    let env = keeping();
    let file = env.outside("copy.txt");
    let params = writing(&file);
    let dry = env.json(&["echo", "keep", "--params", &params, "--dry-run"]);
    assert_eq!((dry["outcome"].as_str(), dry["applied"].as_bool()), (Some("dry_run"), Some(false)), "{dry}");
    assert_eq!(dry["plan"]["operation"], "echo.command.keep");
    assert_eq!(dry["plan"]["files"][0]["exists"], false);
    assert!(!file.exists(), "a dry run writes nothing");
    let shown = env.run(&["echo", "keep", "--params", &params]);
    assert_eq!(shown.code, 3, "{}", shown.err);
    assert!(shown.out.contains("write 1 file(s) (7 B)") && shown.out.contains("a copy"), "{}", shown.out);
    assert!(!file.exists(), "not confirmed: nothing runs");
    let ran = env.json(&["--yes", "echo", "keep", "--params", &params]);
    assert_eq!(ran["outcome"], "applied", "{ran}");
    let (planned, run) = (&ran["plan"]["summary"][0], &ran["report"]["request"]);
    let planned: Value = serde_json::from_str(planned.as_str().unwrap()).unwrap();
    assert_eq!((planned["dry_run"].as_bool(), run.get("dry_run")), (Some(true), None), "{ran}");
    assert_eq!(planned["params"], run["params"]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "written");
    let failing = json!({ "fail": "conflict", "plan": { "files": [{ "path": env.outside("never.txt") }] } });
    let r = env.run(&["--yes", "echo", "keep", "--params", &failing.to_string()]);
    assert_ne!(r.code, 0, "{}", r.out);
    assert!(!env.outside("never.txt").exists());
}

/// A command about the collection alone takes no drive, and `collection --help` lists it.
#[test]
fn a_command_about_the_collection_is_listed_with_it() {
    let env = keeping();
    let help = env.ok(&["collection", "--help"]);
    assert!(help.contains("Plugins' commands about the collection:\n  locallibrary echo keep  Keep a copy"), "{help}");
    let r = env.run(&["echo", "keep", "USB", "--dry-run"]);
    assert_eq!(r.code, 2, "no drive to give: {}", r.err);
}

/// The sidecar plans a command planned first (`plugins.command.plan`) and applies the plan by its
/// id, running it as it was planned; `plugins.run` refuses it.
#[test]
fn the_sidecar_plans_then_applies_a_planned_command() {
    let env = keeping();
    let file = env.outside("copy.txt");
    let mut s = Rpc::start(env.command());
    let params: Value = serde_json::from_str(&writing(&file)).unwrap();
    // A drive sent along is dropped: the command is about the collection alone.
    let keep = json!({ "plugin": "echo", "command": "keep", "params": params, "context": { "drive": "USB" } });
    let refused = s.call(1, "plugins.run", keep.clone());
    assert_eq!(refused["error"]["data"]["kind"], "invalid", "{refused}");
    let plan = s.call(2, "plugins.command.plan", keep);
    let plan = plan.get("result").cloned().unwrap_or_else(|| panic!("{plan}"));
    assert_eq!((plan["operation"].as_str(), plan["destructive"].as_bool()), (Some("echo.command.keep"), Some(false)));
    assert!(!file.exists());
    let dry: Value = serde_json::from_str(plan["summary"][0].as_str().unwrap()).unwrap();
    assert_eq!(dry["context"].get("drive"), None, "{dry}");
    let applied = s.call(3, "plugins.command.apply", json!({ "plan_id": plan["plan_id"] }));
    let applied = applied.get("result").cloned().unwrap_or_else(|| panic!("{applied}"));
    assert_eq!(applied["request"]["params"], params);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "written");
}
