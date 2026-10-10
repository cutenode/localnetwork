//! A library's drives are clones of it: a write to the library shows one plan that ends on the
//! clones it's copied onto, `sync` with no drives syncs every mounted library, and the commands
//! that take several drives take a library's name for its mounted ones.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::support::{Env, Rpc, path};

/// The library `library` on two drives: `lead` (the primary) and `clone`, synced from it.
fn library(env: &Env, library: &str, lead: &str, clone: &str) -> (PathBuf, PathBuf) {
    let a = env.demo(&format!("{lead} drive"));
    env.ok(&["--yes", "adopt", path(&a), "--name", lead]);
    env.ok(&["pull", lead, "--library", library, "--new-library"]);
    let b = env.blank(&format!("{clone} drive"));
    env.ok(&["--yes", "sync", "--from", lead, path(&b)]);
    env.ok(&["--yes", "adopt", path(&b), "--name", clone]);
    env.ok(&["--yes", "drive", "role", lead, "primary"]);
    (a, b)
}

/// Run `args` with the drive at `root` unplugged, so what they write doesn't reach it.
fn without(env: &Env, root: &Path, args: &[&str]) {
    std::fs::rename(root, env.outside("away")).unwrap();
    env.ok(args);
    std::fs::rename(env.outside("away"), root).unwrap();
}

#[test]
fn a_write_to_a_library_is_one_plan_for_every_clone() {
    let env = Env::new();
    library(&env, "Home", "Main", "Backup");
    let plan = env.ok(&["playlist", "new", "Home", "Friday", "--dry-run"]);
    assert!(plan.starts_with("Main: export.pdb\n"), "{plan}");
    assert!(plan.contains("\n  then copied onto Backup\n"), "{plan}");
    let v = env.json(&["playlist", "new", "Home", "Friday", "--dry-run"]);
    assert_eq!(v["plan"]["clones"][0]["name"], "Backup", "{v}");
    assert!(
        env.ok(&["tags", "add", "Home", "Genre/Deep", "--track", "1", "--dry-run"]).contains("then copied onto Backup")
    );

    env.ok(&["--yes", "playlist", "new", "Home", "Friday"]);
    assert!(env.ok(&["playlist", "ls", "Backup"]).contains("Friday"));
    assert!(env.ok(&["compare", "Main", "Backup"]).starts_with("Main and Backup are identical"));

    // Given no drive, a write goes to each library once, on its lead.
    env.ok(&["--yes", "smart", "add", "Smart/All", "--filter", "bpm:>=0"]);
    let v = env.json(&["smart", "apply", "--dry-run"]);
    let plans = v["plan"].as_array().unwrap();
    assert_eq!(
        (plans.len(), &plans[0]["drive"], &plans[0]["clones"][0]["name"]),
        (1, &json!("Main"), &json!("Backup"))
    );

    // The sidecar plans and applies it the same way.
    let mut rpc = Rpc::start(env.command());
    let edit = json!({ "drive": "Home", "edits": [{ "op": "create_playlist", "path": "Saturday" }] });
    let plan = rpc.call(1, "edit.plan", edit);
    assert_eq!(plan["result"]["clones"][0]["name"], "Backup", "{plan}");
    let applied = rpc.call(2, "edit.apply", json!({ "plan_id": plan["result"]["plan_id"] }));
    assert!(applied["error"].is_null(), "{applied}");
    drop(rpc);
    assert!(env.ok(&["playlist", "ls", "Backup"]).contains("Saturday"));
}

#[test]
fn sync_with_no_drives_syncs_every_mounted_library() {
    let env = Env::new();
    let (_, backup) = library(&env, "Home", "Main", "Backup");
    let (_, spare) = library(&env, "Away", "Other", "Spare");
    let behind = |rating: &str| {
        without(&env, &backup, &["--yes", "track", "set", "Main", "--track", "1", "--rating", rating]);
        without(&env, &spare, &["--yes", "track", "set", "Other", "--track", "1", "--rating", rating]);
    };
    let identical = |a: &str, b: &str| env.ok(&["compare", a, b]).starts_with(&format!("{a} and {b} are identical"));

    behind("3");
    let plan = env.ok(&["sync", "--dry-run"]);
    assert!(plan.contains("Main is the newest\n") && plan.contains("Other is the newest\n"), "{plan}");
    env.ok(&["--yes", "sync"]);
    assert!(identical("Main", "Backup") && identical("Other", "Spare"));

    // A library's name: that library's drives only.
    behind("4");
    let plan = env.ok(&["sync", "Home", "--dry-run"]);
    assert!(plan.contains("Main is the newest\n") && !plan.contains("Other"), "{plan}");
    env.ok(&["--yes", "sync", "Home"]);
    assert!(identical("Main", "Backup") && !identical("Other", "Spare"));
}

#[test]
fn a_library_name_means_its_mounted_drives() {
    let env = Env::new();
    library(&env, "Home", "Main", "Backup");
    library(&env, "Away", "Other", "Spare");
    let v = env.json(&["check", "Home"]);
    assert_eq!(v["drives"], json!(["Main", "Backup"]), "{v}");
    // Settings are each drive's own: each of the library's drives is written.
    env.ok(&["--yes", "settings", "set", "quantize=off"]);
    let v = env.json(&["settings", "apply", "Home", "--dry-run"]);
    let targets: Vec<&serde_json::Value> =
        v["plan"]["targets"].as_array().unwrap().iter().map(|t| &t["drive"]).collect();
    assert_eq!(targets, [&json!("Main"), &json!("Backup")], "{v}");
    let prep = env.ok(&["prep", "Away", "--dry-run"]);
    assert!(prep.contains("Get Other, Spare ready for a set"), "{prep}");
    let pull = env.ok(&["pull", "Home"]);
    assert!(pull.contains("Main") && pull.contains("Backup") && !pull.contains("Other"), "{pull}");
}

#[test]
fn clones_named_together_are_written_once() {
    let env = Env::new();
    library(&env, "Home", "Main", "Backup");
    env.ok(&["--yes", "smart", "add", "Smart/All", "--filter", "bpm:>=0"]);
    let v = env.json(&["smart", "apply", "Main", "Backup", "--dry-run"]);
    assert_eq!(v["plan"].as_array().map(Vec::len), Some(1), "{v}");
    env.ok(&["--yes", "smart", "apply", "Backup", "Main"]);
    assert!(env.ok(&["playlist", "ls", "Backup"]).contains("All"));
    assert!(env.ok(&["compare", "Main", "Backup"]).starts_with("Main and Backup are identical"));
}

#[test]
fn a_library_is_not_pulled_into_another() {
    let env = Env::new();
    library(&env, "Home", "Main", "Backup");
    let err = env.fails(2, &["pull", "Home", "--library", "Away"]);
    assert!(err.contains("--library picks the library a drive is pulled into; Home is already a library"), "{err}");
}

#[test]
fn a_library_is_given_once_to_sync() {
    let env = Env::new();
    library(&env, "Home", "Main", "Backup");
    let err = env.fails(2, &["sync", "Home", "--library", "Away"]);
    assert!(err.contains("Home is a library: give it or --library, not both"), "{err}");
}
