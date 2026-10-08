//! Taking changes back: `undo` (and `drive restore`) puts a backup back, saying what it changes
//! first; `restore --playlist` brings back one playlist or folder from a snapshot or a backup.

use rekordbox_pdb::PdbEditor;

use crate::support::{Env, PDB, line, path};

const TRACK_2: &str = "Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3";

#[test]
fn undo_says_what_putting_the_backup_back_changes_and_loses() {
    let env = Env::new();
    let main = env.drive("Main");
    assert!(env.ok(&["undo", "--list"]).contains("Nothing to undo on the mounted drives."));
    let out = env.ok(&["undo", "--list", path(&main)]);
    assert!(out.starts_with(&format!("Main ({})\n  no backups", main.display())), "{out}");
    env.ok(&["--yes", "playlist", "new", path(&main), "Extra"]);
    let out = env.ok(&["undo", "--list"]);
    assert!(line(&out, "before editing").ends_with("(2 file(s), 1 recorded edit(s)) <- undo"), "{out}");
    // Since the backup: a player recorded a history list, and an audio file went.
    let mut ed = PdbEditor::from_file(main.join(PDB)).unwrap();
    ed.create_history_playlist("HISTORY 002").unwrap();
    ed.save(main.join(PDB)).unwrap();
    std::fs::remove_file(main.join(TRACK_2)).unwrap();

    // The drive changed since: only with --force.
    assert!(env.fails(1, &["undo", path(&main), "--dry-run"]).contains("Main changed since backup"));
    let out = env.ok(&["undo", path(&main), "--dry-run", "--force"]);
    assert!(out.starts_with("Main: backup ") && out.contains("(before editing, "), "{out}");
    assert!(line(&out, "PIONEER/rekordbox/export.pdb").trim_start().starts_with("replaced"), "{out}");
    assert!(out.contains("- playlists (1):\n    Extra"), "{out}");
    assert!(line(&out, "Recorded edits taken off").ends_with(":"), "{out}");
    assert!(line(&out, "  - create playlist Extra").starts_with("  - "), "{out}");
    assert!(out.contains("history lists players recorded since are dropped: HISTORY 002"), "{out}");
    assert!(line(&out, "no audio on the drive for").contains("Demo Track 2"), "{out}");

    assert!(env.ok(&["--yes", "undo", path(&main), "--force"]).contains("put back backup"));
    assert!(!env.ok(&["playlist", "ls", path(&main)]).contains("Extra"));
}

#[test]
fn a_backup_that_matches_the_drive_has_nothing_to_put_back() {
    let env = Env::new();
    let main = env.drive("Main");
    env.ok(&["--yes", "drive", "contact", path(&main), "sam@example.com"]);
    std::fs::remove_file(main.join("IF FOUND.txt")).unwrap();
    let out = env.ok(&["drive", "restore", path(&main)]);
    assert!(out.contains("no file differs from the backup"), "{out}");
}

#[test]
fn a_playlist_comes_back_from_a_snapshot_without_the_tracks_that_left() {
    let env = Env::new();
    let main = env.pulled("Main");
    env.ok(&["--yes", "playlist", "new", "Main", "Gigs/Friday", "--playlist", "Set"]);
    env.ok(&["pull", "Main"]);
    env.ok(&["--yes", "playlist", "delete", "Main", "Set"]);
    env.ok(&["--yes", "track", "rm", "Main", "--query", "Demo Track 2"]);
    env.ok(&["--yes", "playlist", "new", "Main", "Gigs/Saturday"]);
    assert!(!main.join(TRACK_2).exists());

    let out = env.ok(&["drive", "restore", "Main", "--playlist", "Set", "--dry-run"]);
    assert!(out.starts_with("Restoring Set from snapshot ") && line(&out, "Restoring").contains(" of Main ("), "{out}");
    assert!(out.contains("1 entr(ies) left out: their tracks aren't on the drive any more"), "{out}");
    assert!(line(&out, "Set #2: ").contains("Demo Track 2"), "{out}");
    let out = env.ok(&["--yes", "restore", "Main", "--playlist", "Set"]);
    assert!(
        out.contains("  - fill Set with 1 entr(ies)\n") && out.contains("2 change(s) written to export.pdb"),
        "{out}"
    );
    assert!(line(&env.ok(&["playlist", "ls", "Main"]), "Set").contains("(1)"));
    let out = env.ok(&["restore", "Main", "--playlist", "Set"]);
    assert!(out.contains("Set on Main is already as it was"), "{out}");

    // A folder: what's in the snapshot comes back, what isn't is left as it is.
    let out = env.ok(&["restore", "Main", "--playlist", "Gigs", "--dry-run"]);
    assert!(line(&out, "Restoring folder Gigs (1 playlists)").contains("from snapshot"), "{out}");
    assert!(out.contains("Left as they are (not in the snapshot): Gigs/Saturday"), "{out}");
}

#[test]
fn a_playlist_comes_back_from_a_backup() {
    let env = Env::new();
    let main = env.drive("Main");
    env.ok(&["--yes", "playlist", "new", path(&main), "Gigs/Friday", "--playlist", "Set"]);
    env.ok(&["--yes", "playlist", "delete", path(&main), "Set"]);
    let backup = env.ok(&["drive", "backups", path(&main)]).split_whitespace().next().unwrap().to_string();
    env.ok(&["--yes", "playlist", "new", path(&main), "Gigs/Saturday"]);
    let out = env.ok(&["restore", path(&main), "--playlist", "Gigs", "--backup", &backup, "--dry-run"]);
    assert!(out.contains("Left as they are (not in the backup): Gigs/Saturday"), "{out}");

    let out = env.ok(&["drive", "restore", path(&main), "--playlist", "Set", "--backup", &backup, "--dry-run"]);
    assert!(line(&out, "Restoring Set from backup").contains(&format!("{backup} (before editing, ")), "{out}");
    env.ok(&["--yes", "drive", "restore", path(&main), "--playlist", "Set", "--backup", &backup]);
    assert!(line(&env.ok(&["playlist", "ls", path(&main)]), "Set").contains("(2)"));
}
