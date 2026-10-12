//! Property tests of `locallibrary profile`: the report printed for people says what its `--json`
//! says for the same drives and choices, and the choices (`include`, `pin`, `hide`, `alias`...)
//! made in any order agree with a model of them, printed or as JSON.

use std::collections::BTreeSet;

use indexmap::IndexMap;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, path, play};

/// Every section, in the order the report prints them, with its heading (a session report's own
/// for two of them).
const SECTIONS: [(&str, &str); 15] = [
    ("summary", "Summary"),
    ("sets", "Sets"),
    ("tempo", "Tempo"),
    ("arc", "Set shape"),
    ("mixing", "Mixing"),
    ("keys", "Keys"),
    ("energy", "Energy"),
    ("genres", "Genres"),
    ("labels", "Labels"),
    ("artists", "Artists"),
    ("era", "Era"),
    ("signature", "Signature tracks"),
    ("prep", "Prep"),
    ("collection", "Collection"),
    ("style", "Player style"),
];

/// Published unless they're turned off.
fn by_default() -> BTreeSet<String> {
    let opt_in = ["labels", "artists", "signature", "prep", "collection"];
    SECTIONS.iter().map(|(s, _)| s.to_string()).filter(|s| !opt_in.contains(&s.as_str())).collect()
}

/// A section's heading in a report on `basis`.
fn title(section: &str, basis: &str) -> &'static str {
    match (section, basis) {
        ("sets", "sessions") => "Sessions",
        ("arc", "sessions") => "Session shape",
        _ => SECTIONS.iter().find(|(s, _)| *s == section).unwrap().1,
    }
}

/// `n` of something, as the report counts it.
fn plural(n: u64, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

/// The section headings a printed report has: (title, whether it's marked as in the document).
fn headings(out: &str) -> Vec<(String, bool)> {
    out.lines()
        .filter_map(|l| {
            let (mark, rest) = l.split_once(' ')?;
            let published = match mark {
                "●" | "*" => true,
                "○" | "o" => false,
                _ => return None,
            };
            let title = rest.split("  ").next()?.to_string();
            Some((title, published))
        })
        .collect()
}

/// The club drive's tracks played in sessions a player recorded (most long enough to count, some
/// tracks in several), maybe saved and sets declared from some, a drawn choice or two; then for
/// both drives, the report for any sections, with or without the list of sets: its printed
/// headings are the sections asked for, each marked as in the document as the JSON's `published`
/// and basis say, and its numbers, headline, sets and signature tracks are the JSON's; and the
/// document printed, as JSON and written to a file is the same one.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn the_printed_report_says_what_the_json_does(tc: TestCase) {
    let fx = Env::from_template();
    let club = fx.volumes.join("Club");
    let ids: Vec<u32> = (1..=12).collect();
    let lists = tc.draw(gs::integers::<usize>().max_value(4));
    let mut last: Vec<u32> = Vec::new();
    for i in 1..=lists {
        // Most sessions long enough to count, some tracks in more than one; a session played
        // again as it was makes its moves from one track to the next signature moves.
        let n = tc.draw(gs::integers::<usize>().max_value(12));
        let tracks: Vec<u32> = match tc.draw(gs::weighted_booleans(0.3)) && !last.is_empty() {
            true => last.clone(),
            false => tc.draw(gs::permutations(ids.clone())).into_iter().take(n).collect(),
        };
        play(&club, &format!("HISTORY {i:03}"), &tracks);
        last = tracks;
    }
    if tc.draw(gs::weighted_booleans(0.7)) {
        fx.ok(&["pull", path(&club), "--library", "Club", "--new-library"]);
        // Sets declared from some sessions: the report is then of sets, not sessions.
        let sessions = fx.json(&["plays", "lists", "Club"]);
        for s in sessions["sessions"].as_array().unwrap() {
            if tc.draw(gs::weighted_booleans(0.5)) {
                let id = &s["id"].as_str().unwrap()[..8];
                let day = tc.draw(gs::sampled_from(&["2026-09-01", "2026-10-03"][..]));
                fx.ok(&[
                    "--yes",
                    "sets",
                    "new",
                    "--from",
                    id,
                    "--title",
                    "Friday",
                    "--venue",
                    "Warehouse",
                    "--date",
                    day,
                ]);
            }
        }
    }
    // Choices the report shows: a headline of the DJ's own, an artist never named, a pin, an alias.
    if tc.draw(gs::booleans()) {
        fx.ok(&["--yes", "profile", "headline", "Warm-ups and closing sets"]);
    }
    if tc.draw(gs::booleans()) {
        fx.ok(&["--yes", "profile", "hide", "[a]pendics.shuffle", "--artist"]);
    }
    if tc.draw(gs::booleans()) {
        fx.ok(&["--yes", "profile", "pin", "Somebody - Never Played", "--exact"]);
    }
    if tc.draw(gs::booleans()) {
        fx.ok(&["--yes", "profile", "include", "labels,artists,signature,prep,collection"]);
    }
    // Drive A has analysis files (phrase moods, cues) and a session; Club neither.
    for drive in ["Club", "A"] {
        report_agrees(&tc, &fx, path(&fx.volumes.join(drive)));
        document_agrees(&tc, &fx, path(&fx.volumes.join(drive)));
    }
}

