//! Property tests of the command line: random commands, with random arguments and global flags,
//! on a DJ's drive and its library, keep the exit status and output contract (`locallibrary
//! --help`: JSON that parses with its `schema` and an `outcome` that says the status; errors on
//! stderr), reads change no file, writes wait for `--yes`; and edits made through it agree with a
//! model of the playlists, the store's records and the drives' details.

use std::collections::BTreeSet;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, Run, path};

/// Placeholders in a command's words, filled in for each run: `@d` a drive (or a library, or
/// something that's neither), `@t` a track id, `@p` a playlist, `@f` a filter, `@s` an order,
/// `@o` a file on this computer that isn't there yet, `@img` an image on this computer, `@dir`
/// a folder on this computer.
#[hegel::test_helper]
fn fill(tc: &TestCase, fx: &Env, words: &[&str]) -> Vec<String> {
    let a = path(&fx.volumes.join("A")).to_string();
    let pick = |options: &[&str]| tc.draw(gs::sampled_from(options)).to_string();
    words
        .iter()
        .map(|w| match *w {
            "@d" => pick(&["A", "A", &a, "Club", "Nope"]),
            "@t" => pick(&["1", "2", "99"]),
            "@p" => pick(&["Set", "Set", "Nope"]),
            "@f" => pick(&["bpm:>=0", "key:8A~", "demo", "rating:>=4 -x", "bpm:"]),
            "@s" => pick(&["bpm", "-rating", "key,bpm", "nonsense"]),
            "@o" => path(&fx.home(&format!("out-{}", tc.draw(gs::integers::<u32>())))).to_string(),
            "@img" => path(&fx.home("cover.png")).to_string(),
            "@dir" => path(&fx.home("")).trim_end_matches('/').to_string(),
            w => w.to_string(),
        })
        .collect()
}

/// Global flags before the command: `--json` or not, and the ways human-readable output can look.
#[hegel::test_helper]
fn flags(tc: &TestCase) -> Vec<String> {
    let mut out = Vec::new();
    for flag in ["--json", "--ascii", "--quiet", "--no-pager"] {
        if tc.draw(gs::booleans()) {
            out.push(flag.to_string());
        }
    }
    if tc.draw(gs::booleans()) {
        out.push("--color".into());
        out.push(tc.draw(gs::sampled_from(&["never", "always", "auto"][..])).into());
    }
    out
}

/// The exit status an outcome stands for (see `locallibrary --help`).
fn status_of(outcome: &str, kind: &Value) -> i32 {
    match outcome {
        "applied" | "nothing_to_do" | "dry_run" => 0,
        "declined" | "not_confirmed" => 3,
        "problems" => 4,
        "partial" => 5,
        "cancelled" => 130,
        "error" if kind == "usage" => 2,
        "error" => 1,
        other => panic!("an outcome the help doesn't name: {other}"),
    }
}

/// What every run promises, whatever the command: an exit status the help names and no panic;
/// under `--json`, JSON on stdout whose objects carry `schema`, an `outcome` that says the exit
/// status, and an error's kind and message; otherwise an error on stderr, and nothing on stdout
/// for a command line that can't be read. The last JSON document, under `--json`.
fn keeps_the_contract(args: &[String], r: &Run) -> Option<Value> {
    let said = || format!("{args:?} exited {}\nstdout: {}\nstderr: {}", r.code, r.out, r.err);
    assert!(!r.err.contains("panicked at"), "{}", said());
    assert!([0, 1, 2, 3, 4, 5].contains(&r.code), "{}", said());
    // Help prints as it is, `--json` or not (errors don't).
    let raw = r.code == 0 && args.iter().any(|a| a == "--help");
    let json = args.iter().any(|a| a == "--json") && !raw;
    if !json {
        if [1, 2].contains(&r.code) {
            assert!(r.err.contains("error:"), "an error that doesn't say so: {}", said());
        }
        return None;
    }
    let docs: Vec<Value> = serde_json::Deserializer::from_str(&r.out)
        .into_iter::<Value>()
        .collect::<Result<_, _>>()
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", said()));
    let last = docs.last().cloned().unwrap_or_else(|| panic!("no JSON: {}", said()));
    for d in &docs {
        if d.is_object() {
            assert_eq!(d["schema"], 1, "{}", said());
        }
    }
    if let Some(outcome) = last.get("outcome").and_then(Value::as_str) {
        assert_eq!(r.code, status_of(outcome, &last["error"]["kind"]), "{}", said());
    }
    if [1, 2].contains(&r.code) {
        let e = &last["error"];
        assert!(e["kind"].is_string() && e["message"].as_str().is_some_and(|m| !m.is_empty()), "{}", said());
        assert_eq!(e["kind"], e["code"], "{}", said());
    } else {
        assert!(last.get("error").is_none(), "{}", said());
    }
    Some(last)
}

/// Runs `flags` and `words` (placeholders filled in) and checks the contract.
/// A run of `args`, words made for it.
fn run(fx: &Env, args: &[String]) -> Run {
    fx.run(&args.iter().map(String::as_str).collect::<Vec<_>>())
}

#[hegel::test_helper]
fn run_checked(tc: &TestCase, fx: &Env, flags: Vec<String>, words: &[&str]) -> (Vec<String>, Run, Option<Value>) {
    let mut args = flags;
    args.extend(fill(tc, fx, words));
    let r = run(fx, &args);
    let json = keeps_the_contract(&args, &r);
    (args, r, json)
}

