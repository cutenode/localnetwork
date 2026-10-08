//! `setup`, `refresh` and `pull` in a terminal: what setup says about each drive and step, and a
//! job that stops part way.

use crate::support::{Env, path};

#[test]
fn setup_says_what_it_leaves_out_and_why() {
    let env = Env::ascii();
    let err = env.fails(1, &["--yes", "setup", "--library", "Home"]);
    assert!(err.contains("no rekordbox drive is mounted"), "{err}");
    let club = env.club("Club");
    let demo = env.demo("REKORDBOX");
    let err = env.fails(2, &["--yes", "setup", "--library", "Home", "--name", "REKORDBOX"]);
    assert!(err.contains("--name takes DRIVE=VALUE, not \"REKORDBOX\""), "{err}");
    let err = env.fails(2, &["--yes", "setup", "--library", "Home", "--primary", "Club", "--role", "REKORDBOX=dj"]);
    assert!(err.contains("unknown role \"dj\" (primary, backup, set or archive)"), "{err}");

    // Both drives, unasked; the demo drive holds another library, and has no OneLibrary database.
    let setup = ["--yes", "setup", "--library", "Home", "--primary", "Club", "--player-name", "REKORDBOX=Main"];
    let out = env.ok(&setup);
    assert!(out.contains("- roles: Club primary, REKORDBOX backup\n"), "{out}");
    assert!(out.contains("  REKORDBOX isn't in Home yet ("), "{out}");
    assert!(out.contains("`locallibrary sync Club REKORDBOX` brings it up to date"), "{out}");
    assert!(out.contains("  Club has no My Settings files to import\n"), "{out}");
    assert!(out.contains("Device Names\n  REKORDBOX has no OneLibrary database"), "{out}");
    assert!(!demo.join("PIONEER/rekordbox/exportLibrary.db").exists());

    // A player model it doesn't know is refused, after the rest is done.
    let err = env.fails(1, &["--yes", "setup", "--library", "Home", "--primary", path(&club), "--players", "CDJ-1"]);
    assert!(err.contains("not a model locallibrary knows: CDJ-1"), "{err}");
}

#[test]
fn setup_leaves_out_the_drives_of_another_library_unless_named() {
    let env = Env::ascii();
    let friend = env.demo("FRIEND");
    env.ok(&["pull", path(&friend), "--library", "Theirs", "--new-library"]);
    let err = env.fails(1, &["--yes", "setup", "--library", "Home"]);
    assert!(err.contains("no drive to set up: name the drives"), "{err}");
}

#[test]
fn setup_gives_a_drive_it_knew_its_identity_back_and_imports_settings_once() {
    let env = Env::ascii();
    let demo = env.demo("REKORDBOX");
    let copy = env.demo("COPY");
    env.ok(&["pull", path(&demo), "--library", "Home", "--new-library"]);
    // The copy holds the same library, but isn't one of its drives yet.
    let out = env.ok(&["--yes", "setup", path(&copy), "--library", "Home"]);
    assert!(out.contains("- save COPY in \"Home\", a library the store has\n"), "{out}");
    let out = env.ok(&["--yes", "setup", path(&copy), "--library", "Home"]);
    assert!(out.contains("  the profile has those settings already\n"), "{out}");
    // An archive drive is left out of the night-before job, and said to be.
    env.ok(&["--yes", "drive", "role", "COPY", "archive"]);
    let out = env.ok(&["prep", "--dry-run"]);
    assert!(out.starts_with("! left out COPY: archive and retired drives aren't taken unless named\n"), "{out}");

    // Erased (its identity gone), the drive is recognised by what it holds.
    std::fs::remove_dir_all(demo.join(".locallibrary")).unwrap();
    let out = env.ok(&["--yes", "setup", path(&demo), "--library", "Home", "--no-settings"]);
    assert!(out.contains("/REKORDBOX the identity of REKORDBOX ("), "{out}");
}

#[test]
fn setup_names_a_drive_for_players_that_read_onelibrary() {
    let env = Env::ascii();
    let demo = env.demo("REKORDBOX");
    env.ok(&["--yes", "convert", "--to", "onelibrary", path(&demo)]);
    let out = env.ok(&["--yes", "setup", "--library", "Home", "--player-name", "REKORDBOX=Main"]);
    assert!(out.contains("Device Names\n- players show REKORDBOX as \"Main\"\n"), "{out}");
}

/// A job's steps run in turn: the first that fails stops it, and the ones before stay done (exit
/// status 5); when the first fails, nothing was done and it's an error. The store's folders are
/// made read-only to fail the pull.
#[cfg(unix)]
#[test]
fn a_job_that_stops_part_way_says_where_and_what_stays_done() {
    use crate::support::{PDB, permissions_enforced, set_mode};
    if !permissions_enforced() {
        return;
    }
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    let exported = std::fs::read(club.join(PDB)).unwrap();
    env.ok(&["pull", d, "--library", "Club", "--new-library"]);
    env.ok(&["--yes", "playlist", "new", d, "Friday", "--track", "1"]);
    std::fs::write(club.join(PDB), &exported).unwrap();
    let store = &env.store;
    let folders = [store.join("objects"), store.join("snapshots")];
    let lock = |mode| folders.iter().for_each(|f| set_mode(f, mode));
    lock(0o555);

    let out = env.exits(5, &["--yes", "refresh", d]);
    assert!(out.contains("v create playlist Friday\n"), "{out}");
    assert!(out.contains("! stopped at \"Save the drive in the store\": "), "{out}");
    assert!(out.contains("The steps before it stay done; `locallibrary undo <drive>`"), "{out}");
    let err = env.fails(1, &["--yes", "refresh", d]);
    assert!(err.contains("Save the drive in the store: ") && err.contains("Permission denied"), "{err}");
    let err = env.fails(1, &["pull"]);
    assert!(err.contains("Permission denied"), "{err}");
    lock(0o755);

    let out = env.ok(&["pull"]);
    assert!(out.starts_with("v "), "{out}");
}
