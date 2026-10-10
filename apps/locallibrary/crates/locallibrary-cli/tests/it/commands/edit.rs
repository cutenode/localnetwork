//! `playlist`, `track`, `smart` and `edits`: each edit's plan as it's shown, what applying it
//! says, and the command lines that don't say enough.

use crate::support::{Env, PDB, path};

/// The playlists on the drive and how many entries each holds, from `playlist ls`.
fn tree(env: &Env, drive: &str) -> String {
    env.ok(&["playlist", "ls", drive])
}

#[test]
fn playlists_are_made_filled_reordered_moved_and_deleted() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    let yes = |args: &[&str]| env.ok(&[&["--yes", "playlist"], args].concat());

    let out = yes(&["new", d, "Sets/Friday", "--track", "1", "--track", "2", "--track", "3"]);
    assert!(out.starts_with("Club: export.pdb\n  - create playlist Sets/Friday\n  - add 3 track(s) to Sets/Friday"));
    assert!(out.contains("    + Sets/\n    + Sets/Friday\n"), "{out}");
    assert!(out.contains("2 change(s) written to export.pdb"), "{out}");
    assert!(out.contains(&format!("Undo with `locallibrary undo {d}`")), "{out}");

    let out = yes(&["add", d, "Friday", "--track", "4", "--position", "1"]);
    assert!(out.contains("    ~ Sets/Friday +1, 3 -> 4 entries"), "{out}");
    let out = yes(&["reorder", d, "Friday", "--at", "3", "--to", "1"]);
    assert!(out.contains("    ~ Sets/Friday reordered, 4 -> 4 entries"), "{out}");
    let out = yes(&["remove", d, "Friday", "--at", "1"]);
    assert!(out.contains("    ~ Sets/Friday -1, 4 -> 3 entries"), "{out}");
    // `move --at --to` is `reorder`'s old spelling.
    let out = yes(&["move", d, "Friday", "--at", "1", "--to", "3"]);
    assert!(out.contains("  - move 1 entr(ies) in Sets/Friday to position 3"), "{out}");
    let out = yes(&["sort", d, "Friday", "--sort", "-bpm"]);
    assert!(out.contains("  - sort Sets/Friday by -bpm"), "{out}");
    // A repeat neither adds a track nor removes one: the entries change.
    let out = yes(&["add", d, "Friday", "--track", "1", "--allow-repeats"]);
    assert!(out.contains("    ~ Sets/Friday entries changed, 3 -> 4 entries"), "{out}");
    let out = yes(&["dedupe", d, "Friday"]);
    assert!(out.contains("  - drop 1 repeat(s) from Sets/Friday"), "{out}");
    let out = yes(&["dedupe", d, "Friday", "--recordings"]);
    assert!(out.contains("nothing to change"), "{out}");

    let out = yes(&["order", d, "Friday", "--start", "1", "--into", "Sets/Ordered"]);
    assert!(out.contains("  - order Sets/Friday for mixing into Sets/Ordered"), "{out}");
    let out = yes(&["fill", d, "Peak", "--filter", "bpm:>=125", "--sort", "-bpm", "--limit", "3"]);
    assert!(out.contains("  - fill Peak with 3 track(s)"), "{out}");
    yes(&["unfiled", d]);
    assert_eq!(tree(&env, d), "aab (12)\nSets/\n  Friday (3)\n  Ordered (3)\nPeak (3)\nUnfiled (0)\n");

    // Renamed, moved into a folder, among its contents, and back to the top level.
    let out = yes(&["rename", d, "Peak", "Peak time"]);
    assert!(out.contains("    + Peak time\n    - Peak\n"), "{out}");
    yes(&["move", d, "Peak time", "--into", "Sets", "--position", "1"]);
    assert!(tree(&env, d).contains("Sets/\n  Peak time (3)\n  Friday (3)\n"), "{}", tree(&env, d));
    let out = yes(&["move", d, "Sets/Peak time", "--position", "2"]);
    assert!(out.contains("  - move Sets/Peak time into Sets at position 2"), "{out}");
    let out = yes(&["move", d, "Sets/Peak time", "--top"]);
    assert!(out.contains("  - move Sets/Peak time into the top level"), "{out}");
    let out = yes(&["delete", d, "Sets"]);
    assert!(out.contains("    - Sets/\n    - Sets/Friday\n    - Sets/Ordered\n"), "{out}");
    // With no subcommand, or no drive, the tree of every drive there is (here, the one).
    assert_eq!(env.ok(&["playlist", d]), tree(&env, d));
    assert_eq!(env.ok(&["playlist"]), tree(&env, d));
}

