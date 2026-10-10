//! The reports that read drives, as a DJ reads them: doctor, diff, check and export for other
//! DJ software. Their `--json` shapes are tested in `reports` (the module beside this one).

use rekordbox_pdb::PdbEditor;

use crate::support::{Env, PDB, line, path};

const TRACK_1: &str = "Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3";
const TRACK_2: &str = "Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3";

#[test]
fn doctor_reports_each_problem_with_what_fixes_it() {
    let env = Env::new();
    let main = env.drive("Main");
    let old = main.join("Contents/Old");
    std::fs::create_dir_all(&old).unwrap();
    for i in 1..=16 {
        std::fs::write(old.join(format!("forgotten {i:02}.mp3")), "old").unwrap();
    }
    // The second track's audio is a copy of the first's, and then it goes missing.
    std::fs::copy(main.join(TRACK_1), main.join(TRACK_2)).unwrap();
    let out = env.exits(4, &["doctor", path(&main), "--hash-audio"]);
    assert!(out.starts_with(&format!("Main {}\n  2 tracks", main.display())), "{out}");
    assert!(line(&out, "Duplicates (1 group(s))").starts_with("Duplicates"), "{out}");
    assert!(line(&out, "(same audio)").contains("Demo Track"), "{out}");
    assert!(line(&out, "keep").contains("1 playlist(s)"), "{out}");
    assert!(out.contains("`locallibrary track dedupe \"Main\"` replaces each"), "{out}");
    assert!(line(&out, "Orphaned files (16, 48 B)").starts_with("Orphaned"), "{out}");
    assert!(out.contains("forgotten 15.mp3") && !out.contains("forgotten 16.mp3"), "{out}");
    assert!(out.contains("... and 1 more") && out.contains("locallibrary doctor \"Main\" --fix-orphans"), "{out}");
    assert!(out.contains("every track is in a playlist"), "{out}");

    std::fs::remove_file(main.join(TRACK_2)).unwrap();
    std::fs::remove_dir_all(main.join("PIONEER/USBANLZ/P053")).unwrap();
    env.ok(&["--yes", "playlist", "delete", path(&main), "Set"]);
    let out = env.exits(4, &["doctor", path(&main), "--only", "missing,unfiled,duplicates,large-files"]);
    assert!(line(&out, "no audio file").contains("Demo Track 2"), "{out}");
    assert!(line(&out, "no analysis").contains("PIONEER/USBANLZ/P053"), "{out}");
    assert!(out.contains("no duplicates") && out.contains("no files of 4 GB or more"), "{out}");
    assert!(line(&out, "In no playlist (2)").starts_with("In no playlist"), "{out}");
    assert!(out.contains("`locallibrary playlist unfiled \"Main\"` collects them"), "{out}");
    assert!(!out.contains("Orphaned"), "only the sections asked for: {out}");

    // Placeholder audio doesn't decode.
    let out = env.exits(4, &["doctor", path(&main), "--only", "audio"]);
    assert!(line(&out, "Audio files with problems (1 of 1)").starts_with("Audio"), "{out}");

    // FAT32 holds files up to 4 GB. `set_len` writes nothing: the file is sparse on macOS and
    // Linux, and NTFS reserves the space without filling it.
    std::fs::File::create(main.join("Contents/whole night.wav")).unwrap().set_len(4 << 30).unwrap();
    let out = env.exits(4, &["doctor", path(&main), "--only", "large-files"]);
    assert!(out.contains("Files FAT32 can't hold (1)\n    Contents/whole night.wav  4.3 GB"), "{out}");
}

#[test]
fn doctor_says_when_export_pdb_and_onelibrary_drift_apart() {
    let env = Env::new();
    let main = env.drive("Main");
    env.ok(&["--yes", "convert", path(&main), "--to", "onelibrary"]);
    assert!(env.ok(&["doctor", path(&main), "--only", "drift"]).contains("export.pdb and OneLibrary agree"));
    // A playlist only export.pdb has, as an older rekordbox would write it.
    let mut ed = PdbEditor::from_file(main.join(PDB)).unwrap();
    ed.create_playlist("Only here", 0, false).unwrap();
    ed.save(main.join(PDB)).unwrap();
    let out = env.exits(4, &["doctor", path(&main), "--only", "drift"]);
    assert!(
        out.contains("export.pdb and OneLibrary differ") && line(&out, "1 playlist(s)").contains("0 track(s) only"),
        "{out}"
    );
    assert!(out.contains("`locallibrary convert --replace` rebuilds one from the other"), "{out}");
    std::fs::write(main.join("PIONEER/rekordbox/exportLibrary.db"), vec![0x5a; 8192]).unwrap();
    let out = env.exits(4, &["doctor", path(&main), "--only", "drift"]);
    assert!(line(&out, "OneLibrary can't be read: ").starts_with("! "), "{out}");
}

