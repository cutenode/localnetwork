//! Property tests of what the editing commands list: My Tags, cues, a track's details and the
//! tracks that mix into it, the edits recorded and the backups `undo` can put back, rule-based
//! playlists, and tracks whose audio went missing found again. Printed for people, each says
//! what its `--json` does on the same drive.

use indexmap::IndexMap;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::props_cli_edits::{edit_plan_is_printed, has_line, plan_is_printed};
use crate::support::{Env, path};

/// My Tags put on drive A's tracks and cues set on them, in any order, each plan printed as its
/// JSON has it: `tags list` printed (with and without the tracks) lists every category and tag
/// the JSON does, each with its count of tracks, and `cues list` prints each track's cues as its
/// JSON has them.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn tags_and_cues_print_what_their_json_says(tc: TestCase) {
    let fx = Env::from_template();
    // With a OneLibrary database beside the export, My Tags are in both.
    if tc.draw(gs::weighted_booleans(0.3)) {
        fx.ok(&["--yes", "convert", "A", "--to", "onelibrary"]);
    }
    // A drive without a My Tags database says so.
    let none = fx.ok(&["tags", "list", "Club"]);
    assert!(none.starts_with("Club: no My Tags") && none.contains("has no My Tags database"), "{none}");
    let tags = ["Genre/Deep", "Genre/Techno", "Situation/Warm-up", "Situation/Main Floor"];
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(8)) {
        let track = tc.draw(gs::sampled_from(&["1", "2"][..]));
        let words: Vec<String> = match tc.draw(gs::integers::<u8>().max_value(10)) {
            // Nothing to change: a tag renamed to its own name, or taken off a track without it.
            9 => ["tags", "rename", "A", "Genre/Techno", "Techno"].map(String::from).into(),
            10 => ["tags", "untag", "A", "Genre/Trap", "--track", track].map(String::from).into(),
            0 | 1 => {
                ["tags", "add", "A", tc.draw(gs::sampled_from(&tags[..])), "--track", track].map(String::from).into()
            }
            2 => ["tags", "rename", "A", "Genre/Deep", "Dark"].map(String::from).into(),
            3 => {
                let hot = tc.draw(gs::sampled_from(&["A", "B", "H"][..]));
                let at = format!(
                    "{}.{}",
                    tc.draw(gs::integers::<u32>().max_value(90)),
                    tc.draw(gs::integers::<u32>().max_value(9))
                );
                let mut w: Vec<String> =
                    ["cues", "set", "A", "--track", track, "--hot", hot, "--at", &at].map(String::from).into();
                if tc.draw(gs::booleans()) {
                    w.extend(["--loop", "4", "--name", "Drop"].map(String::from));
                }
                if tc.draw(gs::booleans()) {
                    w.extend(
                        ["--colour", tc.draw(gs::sampled_from(&["red", "#123456", "none"][..]))].map(String::from),
                    );
                }
                w
            }
            4 => ["cues", "set", "A", "--track", track, "--memory", "--beat", "3"].map(String::from).into(),
            5 => ["cues", "rename", "A", "--track", track, "--all", "Intro"].map(String::from).into(),
            6 => ["cues", "colour", "A", "--track", track, "--hot", "B", "blue"].map(String::from).into(),
            7 => ["cues", "rm", "A", "--track", track, "--memory-beat", "3"].map(String::from).into(),
            _ => ["cues", "copy", "A", "--from", track, "--to", if track == "1" { "2" } else { "1" }]
                .map(String::from)
                .into(),
        };
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        // The plan printed is its JSON's; applied, printed or as JSON, it says so.
        let dry = [&words[..], &["--dry-run"]].concat();
        let (planned, printed) = (fx.run(&[&["--json"][..], &dry].concat()), fx.run(&dry));
        let said = format!("{dry:?}\n{}\n{}\n{}", printed.out, printed.err, planned.out);
        assert_eq!(planned.code, printed.code, "{said}");
        let v: Value = serde_json::from_str(&planned.out).unwrap();
        if v["outcome"] == "dry_run" {
            plan_is_printed(&v["plan"], &printed.out, &said);
        }
        let json = tc.draw(gs::booleans());
        let mut args = vec!["--yes"];
        if json {
            args.push("--json");
        }
        args.extend(&words);
        let r = fx.run(&args);
        assert_eq!(r.code, planned.code, "{args:?}\n{}\n{}", r.out, r.err);
        if v["outcome"] == "dry_run" && !json {
            let updated = if words[0] == "cues" { "✓ updated the cues of " } else { "✓ updated My Tags in " };
            assert!(r.out.contains(updated) && r.out.contains("undo with: locallibrary undo"), "{}", r.out);
        }
    }

    let with_tracks = tc.draw(gs::booleans());
    let mut args = vec!["tags", "list", "A"];
    if with_tracks {
        args.push("--tracks");
    }
    let v = fx.json(&args);
    let out = fx.ok(&args);
    let said = format!("{args:?}\n{out}\n{v}");
    let mut lines = out.lines();
    for c in v["categories"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        lines.find(|l| l.starts_with(name)).unwrap_or_else(|| panic!("no category {name}: {said}"));
        for t in c["tags"].as_array().unwrap() {
            let line = lines
                .find(|l| l.starts_with(&format!("  {}", t["name"].as_str().unwrap())))
                .unwrap_or_else(|| panic!("no tag {t}: {said}"));
            let n = t["tracks"].as_array().unwrap().len();
            let count = if n == 0 { "-".to_string() } else { n.to_string() };
            assert_eq!(line.split_whitespace().last(), Some(count.as_str()), "{line:?}: {said}");
            if with_tracks {
                for r in t["tracks"].as_array().unwrap() {
                    let line = lines.next().unwrap_or_else(|| panic!("{said}"));
                    assert_eq!(line.trim(), format!("{}  #{}", r["name"].as_str().unwrap(), r["id"]), "{said}");
                }
            }
        }
    }
    for w in v["warnings"].as_array().unwrap() {
        assert!(out.contains(w.as_str().unwrap()), "{said}");
    }

    let cues = fx.json(&["cues", "list", "A", "--track", "1"]);
    let out = fx.ok(&["cues", "list", "A", "--track", "1"]);
    let said = format!("{out}\n{cues}");
    for t in cues.as_array().unwrap() {
        has_line(
            &out,
            &format!(
                "{}  #{}, {:.2} BPM",
                t["track"]["name"].as_str().unwrap(),
                t["track"]["id"],
                t["bpm"].as_f64().unwrap()
            ),
            &said,
        );
        let list = t["cues"].as_array().unwrap();
        assert_eq!(out.contains("  no cues"), list.is_empty(), "{said}");
        let cue_lines: Vec<&str> =
            out.lines().skip(1).filter(|l| l.starts_with("  ") && !l.contains("no cues")).collect();
        assert_eq!(cue_lines.len(), list.len(), "{said}");
        for (line, c) in cue_lines.iter().zip(list) {
            let mark = c["hot_cue"].as_str().unwrap_or("◆");
            assert!(line.trim_start().starts_with(&format!("{mark}  ")), "{line:?}: {c}\n{said}");
            assert_eq!(line.contains(" loop "), !c["end_ms"].is_null(), "{line:?}: {c}");
            if let Some(colour) = c["color"].as_str() {
                assert!(line.ends_with(colour), "{line:?}: {c}");
            }
            if !c["name"].as_str().unwrap().is_empty() {
                assert!(line.contains(&format!("{:?}", c["name"].as_str().unwrap())), "{line:?}: {c}");
            }
        }
    }
}