#[test]
fn playlist_command_lines_that_dont_say_enough_are_refused() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    let err = env.fails(1, &["playlist", "new", d, "Archive", "--folder", "--track", "1"]);
    assert!(err.contains("a folder holds playlists, not tracks"), "{err}");
    let err = env.fails(2, &["playlist", "add", d, "aab"]);
    assert!(err.contains("say which tracks: --track, --query"), "{err}");
    let err = env.fails(2, &["playlist", "remove", d, "aab"]);
    assert!(err.contains("or which positions (--at 3,4)"), "{err}");
    let err = env.fails(1, &["playlist", "move", d, "nope"]);
    assert!(err.contains("no playlist \"nope\""), "{err}");
    let err = env.fails(2, &["playlist", "lss"]);
    assert!(err.contains("did you mean `locallibrary playlist ls`"), "{err}");
    let err = env.fails(1, &["playlist", "import", d, "list.txt", "--into", "I", "--choose", "0=1"]);
    assert!(err.contains("--choose takes ENTRY=TRACK"), "{err}");
}

#[test]
fn a_tracklist_is_imported_as_a_playlist() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    let list = env.outside("list.txt");
    // Found, two tracks called "In Charge", and one the drive doesn't have.
    std::fs::write(&list, "[a]pendics.shuffle - Deluxe Cynic\nIn Charge\nNobody - Nothing\n").unwrap();
    let import = ["playlist", "import", d, path(&list), "--into", "Imported"];
    let out = env.ok(&[&["--yes"], &import[..]].concat());
    assert!(out.contains("list.txt: 3 entr(ies) as Text: 1 found, 1 ambiguous, 1 missing"), "{out}");
    assert!(out.contains("   2. In Charge could be:\n         #2  [a]pendics.shuffle - A2 - In Charge\n"), "{out}");
    assert!(out.contains("  ?   3. Nobody - Nothing"), "{out}");
    assert!(out.contains("Settle ambiguous entries with --choose ENTRY=TRACK"), "{out}");
    assert!(out.contains("    + Imported\n"), "{out}");
    let out = env.ok(&[&["--yes"], &import[..], &["--choose", "2=#2", "--append"]].concat());
    assert!(out.contains("    ~ Imported +1, 1 -> 3 entries"), "{out}");

    // Not made without a terminal or --yes.
    let err = env.fails(3, &[&import[..], &["--choose", "2=2"]].concat());
    assert!(err.contains("not confirmed"), "{err}");

    // Nothing on the drive: nothing to make.
    std::fs::write(&list, "Nobody - Nothing\n").unwrap();
    let out = env.ok(&[&["--yes"], &import[..]].concat());
    assert!(out.contains("none of the list's tracks are on the drive"), "{out}");
    let v = env.json(&import);
    assert_eq!((&v["applied"], &v["outcome"]), (&false.into(), &"nothing_to_do".into()), "{v}");
}

