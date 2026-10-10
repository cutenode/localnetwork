//! A drive's health and space: verify (and repair), the logbook's checks, space, tidy, and the
//! commands that would run the system's disk tools, which here may only refuse or plan.

use std::path::Path;

use crate::support::{Env, line, path};

const TRACK_1: &str = "Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3";
const TRACK_2: &str = "Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3";

#[test]
fn verify_finds_damaged_and_missing_files_and_repairs_them_from_the_store() {
    let env = Env::new();
    let main = env.pulled("Main");
    let out = env.ok(&["drive", "verify", "Main"]);
    assert!(line(&out, "compared with snapshot").starts_with("  "), "{out}");
    assert!(out.contains("10 intact, 0 without a reference") && out.contains("Main is intact"), "{out}");

    let track_1 = main.join(TRACK_1);
    let good = damage(&track_1);
    std::fs::remove_file(main.join(TRACK_2)).unwrap();
    let out = env.exits(4, &["drive", "verify", "Main"]);
    assert!(line(&out, "damaged").ends_with("Demo Track 1.mp3 (good copy in the store)"), "{out}");
    assert!(line(&out, "missing").ends_with("Demo Track 2.mp3 (good copy in the store)"), "{out}");
    assert!(out.contains("affects 2 track(s) in 1 playlist(s):\n    Loopmasters - Demo Track 1"), "{out}");
    assert!(out.contains("`locallibrary drive verify --repair` rewrites them"), "{out}");

    // The logbook and the registry remember what the check found.
    let out = env.ok(&["drive", "log", "Main"]);
    assert!(line(&out, "last check").contains("problems: 1 damaged, 0 unreadable, 1 missing"), "{out}");
    assert!(line(&out, "damaged_files").starts_with("! "), "{out}");
    assert!(line(&env.ok(&["drive", "ls", "--known"]), "Main: 1 file(s)").starts_with("! "));
    assert!(line(&env.ok(&["status"]), "last check").contains("1 damaged"));

    // Repairing asks first, then rewrites both from the store.
    assert!(env.fails(3, &["drive", "verify", "Main", "--repair"]).contains("not confirmed"));
    assert!(!main.join(TRACK_2).exists());
    assert!(env.ok(&["--yes", "drive", "verify", "Main", "--repair"]).contains("rewrote 2 file(s)"));
    assert_eq!(std::fs::read(&track_1).unwrap(), good);
    assert!(main.join(TRACK_2).is_file());
    assert!(env.ok(&["drive", "verify", "Main"]).contains("Main is intact"));
    assert!(line(&env.ok(&["drive", "log", "Main"]), "written").contains("in 1 operation(s)"));
}

/// Damage keeps a file's size and modification time; the bytes differ. Returns the good bytes.
fn damage(file: &Path) -> Vec<u8> {
    let (good, modified) = (std::fs::read(file).unwrap(), std::fs::metadata(file).unwrap().modified().unwrap());
    std::fs::write(file, good.to_ascii_uppercase()).unwrap();
    std::fs::File::options().write(true).open(file).unwrap().set_modified(modified).unwrap();
    good
}

#[test]
fn without_the_stores_copy_a_file_is_repaired_from_another_drive() {
    let env = Env::new();
    let main = env.pulled("Main");
    let backup = env.blank("Backup");
    env.ok(&["--yes", "sync", "Main", path(&backup)]);
    let good = damage(&main.join(TRACK_1));
    // The store lost its files (a disk that failed, a folder deleted by hand).
    std::fs::remove_dir_all(env.store.join("objects")).unwrap();
    let out = env.exits(4, &["--yes", "drive", "verify", "Main", "--repair"]);
    assert!(line(&out, "damaged").ends_with("Demo Track 1.mp3"), "no good copy: {out}");
    assert!(out.contains("no known-good copy of the damaged files is available"), "{out}");

    let out = env.ok(&["--yes", "drive", "verify", "Main", "--repair", "--against", path(&backup)]);
    assert!(line(&out, "damaged").ends_with("Demo Track 1.mp3 (good copy on Backup)"), "{out}");
    assert!(out.contains("rewrote 1 file(s)"), "{out}");
    assert_eq!(std::fs::read(main.join(TRACK_1)).unwrap(), good);
}

