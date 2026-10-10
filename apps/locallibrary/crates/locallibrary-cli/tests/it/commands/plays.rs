//! `locallibrary plays`: a drive's sessions printed and written out, cleared from the drive, and
//! its play counts.

use crate::support::{Env, path, play};

#[test]
fn a_session_is_printed_and_written_in_each_format() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    // The third entry is a track no longer in the library.
    play(&club, "HISTORY 001", &[1, 2, 99]);

    let out = env.ok(&["plays", "lists", d, "HISTORY 001"]);
    assert!(out.starts_with("HISTORY 001\n"), "{out}");
    assert!(out.contains("[a]pendics.shuffle - A1 - It's In The Eyes"), "{out}");
    assert!(out.contains("! 1 entr(ies) refer to tracks no longer in the library"), "{out}");
    let out = env.ok(&["plays", "lists", d, "HISTORY 001", "--format", "m3u8"]);
    assert!(out.starts_with("#EXTM3U\n"), "{out}");
    assert!(out.contains("/Contents/[a]pendics.shuffle/Framed And Defamed/02 A2 - In Charge.mp3"), "{out}");
    let err = env.fails(1, &["plays", "lists", d, "HISTORY 009"]);
    assert!(err.contains("no session \"HISTORY 009\" on Club (--all also looks in the store)"), "{err}");

    let csv = env.outside("friday.csv");
    let out = env.ok(&["plays", "export", d, "HISTORY 001", "-o", path(&csv)]);
    assert!(out.contains(&format!("wrote {} (HISTORY 001, 2 tracks)", csv.display())), "{out}");
    assert!(std::fs::read_to_string(&csv).unwrap().contains("It's In The Eyes"));
    let err = env.fails(1, &["plays", "lists", d, "-o", path(&csv)]);
    assert!(err.contains("name the history list to write"), "{err}");
    let err = env.fails(1, &["plays", "lists", "-o", path(&csv)]);
    assert!(err.contains("name a drive"), "{err}");
}

#[test]
fn sessions_are_listed_and_cleared_from_the_drive_once_the_store_has_them() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    let out = env.ok(&["plays", d]);
    assert!(out.contains("On Club\n  no sessions\n"), "{out}");
    play(&club, "HISTORY 001", &[1, 2, 3]);
    let out = env.ok(&["plays", "lists", d, "--all"]);
    assert!(out.contains("there's no store yet, so nothing is archived"), "{out}");

    // Not kept by the store yet: deleting it needs --force.
    let err = env.fails(1, &["--yes", "plays", "lists", d, "--delete"]);
    assert!(err.contains("--force"), "{err}");
    env.ok(&["pull", d, "--library", "Club", "--new-library"]);
    let out = env.ok(&["plays", "lists", d, "--delete", "--dry-run"]);
    assert!(out.contains("    - 1 history list(s) removed"), "{out}");
    env.ok(&["--yes", "plays", "lists", d, "--delete"]);
    let out = env.ok(&["plays", "lists", d, "--all"]);
    assert!(out.contains("On Club\n  no sessions\nOnly in the store (1)\n"), "{out}");
    assert!(out.contains("HISTORY 001") && out.contains("in the store"), "{out}");
}

#[test]
fn play_counts_and_what_was_played_since_a_snapshot() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    env.ok(&["pull", d, "--library", "Club", "--new-library"]);
    play(&club, "HISTORY 001", &[2]);
    env.ok(&["--yes", "track", "set", d, "--track", "2", "--play-count", "3"]);

    let out = env.ok(&["plays", "stats", d]);
    assert!(out.contains("  12 tracks, 1 played, 11 never played, 1 history list(s)"), "{out}");
    assert!(out.contains("Never played (11)") && out.contains("  ... and 1 more (--all lists them all)"), "{out}");
    assert!(out.contains("Most played\n     3x  in 1 list(s) [a]pendics.shuffle - A2 - In Charge"), "{out}");
    let out = env.ok(&["plays", "stats", d, "--all"]);
    assert!(!out.contains("... and"), "{out}");

    // The pull before the session is the newest snapshot from before 2099.
    let out = env.ok(&["plays", "stats", d, "--since", "2099"]);
    assert!(out.contains("Since snapshot "), "{out}");
    assert!(out.contains("  1 new history list(s): HISTORY 001"), "{out}");
    assert!(out.contains("    +3  in 1 list(s) [a]pendics.shuffle - A2 - In Charge"), "{out}");
    env.ok(&["pull", d]);
    let out = env.ok(&["plays", "stats", d, "--since", "2099"]);
    assert!(out.contains("  nothing played"), "{out}");
}