/// Commands that only read: whatever they're given, they change nothing on the drives.
const READS: &[&[&str]] = &[
    &["status"],
    &["status", "--all"],
    &["drive"],
    &["drive", "ls"],
    &["drive", "ls", "--known"],
    &["drive", "info", "@d"],
    &["drive", "@d"],
    &["drive", "log", "@d"],
    &["drive", "groups"],
    &["drive", "space", "@d"],
    &["drive", "backups", "@d"],
    &["drive", "recordings", "@d"],
    &["drive", "verify", "@d"],
    &["drive", "verify", "@d", "--all"],
    &["library"],
    &["library", "ls"],
    &["library", "log", "@d"],
    &["plays", "@d"],
    &["plays", "lists", "@d"],
    &["plays", "lists", "@d", "--all"],
    &["plays", "stats", "@d"],
    &["sets"],
    &["sets", "list"],
    &["sets", "show", "last"],
    &["sets", "suggest"],
    &["tags", "@d"],
    &["tags", "list", "@d"],
    &["cues", "list", "@d", "--track", "@t"],
    &["art", "@d"],
    &["art", "missing", "@d"],
    &["edits", "list", "@d"],
    &["smart"],
    &["smart", "list"],
    &["settings"],
    &["settings", "schema"],
    &["settings", "show", "@d"],
    &["profile"],
    &["profile", "show"],
    &["profile", "prefs"],
    &["profile", "document", "@d"],
    &["account"],
    &["account", "status"],
    &["collection", "info"],
    &["players"],
    &["players", "models"],
    &["players", "show"],
    &["store"],
    &["store", "info"],
    &["store", "verify"],
    &["store", "copies"],
    &["tracks", "@d"],
    &["tracks", "@d", "-f", "@f", "--sort", "@s", "-n", "3"],
    &["tracks", "@d", "-p", "@p"],
    &["tracks", "@d", "-s", "demo"],
    &["playlist", "ls", "@d"],
    &["track", "show", "@d", "--track", "@t"],
    &["track", "show", "@d", "--filter", "@f"],
    &["track", "matches", "@d", "@t"],
    &["diff", "@d", "@d"],
    &["diff", "--summary", "@d", "@d"],
    &["check"],
    &["check", "@d"],
    &["doctor"],
    &["doctor", "@d"],
    &["doctor", "@d", "--only", "analysis"],
    &["undo", "--list"],
    &["undo", "@d", "--dry-run"],
    &["refresh", "@d", "--dry-run"],
    &["prep", "--dry-run", "--no-eject"],
    &["merge", "@d", "@d", "--dry-run"],
    &["clean", "@d", "--dry-run"],
    &["convert", "@d", "--to", "onelibrary", "--dry-run"],
    &["tags", "add", "@d", "Genre/Deep", "--track", "@t", "--dry-run"],
    &["playlist", "rename", "@d", "@p", "Renamed", "--dry-run"],
    &["playlist", "delete", "@d", "@p", "--dry-run"],
    &["drive", "contact", "@d", "dj@example.com", "--dry-run"],
    &["drive", "player", "@d", "--name", "DJ", "--dry-run"],
    &["drive", "rename", "@d", "Renamed", "--dry-run"],
    &["players", "set", "CDJ-3000", "--dry-run"],
    &["completions", "bash"],
    &["completions", "zsh", "--static"],
    &["completions", "fish"],
    &["profile", "sections"],
    &["profile", "default-sections"],
    &["settings", "base"],
    &["sets", "played", "demo"],
    &["store", "gc", "--dry-run"],
    &["store", "prune", "--keep-last", "1", "--dry-run"],
    &["library", "prune", "--keep-last", "1", "--dry-run"],
    &["store", "copy", "@o", "--dry-run"],
    &["drive", "tidy", "@d", "--dry-run"],
    &["drive", "recordings", "@d", "--dry-run"],
    &["drive", "reidentify", "@d", "--dry-run"],
    &["drive", "firmware", "@d", "--dry-run"],
    &["edits", "replay", "@d", "--dry-run"],
    &["edits", "clear", "@d", "--dry-run"],
    &["art", "auto", "@d", "--dry-run"],
    &["art", "unembed", "--list"],
    &["track", "rm", "@d", "--track", "@t", "--dry-run"],
    &["track", "dedupe", "@d", "--dry-run"],
    &["track", "relocate", "@d", "--look-in", "@o", "--dry-run"],
    &["playlist", "@d"],
    &["playlist", "sort", "@d", "@p", "--sort", "@s", "--dry-run"],
    &["playlist", "dedupe", "@d", "@p", "--dry-run"],
    &["playlist", "fill", "@d", "@p", "--filter", "@f", "--dry-run"],
    &["playlist", "unfiled", "@d", "--dry-run"],
    &["cues", "rm", "@d", "--track", "@t", "--all", "--dry-run"],
    &["cues", "copy", "@d", "--from", "1", "--to", "2", "--dry-run"],
    &["tags", "rename", "@d", "Genre/Deep", "Genre/Dark", "--dry-run"],
    &["tags", "delete", "@d", "Genre/Deep", "--dry-run"],
    &["convert", "@d", "--to", "device-library", "--dry-run"],
    &["pack", "@d", "Club", "--playlist", "@p", "--dry-run"],
    &["setup", "--library", "A", "--dry-run"],
    &["check", "--players", "CDJ-2000NXS2"],
    &["mix"],
    &["mix", "@o", "--drive", "@d"],
    &["sync", "--dry-run"],
    &["sync", "A", "Blank", "--dry-run"],
    &["sync", "A", "Club", "Blank", "--dry-run"],
    &["sync", "--from", "A", "Blank", "--dry-run", "--verify"],
    &["pack", "A", "Blank", "--playlist", "Set", "--dry-run"],
    &["art", "set", "@d", "@img", "--track", "@t", "--dry-run"],
    &["art", "embed", "@img", "@o", "--dry-run"],
    &["drive", "recordings", "@d", "--copy-to", "@o", "--dry-run"],
    &["settings", "import", "@d", "--dry-run"],
    &["track", "relocate", "@d", "--look-in", "@dir", "--dry-run"],
    &["track", "upgrade", "@d", "--from", "@dir", "--dry-run"],
    &["provision", "Club", "--library", "A", "--dry-run"],
    &["drive", "restore", "@d", "--dry-run"],
    // Refused while the machine's own disks are hidden (`LOCALLIBRARY_VOLUMES`).
    &["drive", "disks"],
    &["drive", "format", "@d", "--dry-run"],
    &["drive", "fsck", "@d"],
    &["doctor", "@d", "--hash-audio"],
    // These write a file on this computer, never to a drive.
    &["xml", "@d", "-o", "@o"],
    &["export", "@d", "@p", "--output", "@o"],
    &["plays", "export", "@d", "HISTORY 001", "-o", "@o"],
    &["art", "get", "@d", "@t", "-o", "@o"],
    &["sets", "export", "last"],
];