/// `profile document` printed is the document `--json` gives (but for when it was made), and
/// written to a file with `-o`, the file holds it; a file that's there is kept unless replaced.
#[hegel::test_helper]
fn document_agrees(tc: &TestCase, fx: &Env, drive: &str) {
    // When it was made, and the envelope's version.
    let without_time = |mut v: Value| {
        v.as_object_mut().unwrap().remove("generatedAt");
        v.as_object_mut().unwrap().remove("schema");
        v
    };
    let printed: Value = serde_json::from_str(&fx.ok(&["profile", "document", drive, "--no-analysis"])).unwrap();
    let doc = fx.json(&["profile", "document", drive, "--no-analysis"]);
    assert_eq!(without_time(printed), without_time(doc.clone()));
    let file = fx.home("profile.json");
    if tc.draw(gs::booleans()) {
        std::fs::write(&file, "mine").unwrap();
    }
    let replace = tc.draw(gs::booleans());
    let mut args = vec!["profile", "document", drive, "--no-analysis", "-o", path(&file)];
    if replace {
        args.push("--replace");
    }
    let was = std::fs::read_to_string(&file).ok();
    let json = tc.draw(gs::booleans());
    let r = fx.run(&[&[if json { "--json" } else { "--quiet" }][..], &args].concat());
    if was.is_some() && !replace {
        assert_eq!(r.code, 1, "{}", r.err);
        assert_eq!(std::fs::read_to_string(&file).ok(), was);
        return;
    }
    assert_eq!(r.code, 0, "{}", r.err);
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(without_time(written.clone()), without_time(doc), "{}", r.out);
    match json {
        true => assert_eq!(serde_json::from_str::<Value>(&r.out).unwrap()["report"]["document"], written),
        false => assert_eq!(r.out, format!("✓ wrote {}\n", file.display())),
    }
}

