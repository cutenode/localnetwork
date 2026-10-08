//! `provision`: a blank drive made a working copy of the library in one step. Formatting first
//! needs a real disk, so here the drive is a folder and only the steps after it run.

use crate::support::{Env, line, path};

#[test]
fn provision_lists_its_steps_then_makes_a_ready_drive() {
    let env = Env::new();
    env.pulled("Main");
    env.ok(&["--yes", "settings", "set", "quantize=off"]);
    let spare = env.blank("Spare");
    let out = env.ok(&["provision", path(&spare), "--name", "Spare", "--role", "backup", "--dry-run"]);
    assert!(out.starts_with("Steps\n  1. ") && line(&out, "1. ").ends_with(" the name \"Spare\""), "{out}");
    assert!(line(&out, "2. ").starts_with("  2. write \"Main\"'s latest snapshot ("), "{out}");
    let rest =
        "  3. write the My Settings profile\n  4. mark it as a backup drive\n  5. read every file back and check it\n";
    assert!(out.ends_with(rest), "{out}");
    assert!(!spare.join("PIONEER").exists());
    // Unconfirmed, nothing is made.
    assert_eq!(env.run(&["provision", path(&spare), "--name", "Spare"]).code, 3);
    assert!(!spare.join("PIONEER").exists());

    let out = env.ok(&["--yes", "provision", path(&spare), "--name", "Spare", "--role", "backup"]);
    assert!(out.contains("settings written") && out.contains("Spare is ready"), "{out}");
    assert!(out.contains("Spare is intact"), "it reads everything back: {out}");
    assert!(line(&env.ok(&["drive", "ls", "--known"]), "Spare [backup]").contains("up to date"));

    // A step that can't be made stops the rest: the steps before it stay made.
    let other = env.blank("Other");
    let r = env.run(&["--yes", "provision", path(&other), "--name", "Other", "--player-name", "OTHER"]);
    assert_eq!(r.code, 5, "{}", r.err);
    assert!(line(&r.out, "stopped at \"set the player name\"").contains("no OneLibrary database"), "{}", r.out);
    assert!(other.join("PIONEER/rekordbox/export.pdb").is_file());

    // Like another drive, a drive that holds a library already is replaced, and says so.
    let out = env.ok(&["provision", path(&spare), "--like", "Main", "--no-settings", "--no-verify", "--dry-run"]);
    assert!(out.contains("already holds a rekordbox library; it will be replaced"), "{out}");
}
