//! Property tests of the commands that edit a drive in place (`playlist`, `track`, `tags`,
//! `cues`, `edits`, `undo`, `restore`, `plays lists --delete`): whatever they're asked, the plan
//! they print for people is the one their `--json` dry run gives, and applying it does what the
//! plan said, by the drive's own listings.

use std::collections::BTreeSet;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, Run, path};

/// A database by the name a plan gives it, as people see it.
fn file_name(database: &str) -> &str {
    match database {
        "export" => "export.pdb",
        "export_ext" => "exportExt.pdb",
        "onelibrary" | "one_library" => "exportLibrary.db",
        other => other,
    }
}

/// `out` has the line `want` (its ends trimmed), or panics saying where.
pub fn has_line(out: &str, want: &str, said: &str) {
    assert!(out.lines().any(|l| l.trim() == want.trim()), "no line {want:?}:\n{said}");
}

/// The printed plan of an edit (`playlist ...`, `track ...`, `edits replay`) lists what its JSON
/// does: the drive and its databases, each change, every playlist made, deleted and changed, the
/// tracks changed and removed, the history lists removed and the warnings.
pub fn edit_plan_is_printed(p: &Value, out: &str, said: &str) {
    let dbs: Vec<&str> = p["databases"].as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
    has_line(out, &format!("{}: {}", p["drive"].as_str().unwrap(), dbs.join(" and ")), said);
    for c in p["changes"].as_array().unwrap() {
        has_line(out, &format!("- {}", c.as_str().unwrap()), said);
    }
    for (key, sign) in [("playlists_created", "+"), ("playlists_deleted", "-")] {
        for path in p[key].as_array().unwrap() {
            has_line(out, &format!("{sign} {}", path.as_str().unwrap()), said);
        }
    }
    for c in p["playlists_changed"].as_array().unwrap() {
        let entries = format!("{} -> {} entries", c["before"], c["after"]);
        let line = out
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("~ {} ", c["path"].as_str().unwrap())))
            .unwrap_or_else(|| panic!("no line for {c}:\n{said}"));
        assert!(line.ends_with(&entries), "{line:?} for {c}:\n{said}");
        let (added, removed) = (c["added"].as_u64().unwrap(), c["removed"].as_u64().unwrap());
        let what = match (c["reordered"] == true, added, removed) {
            (true, _, _) => "reordered".to_string(),
            (false, 0, 0) => "entries changed".to_string(),
            (false, a, 0) => format!("+{a}"),
            (false, 0, r) => format!("-{r}"),
            (false, a, r) => format!("+{a} -{r}"),
        };
        assert!(line.contains(&format!(" {what}, ")), "{line:?}: {c}\n{said}");
    }
    let changed = p["tracks_changed"].as_u64().unwrap();
    assert_eq!(out.contains(&format!("~ {changed} track(s) changed")), changed > 0, "{said}");
    let removed = p["tracks_removed"].as_array().unwrap();
    if !removed.is_empty() {
        assert!(out.contains(&format!("- {} track(s) removed", removed.len())), "{said}");
        for t in removed.iter().take(10) {
            has_line(out, t["name"].as_str().unwrap(), said);
        }
        if removed.len() > 10 {
            has_line(out, &format!("and {} more", removed.len() - 10), said);
        }
    }
    let lists = p["history_removed"].as_u64().unwrap();
    assert_eq!(out.contains(&format!("- {lists} history list(s) removed")), lists > 0, "{said}");
    for w in p["warnings"].as_array().unwrap() {
        has_line(out, &format!("! {}", w.as_str().unwrap()), said);
    }
}