/// The report for drive `drive` with drawn sections and options, printed and as JSON, agrees.
#[hegel::test_helper]
fn report_agrees(tc: &TestCase, fx: &Env, drive: &str) {
    // A section the report doesn't have is a usage error that lists those it has.
    let r = fx.run(&["profile", drive, "-s", "tempo,nope"]);
    assert!(r.code == 2 && r.err.contains("no section \"nope\" (sections: summary, sets, "), "{}", r.err);
    let names: Vec<&str> = SECTIONS.iter().map(|(s, _)| *s).collect();
    let only: Vec<&str> = tc.draw(gs::subsequences(names.clone()));
    let mut args: Vec<String> = vec!["profile".into(), drive.into()];
    if !only.is_empty() {
        args.push("-s".into());
        args.push(only.join(","));
    }
    let min_tracks = tc.draw(gs::integers::<u32>().min_value(1).max_value(7));
    args.extend(["--min-tracks".into(), min_tracks.to_string()]);
    let list = tc.draw(gs::booleans());
    if list {
        args.push("--sets".into());
    }
    if tc.draw(gs::weighted_booleans(0.3)) {
        args.push("--no-analysis".into());
    }
    if tc.draw(gs::weighted_booleans(0.2)) {
        args.extend(["--since".into(), tc.draw(gs::sampled_from(&["2000", "2099-01"][..])).into()]);
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let v = fx.json(&args);
    let out = fx.ok(&args);
    let said = format!("{args:?}\n{out}\n{v}");

    let r = &v["report"];
    let basis = r["basis"].as_str().unwrap();
    let published: BTreeSet<&str> = v["published"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    let asked: Vec<&str> = if only.is_empty() { names } else { only };
    let expected: Vec<(String, bool)> = asked
        .iter()
        .map(|s| (title(s, basis).to_string(), published.contains(s) && !(basis == "sessions" && *s == "sets")))
        .collect();
    assert_eq!(headings(&out), expected, "{said}");

    let sets = &r["sets"];
    let count = sets["count"].as_u64().unwrap();
    let unit = if basis == "sessions" { "session" } else { "set" };
    let overall = match basis {
        "collection" => "no sets found: describing the collection".to_string(),
        "sessions" => format!(
            "no sets declared: {} in {}",
            plural(sets["plays"].as_u64().unwrap(), "play"),
            plural(count, "session")
        ),
        _ => format!("{} in {}", plural(sets["plays"].as_u64().unwrap(), "play"), plural(count, "set")),
    };
    assert!(out.contains(&overall), "{overall:?}: {said}");
    if asked.contains(&"summary") {
        let generated = r["summary"]["headline"].as_str().unwrap();
        let headline = v["prefs"]["headline"].as_str().unwrap_or(generated);
        assert!(out.lines().any(|l| l.trim() == headline), "{headline:?}: {said}");
    }
    if asked.contains(&"sets") && count > 0 {
        assert!(out.contains(&format!("  {} · ", plural(count, unit))), "{said}");
        for s in sets["list"].as_array().unwrap() {
            let id = &s["id"].as_str().unwrap()[..8.min(s["id"].as_str().unwrap().len())];
            assert_eq!(out.contains(id), list, "set {id} listed only with --sets: {said}");
        }
    }
    if asked.contains(&"signature") {
        for t in r["signature"]["tracks"].as_array().unwrap() {
            assert!(out.contains(t["name"].as_str().unwrap()), "{t}: {said}");
        }
        for pin in v["prefs"]["pinned"].as_array().unwrap() {
            assert!(out.contains(&format!("★ {}", pin.as_str().unwrap())), "{pin}: {said}");
        }
    }
    for (section, key) in [("genres", "genres"), ("labels", "labels"), ("artists", "artists")] {
        if !asked.contains(&section) {
            continue;
        }
        for x in r[key]["top"].as_array().unwrap() {
            let name = x["name"].as_str().unwrap();
            if name.chars().count() < 26 {
                assert!(out.contains(name), "{section} {name}: {said}");
            }
        }
    }
    for w in r["warnings"].as_array().unwrap() {
        assert!(out.contains(w.as_str().unwrap()), "{w}: {said}");
    }
}

/// What a model knows of the profile choices.
#[derive(Default, Debug)]
struct Prefs {
    sections: Option<BTreeSet<String>>,
    exact: bool,
    pinned: Vec<String>,
    hidden: Vec<String>,
    hidden_artists: Vec<String>,
    aliases: IndexMap<String, String>,
    headline: Option<String>,
}

/// Names compared as the profile compares them: case and runs of spaces don't count.
fn norm(s: &str) -> String {
    s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The one entry of `list` that `query` names: the one it spells, else the only one with it in.
fn named<'a>(list: &'a [String], query: &str) -> Option<&'a String> {
    let q = norm(query);
    list.iter().find(|e| norm(e) == q).or_else(|| {
        match list.iter().filter(|e| norm(e).contains(&q)).collect::<Vec<_>>()[..] {
            [one] => Some(one),
            _ => None,
        }
    })
}

impl Prefs {
    fn published(&self) -> BTreeSet<String> {
        self.sections.clone().unwrap_or_else(by_default)
    }

    /// As `profile prefs --json` has it.
    fn json(&self) -> Value {
        let mut v = serde_json::json!({
            "genre_aliases": self.aliases,
            "hidden": self.hidden,
            "hidden_artists": self.hidden_artists,
            "pinned": self.pinned,
            "precision": if self.exact { "exact" } else { "rounded" },
            "schema": 1,
        });
        if let Some(s) = &self.sections {
            // In the order the sections come in.
            let ordered: Vec<&str> = SECTIONS.iter().map(|(n, _)| *n).filter(|n| s.contains(*n)).collect();
            v["sections"] = serde_json::json!(ordered);
        }
        if let Some(h) = &self.headline {
            v["headline"] = h.clone().into();
        }
        v
    }
}

/// Track and artist names, some the same but for case and spacing.
const NAMES: [&str; 5] = ["Floorplan - Never Grow Old", "floorplan -  never grow old", "ID - ID", "Bootleg", "Ünïcödé"];

/// Choices made in any order, with `--json` or not: each succeeds or fails as the model says,
/// and afterwards `profile prefs` (as JSON, and as printed) and `profile sections` hold what the
/// model does.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn profile_choices_agree_with_a_model(tc: TestCase) {
    let fx = Env::from_template();
    let mut m = Prefs::default();
    let names: Vec<&str> = SECTIONS.iter().map(|(s, _)| *s).collect();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(8)) {
        let name = tc.draw(gs::sampled_from(&NAMES[..])).to_string();
        let step = tc.draw(gs::integers::<u8>().max_value(13));
        let mut ok = true;
        // Whether the step changes the sections, which its message then lists.
        let sections_changed = [0, 1, 2, 13].contains(&step);
        let words: Vec<String> = match step {
            0 | 1 => {
                let some: Vec<&str> = tc.draw(gs::subsequences(names.clone()));
                if some.is_empty() {
                    continue;
                }
                let mut s = m.published();
                for x in &some {
                    match step {
                        0 => s.insert(x.to_string()),
                        _ => s.remove(*x),
                    };
                }
                m.sections = Some(s);
                let verb = if step == 0 { "include" } else { "exclude" };
                vec![verb.into(), some.join(",")]
            }
            2 => {
                m.sections = None;
                vec!["default-sections".into()]
            }
            3 => {
                ok = !m.hidden.iter().any(|h| norm(h) == norm(&name));
                if ok && !m.pinned.iter().any(|p| norm(p) == norm(&name)) {
                    m.pinned.push(name.trim().to_string());
                }
                vec!["pin".into(), name, "--exact".into()]
            }
            4 => {
                let query = tc.draw(gs::sampled_from(&["grow", "ID", "boot", "o", "nothing like it"][..])).to_string();
                match named(&m.pinned, &query).cloned() {
                    Some(p) => m.pinned.retain(|x| norm(x) != norm(&p)),
                    None => ok = false,
                }
                vec!["unpin".into(), query]
            }
            5 => {
                if !m.hidden.iter().any(|h| norm(h) == norm(&name)) {
                    m.hidden.push(name.trim().to_string());
                }
                m.pinned.retain(|p| norm(p) != norm(&name));
                vec!["hide".into(), name, "--exact".into()]
            }
            6 => {
                if !m.hidden_artists.iter().any(|h| norm(h) == norm(&name)) {
                    m.hidden_artists.push(name.trim().to_string());
                }
                vec!["hide".into(), name, "--artist".into()]
            }
            7 | 8 => {
                let artist = step == 8;
                let query =
                    tc.draw(gs::sampled_from(&["grow", "id - id", "Bootleg", "o", "nothing like it"][..])).to_string();
                let list = if artist { &mut m.hidden_artists } else { &mut m.hidden };
                match named(list, &query).cloned() {
                    Some(h) => list.retain(|x| norm(x) != norm(&h)),
                    None => ok = false,
                }
                let mut w = vec!["unhide".to_string(), query];
                if artist {
                    w.push("--artist".into());
                }
                w
            }
            9 => {
                let from = tc.draw(gs::sampled_from(&["electronic", "Electronic ", "dnb"][..]));
                let to = tc.draw(gs::sampled_from(&["Electronica", "Drum & Bass"][..]));
                m.aliases.retain(|k, _| norm(k) != norm(from));
                m.aliases.insert(from.trim().to_string(), to.to_string());
                vec!["alias".into(), from.into(), to.into()]
            }
            10 => {
                let from = tc.draw(gs::sampled_from(&["electronic", "dnb"][..]));
                let before = m.aliases.len();
                m.aliases.retain(|k, _| norm(k) != norm(from));
                ok = m.aliases.len() < before;
                vec!["alias".into(), from.into()]
            }
            11 => {
                let text = tc.draw(gs::sampled_from(&["Techno warm-ups", "  ", ""][..]));
                m.headline = Some(text.trim().to_string()).filter(|t| !t.is_empty());
                let mut w = vec!["headline".to_string()];
                if !text.is_empty() {
                    w.push(text.into());
                }
                w
            }
            12 => {
                m.exact = tc.draw(gs::booleans());
                vec!["precision".into(), if m.exact { "exact" } else { "rounded" }.into()]
            }
            _ => {
                m = Prefs::default();
                vec!["reset".into()]
            }
        };
        let json = tc.draw(gs::booleans());
        let mut args = vec!["--yes".to_string()];
        if json {
            args.push("--json".into());
        }
        args.push("profile".into());
        args.extend(words);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let r = fx.run(&args);
        let said = format!("{args:?} exited {}\n{}\n{}\nmodel: {m:?}", r.code, r.out, r.err);
        assert_eq!(r.code == 0, ok, "{said}");
        if ok && !json && sections_changed {
            let listed: Vec<&str> = names.iter().copied().filter(|s| m.published().contains(*s)).collect();
            assert!(r.out.contains(&format!("the document includes: {}", listed.join(", "))), "{said}");
        }

        let prefs = fx.json(&["profile", "prefs"]);
        assert_eq!(prefs, m.json(), "{said}");
        let printed = fx.ok(&["profile", "prefs"]);
        let listed: Vec<&str> = names.iter().copied().filter(|s| m.published().contains(*s)).collect();
        let defaults = if m.sections.is_none() { " (the defaults)" } else { "" };
        assert!(printed.contains(&format!("sections: {}{defaults}\n", listed.join(", "))), "{printed}\n{said}");
        assert!(printed.contains(&format!("numbers: {}", if m.exact { "exact" } else { "rounded" })), "{printed}");
        assert_eq!(printed.contains("headline: "), m.headline.is_some(), "{printed}");
        for x in m.pinned.iter().chain(&m.hidden).chain(&m.hidden_artists) {
            assert!(printed.contains(&format!("    {x}\n")), "{x:?}: {printed}");
        }
        for (from, to) in &m.aliases {
            assert!(printed.contains(&format!("{from} → {to}")), "{printed}");
        }
        let sections = fx.json(&["profile", "sections"]);
        let on: BTreeSet<String> = sections
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["published"] == true)
            .map(|s| s["section"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(on, m.published(), "{sections}");
        let marks = fx.ok(&["--ascii", "profile", "sections"]);
        let marked = marks.lines().filter(|l| l.starts_with("  * ")).count();
        assert_eq!(marked, on.len(), "{marks}");
    }
}

