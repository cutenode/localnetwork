//! The store's upkeep: pulling into it, labelling and pruning snapshots, and copying it
//! elsewhere.

use std::path::PathBuf;

use crate::support::{Env, line, path};

/// Three snapshots of Main: as pulled, then after two edits.
fn three_snapshots(env: &Env) {
    let main = env.pulled("Main");
    for name in ["First", "Second"] {
        env.ok(&["--yes", "playlist", "new", path(&main), name]);
        env.ok(&["pull", "Main", "-m", name]);
    }
}

#[test]
fn pull_never_starts_a_library_by_mistake() {
    let env = Env::new();
    env.pulled("Main");
    let other = env.drive("Other");
    // A typo in --library is refused when nobody can be asked.
    let err = env.fails(1, &["pull", path(&other), "--library", "Mian"]);
    assert!(err.contains("error: not found: library \"Mian\" in the store (it has \"Main\")"), "{err}");
    assert!(err.contains("hint: Start the library \"Mian\": add --new-library"), "{err}");
    let out = env.ok(&["pull", path(&other), "--library", "Club", "--new-library"]);
    assert!(out.contains("of Club: 2 tracks"), "{out}");
    assert!(out.contains("1 new session"), "the history list it came with: {out}");
    assert!(env.ok(&["pull", path(&other)]).contains("up to date"));
}

/// In a terminal, an unknown library name is asked about rather than refused.
#[cfg(unix)]
#[test]
fn pull_asks_in_a_terminal_before_starting_a_library() {
    use std::io::Write;
    let env = Env::new();
    env.pulled("Main");
    let other = env.drive("Other");
    for answer in ["n", "y"] {
        let (mut keyboard, stdin) = crate::support::terminal();
        writeln!(keyboard, "{answer}").unwrap();
        let mut command = env.command();
        command.args(["pull", path(&other), "--library", "Mian"]).stdin(std::process::Stdio::from(stdin));
        let out = command.output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("There's no library named \"Mian\" (the store has \"Main\"). Start a new one?"), "{err}");
        assert_eq!(out.status.success(), answer == "y", "{err}");
    }
    assert!(env.ok(&["library"]).contains("Mian  "));
}

#[test]
fn snapshots_are_labelled_and_pruned_keeping_what_matters() {
    let env = Env::new();
    three_snapshots(&env);
    let log = env.ok(&["log", "Main"]);
    let first = log.lines().last().unwrap().split_whitespace().next().unwrap().to_string();
    let out = env.ok(&["--yes", "store", "label", &first, "keep me"]);
    assert!(out.contains(&format!("label snapshot {first} \"keep me\"")) && out.contains("is \"keep me\""), "{out}");

    let out = env.ok(&["store", "prune", "--keep-last", "1", "--dry-run"]);
    assert!(out.starts_with("Main: keeping 2, removing 1"), "{out}");
    assert!(line(&out, "+ ").contains("latest"), "{out}");
    assert!(line(&out, &first).contains("labelled keep me"), "{out}");
    assert!(line(&out, "- ").ends_with("  First"), "the one between: {out}");
    assert!(line(&out, "Frees ").contains("file(s) no snapshot left uses"), "{out}");
    let out = env.ok(&["store", "prune", "--keep-daily", "1", "--keep-weekly", "1", "--dry-run"]);
    assert!(
        line(&out, "latest").contains("newest that day") && line(&out, "latest").contains("newest that week"),
        "{out}"
    );
    let out = env.ok(&["--yes", "library", "prune", "--keep-last", "1"]);
    assert!(out.contains("removed 1 snapshot(s); freed"), "{out}");

    // With the label off, the oldest goes too; then every snapshot is kept.
    let out = env.ok(&["--yes", "store", "unlabel", "keep me"]);
    assert!(
        out.contains("take the label \"keep me\" off its snapshot") && out.contains("took \"keep me\" off snapshot"),
        "{out}"
    );
    env.ok(&["--yes", "store", "prune", "--keep-last", "1"]);
    assert!(env.ok(&["store", "prune", "--keep-last", "1"]).contains("nothing to prune: every snapshot is kept"));
}