/// The printed plan of a cue edit names each track, with a `+` line for each cue it gains and a
/// `-` line for each it loses (their names and colours as the JSON has them).
fn cue_plan_is_printed(p: &Value, out: &str, said: &str) {
    let tracks = p["tracks"].as_array().unwrap();
    has_line(out, &format!("{}: change the cues of {} track(s)", p["drive"].as_str().unwrap(), tracks.len()), said);
    let (mut plus, mut minus) = (0, 0);
    for t in tracks {
        has_line(out, t["track"]["name"].as_str().unwrap(), said);
        let (before, after) = (t["before"].as_array().unwrap(), t["after"].as_array().unwrap());
        for (list, other, sign) in [(after, before, "+"), (before, after, "-")] {
            for c in list.iter().filter(|c| !other.contains(c)) {
                match sign {
                    "+" => plus += 1,
                    _ => minus += 1,
                }
                let line = out
                    .lines()
                    .filter(|l| l.trim_start().starts_with(&format!("{sign} ")))
                    .find(|l| {
                        let named = c["name"].as_str().unwrap();
                        (named.is_empty() || l.contains(&format!("{named:?}")))
                            && c["color"].as_str().is_none_or(|colour| l.contains(colour))
                            && c["hot_cue"].as_str().is_none_or(|h| l.contains(&format!("{sign} {h}  ")))
                    })
                    .unwrap_or_else(|| panic!("no {sign} line for {c}:\n{said}"));
                assert_eq!(line.contains("loop "), !c["end_ms"].is_null(), "{line:?}: {c}\n{said}");
            }
        }
        let files = t["files"].as_array().unwrap().len();
        let databases = if t["onelibrary"] == true { " and exportLibrary.db" } else { "" };
        has_line(out, &format!("{files} analysis file(s){databases}"), said);
    }
    let lines = |sign: &str| out.lines().filter(|l| l.trim_start().starts_with(&format!("{sign} "))).count();
    assert_eq!((lines("+"), lines("-")), (plus, minus), "{said}");
}

/// The printed plan of a My Tags edit says what it does to how many tracks, names the first 15
/// of them, the tags it makes and the databases it changes.
fn tag_plan_is_printed(p: &Value, out: &str, said: &str) {
    let (drive, tag) = (p["drive"].as_str().unwrap(), p["tag"].as_str().unwrap());
    let tracks = p["tracks"].as_array().unwrap();
    let n = tracks.len();
    let head = match p["action"].as_str().unwrap() {
        "add" => format!("{drive}: add {tag} to {n} track(s)"),
        "remove" => format!("{drive}: remove {tag} from {n} track(s)"),
        "rename" => format!("{drive}: rename {tag} to {}", p["to"].as_str().unwrap()),
        _ => format!("{drive}: delete {tag} (from {n} track(s))"),
    };
    has_line(out, &head, said);
    let sign = if p["action"] == "add" { "+" } else { "-" };
    for t in tracks.iter().take(15) {
        has_line(out, &format!("{sign} {}", t["name"].as_str().unwrap()), said);
    }
    for c in p["created"].as_array().unwrap() {
        let file = file_name(c["database"].as_str().unwrap());
        has_line(out, &format!("create {} in {file}", c["path"].as_str().unwrap()), said);
    }
    let dbs: Vec<&str> = p["databases"].as_array().unwrap().iter().map(|d| file_name(d.as_str().unwrap())).collect();
    has_line(out, &format!("changes {}", dbs.join(" and ")), said);
}

/// The edit plan `p` is or holds (a restored playlist's).
fn edit_plan(p: &Value) -> Option<&Value> {
    [p, &p["plan"]].into_iter().find(|p| p.get("changes").is_some() && p.get("playlists_created").is_some())
}

/// Whichever kind of plan `p` is, its printed form `out` lists what it holds.
pub fn plan_is_printed(p: &Value, out: &str, said: &str) {
    match p["operation"].as_str() {
        Some("cues") => return cue_plan_is_printed(p, out, said),
        Some("tags") => return tag_plan_is_printed(p, out, said),
        _ => {}
    }
    // A tracklist imported (the tracklist plugin's `import`): what was found in it comes first.
    // What could be one of several tracks and what isn't on the drive are its warnings, which
    // every edit plan prints.
    if p["operation"] == "tracklist.import" {
        let found = p["summary"][0].as_str().unwrap();
        assert!(found.contains(" found, ") && out.lines().next() == Some(found), "{found:?}:\n{said}");
    }
    // Plans with a summary of their own (`edits clear`) print it; the others print what follows.
    let mut own = false;
    // A playlist brought back from a snapshot or a backup.
    if p.get("source").is_some() && p.get("untouched").is_some() {
        assert!(out.starts_with("Restoring "), "{said}");
    }
    // A backup put back: the files it puts back.
    if let Some(backup) = p["backup"]["id"].as_str() {
        own = true;
        assert!(out.lines().next().is_some_and(|l| l.contains(&format!(": backup {backup} ("))), "{said}");
        for f in p["files"].as_array().unwrap().iter().take(20) {
            assert!(out.contains(f["path"].as_str().unwrap()), "{f}: {said}");
        }
    }
    // A drive packed for a set: what's copied, removed and left.
    if let (Some(copies), Some(deletes)) = (p["copies"].as_array(), p["deletes"].as_array()) {
        own = true;
        let line = format!("  copy {} file(s), ", copies.len());
        assert!(out.contains(&line) && out.contains(&format!("; remove {} file(s), ", deletes.len())), "{said}");
    }
    match edit_plan(p) {
        Some(e) => edit_plan_is_printed(e, out, said),
        None if !own => {
            for l in p["summary"].as_array().into_iter().flatten() {
                assert!(out.contains(l.as_str().unwrap()), "{l}: {said}");
            }
        }
        None => {}
    }
}