/// Reads, every one of them with any arguments and flags, keep the output contract and leave
/// every file on the drives as it was.
// One case runs every command in the list: each run tries other arguments, flags and order.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
fn reads_keep_the_contract_and_change_no_drive(tc: TestCase) {
    let fx = Env::from_template();
    let before = Env::files_in(&fx.volumes);
    for words in tc.draw(gs::permutations(READS)) {
        let flags = flags(&tc);
        let (args, r, _) = run_checked(&tc, &fx, flags, words);
        assert!(Env::files_in(&fx.volumes) == before, "{args:?} changed a drive\n{}\n{}", r.out, r.err);
    }
}

/// Commands that change a drive or the store, and ask before they do.
const WRITES: &[&[&str]] = &[
    &["playlist", "new", "@d", "Friday"],
    &["playlist", "rename", "@d", "@p", "Renamed"],
    &["playlist", "delete", "@d", "@p"],
    &["playlist", "add", "@d", "@p", "--track", "@t"],
    &["playlist", "remove", "@d", "@p", "--track", "@t"],
    &["playlist", "move", "@d", "@p", "--top"],
    &["track", "set", "@d", "--track", "@t", "--rating", "3"],
    &["tags", "add", "@d", "Genre/Deep", "--track", "@t"],
    &["tags", "untag", "@d", "Genre/Deep", "--track", "@t"],
    &["drive", "role", "@d", "backup"],
    &["drive", "contact", "@d", "dj@example.com"],
    &["drive", "player", "@d", "--name", "DJ"],
    &["drive", "rename", "@d", "Renamed"],
    &["drive", "retire", "@d"],
    &["drive", "forget", "@d"],
    &["sync", "@d", "@d"],
    &["sync", "--from", "@d", "@d"],
    &["merge", "@d", "@d"],
    &["clean", "@d"],
    &["convert", "@d", "--to", "onelibrary"],
    &["undo", "@d"],
    &["edits", "clear", "@d"],
    &["smart", "apply", "@d"],
    &["settings", "apply", "@d"],
    &["settings", "set", "quantize=off"],
    &["settings", "unset", "quantize"],
    &["players", "set", "CDJ-3000"],
    &["players", "clear"],
    &["library", "rename", "A", "Home"],
    &["profile", "reset"],
];

/// Without `--yes`, a command that asks first (with no terminal to ask on) writes nothing: it
/// says it wasn't confirmed (status 3), that there's nothing to do, or why it can't, and the
/// drives and the store are as they were.
// One case runs every command in the list: each run tries other arguments, flags and order.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
fn writes_wait_for_yes(tc: TestCase) {
    let fx = Env::from_template();
    let before = fx.files();
    for words in tc.draw(gs::permutations(WRITES)) {
        let flags = flags(&tc);
        let (args, r, json) = run_checked(&tc, &fx, flags, words);
        assert!([0, 1, 2, 3].contains(&r.code), "{args:?}\n{}\n{}", r.out, r.err);
        if let Some(v) = json {
            let outcome = v["outcome"].as_str().unwrap_or_else(|| panic!("{args:?}: no outcome in {v}"));
            assert!(["not_confirmed", "nothing_to_do", "error"].contains(&outcome), "{args:?}: {v}");
            assert_ne!(v["applied"], true, "{args:?}: {v}");
        }
        assert!(fx.files() == before, "{args:?} wrote without --yes\n{}\n{}", r.out, r.err);
    }
}

/// Commands that change a drive or the store (with `--yes`), the ones above and those that
/// don't ask.
const CHANGES: &[&[&str]] = &[
    &["pull", "@d"],
    &["pull", "Club", "--library", "Club", "--new-library"],
    &["watch", "--once", "--settle", "0"],
    &["refresh", "@d"],
    &["prep", "--no-eject"],
    &["store", "gc"],
    &["store", "prune", "--keep-last", "1"],
    &["library", "prune", "--keep-last", "1"],
    &["store", "copy", "@o"],
    &["store", "label", "A", "before"],
    &["store", "unlabel", "before"],
    &["cues", "set", "@d", "--track", "@t", "--hot", "B", "--at", "1.5"],
    &["cues", "set", "@d", "--track", "@t", "--memory", "--beat", "2", "--loop", "4"],
    &["cues", "rm", "@d", "--track", "@t", "--all"],
    &["cues", "copy", "@d", "--from", "1", "--to", "2"],
    &["tags", "rename", "@d", "Genre/Deep", "Genre/Dark"],
    &["tags", "delete", "@d", "Genre/Deep"],
    &["track", "rm", "@d", "--track", "@t"],
    &["track", "dedupe", "@d"],
    &["playlist", "new", "@d", "Folder", "--folder"],
    &["playlist", "sort", "@d", "@p", "--sort", "@s"],
    &["playlist", "dedupe", "@d", "@p"],
    &["playlist", "fill", "@d", "@p", "--filter", "@f"],
    &["playlist", "unfiled", "@d"],
    &["drive", "tidy", "@d"],
    &["drive", "retire", "@d", "--undo"],
    &["drive", "recognise", "@d"],
    &["drive", "reidentify", "@d", "--force"],
    &["edits", "replay", "@d"],
    &["art", "auto", "@d"],
    &["sets", "new", "--from", "HISTORY 001", "--title", "Friday"],
    &["sets", "edit", "last", "--title", "Saturday"],
    &["sets", "playlist", "last", "@d"],
    &["sets", "delete", "last"],
    &["profile", "include", "signature"],
    &["profile", "exclude", "signature"],
    &["profile", "pin", "Demo Track 1", "--exact"],
    &["profile", "unpin", "Demo Track 1"],
    &["profile", "hide", "Loopmasters", "--artist", "--exact"],
    &["profile", "unhide", "Loopmasters", "--artist"],
    &["profile", "alias", "electronic", "Electronica"],
    &["profile", "precision", "exact"],
    &["profile", "headline", "Warm-ups"],
    &["smart", "add", "Smart/All", "--filter", "bpm:>=0"],
    &["pack", "@d", "Club", "--playlist", "Set"],
    &["drive", "adopt", "Club", "--name", "Club"],
    &["account", "logout"],
    &["provision", "Club", "--library", "A"],
    &["pull"],
    &["sync"],
    &["sync", "-l", "A"],
    &["sync", "A", "Club", "Blank"],
    &["sync", "A", "Blank", "--verify", "--keep-extra"],
    &["art", "set", "@d", "@img", "--track", "@t"],
    &["art", "unembed"],
    &["settings", "import", "@d"],
    &["drive", "restore", "@d"],
    // Never with `--watch`: that installs a login item.
    &["setup", "@d", "--library", "A", "--players", "CDJ-3000"],
];