#[test]
fn tracks_are_shown_matched_changed_replaced_and_deleted() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    let out = env.ok(&["track", d, "--track", "2"]);
    assert!(out.starts_with("#2  [a]pendics.shuffle - A2 - In Charge\n"), "{out}");
    assert!(out.contains("  playlists   aab"), "{out}");
    assert_eq!(env.ok(&["track", "show", d, "--track", "2"]), out);
    let out = env.ok(&["track", "matches", d, "2", "-n", "2"]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(lines[0].starts_with("   #12  [A]pendics.Shuffle - On Yo Back") && lines[0].ends_with("same key, 0.8%"));

    let fields = ["--rating", "5", "--colour", "red", "--genre", "Deep", "--title", "T", "--artist", "A"];
    let more = ["--album-name", "Al", "--label", "La", "--key", "8A", "--remixer", "R", "--comment", "C"];
    let rest = ["--mix", "Extended Mix", "--year", "2020", "--play-count", "3"];
    let out = env.ok(&[&["--yes", "track", "set", d, "--track", "1"][..], &fields, &more, &rest].concat());
    assert!(out.contains(r#"  - set title "T", artist "A", album "Al", genre "Deep", label "La", key "8A""#), "{out}");
    assert!(out.contains("    ~ 1 track(s) changed"), "{out}");
    let v = &env.json(&["tracks", d, "--filter", "genre:Deep"])["tracks"][0];
    assert_eq!((&v["title"], &v["rating"], &v["year"]), (&"T".into(), &5.into(), &2020.into()), "{v}");

    let out = env.ok(&["--yes", "track", "replace", d, "11", "9", "--keep-duplicate"]);
    assert!(out.contains("  - replace [A]pendics.Shuffle - Your Moves (Losoul's High Tide Remix) with"), "{out}");
    let out = env.ok(&["--yes", "track", "rm", d, "--track", "12", "--keep-files"]);
    assert!(out.contains("    - 1 track(s) removed\n        [A]pendics.Shuffle - On Yo Back\n"), "{out}");
    let out = env.ok(&["track", "rm", d, "--filter", "bpm:>0", "--keep-files", "--dry-run"]);
    assert!(out.contains("    - 11 track(s) removed\n") && out.ends_with("        and 1 more\n"), "{out}");
    let out = env.ok(&["--yes", "track", "dedupe", d]);
    assert!(out.contains("no copies by file, audio or Sync Manager's naming"), "{out}");
    assert!(out.contains("nothing to change"), "{out}");
}

#[test]
fn deleting_a_track_says_what_its_files_free() {
    let env = Env::ascii();
    let demo = env.demo("Demo");
    let d = path(&demo);
    let out = env.ok(&["--yes", "track", "rm", d, "--track", "2"]);
    // The audio and its three analysis files.
    assert!(out.contains("    - 1 track(s) removed, 4 file(s), "), "{out}");
    assert!(out.contains("; 4 file(s) deleted, "), "{out}");
    assert_eq!(env.json(&["tracks", d])["tracks"].as_array().unwrap().len(), 1);
}

#[test]
fn track_command_lines_that_dont_say_enough_are_refused() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    let err = env.fails(2, &["track"]);
    assert!(err.contains("give a library or drive and the tracks"), "{err}");
    let err = env.fails(2, &["track", "set", d, "--rating", "4"]);
    assert!(err.contains("say which tracks"), "{err}");
    let err = env.fails(2, &["track", "sett"]);
    assert!(err.contains("did you mean `locallibrary track set`"), "{err}");
}

#[test]
fn smart_playlists_are_kept_in_the_store_and_written_to_drives() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    let out = env.ok(&["smart"]);
    assert!(out.contains("no smart playlists (add one with"), "{out}");
    let add = ["--yes", "smart", "add", "Smart/Peak", "--filter", "bpm:>=125", "--sort", "-bpm", "--limit", "5"];
    let out = env.ok(&[&add[..], &["--drive", d]].concat());
    assert!(out.contains(r#"add a rule for "Smart/Peak": "bpm:>=125""#), "{out}");
    assert!(out.contains("Smart/Peak  bpm:>=125  sorted by -bpm, at most 5, some libraries only"), "{out}");
    assert!(out.contains("Run `locallibrary smart apply`"), "{out}");
    let out = env.ok(&["--yes", "smart", "add", "Smart/Peak", "--filter", "bpm:>=126"]);
    assert!(out.contains(r#"replace the rule for "Smart/Peak" (bpm:>=125) with "bpm:>=126""#), "{out}");
    let err = env.fails(1, &["smart", "add", "Bad", "--filter", "bpm:<<<"]);
    assert!(err.contains("not a tempo or range"), "{err}");

    let out = env.ok(&["--yes", "smart", "apply", d]);
    assert!(out.contains("  - fill Smart/Peak with 4 track(s)\n    + Smart/\n    + Smart/Peak\n"), "{out}");
    // With no drives named, every mounted drive.
    let out = env.ok(&["--yes", "smart", "apply"]);
    assert!(out.contains(&format!("{d}: nothing to change")), "{out}");

    let out = env.ok(&["--yes", "smart", "rm", "Smart/Peak"]);
    assert!(out.contains("remove the rule for \"Smart/Peak\" (drives keep their playlists)"), "{out}");
    assert!(out.contains("no smart playlists"), "{out}");
    let err = env.fails(1, &["smart", "rm", "Smart/Peak"]);
    assert!(err.contains("smart playlist \"Smart/Peak\""), "{err}");
}

#[test]
fn recorded_edits_are_listed_replayed_after_an_export_and_forgotten() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    let exported = std::fs::read(club.join(PDB)).unwrap();
    let out = env.ok(&["edits", d]);
    assert!(out.contains("no edits recorded on this drive"), "{out}");
    env.ok(&["--yes", "playlist", "new", d, "Friday", "--track", "1"]);
    let out = env.ok(&["edits", "list", d]);
    assert!(out.contains("  - create playlist Friday\n  - add 1 track(s) to Friday\n"), "{out}");
    assert_eq!(env.ok(&["edits"]), out);

    // rekordbox exports again, without the playlist; replaying the edits puts it back.
    std::fs::write(club.join(PDB), &exported).unwrap();
    assert!(!tree(&env, d).contains("Friday"));
    let out = env.ok(&["--yes", "edits", "replay", d]);
    assert!(out.contains("  - create playlist Friday"), "{out}");
    assert!(tree(&env, d).contains("Friday (1)"), "{}", tree(&env, d));

    let out = env.ok(&["edits", "clear", d, "--dry-run"]);
    assert!(out.contains("forget 1 recorded edit(s) on Club"), "{out}");
    let out = env.ok(&["--yes", "edits", "clear", d]);
    assert!(out.contains("forgot 1 recorded edit(s)"), "{out}");
    let out = env.ok(&["--yes", "edits", "clear", d]);
    assert!(out.contains("no edits recorded on this drive"), "{out}");
    let err = env.fails(2, &["edits", "lst"]);
    assert!(err.contains("did you mean `locallibrary edits list`"), "{err}");
}

#[test]
fn a_pack_is_shown_and_not_made_unconfirmed() {
    let env = Env::ascii();
    let club = env.club("Club");
    let stick = env.blank("Stick");
    let r = env.run(&["pack", path(&club), path(&stick), "--playlist", "aab"]);
    assert_eq!(r.code, 3, "{}{}", r.out, r.err);
    assert!(r.out.starts_with("Club -> Stick: aab (12 track(s), "), "{}", r.out);
    assert!(r.out.contains("! 12 track(s) have no audio file on Club; players won't load them"), "{}", r.out);
    assert!(r.err.contains("not confirmed") && !stick.join(PDB).exists(), "{}", r.err);
}