/// Every playlist and folder on `drive`, by path.
fn playlist_paths(fx: &Env, drive: &str) -> BTreeSet<String> {
    let r = fx.run(&["--json", "playlist", "ls", drive]);
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(&r.out) else { return BTreeSet::new() };
    let by_id = |id: &Value| list.iter().find(|p| p["id"] == *id);
    list.iter()
        .map(|p| {
            let mut parts = vec![p["name"].as_str().unwrap().to_string()];
            let mut parent = p["parent_id"].clone();
            while let Some(up) = by_id(&parent) {
                parts.insert(0, up["name"].as_str().unwrap().to_string());
                parent = up["parent_id"].clone();
            }
            parts.join("/")
        })
        .collect()
}

/// The names of the tracks on `drive`.
fn track_names(fx: &Env, drive: &str) -> Vec<String> {
    let v = fx.json(&["tracks", drive]);
    v["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| format!("{} - {}", t["artist"].as_str().unwrap(), t["title"].as_str().unwrap()))
        .collect()
}

/// What can be filled into an edit's words on drive A (two tracks, the playlist "Set", the
/// session "HISTORY 001", analysis files) or Club (twelve tracks, the playlist "aab", no audio).
struct Drive {
    name: &'static str,
    tracks: &'static [&'static str],
    playlists: &'static [&'static str],
}

const A: Drive = Drive { name: "A", tracks: &["1", "2", "99"], playlists: &["Set", "Set", "Friday", "Nope"] };
const CLUB: Drive =
    Drive { name: "Club", tracks: &["1", "5", "12", "99"], playlists: &["aab", "aab", "Friday", "Nope"] };

