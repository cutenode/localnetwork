//! `locallibrary sets`, and the sessions (`plays`) sets are made from, end to end: history lists
//! a drive came back with, saved by `pull` and `watch`, suggested, declared, edited, written out,
//! made a playlist of and deleted; and a log imported from Beat Link Trigger.

use std::path::Path;

use locallibrary::model::Track;
use rekordbox_pdb::PdbEditor;
use serde_json::{Value, json};

use crate::support::{Env, PDB, Rpc, path, play};

fn track(root: &Path, id: u32) -> Track {
    let lib = locallibrary::pdb::read_library(&root.join(PDB)).unwrap();
    lib.tracks.into_iter().find(|t| t.id == id).unwrap()
}

fn titles(set: &Value) -> Vec<&str> {
    set["tracks"].as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap()).collect()
}

#[test]
fn sessions_a_drive_came_back_with_become_a_set() {
    let env = Env::new();
    let main = env.club("Main");
    play(&main, "HISTORY 001", &[1, 2, 3, 4, 5, 6]);
    // The first pull keeps the session it found.
    let out = env.ok(&["pull", path(&main), "--library", "Club", "--new-library"]);
    assert!(out.contains("1 new session: `locallibrary sets suggest` says if it looks like a set"), "{out}");
    // Back from a set: `watch` saves the drive as it's plugged in.
    play(&main, "HISTORY 002", &[9, 10, 1, 2, 12, 7]);
    let out = env.ok(&["watch", "--once", "--pull"]);
    assert!(out.contains("pulled into Club") && out.contains("1 new session"), "{out}");
    let out = env.ok(&["pull", path(&main)]);
    assert!(!out.contains("new session"), "nothing new: {out}");

    // Each session with its id, and when it was recorded: the newest between the two saves.
    let v = env.json(&["plays", path(&main)]);
    let sessions = v["sessions"].as_array().unwrap();
    let names: Vec<&str> = sessions.iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["HISTORY 002", "HISTORY 001"], "newest first");
    let (newest, oldest) = (sessions[0]["id"].as_str().unwrap(), sessions[1]["id"].as_str().unwrap());
    let today = chrono::Local::now().format("%-d %b %Y").to_string();
    let out = env.ok(&["plays", path(&main)]);
    let line = out.lines().find(|l| l.contains("HISTORY 002")).unwrap_or_else(|| panic!("{out}"));
    assert!(line.trim_start().starts_with(&newest[..8]), "{line}");
    assert!(
        line.contains(&format!("on {today}")) && line.contains("6 tracks") && line.contains("in the store"),
        "{line}"
    );
    let line = out.lines().find(|l| l.contains("HISTORY 001")).unwrap();
    assert!(line.contains(&format!("before {today}")), "saved on the first pull: {line}");
    // A session is named by its id wherever a list is.
    let v = env.json(&["plays", "lists", path(&main), &newest[..6]]);
    assert_eq!(v["name"], "HISTORY 002");

    // Each is suggested, with the command that makes it a set.
    let v = env.json(&["sets", "suggest"]);
    let candidates = v["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2, "{v}");
    assert_eq!(candidates[0]["sessions"], json!([newest]));
    let out = env.ok(&["sets", "suggest"]);
    assert!(out.contains(&format!("locallibrary sets new --from {}", &newest[..8])), "{out}");
    assert!(out.contains(&format!("{}  HISTORY 002", &newest[..8])) && out.contains("6 tracks: at least"), "{out}");

    // Declared from the session, at a venue on a day.
    let v = env.json(&[
        "sets",
        "new",
        "--from",
        &newest[..8],
        "--title",
        "Friday",
        "--venue",
        "Warehouse",
        "--date",
        "2026-10-03",
    ]);
    assert_eq!(
        (&v["plan"], &v["applied"], &v["outcome"], &v["schema"]),
        (&json!(null), &json!(true), &json!("applied"), &json!(1))
    );
    let set = &v["report"]["set"];
    assert_eq!(
        (&set["date"], &set["venue"], &set["sessions"]),
        (&json!("2026-10-03"), &json!("Warehouse"), &json!([newest]))
    );
    let named = |id| track(&main, id).title;
    let before: Vec<String> = [9, 10, 1, 2, 12, 7].into_iter().map(named).collect();
    assert_eq!(titles(set), before);
    // Made a set, it's no longer suggested; the drive's listing says which set it's in.
    assert_eq!(env.json(&["sets", "suggest"])["candidates"].as_array().unwrap().len(), 1);
    let out = env.ok(&["plays", path(&main)]);
    assert!(out.lines().any(|l| l.contains("HISTORY 002") && l.contains("in 2026-10-03 Friday")), "{out}");

    let out = env.ok(&["sets", "show", "--venue", "WAREHOUSE"]);
    assert!(out.starts_with("Friday\n"), "the latest set there: {out}");
    assert!(out.contains("  Where   Warehouse\n"), "{out}");
    assert!(out.contains(&format!("  From    session {}\n", &newest[..8])), "{out}");
    assert!(out.contains(&format!("    1  {}", track(&main, 9).display_name())), "{out}");
    let out = env.ok(&["sets"]);
    assert!(out.contains("2026-10-03  Friday · Warehouse") && out.contains("6 tracks"), "{out}");

    // Positions name the tracklist as it was: the fifth lands before the second, the first
    // goes, the dubplate is typed in at the end, and the third is published unnamed.
    let v = env.json(&[
        "sets",
        "edit",
        "Friday",
        "--drop",
        "1",
        "--move",
        "5:2",
        "--insert",
        "7:Someone - Dubplate",
        "--unnamed",
        "3",
        "--city",
        "Leeds",
    ]);
    let set = &v["report"]["set"];
    assert_eq!(titles(set), [&*before[4], &before[1], &before[2], &before[3], &before[5], "Dubplate"]);
    assert_eq!(set["tracks"][2]["unnamed"], true);
    assert_eq!((&set["tracks"][5]["origin"], &set["city"]), (&json!("added"), &json!("Leeds")));
    assert!(v["report"]["notes"][0].as_str().unwrap().contains("went in as typed"), "{v}");
    let out = env.ok(&["sets", "show", "last"]);
    assert!(out.contains("Someone - Dubplate  added") && out.contains("unnamed"), "{out}");
    let err = env.fails(1, &["sets", "edit", "Friday", "--drop", "1-9"]);
    assert!(err.contains("there's no track 7: the set has 6"), "{err}");
    let err = env.fails(2, &["sets", "edit", "Friday", "--move", "5"]);
    assert!(err.contains("give FROM:TO"), "{err}");

    // Written out: for Mixcloud (unnamed is ID - ID), as CSV; a CUE sheet needs the recording.
    let out = env.ok(&["sets", "export", "Friday", "--format", "mixcloud"]);
    assert_eq!(out.lines().nth(2), Some("ID - ID"), "{out}");
    assert_eq!(out.lines().last(), Some("Someone - Dubplate"), "{out}");
    let csv = env.outside("friday.csv");
    let v = env.json(&["sets", "export", "Friday", "-o", path(&csv)]);
    assert_eq!((&v["report"]["format"], &v["report"]["tracks"]), (&json!("csv"), &json!(6)));
    assert!(std::fs::read_to_string(&csv).unwrap().starts_with("#,Start,Artist,Title"));
    let err = env.fails(1, &["sets", "export", "Friday", "-o", path(&env.outside("friday.cue"))]);
    assert!(err.contains("attach it to the set first"), "{err}");

    // When and where a track was played.
    let fifth = track(&main, 12);
    let v = env.json(&["sets", "played", &fifth.display_name()]);
    assert_eq!((&v[0]["position"], &v[0]["date"]), (&json!(1), &json!("2026-10-03")));
    let out = env.ok(&["sets", "played", &fifth.title]);
    assert!(out.contains(&format!("{}: played once, last on 2026-10-03 at Warehouse", fifth.display_name())), "{out}");

    // A playlist of it on the drive: the typed-in dubplate isn't there.
    let out = env.ok(&["sets", "playlist", "Friday", path(&main), "--dry-run"]);
    assert!(
        out.contains("Sets/2026-10-03 Friday") && out.contains("6. Someone - Dubplate isn't on the drive"),
        "{out}"
    );
    let v = env.json(&["--yes", "sets", "playlist", "Friday", path(&main)]);
    assert_eq!(v["outcome"], "applied", "{v}");
    let v = env.json(&["tracks", path(&main), "-p", "Sets/2026-10-03 Friday"]);
    assert_eq!(v["tracks"].as_array().unwrap().len(), 5, "{v}");

    // Deleting asks first.
    let err = env.fails(3, &["sets", "delete", "Friday"]);
    assert!(err.contains("not confirmed"), "{err}");
    let out = env.exits(3, &["--json", "sets", "delete", "Friday"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["outcome"], "not_confirmed", "{v}");
    assert_eq!(env.json(&["sets"]).as_array().unwrap().len(), 1);
    let v = env.json(&["--yes", "sets", "delete", "Friday"]);
    assert_eq!(v["report"]["title"], "Friday");
    assert!(env.json(&["sets"]).as_array().unwrap().is_empty());

    // A session that wasn't a set can be dismissed, and suggested again.
    let v = env.json(&["sets", "suggest", "--dismiss", &oldest[..8]]);
    assert_eq!(v["report"], json!([oldest]));
    assert_eq!(env.json(&["sets", "suggest"])["candidates"].as_array().unwrap().len(), 1);
    let out = env.ok(&["plays", "lists", path(&main), "--all"]);
    assert!(out.lines().any(|l| l.contains("HISTORY 001") && l.contains("dismissed")), "{out}");
    env.json(&["sets", "suggest", "--undismiss", &oldest[..8]]);
    assert_eq!(env.json(&["sets", "suggest"])["candidates"].as_array().unwrap().len(), 2);
}

/// Under a drive's heading, its sessions don't name the drive again; those only the store has
/// do. A count of one is singular.
#[test]
fn listings_name_the_drive_once_and_count_one_as_one() {
    let env = Env::new();
    let main = env.club("Main");
    play(&main, "HISTORY 001", &[1, 2, 3]);
    env.ok(&["pull", path(&main), "--library", "Club", "--new-library"]);
    // rekordbox imports the drive's history, taking it off; then a player logs a track check.
    let lib = locallibrary::pdb::read_library(&main.join(PDB)).unwrap();
    let mut ed = PdbEditor::from_file(main.join(PDB)).unwrap();
    ed.remove_history_playlists(&lib.history.iter().map(|h| h.id).collect::<Vec<_>>()).unwrap();
    ed.save(main.join(PDB)).unwrap();
    play(&main, "HISTORY 002", &[4]);

    let out = env.ok(&["plays", "lists", path(&main), "--all"]);
    let (on_drive, stored) = out.split_once("Only in the store").unwrap_or_else(|| panic!("{out}"));
    let line =
        |text: &str, name: &str| text.lines().find(|l| l.contains(name)).unwrap_or_else(|| panic!("{out}")).to_owned();
    let check = line(on_drive, "HISTORY 002");
    assert!(check.contains(" 1 track ") && !check.contains("Main"), "{check}");
    let archived = line(stored, "HISTORY 001");
    assert!(archived.contains(" Main ") && archived.contains(" 3 tracks "), "{archived}");

    let id = env.json(&["plays", path(&main)])["sessions"][0]["id"].as_str().unwrap().to_owned();
    env.ok(&["sets", "new", "--from", &id[..8], "--venue", "Warehouse", "--date", "2026-10-03"]);
    let out = env.ok(&["sets"]);
    assert!(out.trim_end().ends_with("Warehouse · 1 track"), "{out}");
    let out = env.ok(&["sets", "show", "last"]);
    assert!(out.contains(&format!("  From    session {}\n", &id[..8])) && out.contains("  1 track · "), "{out}");
    let out = env.ok(&["profile", "-s", "sets"]);
    assert!(out.contains("1 play in 1 set · ") && out.contains("  1 set · 1 track in a typical set"), "{out}");
    assert!(out.contains("played on 2026-10-03"), "{out}");
}

/// Beat Link Trigger's Playlist Writer logs when each track played: a set made from it is timed.
#[test]
fn a_beat_link_trigger_log_makes_a_timed_set() {
    let env = Env::new();
    let main = env.club("Main");
    let (a, b) = (track(&main, 3), track(&main, 4));
    let log = env.outside("Friday.csv");
    let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    std::fs::write(
        &log,
        format!(
            "Title,Artist,Album,Player,Source,Started,Stopped,Play Time\n\
             {},{},,2,Player 2 USB,Sat Oct 03 23:14:05 UTC 2026,Sat Oct 03 23:19:40 UTC 2026,05:35\n\
             {},{},,3,Player 3 USB,Sat Oct 03 23:19:00 UTC 2026,Sat Oct 03 23:25:00 UTC 2026,06:00\n",
            q(&a.title),
            q(&a.artist),
            q(&b.title),
            q(&b.artist)
        ),
    )
    .unwrap();
    let out = env.ok(&["plays", "import", path(&log), "--drive", path(&main)]);
    assert!(out.contains("kept Friday as session") && out.contains(": 2 plays,"), "{out}");
    let v = env.json(&["plays", "import", path(&log)]);
    let id = v["report"]["id"].as_str().unwrap();
    assert_eq!(v["report"]["plays"][0]["entry"]["file"], a.file_path, "matched to the drive's track");

    let v = env.json(&["sets", "new", "--from", &id[..8], "--venue", "Warehouse"]);
    let set = &v["report"]["set"];
    assert_eq!(
        set["date"],
        chrono::DateTime::parse_from_rfc3339("2026-10-03T23:14:05Z")
            .unwrap()
            .with_timezone(&chrono::Local)
            .date_naive()
            .to_string()
    );
    let starts: Vec<&Value> = set["tracks"].as_array().unwrap().iter().map(|t| &t["start_seconds"]).collect();
    assert_eq!(starts, [&json!(0), &json!(295)]);
    let out = env.ok(&["sets", "show", "last"]);
    assert!(out.contains(&format!("    2      4:55  {}", b.display_name())), "{out}");
    let out = env.ok(&["sets", "export", "last", "--format", "mixcloud"]);
    assert_eq!(out, format!("{} - 00:00:00\n{} - 00:04:55\n", a.display_name(), b.display_name()));
}

/// Over RPC, as on the command line: sessions are listed and suggested, a set declared from one,
/// listed, played, made a playlist of (planned, then applied by the plan's id) and deleted.
#[test]
fn sets_and_sessions_over_rpc() {
    let env = Env::new();
    let main = env.club("Main");
    play(&main, "HISTORY 001", &[1, 2, 3]);
    let mut rpc = Rpc::start(env.command());
    let sessions = rpc.call(1, "sessions.list", json!({ "unassigned": true }));
    let id = sessions["result"]["sessions"][0]["id"].as_str().unwrap_or_else(|| panic!("{sessions}")).to_string();
    let suggested = rpc.call(2, "sets.suggest", json!({}));
    assert_eq!(suggested["result"]["sessions"][0]["id"], id, "{suggested}");
    let made = rpc.call(3, "sets.new", json!({ "sessions": [id], "title": "Friday", "date": "2026-10-03" }));
    assert_eq!(made["result"]["set"]["tracks"].as_array().map(Vec::len), Some(3), "{made}");
    assert_eq!(rpc.call(4, "sets.list", json!({}))["result"].as_array().map(Vec::len), Some(1));
    let plan = rpc.call(5, "sets.playlist.plan", json!({ "set": "Friday", "drive": path(&main) }))["result"].clone();
    assert_eq!(plan["operation"], "sets.playlist", "{plan}");
    let done = rpc.call(6, "sets.playlist.apply", json!({ "plan_id": plan["plan_id"] }));
    assert!(done["result"]["changes"].as_u64().is_some_and(|n| n > 0), "{done}");
    let played = rpc.call(7, "sets.played", json!({ "track": track(&main, 2).title }));
    assert_eq!(played["result"][0]["position"], 2, "{played}");
    let gone = rpc.call(8, "sets.delete", json!({ "set": "Friday" }));
    assert_eq!(gone["result"]["title"], "Friday", "{gone}");
    // The night-before job's methods are `prep.*`, and still answer to `set.*`.
    let prep = rpc.call(9, "set.plan", json!({ "drives": [path(&main)], "eject": false }));
    assert_eq!(prep["result"]["operation"], "set", "{prep}");
}