/// For any of the club drive's tracks (or one it doesn't have), `track show` prints each track
/// its JSON details, under its id and name, and `track matches` prints a line for each track
/// that mixes into it, with its key and how it relates.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn track_details_and_matches_print_what_their_json_says(tc: TestCase) {
    let fx = Env::from_template();
    let drive = tc.draw(gs::sampled_from(&["Club", "A"][..]));
    let selector: Vec<String> = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => vec!["--track".into(), tc.draw(gs::integers::<u32>().min_value(1).max_value(13)).to_string()],
        1 => vec!["--filter".into(), tc.draw(gs::sampled_from(&["bpm:>=125", "key:8A~", "in charge"][..])).into()],
        _ => vec![],
    };
    let mut args = vec!["track", "show", drive];
    args.extend(selector.iter().map(String::as_str));
    let (printed, json) = (fx.run(&args), fx.run(&[&["--json"][..], &args].concat()));
    let said = format!("{args:?}\n{}\n{}\n{}", printed.out, printed.err, json.out);
    assert_eq!(printed.code, json.code, "{said}");
    if selector.is_empty() {
        // Which tracks is asked for, as the command line says it must be.
        assert_eq!(printed.code, 2, "{said}");
    }
    if printed.code == 0 {
        let list: Value = serde_json::from_str(&json.out).unwrap();
        let heads: Vec<&str> = printed.out.lines().filter(|l| l.starts_with('#')).collect();
        assert_eq!(heads.len(), list.as_array().unwrap().len(), "{said}");
        for (h, d) in heads.iter().zip(list.as_array().unwrap()) {
            assert!(h.starts_with(&format!("#{}  ", d["track"]["id"])), "{h:?}: {said}");
        }
    }

    let id = tc.draw(gs::integers::<u32>().min_value(1).max_value(13)).to_string();
    let limit = tc.draw(gs::integers::<u32>().min_value(1).max_value(20)).to_string();
    let args = ["track", "matches", drive, &id, "-n", &limit];
    let (printed, json) = (fx.run(&args), fx.run(&[&["--json"][..], &args[..]].concat()));
    let said = format!("{args:?}\n{}\n{}\n{}", printed.out, printed.err, json.out);
    assert_eq!(printed.code, json.code, "{said}");
    if printed.code != 0 {
        return;
    }
    let list: Value = serde_json::from_str(&json.out).unwrap();
    let list = list.as_array().unwrap();
    assert!(list.len() <= limit.parse().unwrap(), "{said}");
    if list.is_empty() {
        assert!(printed.out.starts_with("no tracks on the drive mix into that one"), "{said}");
    }
    let lines: Vec<&str> = printed.out.lines().collect();
    assert_eq!(lines.len(), list.len().max(1), "{said}");
    for (line, m) in lines.iter().zip(list) {
        assert!(line.trim_start().starts_with(&format!("#{}  ", m["track"]["id"])), "{line:?}: {said}");
        assert!(
            line.contains(&format!(" {:<4} {}, ", m["key"].as_str().unwrap(), m["relation"].as_str().unwrap())),
            "{line:?}: {m}"
        );
    }
}

