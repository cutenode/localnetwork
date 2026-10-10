//! Between drives: `compare` says which is newer, `sync` copies the newer over the older, and
//! `merge` adds one's tracks and playlists to another; on one drive, `convert` writes one database
//! from the other and `clean` takes out rekordbox's demo samples. Each shows its plan first.

use crate::support::{Env, PDB, club_export, line, path};

/// `compare` says which drive is newer and why, what differs, and what to run.
#[test]
fn comparisons_say_which_drive_is_newer_and_what_to_do() {
    let env = Env::new();
    let (a, b) = (env.demo("A"), env.blank("B"));
    env.ok(&["--yes", "sync", "--from", path(&a), path(&b)]);
    assert!(env.ok(&["compare", "A", "B"]).starts_with("A and B are identical"));

    env.ok(&["--yes", "track", "set", "A", "--track", "1", "--rating", "2"]);
    env.ok(&["--yes", "playlist", "new", "A", "Warmup", "--track", "1"]);
    let newer = env.ok(&["compare", "A", "B"]);
    assert!(newer.starts_with("A is newer than B [high confidence]"), "{newer}");
    assert!(line(&newer, "  A ").contains("2 tracks, 1 playlists, exported "), "{newer}");
    assert!(newer.contains("Evidence\n  • A previously held B's current state (favours A)"), "{newer}");
    assert!(newer.contains("~ tracks (1):\n    Loopmasters - Demo Track 1 (rating)"), "{newer}");
    assert!(newer.contains("+ playlists (1):\n    Warmup"), "{newer}");
    assert!(newer.ends_with("run `locallibrary sync` with the same two drives to update the older one\n"));
    assert!(env.ok(&["compare", "B", "A"]).starts_with("A is newer than B"), "either way round");

    env.ok(&["--yes", "track", "set", "B", "--track", "2", "--rating", "1"]);
    let diverged = env.ok(&["compare", "A", "B"]);
    assert!(diverged.starts_with("A and B have both changed since they were last in sync"), "{diverged}");
    assert!(diverged.contains("pick a winner, or `locallibrary merge` to combine"), "{diverged}");

    // Another library: long lists are cut short.
    env.club("Other");
    let unrelated = env.ok(&["compare", "A", "Other"]);
    assert!(unrelated.starts_with("A and Other hold different libraries (0% overlap)"), "{unrelated}");
    assert!(unrelated.contains("+ tracks (12):") && unrelated.contains("    ... and 2 more"), "{unrelated}");
    assert!(unrelated.ends_with("use `locallibrary merge <source> <target>` to combine them\n"), "{unrelated}");
}

/// A plan says what it would copy, and applying it what it did; several drives are each planned.
#[test]
fn syncing_shows_the_plan_then_what_it_copied() {
    let env = Env::new();
    let (a, b) = (env.demo("A"), env.blank("B"));
    let plan = env.ok(&["sync", "--from", "A", path(&b), "--dry-run"]);
    assert!(plan.starts_with("A is newer than B"), "{plan}");
    assert!(line(&plan, "A → B").ends_with(path(&b)), "{plan}");
    assert!(plan.contains("  copy   10 files (10 new, 0 changed), "), "{plan}");
    assert!(plan.contains("[2 audio, 6 analysis, 0 artwork, 2 database]"), "{plan}");
    assert!(plan.contains("  keep   0 unchanged files\n  space  "), "{plan}");
    assert!(!plan.contains("copied"), "a dry run copies nothing");
    let done = env.ok(&["--yes", "sync", "--from", "A", path(&b)]);
    assert!(done.contains("✓ copied 10 files ("), "{done}");
    let again = env.ok(&["sync", "--from", "A", path(&b)]);
    assert!(again.starts_with("A and B are identical") && again.ends_with("✓ nothing to copy\n"), "{again}");

    // Onto several drives: each one's plan, then each one's result.
    env.ok(&["--yes", "track", "set", "A", "--track", "1", "--rating", "5"]);
    let c = env.blank("C");
    let plan = env.ok(&["sync", path(&a), path(&b), path(&c), "--dry-run"]);
    assert!(plan.starts_with("A is the newest\n"), "{plan}");
    assert!(plan.contains("A → B") && plan.contains("A → C"), "{plan}");
    let done = env.ok(&["--yes", "sync", path(&a), path(&b), path(&c)]);
    assert!(line(&done, "B: ").contains("copied") && line(&done, "C: ").contains("copied"), "{done}");
    let again = env.ok(&["sync", path(&a), path(&b), path(&c)]);
    assert!(again.contains("✓ B already matches") && again.contains("✓ C already matches"), "{again}");
}

