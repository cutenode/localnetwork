//! What `drive ls`, `drive info` and `status` say of each drive: what it holds, where it stands
//! against the store, and whether its two databases agree.

use crate::support::{Env, PDB, line, path};

#[test]
fn drives_are_described_in_words() {
    let env = Env::new();
    let a = env.demo("A");
    let listed = env.ok(&["drive", "ls"]);
    assert!(line(&listed, path(&a)).starts_with("A  ") && listed.contains("2 tracks, 0 playlists"), "{listed}");

    let info = env.ok(&["drive", "info", "A"]);
    for fact in [
        "id          not adopted (locallibrary drive adopt)",
        "library     2 tracks, 0 playlists in 0 folders, 1 artists",
        "playtime    5m 00s",
        "artwork     2 track(s) without artwork",
        "settings    MYSETTING.DAT",
    ] {
        assert!(info.contains(fact), "{fact:?} in:\n{info}");
    }
    assert!(line(&env.ok(&["status"]), "A ").contains("not in the store (locallibrary pull)"));

    // Saved, then changed.
    env.ok(&["--yes", "drive", "adopt", "A", "--name", "Main"]);
    env.ok(&["pull", "Main"]);
    assert!(line(&env.ok(&["status"]), "Main").ends_with("up to date"));
    let info = env.ok(&["drive", "info", "Main"]);
    assert!(!info.contains("not adopted") && info.contains("last seen   Pull at "), "{info}");
    env.ok(&["--yes", "track", "set", "Main", "--track", "1", "--rating", "2"]);
    assert!(line(&env.ok(&["status"]), "Main").contains("has changes not in the store"));

    // With --all, folders without a library are listed too.
    env.blank("Empty");
    let unreadable = env.blank("Unreadable").join(PDB);
    std::fs::create_dir_all(unreadable.parent().unwrap()).unwrap();
    std::fs::write(&unreadable, "not a database").unwrap();
    let all = env.ok(&["drive", "ls", "--all"]);
    assert!(
        line(&all, "Empty").contains("  no export  ") && line(&all, "Unreadable").contains("  unreadable export  ")
    );
    assert!(!env.ok(&["drive", "ls"]).contains("Empty"));
    assert!(env.ok(&["drive", "info", "Empty"]).contains("\n  library     none\n"));
    // One drive that can't be read doesn't keep the others from being listed; the run says so.
    let r = env.run(&["tracks"]);
    assert_eq!(r.code, 5, "some of several failed");
    assert!(r.err.contains("! Unreadable left out: ") && r.out.contains("2 track(s), 5m 00s"), "{}", r.err);

    // Nothing mounted.
    let none = Env::new();
    assert!(none.ok(&["drive", "ls"]).starts_with("No rekordbox drives mounted."));
    assert_eq!(none.ok(&["status"]).trim(), "No rekordbox drives mounted.");
}

/// `status` says where each drive stands against the store's snapshots of its library.
#[test]
fn status_says_where_each_drive_stands() {
    let env = Env::new();
    let b = env.blank("B");
    env.demo("A");
    env.ok(&["--yes", "drive", "adopt", "A", "--name", "Main"]);
    env.ok(&["pull", "Main"]);
    env.ok(&["--yes", "sync", "--from", "Main", path(&b)]);
    assert!(line(&env.ok(&["status"]), "B ").ends_with("up to date"));
    // The store moves on without B.
    env.ok(&["--yes", "track", "set", "Main", "--track", "1", "--rating", "3"]);
    env.ok(&["pull", "Main"]);
    assert!(line(&env.ok(&["status"]), "B ").contains("behind the store (locallibrary library push)"));
    env.ok(&["--yes", "track", "set", "B", "--track", "2", "--rating", "1"]);
    assert!(line(&env.ok(&["status"]), "B ").contains("diverged from the store"));

    // Syncing the group needs a decision now, and the error says how to make each one.
    let r = env.run(&["sync"]);
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.err.contains("error: Main and B both have changes the other lacks"), "{}", r.err);
    assert!(line(&r.err, "  hint: Copy Main over B: ").contains(": locallibrary sync --from "), "{}", r.err);
    assert!(line(&r.err, "  hint: Merge B into Main: ").contains(": locallibrary merge "), "{}", r.err);
}

/// A drive with both databases says whether they agree, and what's in one but not the other.
#[test]
fn a_drive_says_when_its_two_databases_differ() {
    let env = Env::new();
    let a = env.demo("A");
    env.ok(&["--yes", "convert", "A", "--to", "onelibrary"]);
    let info = env.ok(&["drive", "info", "A"]);
    assert!(info.contains("  also        OneLibrary (exportLibrary.db), for CDJ-3000X"), "{info}");
    assert!(info.contains("  databases   export.pdb and OneLibrary agree"), "{info}");

    // export.pdb gets a track and a playlist the OneLibrary database doesn't, loses one it has,
    // and another's title changes.
    let pdb = a.join(PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    ed.add_track(&rekordbox_pdb::NewTrack::new("Only Here", "/Contents/only.mp3")).unwrap();
    ed.create_playlist("Older Players", 0, false).unwrap();
    ed.remove_tracks(&[2]).unwrap();
    ed.set_track_strings(1, &[("title", "Renamed")]).unwrap();
    ed.save(&pdb).unwrap();
    std::fs::write(a.join("Contents/only.mp3"), "audio").unwrap();
    let info = env.ok(&["drive", "info", "A"]);
    assert!(info.contains("! export.pdb and OneLibrary differ, so players don't all show the same library:"));
    assert!(
        info.contains("    1 track(s) only in export.pdb, hidden from CDJ-3000X/OPUS-QUAD/XDJ-AZ: Only Here"),
        "{info}"
    );
    assert!(info.contains("    1 track(s) only in OneLibrary, hidden from older players: Loopmasters - Demo Track 2"));
    assert!(info.contains("    1 track(s) with different details"), "{info}");
    assert!(info.contains("    1 playlist(s) differ: Older Players"), "{info}");
    assert!(
        line(&info, "rebuild one from the other: locallibrary convert ").ends_with("A\" --to onelibrary --replace")
    );
    assert!(line(&env.ok(&["status"]), "A ").ends_with("export.pdb and OneLibrary differ (locallibrary drive info)"));

    std::fs::remove_file(&pdb).unwrap();
    let info = env.ok(&["drive", "info", "A"]);
    assert!(info.contains("  format      OneLibrary only (exportLibrary.db): CDJ-3000X"), "{info}");
}