/// Edits, with placeholders: `@d` the drive, `@t` a track, `@p` a playlist, `@n` a name.
const EDITS: &[&[&str]] = &[
    &["playlist", "new", "@d", "@n"],
    &["playlist", "new", "@d", "@n", "--track", "@t"],
    &["playlist", "new", "@d", "Folder/@n"],
    &["playlist", "new", "@d", "@n", "--folder"],
    &["playlist", "new", "@d", "@n", "--folder", "--track", "@t"],
    &["playlist", "rename", "@d", "@p", "@n"],
    &["playlist", "move", "@d", "@p", "--top"],
    &["playlist", "move", "@d", "@p", "--into", "Folder"],
    &["playlist", "move", "@d", "@p", "--position", "1"],
    &["playlist", "move", "@d", "@p"],
    &["playlist", "move", "@d", "@p", "--at", "2", "--to", "1"],
    &["playlist", "delete", "@d", "@p"],
    &["playlist", "delete", "@d", "@p", "--with-tracks"],
    &["playlist", "add", "@d", "@p", "--track", "@t"],
    &["playlist", "add", "@d", "@p", "--track", "@t", "--position", "1", "--allow-repeats"],
    &["playlist", "add", "@d", "@p", "--filter", "bpm:>=0", "--sort", "-bpm"],
    &["playlist", "add", "@d", "@p"],
    &["playlist", "remove", "@d", "@p", "--track", "@t"],
    &["playlist", "remove", "@d", "@p", "--at", "1"],
    &["playlist", "remove", "@d", "@p"],
    &["playlist", "reorder", "@d", "@p", "--at", "2", "--to", "1"],
    &["playlist", "sort", "@d", "@p", "--sort", "-bpm"],
    &["playlist", "dedupe", "@d", "@p"],
    &["playlist", "order", "@d", "@p"],
    &["playlist", "order", "@d", "@p", "--start", "@t", "--into", "@n"],
    &["playlist", "fill", "@d", "@n", "--filter", "bpm:>=0", "--sort", "bpm", "--limit", "2"],
    &["playlist", "unfiled", "@d"],
    &["track", "set", "@d", "--track", "@t", "--rating", "3", "--color", "red"],
    &["track", "set", "@d", "--filter", "bpm:>=0", "--comment", "warm-up"],
    &["track", "set", "@d", "--rating", "3"],
    &["track", "rm", "@d", "--track", "@t"],
    &["track", "rm", "@d", "--track", "@t", "--keep-files"],
    &["track", "rm", "@d", "--filter", "bpm:>=0"],
    &["track", "replace", "@d", "@t", "@t"],
    &["track", "replace", "@d", "@t", "@t", "--keep-duplicate"],
    &["track", "dedupe", "@d"],
    &["edits", "replay", "@d"],
    &["edits", "clear", "@d"],
    &["undo", "@d"],
    &["undo", "@d", "--force"],
    &["restore", "@d", "--playlist", "@p"],
    &["restore", "@d", "--playlist", "@p", "--backup", "@backup"],
    &["plays", "lists", "@d", "HISTORY 001", "--delete"],
    &["plays", "lists", "@d", "--delete", "--force"],
    &["cues", "set", "@d", "--track", "@t", "--hot", "B", "--at", "1.5", "--name", "Drop", "--colour", "red"],
    &["cues", "set", "@d", "--track", "@t", "--hot", "C", "--beat", "2", "--loop", "4"],
    &["cues", "set", "@d", "--track", "@t", "--memory", "--at", "0:02.5", "--snap"],
    &["cues", "rename", "@d", "--track", "@t", "--all", "Intro"],
    &["cues", "colour", "@d", "--track", "@t", "--hot", "B", "blue"],
    &["cues", "rm", "@d", "--track", "@t", "--all"],
    &["cues", "rm", "@d", "--track", "@t", "--memory-beat", "2"],
    &["cues", "copy", "@d", "--from", "@t", "--to", "@t"],
    &["tags", "add", "@d", "Genre/Deep", "--track", "@t"],
    &["tags", "add", "@d", "Situation/Warm-up", "--filter", "bpm:>=0"],
    &["tags", "untag", "@d", "Genre/Deep", "--track", "@t"],
    &["tags", "rename", "@d", "Genre/Deep", "Dark"],
    &["tags", "delete", "@d", "Genre/Deep"],
    &["playlist", "import", "@d", "@list", "--into", "@n"],
    &["playlist", "import", "@d", "@list", "--into", "@p", "--append", "--choose", "2=2"],
    &["playlist", "import", "@d", "@list", "--into", "@n", "--choose", "x"],
    &["pack", "@d", "Blank", "--playlist", "@p"],
];

#[hegel::test_helper]
fn fill(tc: &TestCase, fx: &Env, d: &Drive, words: &[&str]) -> Vec<String> {
    let pick = |options: &[&str]| options[tc.draw(gs::integers::<usize>().max_value(options.len() - 1))].to_string();
    let names = ["Friday", "Warm-ups", "Ünïcödé", "Set"];
    words
        .iter()
        .map(|w| match *w {
            // By its name, or its mount path.
            "@d" => match tc.draw(gs::weighted_booleans(0.2)) {
                true => path(&fx.volumes.join(d.name)).to_string(),
                false => d.name.to_string(),
            },
            "@t" => pick(d.tracks),
            "@p" => pick(d.playlists),
            "@n" => pick(&names),
            "Folder/@n" => format!("Folder/{}", pick(&names)),
            // The drive's newest backup, if it has one.
            "@backup" => {
                let backups = fx.json(&["drive", "backups", d.name]);
                let newest = backups["backups"]
                    .as_array()
                    .and_then(|b| b.first())
                    .or_else(|| backups.as_array().and_then(|b| b.first()));
                newest.and_then(|b| b["id"].as_str()).unwrap_or("none").to_string()
            }
            "@list" => path(&fx.home(&pick(&["list.txt", "list.m3u8", "nothing.txt"]))).to_string(),
            w => w.to_string(),
        })
        .collect()
}

fn run(fx: &Env, args: &[String]) -> Run {
    fx.run(&args.iter().map(String::as_str).collect::<Vec<_>>())
}