/// With `--yes`, every change (in any order, with any arguments and flags) keeps the output
/// contract: applied or not as its outcome says. Whatever they did, drive A's library still
/// reads, and `doctor` still runs on it.
// One case runs every command in the list: each run tries other arguments, flags and order.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
fn changes_keep_the_contract_and_the_library_readable(tc: TestCase) {
    let fx = Env::from_template();
    let all: Vec<&[&str]> = WRITES.iter().chain(CHANGES).copied().collect();
    for words in tc.draw(gs::permutations(all)) {
        let mut flags = vec!["--yes".to_string()];
        flags.extend(self::flags(&tc));
        let (args, r, json) = run_checked(&tc, &fx, flags, words);
        if let Some(v) = json
            && let Some(applied) = v.get("applied")
        {
            // Applied, a check among its steps may still find problems (`prep`).
            let outcomes: &[&str] = if applied == true {
                &["applied", "partial", "problems"]
            } else {
                &["nothing_to_do", "error", "problems", "dry_run"]
            };
            assert!(outcomes.iter().any(|o| v["outcome"] == *o), "{args:?}: {v}");
        }
        assert_ne!(r.code, 3, "{args:?} asked, with --yes\n{}\n{}", r.out, r.err);
    }
    for words in [&["tracks", "A"][..], &["playlist", "ls", "A"], &["status"]] {
        let (args, r, _) = run_checked(&tc, &fx, vec!["--json".into()], words);
        // Renamed or forgotten, A may not be found by that name any more.
        assert!(
            r.code == 0 || r.err.contains("not_found") || r.out.contains("not_found"),
            "{args:?}\n{}\n{}",
            r.out,
            r.err
        );
    }
    let (args, r, _) = run_checked(&tc, &fx, Vec::new(), &["doctor", path(&fx.volumes.join("A"))]);
    assert!([0, 4].contains(&r.code), "{args:?}\n{}\n{}", r.out, r.err);
}

/// The top-level playlists on drive A, as `playlist ls --json` lists them.
fn playlists(fx: &Env) -> BTreeSet<String> {
    let r = fx.run(&["--json", "playlist", "ls", "A"]);
    let v: Value = serde_json::from_str(&r.out).unwrap_or_else(|e| panic!("{e}: {}\n{}", r.out, r.err));
    let list = v.as_array().unwrap_or_else(|| panic!("{v}"));
    list.iter().filter(|p| p["parent_id"].is_null()).map(|p| p["name"].as_str().unwrap().to_string()).collect()
}

/// Playlists made, renamed and deleted on a drive (with `--yes`, or as a dry run), and the
/// changes undone, agree with a model: a dry run changes nothing, and undo puts back the
/// playlists as they were before the last change it hasn't undone.
#[hegel::test(test_cases = 5)]
fn playlist_edits_and_undo_agree_with_a_model(tc: TestCase) {
    let fx = Env::from_template();
    let mut model = playlists(&fx);
    // What each change replaced, newest last: what undo puts back.
    let mut undo: Vec<BTreeSet<String>> = Vec::new();
    let names = ["Set", "Friday", "Warm-ups", "Ünïcödé"];
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let dry_run = tc.draw(gs::weighted_booleans(0.2));
        let a = tc.draw(gs::sampled_from(&names[..]));
        let b = tc.draw(gs::sampled_from(&names[..]));
        let op = tc.draw(gs::integers::<u8>().max_value(3));
        let mut args: Vec<&str> = match op {
            0 => vec!["playlist", "new", "A", a],
            1 => vec!["playlist", "rename", "A", a, b],
            2 => vec!["playlist", "delete", "A", a],
            _ => vec!["undo", "A"],
        };
        // `playlist new` has no dry run.
        let dry_run = dry_run && op != 0;
        if dry_run {
            args.push("--dry-run");
        }
        let mut flags = vec!["--yes".to_string()];
        flags.extend(self::flags(&tc));
        let (args, r, _) = run_checked(&tc, &fx, flags, &args);
        let said = format!("{args:?}\n{}\n{}", r.out, r.err);
        // What applying it would leave, when it changes anything.
        let changed = match op {
            0 => (!model.contains(a)).then(|| model.iter().cloned().chain([a.to_string()]).collect()),
            1 => (model.contains(a) && !model.contains(b))
                .then(|| model.iter().filter(|p| *p != a).cloned().chain([b.to_string()]).collect::<BTreeSet<_>>()),
            2 => model.contains(a).then(|| model.iter().filter(|p| *p != a).cloned().collect()),
            _ => undo.last().cloned(),
        };
        match (&changed, dry_run) {
            (Some(_), true) => assert_eq!(r.code, 0, "{said}"),
            (Some(after), false) => {
                assert_eq!(r.code, 0, "{said}");
                if op == 3 {
                    undo.pop();
                } else {
                    undo.push(model.clone());
                }
                model = after.clone();
            }
            // Nothing to do (making one that's there, renaming one to itself), or refused.
            (None, _) => assert!([0, 1].contains(&r.code), "{said}"),
        }
        assert_eq!(playlists(&fx), model, "after {args:?}\n{}\n{}", r.out, r.err);
    }
}

