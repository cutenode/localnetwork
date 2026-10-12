//! Property tests of `locallibrary sets` and `plays`: sets declared, edited and deleted in any
//! order agree with a model of their tracklists, and every listing (`sets`, `sets show`, `sets
//! suggest`, `sets played`, `sets export`, `plays`, `plays stats`...) printed for people says
//! what its `--json` does on the same store.

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, Run, path, play};

/// A set as the model has it.
#[derive(Debug, Clone)]
struct ModelSet {
    id: String,
    title: Option<String>,
    /// Each entry's name ("Artist - Title"), and whether it's published unnamed.
    tracks: Vec<(String, bool)>,
}

/// The tracks of the club drive by id, as "Artist - Title".
fn club_names(fx: &Env) -> Vec<(u64, String)> {
    let v = fx.json(&["tracks", "Club"]);
    v["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (t["id"].as_u64().unwrap(), format!("{} - {}", t["artist"].as_str().unwrap(), t["title"].as_str().unwrap()))
        })
        .collect()
}

/// An entry's name as a set prints it.
fn entry_name(e: &Value) -> String {
    let (artist, title) = (e["artist"].as_str().unwrap(), e["title"].as_str().unwrap());
    match (artist.is_empty(), title.is_empty()) {
        (false, false) => format!("{artist} - {title}"),
        (true, _) => title.to_string(),
        (false, true) => artist.to_string(),
    }
}

/// The sets the store has, newest first, as `sets --json` lists them.
fn listed(fx: &Env) -> Vec<Value> {
    fx.json(&["sets"]).as_array().unwrap().clone()
}

fn run(fx: &Env, args: &[String]) -> Run {
    fx.run(&args.iter().map(String::as_str).collect::<Vec<_>>())
}

/// A printed set (`sets show`, and after `sets new` and `sets edit`) has a line for each of its
/// tracks, in order, and the details its JSON has.
fn set_is_printed(set: &Value, out: &str, said: &str) {
    let tracks = set["tracks"].as_array().unwrap();
    let timed = tracks.iter().any(|t| !t["start_seconds"].is_null());
    let mut lines = out.lines();
    for (i, t) in tracks.iter().enumerate() {
        let start = format!("  {:>3}  ", i + 1);
        let line = lines.find(|l| l.starts_with(&start)).unwrap_or_else(|| panic!("no track {}: {said}", i + 1));
        assert!(line.contains(&entry_name(t)), "{line:?} for {t}: {said}");
        assert_eq!(line.contains("unnamed"), t["unnamed"] == true, "{line:?}: {said}");
        if timed && t["start_seconds"].is_null() {
            assert!(
                line.starts_with(&format!("{start}          ")),
                "an untimed entry in a timed set: {line:?}\n{said}"
            );
        }
    }
    let n = tracks.len();
    assert!(out.contains(&format!("  {n} track{} · ", if n == 1 { "" } else { "s" })), "{said}");
    for (label, key) in [("Event", "event"), ("Listen", "link"), ("Notes", "notes")] {
        if let Some(text) = set[key].as_str() {
            assert!(out.contains(&format!("  {label:<7} {text}\n")), "{label}: {said}");
        }
    }
    if let Some(with) = set["with"].as_array() {
        let names: Vec<&str> = with.iter().map(|p| p["name"].as_str().unwrap()).collect();
        assert!(out.lines().any(|l| l.starts_with("  With    ") && names.iter().all(|n| l.contains(n))), "{said}");
    }
    let sessions = set["sessions"].as_array().map_or(0, Vec::len);
    if sessions > 0 {
        let from = if sessions == 1 { "  From    session " } else { "  From    sessions " };
        assert!(out.contains(from), "{said}");
    }
}