#[test]
fn the_store_is_copied_brought_up_to_date_and_mirrored() {
    let env = Env::new();
    three_snapshots(&env);
    let copy = env.outside("NAS");
    assert!(env.ok(&["store", "copies"]).contains("No copies yet."));

    let out = env.ok(&["store", "copy", path(&copy), "--dry-run"]);
    assert!(out.starts_with(&format!("Copying the store to {}", copy.display())), "{out}");
    assert!(line(&out, "new file(s)").contains("3 snapshot(s) of 3"), "{out}");
    assert!(!copy.exists());
    let out = env.ok(&["store", "copy", path(&copy)]);
    assert!(out.contains("and 3 snapshot(s) to") && out.contains("0 file(s) were there already"), "{out}");
    let out = env.ok(&["store", "copies"]);
    assert!(line(&out, path(&copy)).ends_with("up to date"), "{out}");
    assert!(line(&out, "last complete").contains("3 snapshot(s)"), "{out}");

    // A file in the copy that changed (the disk holding it is failing): --verify copies it again.
    let object = std::fs::read_dir(copy.join("objects")).unwrap().flatten().find(|e| e.path().is_dir()).unwrap().path();
    let object = std::fs::read_dir(object).unwrap().flatten().next().unwrap().path();
    let mut bytes = std::fs::read(&object).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&object, bytes).unwrap();
    let out = env.ok(&["store", "copy", path(&copy), "--verify"]);
    assert!(out.contains("1 file(s) in the copy had changed and were copied again"), "{out}");

    // A new snapshot: the copy is behind.
    env.ok(&["--yes", "playlist", "new", "Main", "Third"]);
    env.ok(&["pull", "Main"]);
    assert!(line(&env.ok(&["store", "copies"]), path(&copy)).ends_with("behind"));
    // Pruned in the store: --mirror asks before removing from the copy.
    env.ok(&["--yes", "store", "prune", "--keep-last", "1"]);
    let r = env.run(&["store", "copy", path(&copy), "--mirror"]);
    assert_eq!(r.code, 3, "{}", r.out);
    assert!(line(&r.out, "the store no longer has").contains("3 snapshot(s)"), "{}", r.out);
    let out = env.ok(&["--yes", "store", "copy", path(&copy), "--mirror"]);
    assert!(out.contains("removed 3 snapshot(s) and"), "{out}");

    // A copy that isn't there any more.
    std::fs::rename(&copy, env.outside("NAS (moved)")).unwrap();
    assert!(line(&env.ok(&["store", "copies"]), path(&copy)).ends_with("not there (unmounted?)"));
}

/// Snapshots: what a pull stored, the log, the store's size, and the store's upkeep.
#[test]
fn the_store_says_what_it_holds() {
    let env = Env::new();
    env.demo("A");
    // Reading never makes a store: with none, it says so.
    let none = env.run(&["store"]);
    assert!(none.code == 1 && none.err.contains("no locallibrary store at "), "{}", none.err);
    env.ok(&["--yes", "settings", "set", "quantize=off"]);
    let store = env.ok(&["store"]);
    assert!(store.contains("  data       0 B in 0 files\n  snapshots  0\n  drives     0 known"), "{store}");
    assert!(env.ok(&["log"]).starts_with("The store is empty."));

    env.ok(&["--yes", "drive", "adopt", "A", "--name", "Main"]);
    let pulled = env.ok(&["pull", "Main", "-m", "first", "--label", "before NYE"]);
    let id = pulled.split_whitespace().nth(2).unwrap().to_string();
    assert!(pulled.starts_with(&format!("✓ snapshot {id} [before NYE] of Main: 2 tracks; stored ")), "{pulled}");
    assert_eq!(env.ok(&["pull", "Main"]).trim(), format!("✓ Main is up to date (snapshot {id} [before NYE])"));
    let log = env.ok(&["log"]);
    assert!(log.starts_with("Main\n") && line(&log, &id).contains("[before NYE]"), "{log}");
    assert!(line(&log, &id).contains("2 tracks") && line(&log, &id).ends_with("from Main  first"), "{log}");
    let store = env.ok(&["store", "info"]);
    assert!(store.contains("  snapshots  1\n  drives     1 known\n  library    Main @ "), "{store}");

    // Labels, verifying, collecting.
    assert!(env.ok(&["--yes", "store", "label", &id, "Friday"]).contains("Friday"));
    assert!(line(&env.ok(&["log"]), &id).contains("[before NYE] [Friday]"));
    env.ok(&["--yes", "store", "unlabel", "Friday"]);
    assert!(!env.ok(&["log"]).contains("Friday"));
    assert!(env.ok(&["store", "verify"]).trim().ends_with("objects verified"));
    assert_eq!(env.ok(&["store", "gc"]).trim(), "✓ nothing to collect");

    // A file nothing uses is collected.
    let orphan = env.store.join("objects/ff").join("0".repeat(62));
    std::fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    std::fs::write(&orphan, "junk").unwrap();
    let plan = env.ok(&["store", "gc", "--dry-run"]);
    assert_eq!(plan.trim(), "• delete 0 snapshot(s) and 1 file(s) nothing uses (4 B)");
    assert_eq!(env.ok(&["--yes", "store", "gc"]).lines().last(), Some("✓ freed 4 B (1 objects, 0 snapshots)"));
    assert!(!orphan.exists());

    // A copy says what it's a copy of.
    let copy = env.outside("copy");
    env.ok(&["--yes", "store", "copy", path(&copy)]);
    let info = env.ok(&["--store", path(&copy), "store", "info"]);
    assert!(line(&info, "copy of").contains("store (as of "), "{info}");
    // A change planned for the copy says the next copy to it refuses unless forced.
    let planned = env.ok(&["--store", path(&copy), "players", "set", "cdj-3000", "--dry-run"]);
    let said = format!("{} is a copy of the store at ", copy.display());
    assert!(planned.contains(&said) && planned.contains("refuse unless forced (--force)"), "{planned}");
    let here = env.ok(&["players", "set", "cdj-3000", "--dry-run"]);
    assert!(!here.contains("is a copy of the store"), "{here}");

    // A damaged file and a missing one are found.
    let objects: Vec<PathBuf> = std::fs::read_dir(env.store.join("objects"))
        .unwrap()
        .flat_map(|d| std::fs::read_dir(d.unwrap().path()).unwrap())
        .map(|f| f.unwrap().path())
        .collect();
    std::fs::write(&objects[0], "damaged").unwrap();
    std::fs::remove_file(&objects[1]).unwrap();
    let r = env.run(&["store", "verify"]);
    assert_eq!(r.code, 4, "problems found");
    assert!(r.out.contains("! corrupt: ") && r.out.contains("! missing: "), "{}", r.out);
}