#[cfg(unix)]
#[test]
fn a_file_that_cant_be_read_is_reported() {
    use crate::support::{permissions_enforced, set_mode};
    if !permissions_enforced() {
        return;
    }
    let env = Env::new();
    let main = env.drive("Main");
    let track = main.join(TRACK_2);
    set_mode(&track, 0o000);
    let out = env.exits(4, &["drive", "verify", path(&main)]);
    set_mode(&track, 0o644);
    assert!(line(&out, "unreadable").contains("Demo Track 2.mp3: ") && out.contains("ermission denied"), "{out}");

    // A folder that can't be listed can't be measured.
    let locked = main.join("Locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("notes.txt"), "notes").unwrap();
    set_mode(&locked, 0o000);
    let out = env.ok(&["drive", "space", path(&main)]);
    set_mode(&locked, 0o755);
    assert!(out.contains("1 file(s) couldn't be measured"), "{out}");
}

#[test]
fn space_says_what_uses_a_drive_and_whether_the_library_fits_its_group() {
    let env = Env::new();
    let main = env.pulled("Main");
    let backup = env.blank("Backup");
    env.ok(&["--yes", "sync", "Main", path(&backup)]);
    env.ok(&["--yes", "adopt", path(&backup), "--name", "Backup"]);
    // Audio nothing in the library plays: the first five by name, then how many more.
    for i in 1..=6 {
        let p = main.join(format!("Music/{i}.mp3"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "loose").unwrap();
    }
    // A backup (made by an edit), a recording, what computers leave, the trash and other files.
    env.ok(&["--yes", "playlist", "new", "Main", "New"]);
    std::fs::create_dir_all(main.join("PIONEER REC")).unwrap();
    std::fs::write(main.join("PIONEER REC/REC001.WAV"), "rec").unwrap();
    std::fs::write(main.join(".DS_Store"), "finder").unwrap();
    std::fs::create_dir_all(main.join(".Trashes/501")).unwrap();
    std::fs::write(main.join(".Trashes/501/old.mp3"), "trash").unwrap();
    std::fs::write(main.join("notes.txt"), "set notes").unwrap();
    std::fs::create_dir_all(main.join("PIONEER/Artwork/00001")).unwrap();
    std::fs::write(main.join("PIONEER/Artwork/00001/a1.jpg"), "jpeg").unwrap();
    std::fs::write(main.join("PIONEER/notes.bin"), "rekordbox's").unwrap();
    let out = env.ok(&["drive", "space", "Main"]);
    assert!(out.starts_with("Main: ") && line(&out, "databases").contains("2 files"), "{out}");
    for kind in [
        "locallibrary undo backups",
        "recordings made on the player",
        "macOS/Windows clutter (locallibrary drive tidy)",
        "trash (locallibrary drive tidy --empty-trash)",
        "other files",
        "artwork",
        "other rekordbox files",
    ] {
        assert!(line(&out, kind).ends_with(" files"), "{kind}: {out}");
    }
    assert!(line(&out, "audio outside the library").contains("6 files"), "{out}");
    assert_eq!(out.lines().filter(|l| l.starts_with("    Music/") && l.ends_with(".mp3 (5 B)")).count(), 5, "{out}");
    assert!(out.contains("... and 1 more not in the library"), "{out}");
    assert!(line(&out, "fits on Backup").starts_with("  "), "{out}");
}

#[test]
fn tidy_removes_what_computers_leave_and_keeps_the_trash_unless_told() {
    let env = Env::new();
    let main = env.drive("Main");
    std::fs::write(main.join(".DS_Store"), "finder").unwrap();
    std::fs::create_dir_all(main.join(".Trashes/501")).unwrap();
    std::fs::write(main.join(".Trashes/501/old.mp3"), "trash").unwrap();

    let out = env.ok(&["drive", "tidy", path(&main), "--dry-run"]);
    assert!(line(&out, ".DS_Store").contains("1 files"), "{out}");
    assert!(out.contains("5 B in the trash is kept (--empty-trash deletes it)"), "{out}");
    assert!(main.join(".DS_Store").exists());
    assert!(env.ok(&["--yes", "drive", "tidy", path(&main)]).contains("removed 1 file(s)"));
    assert!(!main.join(".DS_Store").exists());

    // Only the trash is left: nothing to tidy, and a word about the trash.
    let out = env.ok(&["drive", "tidy", path(&main)]);
    assert!(out.contains("5 B in the trash (--empty-trash deletes it)") && out.contains("nothing to tidy"), "{out}");
    let out = env.ok(&["--yes", "drive", "tidy", path(&main), "--empty-trash"]);
    assert!(line(&out, "files in the trash").contains("1 files") && out.contains("removed 1 file(s), 5 B"), "{out}");
    assert!(!main.join(".Trashes/501/old.mp3").exists());
}

#[cfg(unix)]
#[test]
fn tidy_says_what_it_couldnt_remove() {
    use crate::support::{permissions_enforced, set_mode};
    if !permissions_enforced() {
        return;
    }
    let env = Env::new();
    let main = env.drive("Main");
    let locked = main.join("Locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join(".DS_Store"), "finder").unwrap();
    set_mode(&locked, 0o555);
    let out = env.ok(&["--yes", "drive", "tidy", path(&main)]);
    set_mode(&locked, 0o755);
    assert!(out.contains("removed 0 file(s)") && line(&out, "Locked/.DS_Store: ").starts_with("! "), "{out}");
}

/// Checking a file system or ejecting runs the system's own tools on the disk: here the tests
/// only see what happens before that.
#[test]
fn fsck_and_eject_stop_before_touching_the_disk() {
    let env = Env::new();
    let main = env.drive("Main");
    // Repairing asks first: unconfirmed, nothing runs.
    let r = env.run(&["drive", "fsck", path(&main), "--repair"]);
    assert_eq!(r.code, 3, "{}", r.err);
    assert!(r.err.contains("not confirmed"), "{}", r.err);
    // A folder isn't a volume: nothing to eject.
    let r = env.run(&["eject", path(&main)]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.err.contains("isn't the root of a volume mounted from a disk"), "{}", r.err);
}