/// Sets declared (by hand, or from sessions a player recorded), edited (tracks inserted,
/// dropped, moved, unnamed and named again; renamed) and deleted, in any order and printed or
/// as JSON: each succeeds as the model says, `sets --json` lists what the model holds, and the
/// printed list, `sets show` and `sets export` say what their JSON does.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn sets_agree_with_a_model_and_print_what_their_json_says(tc: TestCase) {
    let fx = Env::from_template();
    let club = fx.volumes.join("Club");
    let ids: Vec<u32> = (1..=12).collect();
    let sessions: Vec<Vec<u32>> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(2)))
        .map(|_| tc.draw(gs::permutations(tc.draw(gs::subsequences(ids.clone())))))
        .collect();
    for (i, s) in sessions.iter().enumerate() {
        play(&club, &format!("HISTORY {:03}", i + 1), s);
    }
    fx.ok(&["pull", path(&club), "--library", "Club", "--new-library"]);
    // Sessions by id: A has a "HISTORY 001" too.
    let listed_sessions = fx.json(&["plays", "lists", "Club"]);
    let session_id = |name: &str| -> String {
        let s = listed_sessions["sessions"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap();
        s["id"].as_str().unwrap()[..8].to_string()
    };
    let names = club_names(&fx);
    let name_of = |id: u32| names.iter().find(|(i, _)| *i == u64::from(id)).unwrap().1.clone();
    // Typed in: matched to no track, they go in as they are.
    let typed = ["Someone - Dubplate", "Floorplan - Never Grow Old"];

    let mut model: Vec<ModelSet> = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(7)) {
        let json = tc.draw(gs::booleans());
        let mut args: Vec<String> = vec!["--yes".into()];
        if json {
            args.push("--json".into());
        }
        let step = tc.draw(gs::integers::<u8>().max_value(if model.is_empty() { 1 } else { 7 }));
        let pick = |tc: &TestCase| tc.draw(gs::integers::<usize>().max_value(model.len().max(1) - 1));
        // What the step does to the model, applied once it's known to have succeeded.
        let mut expect: Option<ModelSet> = None;
        let mut target = None;
        match step {
            0 => {
                let title = tc.draw(gs::sampled_from(&["Friday", "Ünïcödé 日本", "Warm-ups"][..]));
                args.extend(["sets", "new", "--title", title, "--date", "2026-10-03"].map(String::from));
                for (flag, value) in [
                    ("--venue", "Warehouse"),
                    ("--kind", "club"),
                    ("--city", "London"),
                    ("--country", "gb"),
                    ("--with", "DJ Two"),
                    ("--with", "did:plc:abc"),
                    ("--event", "at://did:plc:abc/x.event/1"),
                    ("--link", "https://www.mixcloud.com/dj/friday/"),
                    ("--notes", "Warm crowd"),
                ] {
                    if tc.draw(gs::weighted_booleans(0.3)) {
                        args.extend([flag.to_string(), value.to_string()]);
                    }
                }
                expect = Some(ModelSet { id: String::new(), title: Some(title.into()), tracks: Vec::new() });
            }
            1 => {
                // One session, or several in the order given: the set plays them one after another.
                let from: Vec<usize> = tc.draw(gs::permutations((0..sessions.len()).collect::<Vec<_>>()))
                    [..tc.draw(gs::integers::<usize>().min_value(1).max_value(sessions.len()))]
                    .to_vec();
                args.extend(["sets", "new"].map(String::from));
                for i in &from {
                    args.extend(["--from".to_string(), session_id(&format!("HISTORY {:03}", i + 1))]);
                }
                let tracks = from.iter().flat_map(|i| sessions[*i].iter().map(|id| (name_of(*id), false))).collect();
                expect = Some(ModelSet { id: String::new(), title: None, tracks });
            }
            2..=5 => {
                let t = pick(&tc);
                target = Some(t);
                let mut set = model[t].clone();
                // Typed tracks are looked up on the club drive.
                args.extend(["sets", "edit", &set.id[..8], "--drive", "Club"].map(String::from));
                let n = set.tracks.len();
                match step {
                    2 => {
                        let at = tc.draw(gs::integers::<usize>().min_value(1).max_value(n + 1));
                        let name = match tc.draw(gs::booleans()) {
                            true => tc.draw(gs::sampled_from(&typed[..])).to_string(),
                            false => name_of(tc.draw(gs::integers::<u32>().min_value(1).max_value(12))),
                        };
                        args.extend(["--insert".to_string(), format!("{at}:{name}")]);
                        set.tracks.insert(at - 1, (name, false));
                    }
                    3 if n > 0 => {
                        let at = tc.draw(gs::integers::<usize>().min_value(1).max_value(n));
                        args.extend(["--drop".to_string(), at.to_string()]);
                        set.tracks.remove(at - 1);
                    }
                    4 if n > 1 => {
                        let from = tc.draw(gs::integers::<usize>().min_value(1).max_value(n));
                        let to = tc.draw(gs::integers::<usize>().min_value(1).max_value(n));
                        args.extend(["--move".to_string(), format!("{from}:{to}")]);
                        let e = set.tracks.remove(from - 1);
                        set.tracks.insert(to - 1, e);
                    }
                    5 if n > 0 => {
                        let at = tc.draw(gs::integers::<usize>().min_value(1).max_value(n));
                        let unnamed = !set.tracks[at - 1].1;
                        let flag = if unnamed { "--unnamed" } else { "--named" };
                        args.extend([flag.to_string(), at.to_string()]);
                        set.tracks[at - 1].1 = unnamed;
                    }
                    _ => {
                        let title = tc.draw(gs::sampled_from(&["Saturday", "Closing"][..]));
                        args.extend(["--title".to_string(), title.to_string()]);
                        set.title = Some(title.into());
                    }
                }
                expect = Some(set);
            }
            6 => {
                let t = pick(&tc);
                target = Some(t);
                args.extend(["sets", "delete", &model[t].id[..8]].map(String::from));
            }
            _ => {
                // A list of positions that runs backwards is refused before anything is read.
                let t = pick(&tc);
                args.extend(["sets", "edit", &model[t].id[..8], "--drop", "2-1"].map(String::from));
                let r = run(&fx, &args);
                assert_eq!(r.code, 2, "{args:?}\n{}\n{}", r.out, r.err);
                continue;
            }
        }
        let before: Vec<String> = listed(&fx).iter().map(|s| s["id"].as_str().unwrap().to_string()).collect();
        let r = run(&fx, &args);
        let said = format!("{args:?} exited {}\n{}\n{}\nmodel: {model:?}", r.code, r.out, r.err);
        assert_eq!(r.code, 0, "{said}");
        let now = listed(&fx);
        match (step, expect) {
            (6, _) => {
                model.remove(target.unwrap());
            }
            (_, Some(mut set)) => {
                let id = match target {
                    Some(t) => model[t].id.clone(),
                    None => now
                        .iter()
                        .map(|s| s["id"].as_str().unwrap().to_string())
                        .find(|id| !before.contains(id))
                        .unwrap_or_else(|| panic!("no new set: {said}")),
                };
                set.id = id.clone();
                let shown = now.iter().find(|s| s["id"] == id.as_str()).unwrap();
                match json {
                    true => {
                        let v: Value = serde_json::from_str(&r.out).unwrap();
                        assert_eq!(&v["report"]["set"], shown, "{said}");
                    }
                    false => {
                        let did = if target.is_some() { "changed" } else { "declared" };
                        assert!(r.out.starts_with(&format!("✓ {did} {}", shown["date"].as_str().unwrap())), "{said}");
                        set_is_printed(shown, &r.out, &said);
                    }
                }
                match target {
                    Some(t) => model[t] = set,
                    None => model.insert(0, set),
                }
            }
            _ => unreachable!(),
        }

        // The store holds what the model does.
        let mut ids_now: Vec<&str> = now.iter().map(|s| s["id"].as_str().unwrap()).collect();
        let mut ids_model: Vec<&str> = model.iter().map(|s| s.id.as_str()).collect();
        ids_now.sort_unstable();
        ids_model.sort_unstable();
        assert_eq!(ids_now, ids_model, "{said}");
        for m in &model {
            let s = now.iter().find(|s| s["id"] == m.id.as_str()).unwrap();
            let tracks: Vec<(String, bool)> =
                s["tracks"].as_array().unwrap().iter().map(|e| (entry_name(e), e["unnamed"] == true)).collect();
            assert_eq!(tracks, m.tracks, "{}\n{said}", s);
            if let Some(t) = &m.title {
                assert_eq!(s["title"].as_str(), Some(t.as_str()), "{said}");
            }
        }
        // Printed, the list has a line for each, and a set's details and export are its JSON's.
        let printed = fx.ok(&["sets"]);
        for s in &now {
            let short = &s["id"].as_str().unwrap()[..8];
            let line = printed.lines().find(|l| l.starts_with(short)).unwrap_or_else(|| panic!("{short}: {printed}"));
            assert!(line.contains(s["date"].as_str().unwrap()), "{line}");
            let n = s["tracks"].as_array().unwrap().len();
            assert!(line.ends_with(&format!(" {n} track{}", if n == 1 { "" } else { "s" })), "{line}");
        }
        if now.is_empty() {
            assert!(printed.starts_with("no sets yet"), "{printed}");
            continue;
        }
        let s = &now[tc.draw(gs::integers::<usize>().max_value(now.len() - 1))];
        let short = s["id"].as_str().unwrap()[..8].to_string();
        let mut shown = fx.json(&["sets", "show", &short]);
        shown.as_object_mut().unwrap().remove("schema");
        assert_eq!(&shown, s, "{said}");
        set_is_printed(s, &fx.ok(&["sets", "show", &short]), &said);
        let format = tc.draw(gs::sampled_from(&["text", "mixcloud", "1001", "csv", "youtube", "cue", "m3u8"][..]));
        let export = ["sets", "export", &short, "--format", format];
        let (printed, as_json) = (fx.run(&export), fx.run(&[&["--json"][..], &export].concat()));
        assert_eq!(printed.code, as_json.code, "{export:?}\n{}\n{}", printed.err, as_json.out);
        if printed.code == 0 {
            let v: Value = serde_json::from_str(&as_json.out).unwrap();
            assert_eq!(printed.out, v["text"].as_str().unwrap(), "{export:?}");
        }
        // Written to a file, the format is the one its extension names (text when none does):
        // the file holds what that format prints.
        let (ext, named) = tc.draw(gs::sampled_from(
            &[("txt", "text"), ("csv", "csv"), ("cue", "cue"), ("m3u8", "m3u8"), ("list", "text")][..],
        ));
        let file = fx.home(&format!("set-{}.{ext}", now.len()));
        let mut args = vec!["sets", "export", &short, "-o", path(&file), "--replace"];
        if named == "m3u8" {
            args.extend(["--drive", "Club"]);
        }
        let r = fx.run(&args);
        let as_printed = fx.run(&["sets", "export", &short, "--format", named, "--drive", "Club"]);
        assert_eq!(r.code, as_printed.code, "{args:?}\n{}\n{}", r.err, as_printed.err);
        if r.code == 0 {
            assert!(r.out.starts_with(&format!("✓ wrote {} (", file.display())), "{}", r.out);
            if named != "m3u8" {
                assert_eq!(std::fs::read_to_string(&file).unwrap(), as_printed.out, "{args:?}");
            }
        }
        // A playlist of the set on the club drive: its plan printed is its JSON's.
        let playlist = ["sets", "playlist", &short, "Club", "--dry-run"];
        let (planned, printed) = (fx.run(&[&["--json"][..], &playlist].concat()), fx.run(&playlist));
        let said = format!("{playlist:?}\n{}\n{}\n{}", printed.out, printed.err, planned.out);
        assert_eq!(planned.code, printed.code, "{said}");
        let v: Value = serde_json::from_str(&planned.out).unwrap();
        if v["outcome"] == "dry_run" {
            crate::props_cli_edits::plan_is_printed(&v["plan"], &printed.out, &said);
        }
    }
}