/// Merging another drive in, converting between the databases and cleaning out demo samples:
/// each shows its plan, does it, and says how to undo it.
#[test]
fn merge_convert_and_clean_show_their_plan_and_how_to_undo() {
    let env = Env::new();
    env.demo("A");
    env.ok(&["--yes", "playlist", "new", "A", "Sets/Friday", "--track", "1", "--track", "2"]);
    env.ok(&["--yes", "tags", "add", "A", "Genre/Deep", "--track", "2"]);
    // Another library, on a drive with the demo's audio files.
    let other = env.demo("Other");
    std::fs::copy(club_export(), other.join(PDB)).unwrap();

    let plan = env.ok(&["merge", "A", "Other", "--dry-run"]);
    assert!(plan.contains("  2 new track(s), 0 already present, "), "{plan}");
    // The other drive has files at the same paths: the new ones are copied beside them.
    let renamed = line(&plan, "+ Loopmasters - Demo Track 1");
    assert!(renamed.ends_with("(as Contents/Loopmasters/UnknownAlbum/Demo Track 1 (2).mp3)"), "{plan}");
    assert!(plan.contains("  create playlist A/Sets/Friday (+2 tracks)"), "{plan}");
    assert!(plan.contains("  My Tags for 1 track(s)\n  create My Tag Genre/Deep in exportExt.pdb"), "{plan}");
    let done = env.ok(&["--yes", "merge", "A", "Other"]);
    assert!(done.contains("✓ added 2 track(s), 3 playlist(s), 2 playlist entries"), "{done}");
    assert!(done.contains("  and My Tags on 1 track(s)\nundo with: locallibrary undo \"Other\" (backup "), "{done}");
    let again = env.ok(&["merge", "A", "Other", "--playlists", "none"]);
    assert!(again.contains("nothing to merge: the target already has everything"), "{again}");

    let plan = env.ok(&["convert", "A", "--to", "onelibrary", "--dry-run"]);
    assert!(plan.starts_with("A: create OneLibrary (exportLibrary.db) from export.pdb\n"), "{plan}");
    assert!(plan.contains("  2 track(s), 1 playlist(s) in 1 folder(s), 0 history playlist(s), 0 artwork image(s)"));
    assert!(plan.contains("  My Tags on 1 track(s)"), "{plan}");
    let done = env.ok(&["--yes", "convert", "A", "--to", "onelibrary"]);
    assert!(done.contains("✓ converted 2 track(s), 1 playlist(s)"), "{done}");
    assert!(done.contains("undo with: locallibrary undo \"A\" (backup "), "{done}");
    let rebuild = env.ok(&["convert", "A", "--to", "device-library", "--replace", "--dry-run"]);
    assert!(rebuild.starts_with("A: rebuild export.pdb from OneLibrary (exportLibrary.db)"), "{rebuild}");
    assert!(env.ok(&["drive", "info", "A"]).contains("databases   export.pdb and OneLibrary agree"));

    // rekordbox's demo samples, the same one exported twice, in a playlist.
    assert_eq!(env.ok(&["clean", "A"]).trim(), "✓ nothing to clean");
    let a = env.volumes.join("A");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(a.join(PDB)).unwrap();
    let samples = ed.create_playlist("Samples", 0, false).unwrap();
    for file in ["House1.wav", "House1 (2).wav"] {
        let file = format!("/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES/{file}");
        let sample = rekordbox_pdb::NewTrack {
            artist: Some("rekordbox".into()),
            ..rekordbox_pdb::NewTrack::new("House 1", &file)
        };
        let id = ed.add_track(&sample).unwrap();
        ed.add_to_playlist(samples, id, None).unwrap();
        let audio = a.join(file.trim_start_matches('/'));
        std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
        std::fs::write(&audio, "demo sample").unwrap();
    }
    ed.save(a.join(PDB)).unwrap();
    let plan = env.ok(&["clean", "A", "--dry-run"]);
    assert!(plan.starts_with("A: remove 2 track(s) and 2 file(s), "), "{plan}");
    assert!(plan.contains("    - rekordbox - House 1  ×2"), "the same sample, listed once: {plan}");
    assert!(plan.contains("  remove 2 entries from playlist Samples"), "{plan}");
    let done = env.ok(&["--yes", "clean", "A"]);
    assert!(done.contains("✓ removed 2 track(s) and 2 file(s), "), "{done}");
    assert!(done.contains("undo with: locallibrary undo \"A\" (backup "), "{done}");
}