/// The JSON a run printed, if it did.
fn json(r: &Run) -> Option<Value> {
    serde_json::from_str(&r.out).ok()
}

/// Tracklists to import: entries a drive has, one that could be either of two tracks, one it
/// doesn't, and nothing at all.
fn tracklists(fx: &Env) {
    let entries = "[a]pendics.shuffle - Deluxe Cynic\nIn Charge\nSomeone - Dubplate\nLoopmasters - Demo Track 2\n";
    std::fs::write(fx.home("list.txt"), entries).unwrap();
    let m3u = "#EXTM3U\n#EXTINF:300,Loopmasters - Demo Track 1\n/Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3\n";
    std::fs::write(fx.home("list.m3u8"), m3u).unwrap();
    std::fs::write(fx.home("nothing.txt"), "Nobody - Nothing\n").unwrap();
}

/// Every edit in the list above, in any order, on drive A and Club: the dry run printed lists
/// what the JSON dry run plans (or fails as it does), and applying the plan (printed or as JSON)
/// succeeds, says it did, and leaves the playlists, tracks, cues and tags the plan says.
// Each case runs every edit: each run tries another order, other drives and arguments.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn edits_print_their_plan_and_do_what_it_says(tc: TestCase) {
    let fx = Env::from_template();
    tracklists(&fx);
    // The blank drive made a copy of library A, or not: an edit to A then says it's copied onto
    // its other drive when it's synced.
    if tc.draw(gs::booleans()) {
        fx.ok(&["--yes", "sync", "A", "Blank"]);
    }
    for template in tc.draw(gs::permutations(EDITS)) {
        for d in tc.draw(gs::permutations(vec![0, 1])).into_iter().map(|i| [&A, &CLUB][i]) {
            let words = fill(&tc, &fx, d, template);
            let with = |flags: &[&str], dry: bool| {
                let mut a: Vec<String> = flags.iter().map(|f| f.to_string()).collect();
                a.extend(words.iter().cloned());
                if dry {
                    a.push("--dry-run".into());
                }
                a
            };
            let planned = run(&fx, &with(&["--json"], true));
            let printed = run(&fx, &with(&[], true));
            let said =
                format!("{words:?}\njson: {}\n{}\nprinted: {}\n{}", planned.out, planned.err, printed.out, printed.err);
            assert_eq!(planned.code, printed.code, "{said}");
            assert!(!printed.err.contains("panicked"), "{said}");
            let Some(v) = json(&planned) else { panic!("not JSON: {said}") };
            match v["outcome"].as_str().unwrap() {
                "dry_run" => {
                    // A plan that changes nothing says so, rather than asking to apply it.
                    assert_ne!(v["plan"]["noop"], true, "{said}");
                    plan_is_printed(&v["plan"], &printed.out, &said)
                }
                "nothing_to_do" => {
                    let p = &v["plan"];
                    match p["missing"].as_array() {
                        // A playlist restored as it was: what it leaves out is listed, each entry.
                        Some(missing) => {
                            let left_out = format!("{} entr(ies) left out", missing.len());
                            assert_eq!(printed.out.contains(&left_out), !missing.is_empty(), "{said}");
                        }
                        // Undo names the tracks without audio itself, five and a count of the rest.
                        None if words[0] == "undo" => {
                            let gone: Vec<&str> =
                                p["missing_audio"].as_array().unwrap().iter().map(|m| m.as_str().unwrap()).collect();
                            for m in gone.iter().take(5) {
                                assert!(printed.out.contains(&format!("no audio on the drive for {m}")), "{said}");
                            }
                            let more = format!("and {} more track(s) with no audio", gone.len().saturating_sub(5));
                            assert_eq!(printed.out.contains(&more), gone.len() > 5, "{said}");
                        }
                        None => {
                            for w in p["warnings"].as_array().into_iter().flatten() {
                                assert!(printed.out.contains(w.as_str().unwrap()), "{w}: {said}");
                            }
                        }
                    }
                    continue;
                }
                // Refused (no such playlist, nothing to undo...): the printed run says why.
                "error" => {
                    let message = v["error"]["message"].as_str().unwrap();
                    assert!(printed.err.contains(message.lines().next().unwrap()), "{said}");
                    continue;
                }
                other => panic!("a dry run came to {other}: {said}"),
            }
            let before = (playlist_paths(&fx, d.name), track_names(&fx, d.name));
            let as_json = tc.draw(gs::booleans());
            let applied = run(&fx, &with(if as_json { &["--yes", "--json"] } else { &["--yes"] }, false));
            let said = format!("{said}\napplied: {}\n{}", applied.out, applied.err);
            assert_eq!(applied.code, 0, "{said}");
            let p = &v["plan"];
            if as_json {
                let a = json(&applied).unwrap_or_else(|| panic!("{said}"));
                assert_eq!((&a["outcome"], &a["applied"]), (&"applied".into(), &true.into()), "{said}");
            } else {
                assert!(applied.out.lines().any(|l| l.starts_with("✓ ")), "{said}");
                // A replay of edits the drive still has writes nothing, and says so.
                let replayed_nothing = words[..2] == ["edits", "replay"]
                    && applied.out.contains("has these edits already: nothing written");
                if let Some(e) = edit_plan(p).filter(|_| !replayed_nothing) {
                    let dbs: Vec<&str> =
                        e["databases"].as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
                    if words[..2] == ["edits", "replay"] {
                        // A recorded edit that no longer applies writes nothing, so a replay writes
                        // some of the databases its plan names, and none it doesn't.
                        let line =
                            applied.out.lines().find(|l| l.contains("written to ")).unwrap_or_else(|| panic!("{said}"));
                        let written = line.split("written to ").nth(1).unwrap().split(';').next().unwrap();
                        assert!(written.split(" and ").all(|w| dbs.contains(&w)), "{written}: {said}");
                    } else {
                        assert!(applied.out.contains(&format!("written to {}", dbs.join(" and "))), "{said}");
                    }
                    let drive = words.iter().find(|w| *w == d.name || w.ends_with(&format!("/{}", d.name))).unwrap();
                    assert!(applied.out.contains(&format!("locallibrary undo {drive}")), "{said}");
                }
            }
            // What the plan said, by the drive's listings.
            let (paths, names) = (playlist_paths(&fx, d.name), track_names(&fx, d.name));
            if let Some(p) = edit_plan(p) {
                for made in p["playlists_created"].as_array().unwrap() {
                    // A folder's path ends in a slash.
                    let made = made.as_str().unwrap().trim_end_matches('/');
                    assert!(paths.contains(made), "{made} isn't there: {paths:?}\n{said}");
                }
                for gone in p["playlists_deleted"].as_array().unwrap() {
                    let gone = gone.as_str().unwrap().trim_end_matches('/');
                    let made_again = p["playlists_created"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|m| m.as_str().unwrap().trim_end_matches('/') == gone);
                    assert!(!paths.contains(gone) || made_again, "{gone} is still there: {paths:?}\n{said}");
                }
                let removed: Vec<&str> =
                    p["tracks_removed"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
                let left = before.1.len() - removed.len();
                assert_eq!(names.len(), left, "{removed:?} removed from {:?}: {names:?}\n{said}", before.1);
            }
            if p["operation"] == "cues" {
                for t in p["tracks"].as_array().unwrap() {
                    let id = t["track"]["id"].to_string();
                    let now = fx.json(&["cues", "list", d.name, "--track", &id]);
                    assert_eq!(now[0]["cues"], t["after"], "track {id}\n{said}");
                }
            }
            if p["operation"] == "tags" {
                let tags = fx.json(&["tags", "list", d.name]);
                let paths: BTreeSet<&str> = tags["categories"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|c| c["tags"].as_array().unwrap())
                    .map(|t| t["path"].as_str().unwrap())
                    .collect();
                let tag = p["tag"].as_str().unwrap();
                match p["action"].as_str().unwrap() {
                    "add" => assert!(paths.contains(tag), "{paths:?}\n{said}"),
                    "rename" => {
                        // The new name, in the category the tag was in.
                        let to = p["to"].as_str().unwrap();
                        let renamed = tag.rsplit_once('/').map_or(to.to_string(), |(c, _)| format!("{c}/{to}"));
                        assert!(
                            paths.contains(renamed.as_str()) && !paths.contains(tag),
                            "{renamed}: {paths:?}\n{said}"
                        )
                    }
                    "delete" => assert!(!paths.contains(tag), "{paths:?}\n{said}"),
                    _ => {}
                }
            }
        }
    }
}