/// Edits made on a drive in any order: `edits list` prints each recorded edit with its changes,
/// and `undo --list` each drive's backups, the next one marked, as their JSON has them; `edits
/// clear` says what it forgets before it does.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn recorded_edits_and_backups_print_what_their_json_says(tc: TestCase) {
    let fx = Env::from_template();
    let drive = tc.draw(gs::sampled_from(&["Club", "A"][..]));
    let edits: [&[&str]; 4] = [
        &["playlist", "new", drive, "Friday"],
        &["track", "set", drive, "--track", "1", "--rating", "4"],
        &["playlist", "rename", drive, "Friday", "Saturday"],
        &["tags", "add", drive, "Genre/Deep", "--track", "1"],
    ];
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let mut args = vec!["--yes", "--json"];
        args.extend(tc.draw(gs::sampled_from(&edits[..])).iter());
        let r = fx.run(&args);
        assert!([0, 1].contains(&r.code), "{args:?}: {}", r.out);
    }
    // A group given only a drive lists what's on it: the same as its listing command.
    for (short, long) in [
        (&["edits", drive][..], &["edits", "list", drive][..]),
        (&["playlist", drive], &["playlist", "ls", drive]),
        (&["tags", drive], &["tags", "list", drive]),
        (&["track", drive, "--track", "1"], &["track", "show", drive, "--track", "1"]),
        (&["cues", drive, "--track", "1"], &["cues", "list", drive, "--track", "1"]),
    ] {
        let (a, b) = (fx.run(short), fx.run(long));
        assert_eq!((a.code, a.out), (b.code, b.out), "{short:?}");
    }
    // `track` needs a drive and the tracks; the others list every drive without one.
    assert_eq!(fx.run(&["track"]).code, 2);
    let every = fx.ok(&["edits"]);
    assert!(every.contains("A\n") && every.contains("Club\n"), "{every}");
    let v = fx.json(&["edits", "list", drive]);
    let out = fx.ok(&["edits", "list", drive]);
    let said = format!("{out}\n{v}");
    let list = v.as_array().unwrap();
    assert_eq!(out.starts_with("no edits recorded on this drive"), list.is_empty(), "{said}");
    let mut lines = out.lines();
    for e in list {
        // Each under its time (in this computer's time zone).
        lines.find(|l| !l.starts_with(' ')).unwrap_or_else(|| panic!("{said}"));
        for c in e["changes"].as_array().unwrap() {
            assert_eq!(lines.next(), Some(format!("  - {}", c.as_str().unwrap()).as_str()), "{said}");
        }
    }

    let v = fx.json(&["undo", "--list"]);
    let out = fx.ok(&["undo", "--list"]);
    let said = format!("{out}\n{v}");
    let drives = v["drives"].as_array().unwrap();
    assert_eq!(
        out.starts_with("Nothing to undo on the mounted drives."),
        drives.is_empty() && v["embeds"].as_array().unwrap().is_empty(),
        "{said}"
    );
    for d in drives {
        assert!(out.contains(&format!("{} (", d["drive"].as_str().unwrap())), "{said}");
        let backups = d["backups"].as_array().unwrap();
        if backups.is_empty() {
            assert!(out.contains("  no backups"), "{said}");
        }
        for b in backups {
            let id = b["id"].as_str().unwrap();
            let line =
                out.lines().find(|l| l.starts_with(&format!("  {id}  "))).unwrap_or_else(|| panic!("{id}: {said}"));
            assert_eq!(line.ends_with("<- undo"), b["next"] == true, "{line:?}");
            let journal = b["journal"].as_u64().unwrap();
            assert_eq!(line.contains(&format!(", {journal} recorded edit(s)")), journal > 0, "{line:?}");
        }
    }

    let clear = ["edits", "clear", drive, "--dry-run"];
    let (planned, printed) = (fx.run(&[&["--json"][..], &clear[..]].concat()), fx.run(&clear));
    let p: Value = serde_json::from_str(&planned.out).unwrap();
    let said = format!("{}\n{}", printed.out, planned.out);
    match p["outcome"].as_str().unwrap() {
        "nothing_to_do" => assert!(printed.out.contains("no edits recorded on this drive"), "{said}"),
        "dry_run" => {
            for l in p["plan"]["summary"].as_array().unwrap_or_else(|| panic!("{said}")) {
                has_line(&printed.out, &format!("• {}", l.as_str().unwrap()), &said);
            }
            let r = fx.json(&["--yes", "edits", "clear", drive]);
            assert_eq!(r["outcome"], "applied", "{r}");
            assert_eq!(fx.json(&["edits", "list", drive]), serde_json::json!([]));
        }
        _ => assert_eq!(planned.code, printed.code, "{said}"),
    }
}