#[test]
fn doctor_of_every_mounted_drive_reports_each() {
    let env = Env::new();
    // Given no drive, a write goes to every mounted one: none is an error.
    assert!(env.fails(1, &["art", "auto"]).contains("no rekordbox drives mounted"));
    let main = env.drive("Main");
    let other = env.drive("Other");
    for (file, what) in [
        ("Contents/stray.mp3", "stray"),
        ("PIONEER/USBANLZ/P999/00000000/ANLZ0000.DAT", "analysis"),
        ("PIONEER/Artwork/00099/a99.jpg", "artwork"),
    ] {
        std::fs::create_dir_all(other.join(file).parent().unwrap()).unwrap();
        std::fs::write(other.join(file), what).unwrap();
    }
    let out = env.exits(4, &["doctor", "--only", "orphans"]);
    assert!(line(&out, "Main ").ends_with(path(&main)) && line(&out, "Other ").ends_with(path(&other)), "{out}");
    assert!(out.contains("no orphaned files") && line(&out, "Contents/stray.mp3").contains("audio"), "{out}");
    assert!(line(&out, "ANLZ0000.DAT").contains("analysis ") && line(&out, "a99.jpg").contains("artwork "), "{out}");
    // Nothing to do on any of them: nothing asked, nothing written.
    let out = env.ok(&["art", "auto"]);
    assert!(out.contains("2 still missing"), "{out}");
}

#[test]
fn doctor_deletes_orphans_only_once_shown_and_confirmed() {
    let env = Env::new();
    let main = env.drive("Main");
    assert!(env.ok(&["doctor", path(&main), "--fix-orphans"]).contains("no orphaned files"));
    let old = main.join("Contents/Old");
    std::fs::create_dir_all(&old).unwrap();
    for i in 1..=21 {
        std::fs::write(old.join(format!("{i:02}.mp3")), "old").unwrap();
    }
    let out = env.ok(&["doctor", path(&main), "--fix-orphans", "--dry-run"]);
    assert!(out.starts_with("Main: delete 21 file(s) no track uses, 63 B"), "{out}");
    assert!(line(&out, "Contents/Old/01.mp3").contains("- audio"), "{out}");
    assert!(out.contains("... and 1 more") && out.contains("`locallibrary undo` brings them back"), "{out}");
    let out = env.ok(&["--yes", "doctor", path(&main), "--fix-orphans"]);
    assert!(out.contains("deleted 21 file(s), 63 B") && out.contains("undo with: locallibrary undo"), "{out}");
    assert!(!old.join("01.mp3").exists());
    // The backup holds them, audio that may be the only copy.
    assert!(
        line(&env.ok(&["drive", "backups", path(&main)]), "before removing orphaned files")
            .ends_with(" file(s), 21 audio")
    );
}

#[test]
fn diff_lists_what_changed_in_each_playlist() {
    let env = Env::new();
    let main = env.drive("Main");
    let backup = env.blank("Backup");
    env.ok(&["--yes", "sync", path(&main), path(&backup)]);
    let out = env.ok(&["diff", path(&main), path(&backup)]);
    assert!(out.contains("Main → Backup") && out.contains("no differences (1 playlist(s) the same)"), "{out}");

    env.ok(&["--yes", "playlist", "new", path(&backup), "Extra"]);
    env.ok(&["--yes", "playlist", "reorder", path(&backup), "Set", "--at", "2", "--to", "1"]);
    let out = env.ok(&["diff", path(&main), path(&backup)]);
    assert!(out.contains("  + playlists (1):\n    Extra"), "{out}");
    assert!(line(&out, "~ Set").contains("+0 -0 moved"), "{out}");
    assert!(line(&out, "↕").ends_with("1 → 2. Loopmasters - Demo Track 1"), "{out}");

    // An entry added on one side is removed on the other.
    env.ok(&["--yes", "playlist", "add", path(&backup), "Set", "--query", "Demo Track 1", "--allow-repeats"]);
    let out = env.ok(&["diff", path(&main), path(&backup)]);
    assert!(line(&out, "~ Set").contains("+1 -0"), "{out}");
    assert!(line(&out, "+    3. ").ends_with("Loopmasters - Demo Track 1"), "{out}");
    let out = env.ok(&["diff", path(&backup), path(&main)]);
    assert!(line(&out, "-    3. ").ends_with("Loopmasters - Demo Track 1"), "{out}");
    assert!(out.contains("  - playlists (1):\n    Extra"), "{out}");

    // Another library altogether: the first ten of each list, then how many more.
    let club = env.club("Club");
    let out = env.ok(&["diff", path(&main), path(&club)]);
    assert!(out.contains("  + tracks (12):") && out.contains("    ... and 2 more"), "{out}");

    let out = env.ok(&["diff", path(&main), path(&backup), "--summary"]);
    assert!(out.contains("Backup"), "{out}");
    assert_eq!(env.ok(&["compare", path(&main), path(&backup)]), out);
    let r = env.run(&["compare", path(&main), path(&backup), "--playlist", "Set"]);
    assert_eq!(r.code, 2, "{}", r.out);
    assert!(r.err.contains("--playlist doesn't go with --summary"), "{}", r.err);
}

