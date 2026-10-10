//! `locallibrary sets` in a terminal: sets declared by hand, from several sessions, from a
//! recording and from a timed log, listed, shown, written out and searched.

use crate::support::{Env, path, play};

#[test]
fn with_no_store_there_are_no_sets_and_nothing_played() {
    let env = Env::ascii();
    let out = env.ok(&["sets"]);
    assert!(out.contains("no sets yet\n`locallibrary sets suggest` finds them"), "{out}");
    let out = env.ok(&["sets", "played", "Anything"]);
    assert_eq!(out, "\"Anything\" isn't in any of your sets\n");
    let out = env.ok(&["sets", "suggest"]);
    assert!(out.contains("no sets to suggest: every session long enough is in a set or was dismissed"), "{out}");
}

#[test]
fn a_set_declared_by_hand_shows_every_detail() {
    let env = Env::ascii();
    let details = ["--kind", "club", "--venue", "Warehouse", "--city", "London", "--country", "gb"];
    let more = ["--with", "DJ Two", "--with", "did:plc:abc", "--event", "at://did:plc:abc/x.event/1"];
    let rest = ["--link", "https://www.mixcloud.com/dj/friday/", "--notes", "Warm crowd"];
    let new = [&["sets", "new", "--title", "Friday", "--date", "2026-10-03"][..], &details, &more, &rest].concat();
    let out = env.ok(&new);
    assert!(out.starts_with("v declared 2026-10-03 Friday ("), "{out}");
    for line in [
        "  Where   Warehouse, London, GB\n",
        "  With    DJ Two, did:plc:abc\n",
        "  Event   at://did:plc:abc/x.event/1\n",
        "  Listen  https://www.mixcloud.com/dj/friday/\n",
        "  Notes   Warm crowd\n",
        "  0 tracks - 0m 00s\n",
    ] {
        assert!(out.contains(line), "{line:?} in\n{out}");
    }
    let err = env.fails(2, &["sets", "edit", "Friday", "--drop", "2-1"]);
    assert!(err.contains("\"2-1\" runs backwards: give it like 1-2"), "{err}");
}

#[test]
fn sets_are_listed_written_out_searched_and_deleted() {
    let env = Env::ascii();
    let club = env.club("Club");
    env.ok(&["sets", "new", "--title", "Friday", "--date", "2026-10-03", "--venue", "Warehouse"]);
    let out =
        env.ok(&["sets", "edit", "Friday", "--insert", "1:Floorplan - Never Grow Old", "--insert", "1:Deluxe Cynic"]);
    assert!(out.starts_with("v changed 2026-10-03 Friday ("), "{out}");
    assert!(out.contains("    1  Floorplan - Never Grow Old  added\n"), "{out}");
    assert!(out.contains("- \"Floorplan - Never Grow Old\" went in as typed: no track matches it"), "{out}");
    // One at a venue, untitled; one with neither.
    env.ok(&["sets", "new", "--venue", "Fabric", "--date", "2026-09-01"]);
    env.ok(&["sets", "edit", "2026-09-01", "--insert", "1:Deluxe Cynic"]);
    env.ok(&["sets", "new", "--date", "2026-08-01"]);
    env.ok(&["sets", "edit", "2026-08-01", "--insert", "1:Deluxe Cynic"]);

    let out = env.ok(&["sets"]);
    let lines: Vec<&str> = out.lines().map(|l| l.split_once("  ").unwrap().1).collect();
    assert_eq!(
        lines,
        ["2026-10-03  Friday - Warehouse - 2 tracks", "2026-09-01  Fabric - 1 track", "2026-08-01  untitled - 1 track"]
    );
    let out = env.ok(&["sets", "played", "Deluxe"]);
    assert!(out.contains("  2026-10-03  Friday, Warehouse              track 2 - [a]pendics.shuffle - Deluxe Cynic"));
    assert!(out.contains("  2026-09-01  Fabric                         track 1"), "{out}");
    assert!(out.contains("  2026-08-01  untitled                       track 1"), "{out}");
    assert!(out.ends_with("Deluxe Cynic: played 3 times, last on 2026-10-03 at Warehouse\n"), "{out}");
    let out = env.ok(&["sets", "played", "e"]);
    assert!(out.ends_with("2 tracks matching \"e\": played 4 times, last on 2026-10-03 at Warehouse\n"), "{out}");

    // Written out in every format: printed, or to the file the extension names.
    let export = ["sets", "export", "Friday"];
    assert_eq!(env.ok(&export), "01. Floorplan - Never Grow Old\n02. [a]pendics.shuffle - Deluxe Cynic\n");
    let err = env.fails(1, &[&export[..], &["--format", "youtube"]].concat());
    assert!(err.contains("YouTube needs one for every track"), "{err}");
    assert!(env.ok(&[&export[..], &["--format", "1001"]].concat()).contains("Floorplan - Never Grow Old"));
    let err = env.fails(1, &[&export[..], &["--format", "cue"]].concat());
    assert!(err.contains("attach it to the set first"), "{err}");
    let text = env.outside("friday.txt");
    let out = env.ok(&["sets", "export", "Friday", "-o", path(&text)]);
    assert!(out.contains(&format!("wrote {} (2 tracks)", text.display())), "{out}");
    assert!(std::fs::read_to_string(&text).unwrap().starts_with("01. Floorplan"));
    let m3u8 = env.outside("friday.m3u8");
    let out = env.ok(&["sets", "export", "Friday", "-o", path(&m3u8), "--drive", path(&club)]);
    assert!(out.contains("(1 track)") && out.contains("1. Floorplan - Never Grow Old isn't on the drive"), "{out}");
    let v = env.json(&["sets", "export", "Friday", "--format", "csv"]);
    assert!(v["text"].as_str().unwrap().starts_with("#,Start,Artist,Title"), "{v}");
    assert!(env.json(&["sets", "export", "Friday", "--format", "text"])["text"].is_string());

    let out = env.ok(&["--yes", "sets", "delete", "2026-08-01"]);
    assert!(out.contains("- delete the set 2026-08-01 untitled (") && out.contains("v deleted 2026-08-01 untitled"));
}