/// The sessions on the club drive suggested as sets: each candidate printed with its score and
/// the command that declares it; dismissing sessions takes their candidates away and
/// `--undismiss` brings them back; and a track played in sets is found by a word of its name,
/// printed as its JSON lists it.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn suggestions_and_plays_print_what_their_json_says(tc: TestCase) {
    let fx = Env::from_template();
    let club = fx.volumes.join("Club");
    let ids: Vec<u32> = (1..=12).collect();
    for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let tracks: Vec<u32> = tc.draw(gs::permutations(tc.draw(gs::subsequences(ids.clone()))));
        play(&club, &format!("HISTORY {:03}", i + 1), &tracks);
    }
    fx.ok(&["pull", path(&club), "--library", "Club", "--new-library"]);
    let candidates = |fx: &Env| -> Vec<Vec<String>> {
        let v = fx.json(&["sets", "suggest"]);
        let printed = fx.ok(&["sets", "suggest"]);
        let list: Vec<Vec<String>> = v["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["sessions"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect())
            .collect();
        if list.is_empty() {
            assert!(printed.starts_with("no sets to suggest"), "{printed}");
        }
        for (c, sessions) in v["candidates"].as_array().unwrap().iter().zip(&list) {
            assert!(printed.contains(&format!("score {} of 100", c["score"])), "{printed}");
            let from: Vec<String> = sessions.iter().rev().map(|s| format!("--from {}", &s[..8])).collect();
            assert!(printed.contains(&format!("locallibrary sets new {}", from.join(" "))), "{printed}");
            for reason in c["reasons"].as_array().unwrap() {
                assert!(printed.contains(reason.as_str().unwrap()), "{printed}");
            }
        }
        list
    };
    let before = candidates(&fx);
    if let Some(first) = before.first() {
        let short: Vec<&str> = first.iter().map(|s| &s[..8]).collect();
        let mut args = vec!["sets", "suggest", "--dismiss"];
        args.extend(&short);
        let out = fx.ok(&args);
        assert!(out.contains(&format!("{} won't be suggested", short.join(", "))), "{out}");
        let after = candidates(&fx);
        assert!(!after.contains(first), "{first:?} still suggested: {after:?}");
        let out = fx.ok(&["sets", "suggest", "--undismiss", short[0]]);
        assert!(out.contains(&format!("{} suggested again", short[0])), "{out}");
    }

    // Declared, each candidate's tracks are found by a word of their names.
    for c in before.iter().take(2) {
        let mut args = vec!["--yes", "sets", "new", "--venue", "Warehouse"];
        for s in c {
            args.extend(["--from", &s[..8]]);
        }
        fx.ok(&args);
    }
    let word = tc.draw(gs::sampled_from(&["in charge", "shuffle", "Moves", "nothing like it"][..]));
    let found = fx.json(&["sets", "played", word]);
    let found = found.as_array().unwrap();
    let printed = fx.ok(&["sets", "played", word]);
    let Some(latest) = found.first() else {
        assert_eq!(printed, format!("{word:?} isn't in any of your sets\n"));
        return;
    };
    for a in found {
        let line = format!("track {} · {}", a["position"], a["track"].as_str().unwrap());
        assert!(printed.contains(&line), "{line:?}: {printed}");
    }
    let times = if found.len() == 1 { "once".to_string() } else { format!("{} times", found.len()) };
    let place = latest["venue"].as_str().map(|v| format!(" at {v}")).unwrap_or_default();
    let last = format!("played {times}, last on {}{place}\n", latest["date"].as_str().unwrap());
    assert!(printed.ends_with(&last), "{last:?}: {printed}");
}