/// The `--json` document a run that succeeds prints.
fn json_of(fx: &Env, words: &[&str]) -> Value {
    let mut args = vec!["--json".to_string()];
    args.extend(words.iter().map(|w| w.to_string()));
    let r = run(fx, &args);
    assert_eq!(r.code, 0, "{args:?}\n{}\n{}", r.out, r.err);
    serde_json::from_str(&r.out).unwrap()
}

/// What the model knows of the store and drive A's records.
struct Records {
    smart: BTreeSet<String>,
    quantize: Option<&'static str>,
    players: Vec<&'static str>,
    library: &'static str,
    role: Option<&'static str>,
    name: &'static str,
}

/// Rule-based playlists, My Settings, the players, the library's name and drive A's role and
/// name, changed through the command line in any order, agree with a model after every step.
#[hegel::test(test_cases = 4)]
fn store_and_drive_records_agree_with_a_model(tc: TestCase) {
    let fx = Env::from_template();
    let mut m =
        Records { smart: BTreeSet::new(), quantize: None, players: Vec::new(), library: "A", role: None, name: "A" };
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let mut flags = vec!["--yes".to_string()];
        flags.extend(self::flags(&tc));
        let step = tc.draw(gs::integers::<u8>().max_value(6));
        let (args, r, _) = match step {
            0 => {
                let p = tc.draw(gs::sampled_from(&["Smart/Deep", "Top"][..]));
                m.smart.insert(p.into());
                run_checked(&tc, &fx, flags, &["smart", "add", p, "--filter", "bpm:>=120"])
            }
            1 => {
                let p = tc.draw(gs::sampled_from(&["Smart/Deep", "Top"][..]));
                let had = m.smart.remove(p);
                let run = run_checked(&tc, &fx, flags, &["smart", "rm", p]);
                assert_eq!(run.1.code == 0, had, "{:?}\n{}", run.0, run.1.err);
                run
            }
            2 => {
                let v = tc.draw(gs::sampled_from(&["on", "off"][..]));
                m.quantize = Some(v);
                run_checked(&tc, &fx, flags, &["settings", "set", &format!("quantize={v}")])
            }
            3 => {
                m.quantize = None;
                run_checked(&tc, &fx, flags, &["settings", "unset", "quantize"])
            }
            4 => {
                let models = tc.draw(gs::sampled_from(&["CDJ-3000", "XDJ-AZ,CDJ-3000"][..]));
                m.players = models.split(',').collect();
                run_checked(&tc, &fx, flags, &["players", "set", models])
            }
            5 => {
                let name = tc.draw(gs::sampled_from(&["A", "Home"][..]));
                let from = std::mem::replace(&mut m.library, name);
                run_checked(&tc, &fx, flags, &["library", "rename", from, name])
            }
            _ => {
                let role = tc.draw(gs::sampled_from(&["primary", "backup", "archive"][..]));
                m.role = Some(role);
                let name = tc.draw(gs::sampled_from(&["A", "Main"][..]));
                let run = run_checked(&tc, &fx, flags.clone(), &["drive", "role", m.name, role]);
                assert_eq!(run.1.code, 0, "{:?}\n{}", run.0, run.1.err);
                let run = run_checked(&tc, &fx, flags, &["drive", "rename", m.name, name]);
                // An archive drive is kept as it is.
                match role {
                    "archive" => assert_eq!(run.1.code, 1, "{:?}\n{}", run.0, run.1.err),
                    _ => m.name = name,
                }
                run
            }
        };
        assert!([0, 1].contains(&r.code), "{args:?}\n{}\n{}", r.out, r.err);
        if step != 1 && m.role != Some("archive") {
            assert_eq!(r.code, 0, "{args:?}\n{}\n{}", r.out, r.err);
        }
        let smart = json_of(&fx, &["smart", "list"]);
        let paths: BTreeSet<String> =
            smart["playlists"].as_array().unwrap().iter().map(|p| p["path"].as_str().unwrap().to_string()).collect();
        assert_eq!(paths, m.smart, "{smart}");
        let profile = json_of(&fx, &["settings"]);
        assert_eq!(profile["base"]["quantize"].as_str(), m.quantize, "{profile}");
        let players = json_of(&fx, &["players"]);
        assert_eq!(players["default"], serde_json::json!(m.players), "{players}");
        let libraries = json_of(&fx, &["library", "ls"]);
        let names: Vec<&str> = libraries.as_array().unwrap().iter().filter_map(|l| l["name"].as_str()).collect();
        assert_eq!(names, [m.library], "{libraries}");
        let log = json_of(&fx, &["drive", "log", m.name]);
        assert_eq!((log["name"].as_str(), log["role"].as_str()), (Some(m.name), m.role), "{log}");
    }
}

