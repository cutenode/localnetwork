//! `library`: the libraries in the store, their history, and writing a snapshot onto a drive.

use crate::support::{Env, line, path};

#[test]
fn libraries_are_listed_renamed_and_their_history_shown() {
    let env = Env::new();
    assert!(env.ok(&["library"]).contains("The store has no libraries yet."));
    env.pulled("Main");
    let out = env.ok(&["library"]);
    assert!(line(&out, "Main  ").contains("1 snapshot(s), latest "), "{out}");
    assert!(line(&out, "drives:").ends_with("drives: Main"), "{out}");
    // A library's name shows its history.
    assert_eq!(env.ok(&["library", "Main"]), env.ok(&["library", "log", "Main"]));

    let out = env.ok(&["--yes", "library", "rename", "Main", "Club"]);
    assert!(out.contains("rename the library \"Main\" to \"Club\"") && out.contains("Main is now Club"), "{out}");
    assert!(env.ok(&["library", "ls"]).starts_with("Club  "));
}

#[test]
fn push_writes_a_snapshot_onto_a_drive_once_shown() {
    let env = Env::new();
    env.pulled("Main");
    let spare = env.blank("Spare");
    let out = env.ok(&["library", "push", path(&spare), "--library", "Main", "--dry-run"]);
    assert!(line(&out, "Snapshot ").contains("(2 tracks, "), "{out}");
    assert!(!spare.join("PIONEER").exists());
    let out = env.ok(&["--yes", "push", path(&spare), "--library", "Main", "--eject"]);
    assert!(out.contains("copied") && line(&out, "couldn't eject").contains("nothing to eject"), "{out}");
    assert!(spare.join("PIONEER/rekordbox/export.pdb").is_file());
    let out = env.ok(&["push", path(&spare), "--library", "Main"]);
    assert!(out.contains("drive already matches snapshot"), "{out}");
}