/// The club drive's sessions, some kept by the store and some only on the drive: `plays` lists
/// each (with `--all`, those only the store has too), one session prints its tracks in any
/// format, `plays stats` prints its JSON's counts, and a session written to a file holds what
/// it printed.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn plays_print_what_their_json_says(tc: TestCase) {
    let fx = Env::from_template();
    let club = fx.volumes.join("Club");
    let d = path(&club).to_string();
    let ids: Vec<u32> = (1..=12).chain([99]).collect();
    let n = tc.draw(gs::integers::<usize>().max_value(3));
    let pulled = tc.draw(gs::integers::<usize>().max_value(n));
    for i in 0..n {
        let tracks: Vec<u32> = tc.draw(gs::permutations(tc.draw(gs::subsequences(ids.clone()))));
        play(&club, &format!("HISTORY {:03}", i + 1), &tracks);
        if i + 1 == pulled {
            fx.ok(&["pull", &d, "--library", "Club", "--new-library"]);
        }
    }
    // Once saved, a session may be dismissed, or made a set.
    if pulled > 0 {
        let sessions = fx.json(&["plays", "lists", &d]);
        for s in sessions["sessions"].as_array().unwrap() {
            let id = &s["id"].as_str().unwrap()[..8];
            match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => drop(fx.ok(&["sets", "suggest", "--dismiss", id])),
                1 => drop(fx.ok(&["--yes", "sets", "new", "--from", id, "--title", "Friday"])),
                _ => {}
            }
        }
    }
    let all = tc.draw(gs::booleans());
    // `plays DRIVE`, `plays lists DRIVE` and `history DRIVE` are one listing (`plays DRIVE`
    // without `--all`).
    let mut args = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 if !all => vec!["plays", d.as_str()],
        1 => vec!["history", d.as_str()],
        _ => vec!["plays", "lists", d.as_str()],
    };
    if all {
        args.push("--all");
    }
    let v = fx.json(&args);
    let printed = fx.ok(&args);
    for s in v["sessions"].as_array().unwrap() {
        let line = printed.lines().find(|l| l.contains(&s["id"].as_str().unwrap()[..8])).unwrap();
        if s["dismissed"] == true && s["sets"].as_array().is_none_or(Vec::is_empty) {
            assert!(line.ends_with("dismissed"), "{line}");
        }
        if s["sets"].as_array().is_some_and(|l| !l.is_empty()) {
            assert!(line.contains("· in ") || line.contains("  in "), "{line}");
        }
    }
    // Asked for a session the drive doesn't have, or to write one without naming it: refused.
    let r = fx.run(&["plays", "lists", &d, "HISTORY 999"]);
    assert!(r.code == 1 && r.err.contains("no session \"HISTORY 999\" on Club"), "{}", r.err);
    let r = fx.run(&["plays", "lists", &d, "-o", path(&fx.home("x.txt"))]);
    assert!(r.code == 1 && r.err.contains("name the history list to write"), "{}", r.err);
    assert!(printed.starts_with("On Club\n"), "{printed}");
    for s in v["sessions"].as_array().unwrap() {
        let short = &s["id"].as_str().unwrap()[..8];
        let line = printed.lines().find(|l| l.contains(short)).unwrap_or_else(|| panic!("{short}: {printed}"));
        assert!(line.contains(s["name"].as_str().unwrap()), "{line}");
        let n = s["tracks"].as_array().unwrap().len();
        assert!(line.contains(&format!(" {n} track")), "{line}");
        if pulled > 0 {
            assert!(line.contains("in the store") || line.contains("not in the store yet"), "{line}");
        }
    }
    assert_eq!(printed.contains("Only in the store ("), all, "{printed}");
    if let Some(s) = v["sessions"].as_array().unwrap().first() {
        let name = s["name"].as_str().unwrap();
        let format = tc.draw(gs::sampled_from(&["txt", "m3u8", "csv"][..]));
        // A session only the store has is looked for there with --all.
        let mut args = vec!["plays", "lists", d.as_str(), name, "--format", format];
        if all {
            args.push("--all");
        }
        let list = fx.json(&args);
        let out = fx.ok(&args);
        let tracks = list["tracks"].as_array().unwrap();
        for t in tracks {
            assert!(out.contains(t["title"].as_str().unwrap()), "{t}: {out}");
        }
        let missing = list["missing"].as_u64().unwrap();
        assert_eq!(out.contains(&format!("{missing} entr(ies) refer to tracks no longer")), missing > 0, "{out}");
        let file = fx.home(&format!("session.{format}"));
        let mut args = vec!["plays", "export", d.as_str(), name, "-o", path(&file)];
        if all {
            args.push("--all");
        }
        let r = fx.run(&args);
        assert_eq!(r.code, 0, "{}", r.err);
        assert!(r.out.contains(&format!("wrote {} ({name}, {} tracks)", file.display(), tracks.len())), "{}", r.out);
        let written = std::fs::read_to_string(&file).unwrap();
        for t in tracks {
            assert!(written.contains(t["title"].as_str().unwrap()), "{written}");
        }
    }

    let mut args = vec!["plays", "stats", d.as_str()];
    if all {
        args.push("--all");
    }
    if pulled > 0 && tc.draw(gs::booleans()) {
        args.extend(["--since", "2099"]);
    }
    let s = fx.json(&args);
    let out = fx.ok(&args);
    let never = s["never_played"].as_array().unwrap();
    let head = format!(
        "  {} tracks, {} played, {} never played, {} history list(s)",
        s["tracks"],
        s["played"],
        never.len(),
        s["history_lists"]
    );
    assert!(out.contains(&head), "{head:?}: {out}");
    let shown = if all { never.len() } else { never.len().min(10) };
    for t in &never[..shown] {
        assert!(out.contains(&format!("  {}\n", t["name"].as_str().unwrap())), "{t}: {out}");
    }
    assert_eq!(out.contains("--all lists them all"), !all && never.len() > 10, "{out}");
    for p in s["most_played"].as_array().unwrap() {
        let line = out.lines().find(|l| l.ends_with(p["track"]["name"].as_str().unwrap()) && l.contains('×'));
        assert!(line.is_some_and(|l| l.trim_start().starts_with(&format!("{}×", p["play_count"]))), "{p}: {out}");
    }
    if let Some(since) = s["since"].as_object() {
        assert!(out.contains(&format!("Since snapshot {}", since["short_id"].as_str().unwrap())), "{out}");
        assert_eq!(out.contains("nothing played"), since["tracks"].as_array().unwrap().is_empty(), "{out}");
    }
}