/// Rule-based playlists added (and replaced) and removed in any order agree with a model of the
/// rules, printed or as JSON; applied to a drive, each plan printed is its JSON's, and the drive
/// then has each rule's playlist, holding as many tracks as the rule's filter finds (at most
/// its limit).
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn smart_playlists_agree_with_a_model(tc: TestCase) {
    let fx = Env::from_template();
    // Each rule's filter, limit and whether it's for library A only.
    let mut model: IndexMap<String, (String, Option<u32>, bool)> = IndexMap::new();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        let path = tc.draw(gs::sampled_from(&["Smart/Deep", "Peak", "Smart/Ünïcödé"][..])).to_string();
        let json = tc.draw(gs::booleans());
        let mut args: Vec<String> = vec!["--yes".into()];
        if json {
            args.push("--json".into());
        }
        let ok = match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 | 1 => {
                let filter = tc.draw(gs::sampled_from(&["bpm:>=125", "bpm:>=0", "key:8A~", "bpm:"][..])).to_string();
                args.extend(["smart", "add", &path, "--filter", &filter].map(String::from));
                let limit = tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(5)));
                if let Some(n) = limit {
                    args.extend(["--limit".into(), n.to_string(), "--sort".into(), "-bpm".into()]);
                }
                // For one library only, now and then.
                let only_a = tc.draw(gs::weighted_booleans(0.2));
                if only_a {
                    args.extend(["--drive".into(), "A".into()]);
                }
                // A filter that doesn't read is refused.
                let ok = filter != "bpm:";
                if ok {
                    model.insert(path, (filter, limit, only_a));
                }
                ok
            }
            _ => {
                args.extend(["smart", "rm", &path].map(String::from));
                model.shift_remove(&path).is_some()
            }
        };
        let r = fx.run(&args.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(r.code == 0, ok, "{args:?}\n{}\n{}", r.out, r.err);
        let v = fx.json(&["smart"]);
        let rules: IndexMap<String, (String, Option<u32>, bool)> = v["playlists"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let only = p["drives"].as_array().is_some_and(|d| !d.is_empty());
                (
                    p["path"].as_str().unwrap().into(),
                    (p["filter"].as_str().unwrap().into(), p["limit"].as_u64().map(|n| n as u32), only),
                )
            })
            .collect();
        assert_eq!(rules, model, "{args:?}");
        let out = fx.ok(&["smart"]);
        assert_eq!(out.starts_with("no smart playlists"), model.is_empty(), "{out}");
        for (path, (filter, limit, only)) in &model {
            let line =
                out.lines().find(|l| l.starts_with(&format!("{path}  {filter}"))).unwrap_or_else(|| panic!("{out}"));
            assert_eq!(line.contains(&format!("at most {}", limit.unwrap_or(0))), limit.is_some(), "{line:?}");
            assert_eq!(line.contains("some libraries only"), *only, "{line:?}");
        }
    }

    let drive = tc.draw(gs::sampled_from(&["Club", "A"][..]));
    // Without a drive, the plans are every library's the store knows (A's here); with no rules
    // there's nothing to apply.
    let every = fx.run(&["smart", "apply", "--dry-run"]);
    match model.is_empty() {
        true => assert!(every.code == 1 && every.err.contains("no smart playlists apply"), "{}", every.err),
        false => assert!(every.code == 0 && every.out.starts_with('A'), "{}\n{}", every.out, every.err),
    }
    let apply = ["smart", "apply", drive, "--dry-run"];
    let (planned, printed) = (fx.run(&[&["--json"][..], &apply[..]].concat()), fx.run(&apply));
    let said = format!("{}\n{}\n{}", printed.out, printed.err, planned.out);
    assert_eq!(planned.code, printed.code, "{said}");
    if planned.code != 0 {
        return;
    }
    let p: Value = serde_json::from_str(&planned.out).unwrap();
    for plan in p["plan"].as_array().unwrap() {
        match plan["noop"] == true {
            true => assert!(
                printed.out.contains(&format!("{}: nothing to change", plan["drive"].as_str().unwrap())),
                "{said}"
            ),
            false => edit_plan_is_printed(plan, &printed.out, &said),
        }
    }
    fx.ok(&["--yes", "smart", "apply", drive]);
    // Applied, there's nothing left to change.
    let again = fx.ok(&["smart", "apply", drive, "--dry-run"]);
    assert!(again.contains(&format!("{drive}: nothing to change")), "{again}");
    let lists = fx.json(&["playlist", "ls", drive]);
    // A rule for library A only leaves the club drive alone.
    for (path, (filter, limit, _)) in model.iter().filter(|(_, r)| !r.2 || drive == "A") {
        let name = path.rsplit('/').next().unwrap();
        let list = lists
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == name)
            .unwrap_or_else(|| panic!("no {path}: {lists}"));
        let found = fx.json(&["tracks", drive, "-f", filter])["total"].as_u64().unwrap();
        let want = limit.map_or(found, |n| found.min(u64::from(n)));
        assert_eq!(list["track_ids"].as_array().unwrap().len() as u64, want, "{path} {filter}: {list}");
    }
}