#[test]
fn check_says_whether_drives_are_ready_for_the_players() {
    let env = Env::new();
    env.pulled("Main");
    env.ok(&["--yes", "players", "set", "CDJ-2000NXS2"]);
    let out = env.exits(4, &["check", "Main", "--audio"]);
    assert!(out.starts_with("Main\n  players: CDJ-2000NXS2 (remembered)"), "{out}");
    assert!(out.contains("--verbose lists the tracks."), "{out}");
    assert!(line(&out, "problem(s)").contains("warning(s)"), "{out}");
    let out = env.exits(4, &["check", "Main", "--audio", "--verbose"]);
    assert!(line(&out, "Demo Track 1  can't be opened").ends_with(&format!("/{TRACK_1}")), "{out}");

    env.ok(&["--yes", "players", "set", "XDJ-AZ", "--library", "Main"]);
    let out = env.exits(4, &["check", "Main"]);
    assert!(out.starts_with("Main\n  players: XDJ-AZ (remembered for Main)"), "{out}");
}

#[test]
fn export_writes_playlists_as_lists() {
    let env = Env::new();
    let main = env.drive("Main");
    for (format, file) in [("m3u8", "set.m3u8"), ("csv", "set.csv"), ("txt", "set.txt")] {
        let to = env.outside(file);
        let out = env.ok(&["export", path(&main), "Set", "--format", format, "-o", path(&to)]);
        assert!(line(&out, "wrote").ends_with(&format!("{file} (Set, 2 tracks)")), "{out}");
        assert!(std::fs::read_to_string(&to).unwrap().contains("Demo Track 2"), "{format}");
    }
    // A folder with no playlists in it.
    env.ok(&["--yes", "playlist", "new", path(&main), "Empty", "--folder"]);
    let out = env.ok(&["export", path(&main), "Empty", "--split", "-o", path(&env.outside("Empty"))]);
    assert!(out.contains("no playlists there; nothing written"), "{out}");
}

#[test]
fn export_for_other_dj_software_shows_its_plan_first() {
    let env = Env::new();
    let main = env.drive("Main");
    let to = env.outside("For Traktor");
    let out = env.ok(&["export", path(&main), "--format", "traktor", "-o", path(&to), "--dry-run"]);
    assert!(out.starts_with("Would write for Traktor: 2 tracks, 1 playlists"), "{out}");
    assert!(
        line(&out, "collection.nml").contains("  Traktor collection: tracks, beat grids, cues and playlists, "),
        "{out}"
    );
    assert!(out.contains("nothing was written") && !to.exists(), "{out}");
    let out = env.ok(&["export", path(&main), "--format", "traktor", "-o", path(&to)]);
    assert!(out.starts_with("Writing for Traktor") && out.contains("wrote 1 file(s) for Traktor to"), "{out}");

    // Serato's tags mean copying the audio: asked first.
    let to = env.outside("For Serato");
    let r = env.run(&["export", path(&main), "--format", "serato", "-o", path(&to), "--write-tags"]);
    assert_eq!(r.code, 3, "{}", r.err);
    assert!(r.out.contains("of audio to copy") && r.out.contains("nothing was written"), "{}", r.out);

    for (args, error) in [
        (&["--format", "serato", "--history", "HISTORY 001"][..], "--history and --split don't apply to Serato"),
        (&["--format", "m3u8", "--no-analysis"][..], "are for other DJ software's formats"),
    ] {
        let r = env.run(&[&["export", path(&main), "-o", path(&to)], args].concat());
        assert_eq!(r.code, 1, "{args:?}");
        assert!(r.err.contains(error), "{args:?}: {}", r.err);
    }
}