/// The names `locallibrary WORDS --help` lists as its commands (none for a command without
/// subcommands): lines of two spaces and a name, under a heading.
fn commands_in_help(help: &str) -> Vec<String> {
    let body = help.split("\nOptions:").next().unwrap_or(help);
    body.lines()
        .filter_map(|l| l.strip_prefix("  "))
        .filter(|l| l.starts_with(|c: char| c.is_ascii_lowercase()))
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// What Tab offers after `words` (`COMPLETE=fish`).
fn completions(fx: &Env, words: &[&str]) -> Vec<String> {
    let out = fx.command().env("COMPLETE", "fish").arg("--").arg("locallibrary").args(words).arg("").output().unwrap();
    String::from_utf8(out.stdout).unwrap().lines().map(|l| l.split('\t').next().unwrap().to_string()).collect()
}

/// For any command: `--help` prints its usage and succeeds, Tab offers every command it lists,
/// an option it doesn't have is a usage error (status 2, on stderr, or as JSON with kind
/// `usage`), and a mistyped command is one too, whose "did you mean" names only real commands.
#[hegel::test(test_cases = 8)]
fn every_command_has_help_and_usage_errors(tc: TestCase) {
    let fx = Env::from_template();
    let top = fx.run(&["--help"]);
    assert_eq!(top.code, 0, "{}", top.err);
    let top_commands = commands_in_help(&top.out);
    assert!(top_commands.len() > 20, "{top_commands:?}");
    // `help` takes commands, not options.
    let groups: Vec<&String> = top_commands.iter().filter(|c| *c != "help").collect();
    let group = tc.draw(gs::sampled_from(&groups[..])).to_string();
    let r = fx.run(&[group.as_str(), "--help"]);
    assert_eq!(r.code, 0, "{group} --help: {}", r.err);
    assert!(r.out.contains(&format!("Usage: locallibrary {group}")), "{group} --help:\n{}", r.out);
    let sub = commands_in_help(&r.out);
    let r_help = r.out.clone();
    let offered = completions(&fx, &[&group]);
    for c in sub.iter().filter(|c| *c != "help") {
        // Tab may offer a command by the alias its help shows (`ls ... [alias: list]`, maybe on
        // a line of its description's below).
        let mut entry = String::new();
        let mut current = false;
        for l in r.out.lines() {
            if let Some(name) =
                l.strip_prefix("  ").filter(|l| !l.starts_with(' ')).and_then(|l| l.split_whitespace().next())
            {
                current = name == c;
            }
            if current {
                entry.push_str(l);
            }
        }
        let aliases = entry.split("[alias").nth(1).and_then(|a| a.split(": ").nth(1)).unwrap_or("");
        let aliases = aliases.split(']').next().unwrap_or("");
        let names: Vec<&str> =
            std::iter::once(c.as_str()).chain(aliases.split(", ").filter(|a| !a.is_empty())).collect();
        assert!(
            names.iter().any(|n| offered.iter().any(|o| o == n)),
            "Tab after {group} offers none of {names:?}: {offered:?}"
        );
    }

    let json = tc.draw(gs::booleans());
    let mut args: Vec<String> = json.then(|| "--json".to_string()).into_iter().collect();
    args.push(group.clone());
    let typo_of = |word: &str| -> String {
        let mut chars: Vec<char> = word.chars().collect();
        let i = tc.draw(gs::integers::<usize>().max_value(chars.len() - 1));
        chars[i] = tc.draw(gs::sampled_from(&['q', 'x', 'z'][..]));
        chars.into_iter().collect()
    };
    let wrong = match (sub.iter().filter(|c| *c != "help").count(), tc.draw(gs::booleans())) {
        (n, true) if n > 0 => {
            let known: Vec<&String> = sub.iter().filter(|c| *c != "help").collect();
            let typo = typo_of(tc.draw(gs::sampled_from(&known[..])));
            // A slip that spells another command (or a drive) isn't one.
            if sub.contains(&typo) || ["A", "Club", "Blank"].contains(&typo.as_str()) {
                return;
            }
            typo
        }
        _ => format!("--{}", typo_of("nonsense")),
    };
    args.push(wrong.clone());
    let r = run(&fx, &args);
    keeps_the_contract(&args, &r);
    if wrong.starts_with("--") {
        // An option the command doesn't have: refused before it runs, and only said on stderr.
        assert_eq!(r.code, 2, "{args:?}\n{}\n{}", r.out, r.err);
        if !json {
            assert!(r.out.is_empty(), "{args:?}: {}", r.out);
        }
    }
    if r.code == 0 {
        // A group that takes a drive took it for one, and said it isn't there; or took the word.
        return;
    }
    let said = if json {
        serde_json::from_str::<Value>(&r.out).unwrap()["error"]["message"].as_str().unwrap().to_string()
    } else {
        r.err.clone()
    };
    assert!([1, 2].contains(&r.code), "{args:?}\n{said}");
    let mut words: BTreeSet<String> = top_commands.iter().cloned().collect();
    words.extend(sub.iter().cloned());
    // And the aliases the help shows: `[alias: ls]`, `[aliases: a, b]`, wrapped onto the next line
    // after a long description.
    for part in r_help.split("[alias").skip(1) {
        let names = part.split_once(':').map_or("", |(_, n)| n).split(']').next().unwrap_or("");
        words.extend(names.split(',').map(|name| name.trim().to_string()));
    }
    // Suggestions are quoted 'like this' (clap) or `locallibrary like this`.
    let quoted: Vec<&str> = said.split(['\'', '`']).skip(1).step_by(2).collect();
    for q in quoted {
        let q = q.trim_start_matches("locallibrary ");
        // The word typed, and how to pass it as a value or ask for help, aren't suggestions.
        if q == wrong || q.contains("--") {
            continue;
        }
        for w in q.split(' ').filter(|w| !w.starts_with('-')) {
            assert!(words.contains(w), "{args:?} suggests {q:?}, and {w:?} isn't a command:\n{said}");
        }
    }
}

/// Commands that write a file on this computer: they write it where they're told, refuse to
/// replace one that's there without being asked (keeping it as it was), and replace it when
/// they are.
const EXPORTS: &[&[&str]] = &[
    &["xml", "@d", "-o", "@o"],
    &["export", "@d", "@p", "--output", "@o"],
    &["plays", "export", "@d", "HISTORY 001", "-o", "@o"],
    &["profile", "document", "@d", "-o", "@o"],
];

/// Any of them, with any flags, does as said above.
#[hegel::test(test_cases = 6)]
fn exports_write_where_they_are_told(tc: TestCase) {
    let fx = Env::from_template();
    let words = tc.draw(gs::sampled_from(EXPORTS));
    let flags = flags(&tc);
    let (args, r, _) = run_checked(&tc, &fx, flags.clone(), words);
    let out = Path::new(&args[args.iter().position(|a| a == "-o" || a == "--output").unwrap() + 1]).to_path_buf();
    if r.code != 0 {
        assert!(!out.exists(), "{args:?} failed but wrote {out:?}");
        return;
    }
    let written = std::fs::read(&out).unwrap_or_else(|e| panic!("{args:?}: {e}\n{}", r.out));
    assert!(!written.is_empty(), "{args:?} wrote nothing");
    std::fs::write(&out, b"mine").unwrap();
    let again = run(&fx, &args);
    keeps_the_contract(&args, &again);
    match again.code {
        0 => assert_ne!(std::fs::read(&out).unwrap(), b"mine", "{args:?} said it wrote, and didn't"),
        _ => assert_eq!(std::fs::read(&out).unwrap(), b"mine", "{args:?} failed, and replaced the file"),
    }
    let mut replace = args.clone();
    replace.push("--replace".into());
    let r = run(&fx, &replace);
    keeps_the_contract(&replace, &r);
    if r.code == 0 {
        // Some say when they were made: the same length is as close as a second run comes.
        let rewritten = std::fs::read(&out).unwrap();
        assert_eq!(rewritten.len(), written.len(), "{replace:?} wrote something else");
    }
}

/// Publishing to a pretend data server: a dry run, or a run without `--yes`, writes no record;
/// with it, the record written is the one the plan showed, and publishing again what's
/// published already writes nothing. The My Settings published are the ones set when they were.
#[cfg(feature = "account")]
#[hegel::test(test_cases = 4)]
fn published_records_are_what_was_planned(tc: TestCase) {
    use crate::mock_pds::MockPds;
    use crate::support::account::{DID, PUBLISHING, sign_in};
    let pds = MockPds::start();
    let fx = Env::from_template();
    sign_in(&fx.store, &pds.url, &PUBLISHING, serde_json::json!({}));
    // Records written: the server turns away the first write of a session to hand out a DPoP
    // nonce, and accepts the ones that carry it.
    let writes = || {
        let calls = pds.calls("com.atproto.repo.putRecord");
        calls
            .iter()
            .filter(|c| c.dpop().is_some_and(|(_, claims)| claims["nonce"].as_str().is_some_and(|n| !n.is_empty())))
            .count()
    };
    let mut quantize = None;
    let mut published_quantize = None;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let step = tc.draw(gs::integers::<u8>().max_value(3));
        if step == 0 {
            let v = tc.draw(gs::sampled_from(&["on", "off"][..]));
            let (args, r, _) =
                run_checked(&tc, &fx, vec!["--yes".into()], &["settings", "set", &format!("quantize={v}")]);
            assert_eq!(r.code, 0, "{args:?}\n{}", r.err);
            quantize = Some(v);
            continue;
        }
        let words: &[&str] = match step {
            1 => &["settings", "publish"],
            2 => &["profile", "publish", "A"],
            _ => &["sets", "publish", "last"],
        };
        if step == 3 {
            // A set to publish: the session a player recorded.
            run_checked(&tc, &fx, vec!["--yes".into()], &["sets", "new", "--from", "HISTORY 001", "--title", "Friday"]);
        }
        let how = tc.draw(gs::integers::<u8>().max_value(2));
        let mut flags = self::flags(&tc);
        let mut words = words.to_vec();
        match how {
            0 => words.push("--dry-run"),
            1 => {}
            _ => flags.push("--yes".into()),
        }
        let before = writes();
        let (args, r, json) = run_checked(&tc, &fx, flags, &words);
        // There's nothing to publish until a setting is set.
        if step == 1 && quantize.is_none() {
            assert_eq!(writes(), before, "{args:?} wrote a record\n{}\n{}", r.out, r.err);
            assert!([0, 1, 3].contains(&r.code), "{args:?}\n{}\n{}", r.out, r.err);
            continue;
        }
        if how < 2 {
            assert_eq!(writes(), before, "{args:?} wrote a record\n{}\n{}", r.out, r.err);
            assert!([0, 1, 3].contains(&r.code), "{args:?}\n{}\n{}", r.out, r.err);
            continue;
        }
        assert_eq!(r.code, 0, "{args:?}\n{}\n{}", r.out, r.err);
        if step == 1 {
            published_quantize = quantize;
        }
        if let Some(v) = json
            && v["outcome"] == "applied"
        {
            let uri = v["report"]["uri"].as_str().unwrap_or_else(|| panic!("{v}"));
            let (repo, rest) = uri.strip_prefix("at://").unwrap().split_once('/').unwrap();
            let (collection, rkey) = rest.split_once('/').unwrap();
            assert_eq!(repo, DID);
            let (record, _) = pds.record(repo, collection, rkey).unwrap_or_else(|| panic!("{uri} isn't there"));
            if v["report"]["written"] == true {
                assert_eq!(writes(), before + 1, "{args:?}");
                for (k, planned) in v["plan"]["record"].as_object().into_iter().flatten() {
                    // Times the server or the write set may differ from the plan's.
                    if !k.ends_with("At") {
                        assert_eq!(&record[k], planned, "{args:?}: {k}");
                    }
                }
            } else {
                assert_eq!(writes(), before, "{args:?}: unchanged, yet written");
            }
        }
        // Publishing again what was just published changes nothing.
        let mut again = vec!["--yes".to_string()];
        again.extend(words.iter().map(|w| w.to_string()));
        let before = writes();
        let r = run(&fx, &again);
        keeps_the_contract(&again, &r);
        assert_eq!(writes(), before, "{again:?} wrote what's published already\n{}", r.out);
    }
    if let Some((record, _)) = pds.record(DID, "computer.localnetwork.locallibrary.settings", "self") {
        assert_eq!(record["player"]["quantize"].as_str(), published_quantize, "{record}");
    }
}