/// The club drive has no audio: files named as some of its tracks', put in folders on this
/// computer (looked in) and on the drive itself, are found by `track relocate`. A track with one file of its
/// name is pointed at it (on the drive) or has it copied (from this computer), one with several
/// is left alone, one with none is counted; the plan printed is its JSON's, and applied, each
/// relocated track's audio is on the drive.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn missing_audio_is_found_where_it_was_put(tc: TestCase) {
    let fx = Env::from_template();
    let club = fx.volumes.join("Club");
    let tracks = fx.json(&["tracks", "Club"]);
    let tracks = tracks["tracks"].as_array().unwrap().clone();
    // The last is on the drive, which is always looked through.
    let folders = [fx.home("music/a"), fx.home("music/b"), club.join("Contents/Moved")];
    let mut model: IndexMap<u64, Vec<usize>> = IndexMap::new();
    for t in &tracks {
        let places: Vec<usize> = tc.draw(gs::subsequences(vec![0, 1, 2]));
        let name = t["file_path"].as_str().unwrap().rsplit('/').next().unwrap();
        for p in &places {
            std::fs::create_dir_all(&folders[*p]).unwrap();
            std::fs::write(folders[*p].join(name), format!("audio {p}")).unwrap();
        }
        model.insert(t["id"].as_u64().unwrap(), places);
    }
    let mut args = vec!["track", "relocate", "Club"];
    for f in &folders[..2] {
        if f.exists() {
            args.extend(["--look-in", path(f)]);
        }
    }
    let dry = [&args[..], &["--dry-run"]].concat();
    let (planned, printed) = (fx.run(&[&["--json"][..], &dry[..]].concat()), fx.run(&dry));
    let said = format!("{}\n{}\n{}", printed.out, printed.err, planned.out);
    assert_eq!(planned.code, printed.code, "{said}");
    let v: Value = serde_json::from_str(&planned.out).unwrap();
    let p = &v["plan"];
    let ids = |key: &str| -> Vec<u64> {
        p[key].as_array().unwrap().iter().map(|r| r["track"]["id"].as_u64().unwrap()).collect()
    };
    let ids_of =
        |f: &dyn Fn(&[usize]) -> bool| -> Vec<u64> { model.iter().filter(|(_, p)| f(p)).map(|(id, _)| *id).collect() };
    let sorted = |mut v: Vec<u64>| {
        v.sort_unstable();
        v
    };
    assert_eq!(sorted(ids("repointed")), ids_of(&|p| p == [2]), "{said}");
    assert_eq!(sorted(ids("copied")), ids_of(&|p| p.len() == 1 && p[0] != 2), "{said}");
    assert_eq!(sorted(ids("ambiguous")), ids_of(&|p| p.len() > 1), "{said}");
    let not_found: Vec<u64> = p["not_found"].as_array().unwrap().iter().map(|t| t["id"].as_u64().unwrap()).collect();
    assert_eq!(sorted(not_found), ids_of(&|p| p.is_empty()), "{said}");

    let out = &printed.out;
    for (key, sign) in [("repointed", "~"), ("copied", "+"), ("ambiguous", "?")] {
        for r in p[key].as_array().unwrap() {
            let line = format!("{sign} {}  ", r["track"]["name"].as_str().unwrap());
            assert!(out.lines().any(|l| l.trim_start().starts_with(&line)), "{line:?}: {said}");
        }
    }
    for a in p["ambiguous"].as_array().unwrap() {
        for c in a["candidates"].as_array().unwrap() {
            assert!(out.contains(c["path"].as_str().unwrap()), "{c}: {said}");
        }
    }
    let missing = p["not_found"].as_array().unwrap();
    assert_eq!(
        out.contains(&format!("- {} track(s) with no file of their name, e.g. ", missing.len())),
        !missing.is_empty(),
        "{said}"
    );
    if p["noop"] == true {
        assert_eq!(v["outcome"], "nothing_to_do", "{said}");
        return;
    }
    let applied = fx.run(&[&["--yes"][..], &args].concat());
    assert_eq!(applied.code, 0, "{}\n{}", applied.out, applied.err);
    let n = p["repointed"].as_array().unwrap().len() + p["copied"].as_array().unwrap().len();
    assert!(applied.out.contains(&format!("relocated {n} track(s)")), "{}", applied.out);
    for id in ids("repointed").into_iter().chain(ids("copied")) {
        let d = fx.json(&["track", "show", "Club", "--track", &id.to_string()]);
        assert_eq!(d[0]["audio_present"], true, "track {id}: {}", d[0]);
    }
}