/// Regression: commands that only look something up in the store made an empty one first, then
/// failed or found nothing to do. On a fresh home none is left behind.
#[test]
fn lookups_with_no_store_leave_none_behind() {
    let env = Env::new();
    let a = env.demo("A");
    let copy = env.outside("copy");
    for args in [
        &["--yes", "drive", "forget", "A"][..],
        &["--yes", "drive", "role", "A", "backup"],
        &["--yes", "drive", "retire", "A"],
        &["drive", "adopt", "A", "--as", "Old", "--dry-run"],
        &["drive", "verify", "A"],
        &["drive", "fsck", "A"],
        &["--yes", "sets", "delete", "Nope"],
        &["sets", "playlist", "Nope", "A", "--dry-run"],
        &["--yes", "smart", "rm", "Nope"],
        &["smart", "apply", "--dry-run"],
        &["--yes", "profile", "unpin", "Nope"],
        &["--yes", "profile", "unhide", "Nope"],
        &["--yes", "store", "label", "abc", "Friday"],
        &["--yes", "store", "unlabel", "Friday"],
        &["store", "gc", "--dry-run"],
        &["store", "prune", "--keep-last", "1", "--dry-run"],
        &["store", "copy", path(&copy), "--dry-run"],
        &["library", "prune", "--keep-last", "1", "--dry-run"],
        &["--yes", "library", "rename", "Nope", "New"],
        &["push", path(&a)],
        &["art", "unembed", "--dry-run"],
        &["players", "set", "CDJ-3000", "--dry-run"],
        &["--yes", "players", "clear"],
    ] {
        let r = env.run(args);
        assert!(!env.store.exists(), "{args:?} made a store\n{}\n{}", r.out, r.err);
    }
}

/// A store that won't open is named, whether a read, a lookup or a change reached it.
#[test]
fn a_store_that_wont_open_is_named() {
    let env = Env::new();
    env.demo("A");
    env.demo("B");
    std::fs::create_dir_all(&env.store).unwrap();
    let newer = r#"{"format": 9999, "id": "00000000-0000-0000-0000-000000000000", "created": "2026-01-01T00:00:00Z"}"#;
    std::fs::write(env.store.join("store.json"), newer).unwrap();
    let named = format!("opening store at {}", env.store.display());
    for args in [
        &["diff", "A", "B"][..],
        &["store"],
        &["--yes", "smart", "rm", "Nope"],
        &["--yes", "settings", "set", "quantize=off"],
    ] {
        let err = env.fails(1, args);
        assert!(err.contains(&named) && err.contains("newer than this version"), "{args:?}: {err}");
    }
}