/// Pinning or hiding a track by a word of its name (not `--exact`) picks the one played track it
/// names, from the sessions on the drive: refused when none or several do, and when one does, the
/// pin is that track's full name.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn a_word_picks_the_one_played_track_it_names(tc: TestCase) {
    let fx = Env::from_template();
    let club = fx.volumes.join("Club");
    let ids: Vec<u32> = (1..=12).collect();
    let played: Vec<u32> = tc.draw(gs::subsequences(ids));
    play(&club, "HISTORY 001", &played);
    let drive = path(&club).to_string();
    let report = fx.json(&["profile", &drive, "--no-analysis"]);
    let names: Vec<String> = report["report"]["signature"]["played"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();
    let words = [
        "in charge",
        "a1",
        "shuffle",
        "eyes",
        "nothing like it",
        "A2 - In Charge",
        "[a]pendics.shuffle - A1 - It's In The Eyes",
    ];
    let word = tc.draw(gs::sampled_from(&words[..]));
    let hide = tc.draw(gs::booleans());
    let verb = if hide { "hide" } else { "pin" };
    let r = fx.run(&["--yes", "--json", "profile", verb, word, &drive]);
    let q = norm(word);
    let picked = names.iter().find(|n| norm(n) == q).or_else(|| {
        match names.iter().filter(|n| norm(n).contains(&q)).collect::<Vec<_>>()[..] {
            [one] => Some(one),
            _ => None,
        }
    });
    let said = format!("{verb} {word:?} among {names:?}: {}\n{}", r.out, r.err);
    match picked {
        Some(name) => {
            assert_eq!(r.code, 0, "{said}");
            let list = if hide { "hidden" } else { "pinned" };
            assert_eq!(fx.json(&["profile", "prefs"])[list], serde_json::json!([name]), "{said}");
        }
        None => {
            assert_eq!(r.code, 1, "{said}");
            let message: Value = serde_json::from_str(&r.out).unwrap();
            let message = message["error"]["message"].as_str().unwrap();
            let many = names.iter().filter(|n| norm(n).contains(&q)).count();
            match many {
                0 => assert!(message.contains("--exact"), "{said}"),
                n => assert!(message.contains(&format!("matches {n} tracks")), "{said}"),
            }
        }
    }
}