/// rekordbox's collection (a `master.db` with the tables and columns rekordbox 7 has, as plain
/// SQLite) of random tracks, some deleted, and a playlist of some of them in a folder, read back
/// through `collection`: the tracks that aren't deleted, each once, filtered, sorted and
/// searched as asked, the playlist's in its order, and the counts that sum them up.
#[hegel::test(test_cases = 4)]
fn the_collection_reads_back_as_written(tc: TestCase) {
    let fx = Env::from_template();
    let db = fx.home("master.db");
    let titles = ["Demo Track 1", "Never Grow Old", "Sanctified", "Bootleg", "Ünïcödé"];
    let n = tc.draw(gs::integers::<usize>().max_value(6));
    // (id, title, artist, bpm × 100, deleted)
    let mut tracks = Vec::new();
    for id in 1..=n {
        let title = tc.draw(gs::sampled_from(&titles[..]));
        let artist = tc.draw(gs::sampled_from(&["1", "2"][..]));
        let bpm = tc.draw(gs::integers::<u32>().min_value(9000).max_value(15000));
        tracks.push((id, title, artist, bpm, tc.draw(gs::weighted_booleans(0.2))));
    }
    let live: Vec<_> = tracks.iter().filter(|t| !t.4).collect();
    let some: Vec<usize> = tc.draw(gs::subsequences(live.iter().map(|t| t.0).collect::<Vec<_>>()));
    let entries: Vec<usize> = tc.draw(gs::permutations(some));
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch(
        "CREATE TABLE djmdProperty (DBID TEXT, DBVersion TEXT);
         INSERT INTO djmdProperty VALUES ('1234567890', '7000');
         CREATE TABLE djmdArtist (ID TEXT PRIMARY KEY, Name TEXT);
         INSERT INTO djmdArtist VALUES ('1', 'Loopmasters'), ('2', 'Floorplan');
         CREATE TABLE djmdContent (ID TEXT PRIMARY KEY, FolderPath TEXT, FileNameL TEXT, Title TEXT,
             ArtistID TEXT, BPM INTEGER, Length INTEGER, FileSize INTEGER, rb_local_deleted INTEGER DEFAULT 0);
         CREATE TABLE djmdPlaylist (ID TEXT PRIMARY KEY, Seq INTEGER, Name TEXT, Attribute INTEGER, ParentID TEXT);
         INSERT INTO djmdPlaylist VALUES ('10', 1, 'Shows', 1, 'root'), ('11', 1, 'Friday', 0, '10');
         CREATE TABLE djmdSongPlaylist (ID TEXT PRIMARY KEY, PlaylistID TEXT, ContentID TEXT, TrackNo INTEGER);",
    )
    .unwrap();
    for (id, title, artist, bpm, deleted) in &tracks {
        conn.execute(
            "INSERT INTO djmdContent VALUES (?1, ?2, ?3, ?4, ?5, ?6, 300, 1000, ?7)",
            (
                id.to_string(),
                format!("/Users/dj/Music/{id}.mp3"),
                format!("{id}.mp3"),
                title,
                artist,
                bpm,
                *deleted as i32,
            ),
        )
        .unwrap();
    }
    for (no, id) in entries.iter().enumerate() {
        conn.execute(
            "INSERT INTO djmdSongPlaylist VALUES (?1, '11', ?2, ?3)",
            (format!("e{no}"), id.to_string(), no as i64 + 1),
        )
        .unwrap();
    }
    drop(conn);
    let db = path(&db).to_string();
    let read = |args: &[&str]| -> Value {
        let mut all = vec!["--json", "collection", "--db", &db];
        all.extend(args);
        let all: Vec<String> = all.into_iter().map(String::from).collect();
        let r = run(&fx, &all);
        assert_eq!(r.code, 0, "{all:?}\n{}\n{}", r.out, r.err);
        keeps_the_contract(&all, &r).unwrap()
    };
    let ids = |v: &Value| -> Vec<usize> {
        v.as_array().unwrap().iter().map(|t| t["id"].as_u64().unwrap() as usize).collect()
    };

    let info = read(&[]);
    assert_eq!(info["summary"]["tracks"], live.len(), "{info}");
    assert_eq!((&info["summary"]["playlists"], &info["summary"]["folders"]), (&1.into(), &1.into()), "{info}");
    let all = read(&["tracks"]);
    let mut listed = ids(&all);
    listed.sort_unstable();
    assert_eq!(listed, live.iter().map(|t| t.0).collect::<Vec<_>>(), "deleted tracks aren't listed: {all}");

    let over = tc.draw(gs::integers::<u32>().min_value(90).max_value(150));
    let filtered = read(&["tracks", "--filter", &format!("bpm:>{over}"), "--sort", "-bpm"]);
    let bpms: Vec<f64> = filtered.as_array().unwrap().iter().map(|t| t["bpm"].as_f64().unwrap()).collect();
    assert!(bpms.iter().all(|b| *b > over as f64), "{over}: {bpms:?}");
    assert!(bpms.windows(2).all(|w| w[0] >= w[1]), "highest first: {bpms:?}");
    assert_eq!(bpms.len(), live.iter().filter(|t| f64::from(t.3) / 100.0 > over as f64).count(), "{filtered}");

    let artist = tc.draw(gs::sampled_from(&["Loopmasters", "floorplan"][..]));
    let found = read(&["tracks", "--search", artist]);
    let id_of_artist = if artist == "Loopmasters" { "1" } else { "2" };
    let mut found = ids(&found);
    found.sort_unstable();
    assert_eq!(found, live.iter().filter(|t| t.2 == id_of_artist).map(|t| t.0).collect::<Vec<_>>(), "{artist}");

    let friday = read(&["tracks", "--playlist", "Shows/Friday"]);
    assert_eq!(ids(&friday), entries, "in the playlist's order");
    let lists = read(&["playlists"]);
    assert!(lists.to_string().contains("\"Friday\"") && lists.to_string().contains("\"Shows\""), "{lists}");

    // What drive A lacks against it, and exporting it, which a dry run only plans.
    read(&["diff", "A"]);
    let folder = fx.home("export");
    read(&["export", path(&folder), "--dry-run"]);
    assert!(!folder.exists(), "a dry run made {folder:?}");
}