/// A log of plays (Beat Link Trigger's Playlist Writer) with drawn tracks and start times kept as a
/// session: a set made from it is timed, `sets show` prints each start, and its Mixcloud and
/// YouTube tracklists print what its JSON export does.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn a_timed_log_makes_a_timed_set(tc: TestCase) {
    let fx = Env::from_template();
    let names = club_names(&fx);
    let ids: Vec<usize> = (0..names.len()).collect();
    let picked: Vec<usize> = tc.draw(gs::permutations(tc.draw(gs::subsequences(ids)))).into_iter().collect();
    if picked.is_empty() {
        return;
    }
    let tracks = fx.json(&["tracks", "Club"]);
    let tracks = tracks["tracks"].as_array().unwrap();
    let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut log = String::from("Title,Artist,Album,Player,Source,Started,Stopped,Play Time\n");
    let mut at = chrono::DateTime::parse_from_rfc3339("2026-10-03T23:00:00Z").unwrap();
    let mut starts = Vec::new();
    for i in &picked {
        let t = &tracks[*i];
        let length = tc.draw(gs::integers::<i64>().min_value(30).max_value(600));
        let stop = at + chrono::Duration::seconds(length);
        let fmt = |t: chrono::DateTime<chrono::FixedOffset>| t.format("%a %b %d %H:%M:%S UTC %Y").to_string();
        log.push_str(&format!(
            "{},{},,2,Player 2 USB,{},{},{:02}:{:02}\n",
            q(t["title"].as_str().unwrap()),
            q(t["artist"].as_str().unwrap()),
            fmt(at),
            fmt(stop),
            length / 60,
            length % 60
        ));
        starts.push(at);
        at = stop;
    }
    let file = fx.home("Friday.csv");
    std::fs::write(&file, log).unwrap();
    let as_json = tc.draw(gs::booleans());
    let r = fx.run(&[if as_json { "--json" } else { "--quiet" }, "plays", "import", path(&file), "--drive", "Club"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    if !as_json {
        assert!(r.out.contains("kept Friday as session"), "{}", r.out);
        let plays = if picked.len() == 1 { "1 play".to_string() } else { format!("{} plays", picked.len()) };
        assert!(r.out.contains(&format!(": {plays}, ")), "{}", r.out);
    }
    let sessions = fx.json(&["plays", "lists", "Club", "--all"]);
    let id = sessions["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Friday")
        .unwrap_or_else(|| panic!("{sessions}"))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let v = fx.json(&["--yes", "sets", "new", "--from", &id[..8], "--title", "Friday"]);
    let set = &v["report"]["set"];
    let got: Vec<u64> =
        set["tracks"].as_array().unwrap().iter().map(|t| t["start_seconds"].as_u64().unwrap()).collect();
    let want: Vec<u64> = starts.iter().map(|s| (*s - starts[0]).num_seconds() as u64).collect();
    assert_eq!(got, want, "{set}");
    set_is_printed(set, &fx.ok(&["sets", "show", "last"]), &format!("{set}"));
    for format in ["mixcloud", "youtube", "cue"] {
        let export = ["sets", "export", "last", "--format", format];
        let (printed, j) = (fx.run(&export), fx.run(&[&["--json"][..], &export].concat()));
        assert_eq!(printed.code, j.code, "{format}: {}\n{}", printed.err, j.out);
        if printed.code == 0 {
            let v: Value = serde_json::from_str(&j.out).unwrap();
            assert_eq!(printed.out, v["text"].as_str().unwrap(), "{format}");
        }
    }
}

/// Sessions a player recorded on the club drive since it was last saved: saving it (`pull`, or
/// the `refresh` job's pull) says how many are new, in a hint (none when none are), and `sets
/// suggest` is where to look for sets among them.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn saving_says_how_many_sessions_are_new(tc: TestCase) {
    let fx = Env::from_template();
    let club = fx.volumes.join("Club");
    let d = path(&club).to_string();
    let hint = |n: usize| match n {
        0 => None,
        1 => Some("1 new session: `locallibrary sets suggest` says if it looks like a set".to_string()),
        n => Some(format!("{n} new sessions: `locallibrary sets suggest` says which look like sets")),
    };
    let mut lists = 0;
    for round in 0..2 {
        let new = tc.draw(gs::integers::<usize>().max_value(2));
        for _ in 0..new {
            lists += 1;
            let tracks: Vec<u32> = tc.draw(gs::subsequences((1..=12).collect::<Vec<u32>>()));
            play(&club, &format!("HISTORY {lists:03}"), &tracks);
        }
        let out = match (round, tc.draw(gs::booleans())) {
            (0, _) => fx.ok(&["pull", &d, "--library", "Club", "--new-library"]),
            (_, true) => fx.ok(&["pull", &d]),
            (_, false) => fx.ok(&["--yes", "refresh", &d, "--skip", "clean,replay,smart,player,settings"]),
        };
        match hint(new) {
            Some(h) => assert!(out.contains(&h), "{h:?}: {out}"),
            None => assert!(!out.contains("new session"), "{out}"),
        }
    }
}