#[test]
fn sessions_are_dismissed_and_a_set_is_made_of_two() {
    let env = Env::ascii();
    let club = env.club("Club");
    let d = path(&club);
    play(&club, "HISTORY 001", &[1, 2, 3, 4, 5, 6]);
    play(&club, "HISTORY 002", &[7, 8, 9, 10, 11, 12]);
    env.ok(&["pull", d, "--library", "Club", "--new-library"]);
    let sessions = env.json(&["plays", d])["sessions"].clone();
    let ids: Vec<&str> = sessions.as_array().unwrap().iter().map(|s| &s["id"].as_str().unwrap()[..8]).collect();

    let out = env.ok(&["sets", "suggest", "--dismiss", ids[0], ids[1]]);
    assert!(out.contains(&format!("{}, {} won't be suggested (`--undismiss` takes it back)", ids[0], ids[1])), "{out}");
    assert!(env.ok(&["sets", "suggest"]).contains("no sets to suggest"));
    let out = env.ok(&["sets", "suggest", "--undismiss", ids[0]]);
    assert!(out.contains(&format!("{} suggested again", ids[0])), "{out}");

    let out = env.ok(&["sets", "new", "--from", ids[1], "--from", ids[0], "--date", "2026-10-03"]);
    assert!(out.contains(&format!("  From    sessions {}, {}\n", ids[1], ids[0])), "{out}");
    assert!(out.contains("  12 tracks - "), "{out}");
}

#[test]
fn a_set_from_a_recording_is_timed_and_says_where_each_track_came_from() {
    let env = Env::ascii();
    let (drive, recording) = super::mix::drive_and_recording(&env);
    let new = ["sets", "new", "--recording", path(&recording), "--drive", path(&drive), "--title", "Friday"];
    let out = env.ok(&new);
    let play = out.lines().find(|l| l.contains("Demo Track 1")).unwrap_or_else(|| panic!("{out}"));
    assert!(play.starts_with("    1      0:0") && play.ends_with("  from the recording"), "{play}");
    // A track typed in has no time in the recording.
    let out = env.ok(&["sets", "edit", "Friday", "--insert", "2:Someone - Dubplate"]);
    assert!(out.contains("    2            Someone - Dubplate  added\n"), "{out}");
}
