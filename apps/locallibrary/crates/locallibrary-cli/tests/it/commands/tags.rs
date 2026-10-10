//! `locallibrary tags` in a terminal: My Tags listed, and each edit shown before it's made.

use crate::support::{Env, fixtures, path};

const EXT: &str = "PIONEER/rekordbox/exportExt.pdb";

#[test]
fn my_tags_are_listed_with_their_tracks() {
    let env = Env::ascii();
    let drive = env.demo("Demo");
    let d = path(&drive);
    let out = env.ok(&["tags", d]);
    assert!(out.contains("Demo  My Tags in exportExt.pdb"), "{out}");
    // rekordbox's default categories, with no tracks tagged yet.
    assert!(out.contains("Genre\n") && out.contains("Situation\n"), "{out}");
    assert!(out.lines().any(|l| l.starts_with("  Techno") && l.ends_with('-')), "{out}");
    env.ok(&["--yes", "tags", "add", d, "Genre/Techno", "--track", "2"]);
    let out = env.ok(&["tags", "list", d, "--tracks"]);
    let techno = out.lines().position(|l| l.starts_with("  Techno")).unwrap();
    assert!(out.lines().nth(techno).unwrap().ends_with(" 1"), "{out}");
    assert_eq!(out.lines().nth(techno + 1).unwrap().trim(), "Loopmasters - Demo Track 2  #2");
}

#[test]
fn tag_edits_show_their_plan_and_say_when_theres_nothing_to_do() {
    let env = Env::ascii();
    let drive = env.demo("Demo");
    let d = path(&drive);
    let out = env.ok(&["tags", "add", d, "Genre/Dark", "--query", "Demo Track", "--dry-run"]);
    assert!(out.contains("Demo: add Genre/Dark to 2 track(s)"), "{out}");
    assert!(out.contains("    + Loopmasters - Demo Track 1"), "{out}");
    assert!(out.contains("  create Genre/Dark in exportExt.pdb"), "{out}");
    assert!(out.contains("  changes exportExt.pdb"), "{out}");

    let out = env.ok(&["--yes", "tags", "add", d, "Genre/Dark", "--track", "1"]);
    assert!(out.contains("updated My Tags in exportExt.pdb (1 track(s))"), "{out}");
    assert!(out.contains(&format!("undo with: locallibrary undo {d:?}")), "{out}");
    let out = env.ok(&["tags", "add", d, "Genre/Dark", "--query", "Demo Track", "--dry-run"]);
    assert!(out.contains("1 selected track(s) already have it"), "{out}");
    let out = env.ok(&["--yes", "tags", "add", d, "Genre/Dark", "--track", "1"]);
    assert!(out.contains("nothing to change: the selected track(s) already have Genre/Dark"), "{out}");
    let out = env.ok(&["--yes", "tags", "untag", d, "Genre/Dark", "--track", "2"]);
    assert!(out.contains("nothing to change: none of the selected tracks has Genre/Dark"), "{out}");

    let out = env.ok(&["tags", "remove", d, "Genre/Dark", "--query", "Demo Track", "--dry-run"]);
    assert!(out.contains("Demo: remove Genre/Dark from 1 track(s)"), "{out}");
    assert!(out.contains("    - Loopmasters - Demo Track 1"), "{out}");
    assert!(out.contains("1 selected track(s) don't have it"), "{out}");

    let out = env.ok(&["tags", "rename", d, "Genre/Dark", "Darker", "--dry-run"]);
    assert!(out.contains("Demo: rename Genre/Dark to Darker"), "{out}");
    let out = env.ok(&["--yes", "tags", "rename", d, "Genre/Dark", "Dark"]);
    assert!(out.contains("nothing to change: it's already called Dark"), "{out}");
    let out = env.ok(&["--yes", "tags", "delete", d, "Genre/Dark"]);
    assert!(out.contains("Demo: delete Genre/Dark (from 1 track(s))"), "{out}");
    assert!(out.contains("updated My Tags in exportExt.pdb (1 track(s))"), "{out}");
}

#[test]
fn a_drive_without_my_tags_says_so() {
    let env = Env::ascii();
    let club = env.club("Club");
    let out = env.ok(&["tags", path(&club)]);
    assert_eq!(out, "Club: no My Tags\n! the drive has no My Tags database (exportExt.pdb or exportLibrary.db)\n");
}

#[test]
fn where_the_two_databases_disagree_its_said() {
    let env = Env::ascii();
    let drive = env.demo("Demo");
    let d = path(&drive);
    env.ok(&["--yes", "convert", "--to", "onelibrary", d]);
    let out = env.ok(&["--yes", "tags", "add", d, "Genre/Dark", "--track", "1"]);
    assert!(out.contains("  changes exportExt.pdb and exportLibrary.db\n"), "{out}");
    // rekordbox exports exportExt.pdb again, without the new tag.
    std::fs::copy(fixtures().join("rekordcrate/demo_tracks/PIONEER/rekordbox/exportExt.pdb"), drive.join(EXT)).unwrap();
    let out = env.ok(&["tags", d]);
    assert!(out.starts_with("Demo  My Tags in exportExt.pdb and exportLibrary.db\n"), "{out}");
    assert!(out.contains("  Dark              1 (exportLibrary.db only)\n"), "{out}");
    assert!(
        out.contains("! exportExt.pdb and exportLibrary.db disagree:\n    Genre/Dark is only in exportLibrary.db\n")
    );
}
