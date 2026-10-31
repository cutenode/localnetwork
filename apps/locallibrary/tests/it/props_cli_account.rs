//! Property tests of `locallibrary account`, publishing (`settings publish`, `profile publish`,
//! `sets publish`, `settings import --from-account`) and bug reports (`bug`) against a pretend
//! data server on 127.0.0.1, signed in as a sign-in with `--session-file` leaves it (never the keychain): what
//! each prints is what its `--json` says, and what's published is the record the plan showed.
//!
//! Signing in itself happens in a browser, so it isn't run here: a session is written in place
//! of one.

#![cfg(feature = "account")]

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::{Value, json};

use crate::mock_pds::{MockPds, Writes, granted};
use crate::support::account::{DID, PUBLISHING, sign_in};
use crate::support::{Env, path};

/// Filing bug reports.
const REPORTING: &str = "repo:app.userinput.discussion?action=create";

/// What a session that asked for `scopes` may do, as `account status` says it.
fn allows(scopes: &[&str]) -> String {
    let scopes = granted(scopes);
    let publishing = granted(&PUBLISHING);
    let publishes = scopes.iter().any(|s| s != "atproto" && publishing.contains(s));
    let reports = scopes.iter().any(|s| s == REPORTING);
    if !scopes.iter().all(|s| publishing.contains(s) || s == REPORTING) {
        return format!("allowed: {}", scopes.join(" "));
    }
    match (publishes, reports) {
        (false, false) => "knows who you are; can't post or change anything".into(),
        (true, false) => {
            "can publish your My Settings, profile, sets and their recordings; can't post or change anything else"
                .into()
        }
        (false, true) => "can post bug reports; can't post or change anything else".into(),
        (true, true) => "can publish your My Settings, profile, sets and their recordings and post bug reports; \
                         can't post or change anything else"
            .into(),
    }
}

/// A session with any scopes, for any time left, kept in a file or (only read, never opened
/// here) the keychain, for locallibrary's own client or a hosted one: `account status` prints
/// who it is, where its data is, until when it lasts and what it allows, as its JSON says; checked
/// with the server (when it's kept in a file) it says the server answered, or fails as the
/// server refuses; and signing out says whether the server could be told.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn the_account_says_what_its_session_allows(tc: TestCase) {
    let pds = MockPds::start();
    pds.set_account(DID, "dj.example.com");
    let fx = Env::from_template();
    let scopes: Vec<&str> = match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => vec!["atproto"],
        1 => PUBLISHING.to_vec(),
        2 => vec!["atproto", REPORTING],
        3 => [&PUBLISHING[..], &[REPORTING]].concat(),
        _ => vec!["atproto", "transition:generic"],
    };
    let hours = tc.draw(gs::sampled_from(&[-48i64, 2, 24 * 30][..]));
    let again = (chrono::Utc::now() + chrono::Duration::hours(hours)).to_rfc3339();
    let keychain = tc.draw(gs::weighted_booleans(0.3));
    let hosted = tc.draw(gs::booleans());
    let mut info = json!({ "sign_in_again_by": again });
    if keychain {
        info["storage"] = "keychain".into();
    }
    if hosted {
        info["client"] = json!({ "kind": "hosted", "client_id": "https://dj.example.com/c.json" });
    }
    sign_in(&fx.store, &pds.url, &scopes, info);

    let v = fx.json(&["account", "status"]);
    let a = &v["account"];
    let out = fx.ok(&["account"]);
    let said = format!("{out}\n{v}");
    assert!(out.starts_with(&format!("Atmosphere account @{}\n", a["handle"].as_str().unwrap())), "{said}");
    let host = pds.url.trim_start_matches("http://");
    assert!(
        out.contains(&format!("  {} · data on {host} · signed in at {host}\n", a["did"].as_str().unwrap())),
        "{said}"
    );
    let when = match hours {
        -48 => "the session has likely ended; sign in again".to_string(),
        2 => " (today)".to_string(),
        _ => " days)".to_string(),
    };
    assert!(out.contains(&when), "{when:?}: {said}");
    assert!(out.contains(&allows(&scopes)), "{said}");
    assert_eq!(out.contains("in atmosphere-session.json, readable only by you"), !keychain, "{said}");
    match hosted {
        true => assert!(out.contains("  client https://dj.example.com/c.json\n"), "{said}"),
        false => assert!(out.contains("signed in as the development client"), "{said}"),
    }
    if keychain {
        return;
    }

    let refused = tc.draw(gs::weighted_booleans(0.3));
    if refused {
        pds.set_writes(Writes::RefuseToken);
    }
    let json = tc.draw(gs::booleans());
    let check = fx.run(&[if json { "--json" } else { "--quiet" }, "account", "status", "--check"]);
    let said = format!("{}\n{}", check.out, check.err);
    match refused {
        true => assert_eq!(check.code, 1, "{said}"),
        false if json => {
            assert_eq!(check.code, 0, "{said}");
            let c: Value = serde_json::from_str(&check.out).unwrap();
            assert_eq!(c["auth_server"], pds.url.as_str(), "{said}");
        }
        false => {
            assert_eq!(check.code, 0, "{said}");
            assert!(check.out.contains(&format!("✓ {} answered for this session", pds.url)), "{said}");
        }
    }

    let told = tc.draw(gs::booleans());
    pds.set_revocation(if told { 200 } else { 500 });
    let json = tc.draw(gs::booleans());
    let r = fx.run(&[if json { "--json" } else { "--quiet" }, "account", "logout"]);
    let said = format!("{}\n{}", r.out, r.err);
    assert_eq!(r.code, 0, "{said}");
    match json {
        true => {
            let v: Value = serde_json::from_str(&r.out).unwrap();
            assert_eq!(v["report"]["revoked"], told, "{said}");
        }
        false => {
            let name = "@dj.example.com";
            let line = if told {
                format!("✓ Signed out of {name}\n")
            } else {
                format!("✓ Signed out of {name} on this computer\n")
            };
            assert!(r.out.starts_with(&line), "{said}");
            assert_eq!(r.out.contains("the server couldn't be told"), !told, "{said}");
        }
    }
    // Signed out, with nothing to check; signing in again is offered for the handle remembered.
    let out = fx.ok(&["account"]);
    let remembered = fx.json(&["account"])["last_handle"].clone();
    let handle = remembered.as_str().unwrap_or("<handle>");
    assert!(remembered.is_null() || remembered == "dj.example.com", "{remembered}");
    assert!(out.contains(&format!("`locallibrary account login {handle}` signs in")), "{out}");
    assert_eq!(fx.run(&["account", "status", "--check"]).code, 1);
    let again = fx.run(&["--json", "account", "logout"]);
    assert!(serde_json::from_str::<Value>(&again.out).unwrap()["report"]["account"].is_null(), "{}", again.out);
}

/// The client metadata document for locallibrary's own client or one of the DJ's, with any extra
/// scopes: printed, it's the JSON `--json` writes beside the file; written to a file, the file
/// holds it, and a file that's there is kept unless `--replace` says otherwise; an address that
/// isn't `https://` is refused.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn client_metadata_is_the_document_written(tc: TestCase) {
    let fx = Env::from_template();
    let client = tc.draw(gs::sampled_from(
        &[None, Some("https://dj.example.com/c.json"), Some("http://insecure.example/c.json")][..],
    ));
    let scopes: Vec<&str> = tc.draw(gs::subsequences(vec!["repo:app.example.thing", "transition:generic"]));
    let mut args = vec!["account", "client-metadata"];
    if let Some(c) = client {
        args.extend(["--client-id", c]);
    }
    for s in &scopes {
        args.extend(["--scope", s]);
    }
    let printed = fx.run(&args);
    if client.is_some_and(|c| c.starts_with("http://")) {
        assert!(printed.code == 1 && printed.err.contains("https://"), "{}", printed.err);
        return;
    }
    assert_eq!(printed.code, 0, "{}", printed.err);
    let doc: Value = serde_json::from_str(&printed.out).unwrap();
    let id = doc["client_id"].as_str().unwrap();
    assert!(client.is_none_or(|c| c == id), "{doc}");
    let scope = doc["scope"].as_str().unwrap();
    assert!(scope.starts_with("atproto") && scopes.iter().all(|s| scope.contains(s)), "{scope}");

    let file = fx.home("meta.json");
    let existed = tc.draw(gs::booleans());
    if existed {
        std::fs::write(&file, "mine").unwrap();
    }
    let replace = tc.draw(gs::booleans());
    let json = tc.draw(gs::booleans());
    let mut write = args.clone();
    write.extend(["-o", path(&file)]);
    if replace {
        write.push("--replace");
    }
    if json {
        write.insert(0, "--json");
    }
    let r = fx.run(&write);
    let said = format!("{write:?}\n{}\n{}", r.out, r.err);
    if existed && !replace {
        assert_eq!(r.code, 1, "{said}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
        return;
    }
    assert_eq!(r.code, 0, "{said}");
    assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(&file).unwrap()).unwrap(), doc, "{said}");
    match json {
        true => assert_eq!(serde_json::from_str::<Value>(&r.out).unwrap()["metadata"], doc, "{said}"),
        false => {
            assert!(r.out.contains(&format!("wrote {}", file.display())), "{said}");
            let then = if client.is_none() {
                "; sign-in uses it once it's online".to_string()
            } else {
                format!(", then sign in with --client-id {id}")
            };
            assert!(r.out.contains(&format!("Host it at {id}{then}")), "{said}");
        }
    }
}

/// A printed plan to publish says what its JSON plan does: whose account it goes to (or that
/// it's as it would be), where, whether it replaces what's there, a line for each part of the
/// record, and the notes.
fn publish_plan_is_printed(p: &Value, what: &str, out: &str, said: &str) {
    let head = match p["account"]["handle"].as_str() {
        Some(h) => format!("Publish {what} as @{h}"),
        None => {
            let mut c = what.chars();
            format!("{}{}, as it would be published", c.next().unwrap().to_uppercase(), c.as_str())
        }
    };
    assert!(out.starts_with(&head), "{head:?}: {said}");
    match p["uri"].as_str() {
        Some(uri) => {
            let state = match (p["current"].is_null(), p["changed"] == true) {
                (true, _) => "not published yet",
                (false, false) => "already published, unchanged",
                (false, true) => "replaces what",
            };
            assert!(out.contains(&format!("{uri} · {state}")), "{state:?}: {said}");
        }
        None => {
            assert!(out.contains("not signed in: `locallibrary account login <handle> --publish` signs in"), "{said}")
        }
    }
    for note in p["notes"].as_array().into_iter().flatten() {
        assert!(out.contains(note.as_str().unwrap()), "{note}: {said}");
    }
    assert_eq!(out.contains("this session can't publish yet"), !p["missing_scope"].is_null(), "{said}");
    let r = &p["record"];
    match what {
        "this set" => {
            let tracks = r["tracks"].as_array().unwrap();
            for (i, t) in tracks.iter().enumerate() {
                let line = out
                    .lines()
                    .find(|l| l.starts_with(&format!("  {:>3}. ", i + 1)))
                    .unwrap_or_else(|| panic!("{said}"));
                match t["unidentified"] == true {
                    true => assert!(line.ends_with("ID - ID"), "{line:?}"),
                    false => assert!(line.contains(t["title"].as_str().unwrap()), "{line:?}: {t}"),
                }
            }
            for (field, title) in [("title", "Title"), ("venue", "Venue"), ("date", "Date")] {
                if let Some(text) = r[field].as_str() {
                    assert!(out.contains(&format!("  {title:<7} {text}")), "{field}: {said}");
                }
            }
        }
        "your My Settings" => {
            for group in ["player", "display", "mixer"] {
                let title = match group {
                    "player" => "Player",
                    "display" => "Display",
                    _ => "Mixer",
                };
                let fields = r[group].as_object().map_or(0, |m| m.len());
                assert!(fields == 0 || out.contains(&format!("  {title}\n")), "{group}: {said}");
            }
        }
        _ => {
            if let Some(h) = r["headline"].as_str() {
                assert!(out.contains(&format!("  {:<17} {h}", "Headline")), "{said}");
            }
            if let Some(t) = r["tempo"].as_object() {
                assert!(out.contains(&format!("{}-{} BPM, median {}", t["low"], t["high"], t["median"])), "{said}");
            }
            assert!(out.contains(&format!("{} numbers", r["precision"].as_str().unwrap_or("rounded"))), "{said}");
        }
    }
}

/// My Settings, the profile and a set, each published twice in any order and any way (a dry run, without
/// `--yes`, with it; printed or as JSON), signed in to publish or not, with the server taking
/// writes or refusing them: the plan printed is the JSON's; nothing is written but by a run with
/// `--yes` that succeeds, which writes the record its plan holds; what's published already isn't
/// written again; and My Settings imported back from the account are the ones published.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn publishing_writes_the_record_the_plan_shows(tc: TestCase) {
    let pds = MockPds::start();
    let fx = Env::from_template().with_directory(&pds.url);
    let session = tc.draw(gs::integers::<u8>().max_value(2));
    match session {
        0 => sign_in(&fx.store, &pds.url, &PUBLISHING, json!({})),
        1 => sign_in(&fx.store, &pds.url, &["atproto"], json!({})),
        _ => {}
    }
    fx.ok(&[
        "--yes",
        "sets",
        "new",
        "--from",
        "HISTORY 001",
        "--title",
        "Friday",
        "--venue",
        "Warehouse",
        "--date",
        "2026-10-03",
        "--city",
        "London",
        "--country",
        "gb",
        "--with",
        "DJ Two",
        "--with",
        "did:plc:abc",
        "--link",
        "https://www.mixcloud.com/dj/friday/",
    ]);
    fx.ok(&["--yes", "sets", "edit", "Friday", "--insert", "1:Someone - Dubplate", "--drive", "A"]);
    // The profile with the sections that name tracks and describe the collection, or without.
    if tc.draw(gs::booleans()) {
        fx.ok(&["--yes", "profile", "include", "signature,prep,collection,artists,labels"]);
    }
    // Records published before, from another computer: each plan shows what it replaces.
    if tc.draw(gs::booleans()) {
        let at = "2026-09-01T00:00:00Z";
        let settings = json!({
            "$type": "computer.localnetwork.locallibrary.settings",
            "player": { "quantize": "off", "jogMode": "cdj", "somethingNewer": "x" },
            "display": { "lcdBrightness": "2" },
            "updatedAt": at,
        });
        pds.put(DID, "computer.localnetwork.locallibrary.settings", "self", settings);
        pds.put(
            DID,
            "computer.localnetwork.dj.profile",
            "self",
            json!({ "$type": "computer.localnetwork.dj.profile", "headline": "Old", "precision": "exact", "labels": [{ "name": "Old Label", "share": 1 }] }),
        );
        let set = fx.json(&["sets", "show", "Friday"])["id"].as_str().unwrap().to_string();
        pds.put(
            DID,
            "computer.localnetwork.dj.set",
            &set,
            json!({ "$type": "computer.localnetwork.dj.set", "title": "Thursday", "kind": "radio", "tracks": [], "createdAt": at }),
        );
    }
    let writes = || {
        pds.calls("com.atproto.repo.putRecord")
            .iter()
            .filter(|c| c.dpop().is_some_and(|(_, claims)| claims["nonce"] == "nonce-1"))
            .count()
    };
    let kinds: [(&[&str], &str); 3] = [
        (&["settings", "publish"], "your My Settings"),
        (&["profile", "publish", "A", "--no-analysis"], "your profile"),
        (&["sets", "publish", "Friday", "--hide", "1"], "this set"),
    ];
    // Each kind of record, twice over in any order: the second time, it may be published already.
    let order: Vec<usize> =
        [tc.draw(gs::permutations(vec![0, 1, 2])), tc.draw(gs::permutations(vec![0, 1, 2]))].concat();
    for (words, what) in order.into_iter().map(|i| kinds[i]) {
        if tc.draw(gs::weighted_booleans(0.6)) {
            let v = tc.draw(gs::sampled_from(
                &["quantize=on", "quantize=off", "jog_mode=cdj", "jog_mode=vinyl", "lcd_brightness=3"][..],
            ));
            fx.ok(&["--yes", "settings", "set", v]);
        }
        let refusals = [None, None, None, Some(Writes::RefuseScope), Some(Writes::RefuseRecord)];
        let refusal = tc.draw(gs::sampled_from(&refusals[..]));
        pds.set_writes(refusal.clone().unwrap_or(Writes::Accept));
        let planned = fx.run(&[&["--json"][..], words, &["--dry-run"]].concat());
        let printed = fx.run(&[words, &["--dry-run"][..]].concat());
        let said = format!("{words:?}\n{}\n{}\n{}", printed.out, printed.err, planned.out);
        assert_eq!(planned.code, printed.code, "{said}");
        if planned.code != 0 {
            continue;
        }
        let v: Value = serde_json::from_str(&planned.out).unwrap();
        assert_eq!(v["outcome"], "dry_run", "{said}");
        let p = &v["plan"];
        publish_plan_is_printed(p, what, &printed.out, &said);

        let how = tc.draw(gs::integers::<u8>().max_value(3));
        let flags: &[&str] = match how {
            0 => &[],
            1 => &["--json"],
            2 => &["--yes"],
            _ => &["--yes", "--json"],
        };
        let before = writes();
        let r = fx.run(&[flags, words].concat());
        let said = format!("{flags:?} {words:?} exited {}\n{}\n{}\n{}", r.code, r.out, r.err, planned.out);
        // The order publishing goes in: JSON without --yes stops at the plan; then it needs an
        // account; what's published already is left; a session that may not publish can't sign
        // in again without a terminal; without --yes there's no terminal to ask in.
        let (json, yes) = (how % 2 == 1, how >= 2);
        let expected = if json && !yes {
            3
        } else if session == 2 {
            1
        } else if p["noop"] == true {
            0
        } else if session == 1 {
            1
        } else if !yes {
            3
        } else if refusal.is_some() {
            1
        } else {
            0
        };
        assert_eq!(r.code, expected, "{said}");
        if r.code != 0 || p["noop"] == true {
            // A write the server refused reached it, and changed nothing.
            if refusal.is_none() || r.code == 0 {
                assert_eq!(writes(), before, "{said}");
            }
            if let Some(uri) = p["uri"].as_str() {
                let (repo, rest) = uri.strip_prefix("at://").unwrap().split_once('/').unwrap();
                let (collection, rkey) = rest.split_once('/').unwrap();
                // The plan showed what the directory has, which still has what's published.
                let now = pds.indexed(repo, collection, rkey).map(|r| r.0);
                assert_eq!(now.as_ref(), p["current"].get("value"), "{said}");
            }
            if p["noop"] == true && !json && session != 2 {
                assert!(r.out.contains(&format!("{what} already published, unchanged")), "{said}");
            }
            continue;
        }
        assert_eq!(writes(), before + 1, "{said}");
        let uri = p["uri"].as_str().unwrap();
        let (repo, rest) = uri.strip_prefix("at://").unwrap().split_once('/').unwrap();
        let (collection, rkey) = rest.split_once('/').unwrap();
        let (record, _) = pds.record(repo, collection, rkey).unwrap_or_else(|| panic!("{uri}: {said}"));
        for (k, planned) in p["record"].as_object().unwrap() {
            // When it was made is set as it's written.
            if !k.ends_with("At") {
                assert_eq!(&record[k], planned, "{k}: {said}");
            }
        }
        match how {
            2 => assert!(r.out.contains(&format!("✓ published {what}\n  {uri}")), "{said}"),
            _ => assert_eq!(serde_json::from_str::<Value>(&r.out).unwrap()["report"]["uri"], uri, "{said}"),
        }
    }

    // What's published comes back to a settings profile emptied of it.
    pds.set_writes(Writes::Accept);
    let collection = "computer.localnetwork.locallibrary.settings";
    if pds.record(DID, collection, "self").is_none() {
        pds.put(
            DID,
            collection,
            "self",
            json!({ "$type": collection, "player": { "quantize": "on" }, "updatedAt": "2026-09-01T00:00:00Z" }),
        );
    }
    let (published, _) = pds.record(DID, collection, "self").unwrap();
    for key in ["quantize", "jog_mode", "lcd_brightness"] {
        fx.run(&["--yes", "settings", "unset", key]);
    }
    // From the account signed in, or (signed out) one named.
    let import: Vec<&str> = match session {
        2 => vec!["settings", "import", "--from-handle", DID],
        _ => vec!["settings", "import", "--from-account"],
    };
    let planned = fx.run(&[&["--json"][..], &import, &["--dry-run"]].concat());
    let printed = fx.run(&[&import[..], &["--dry-run"]].concat());
    let said = format!("{}\n{}\n{}", printed.out, printed.err, planned.out);
    assert_eq!((planned.code, printed.code), (0, 0), "{said}");
    let v: Value = serde_json::from_str(&planned.out).unwrap();
    let p = &v["plan"];
    assert!(printed.out.starts_with("My Settings published by "), "{said}");
    let changes = p["changes"].as_array().unwrap();
    assert_eq!(printed.out.contains("the same as your settings profile"), changes.is_empty(), "{said}");
    assert_eq!(printed.out.lines().filter(|l| l.contains(" → ")).count(), changes.len(), "{said}");
    let r = fx.run(&[&["--yes"][..], &import].concat());
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    // The profile's base is then what the plan said each setting would come to; a setting this
    // version doesn't know is left out, saying so.
    let base = fx.json(&["settings", "profile"])["base"].clone();
    for c in changes {
        assert_eq!(&base[c["key"].as_str().unwrap()], &c["to"], "{c}: {base}");
    }
    for (_, fields) in published.as_object().unwrap().iter().filter(|(_, v)| v.is_object()) {
        for field in fields.as_object().unwrap().keys() {
            let snake = field.chars().fold(String::new(), |mut s, c| {
                if c.is_uppercase() {
                    s.push('_');
                }
                s.push(c.to_ascii_lowercase());
                s
            });
            let known = changes.iter().any(|c| c["key"] == snake.as_str()) || !base[&snake].is_null();
            let warned = p["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains(field.as_str()));
            assert!(known || warned, "{field} neither imported nor said to be left out: {said}");
        }
    }
}

/// Signing in starts in a browser, which these tests have none of; asked to sign in without
/// saying to which account, or to a server that isn't one, it says why it can't, and nothing is
/// signed in.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn signing_in_without_an_account_to_sign_in_to_fails(tc: TestCase) {
    let pds = MockPds::start();
    let fx = Env::from_template().with_directory(&pds.url);
    let json = tc.draw(gs::booleans());
    let r = fx.run(&[if json { "--json" } else { "--quiet" }, "account", "login"]);
    assert_eq!(r.code, 1, "{}\n{}", r.out, r.err);
    assert!((r.out.clone() + &r.err).contains("which Atmosphere account?"), "{}\n{}", r.out, r.err);
    let r = fx.run(&["--json", "account", "login", &pds.url, "--no-browser", "--timeout", "1"]);
    assert_ne!(r.code, 0, "{}\n{}", r.out, r.err);
    assert!(fx.json(&["account", "status"])["account"].is_null());
}

/// Each section a profile record can have, with its title in a printed plan and a value for it
/// as one published before (by an older version, or another computer) might hold.
fn published_sections() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("headline", "Headline", json!("Old headline")),
        ("tags", "Traits", json!([{ "name": "Warm-up" }])),
        ("sets", "Sets", json!({ "total": { "atLeast": 20 }, "since": 2019, "medianTracks": 14 })),
        ("tempo", "Tempo", json!({ "low": 120, "high": 128, "median": 124 })),
        ("arc", "Set shape", json!({ "shape": "builds", "builds": 50, "steady": 30, "windsDown": 20 })),
        ("mixing", "Mixing", json!({ "harmonic": 40, "smoothTempo": 70 })),
        ("keys", "Keys", json!({ "top": [{ "key": "8A" }], "minor": 60 })),
        ("energy", "Energy", json!({ "high": 30, "mid": 50, "low": 20 })),
        ("genres", "Genres", json!([{ "name": "Techno", "share": 50 }])),
        ("labels", "Labels", json!([{ "name": "Old Label", "share": 10 }])),
        ("artists", "Artists", json!([{ "name": "Old Artist", "share": 5 }])),
        ("era", "Era", json!({ "medianYear": 2015, "recent": 20 })),
        (
            "signature",
            "Signature tracks",
            json!([{ "artist": "Old Artist", "title": "Anthem" }, { "title": "Untitled" }]),
        ),
        ("prep", "Prep", json!({ "hotCues": 80, "memoryCues": 50, "loops": 10 })),
        ("collection", "Collection", json!({ "tracks": { "count": 100 }, "playlists": { "atLeast": 10 } })),
        ("style", "Player style", json!({ "jogMode": "vinyl" })),
    ]
}

/// Whatever sections a profile published before holds, and whichever the DJ publishes now, the
/// printed plan marks each section it publishes as new or changed against it, and lists each
/// section published before that it no longer publishes; its JSON plan holds that record as what
/// it replaces.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_profile_plan_says_what_it_no_longer_publishes(tc: TestCase) {
    let pds = MockPds::start();
    let fx = Env::from_template().with_directory(&pds.url);
    sign_in(&fx.store, &pds.url, &PUBLISHING, json!({}));
    let all = published_sections();
    let names: Vec<&str> = all.iter().map(|(f, _, _)| *f).collect();
    let before: Vec<&str> = tc.draw(gs::subsequences(names.clone()));
    let mut current = json!({ "$type": "computer.localnetwork.dj.profile", "precision": "exact" });
    for (field, _, value) in all.iter().filter(|(f, _, _)| before.contains(f)) {
        current[*field] = value.clone();
    }
    pds.put(DID, "computer.localnetwork.dj.profile", "self", current.clone());
    // Sections turned on and off now, from the report's own names (`sets` and `summary` stand
    // for the record's `sets`, and `headline` and `tags`).
    let report_sections =
        ["summary", "sets", "tempo", "arc", "mixing", "keys", "energy", "genres", "labels", "artists", "era"];
    let on: Vec<&str> = tc.draw(gs::subsequences(report_sections.to_vec()));
    if !on.is_empty() {
        fx.ok(&["--yes", "profile", "include", &on.join(",")]);
    }
    let off: Vec<&str> = tc.draw(gs::subsequences(report_sections.to_vec()));
    if !off.is_empty() {
        fx.ok(&["--yes", "profile", "exclude", &off.join(",")]);
    }

    let words = ["profile", "publish", "A", "--no-analysis", "--dry-run"];
    let v = fx.json(&words);
    let out = fx.ok(&words);
    let said = format!("before: {before:?}\n{out}\n{v}");
    let p = &v["plan"];
    assert_eq!(p["current"]["value"], current, "{said}");
    let record = &p["record"];
    let gone: Vec<&str> = out.lines().filter_map(|l| l.strip_suffix("no longer published")).map(|l| l.trim()).collect();
    for (field, title, _) in &all {
        let line = out.lines().find(|l| l.starts_with(&format!("  {title:<17} ")) && !l.ends_with("published"));
        match (before.contains(field), record[*field].is_null(), line) {
            // Published before and not now: said so.
            (true, true, _) => assert!(gone.contains(title), "{field}: {said}"),
            (false, false, Some(l)) => assert!(l.ends_with(" (new)"), "{field}: {l:?}: {said}"),
            (true, false, Some(l)) => {
                assert_eq!(l.ends_with(" (changed)"), current[*field] != record[*field], "{field}: {l:?}: {said}")
            }
            _ => {}
        }
        if !before.contains(field) {
            assert!(!gone.contains(title), "{field}: {said}");
        }
    }
}

/// A client document on this computer, which sign-in refuses before fetching anything.
#[cfg(unix)]
const LOCAL_CLIENT: &str = "https://localhost/c.json";

/// Asked in a terminal which account to sign in to, `account login` offers the handle last
/// signed in with and tries what's typed, or (typed nothing) the handle offered: an account it
/// can't sign in to fails saying why (an address that isn't a handle, a client document on this
/// computer, no account at all), and nobody is signed in. What's typed is never a handle that
/// would be looked up on the network, and the handle offered is only tried when there's none.
#[cfg(unix)]
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn login_asks_for_the_account_and_tries_the_one_typed(tc: TestCase) {
    let fx = Env::from_template();
    std::fs::create_dir_all(&fx.store).unwrap();
    let remembered = tc.draw(gs::booleans());
    if remembered {
        std::fs::write(fx.store.join("atmosphere.json"), r#"{ "last_handle": "dj.example.com" }"#).unwrap();
    }
    let typed = match remembered {
        true => tc.draw(gs::sampled_from(&["http://127.0.0.1:9", "@not a handle", "  spaced out  "][..])),
        false => tc.draw(gs::sampled_from(&["", "http://127.0.0.1:9", "@not a handle"][..])),
    };
    let hosted = tc.draw(gs::booleans());
    let client: &[&str] = if hosted { &["--client-id", LOCAL_CLIENT] } else { &["--dev-client"] };
    let args = [&["account", "login", "--session-file", "--no-browser"], client].concat();
    let r = fx.typed_ahead(&args, &format!("{typed}\n"));
    let said = format!("{args:?} typed {typed:?} exited {}\n{}\n{}", r.code, r.out, r.err);
    let offered = if remembered { " [dj.example.com]" } else { "" };
    assert!(r.err.contains(&format!("Your Atmosphere handle, or your account's server{offered}: ")), "{said}");
    assert_eq!(r.code, 1, "{said}");
    let why = if typed.is_empty() {
        "which Atmosphere account?".to_string()
    } else if hosted {
        format!("the client document at {LOCAL_CLIENT} can't be used")
    } else {
        format!("{:?}", typed.trim().trim_start_matches('@'))
    };
    assert!(r.err.contains(&why), "{why:?}: {said}");
    let status = fx.json(&["account", "status"]);
    assert!(status["account"].is_null(), "{status}");
    assert_eq!(status["last_handle"], if remembered { json!("dj.example.com") } else { Value::Null }, "{status}");
}

/// Publishing in a terminal from a session that may not publish asks to sign in again: said no,
/// it stops there as declined; said yes, it signs in again for the same account (by handle, or
/// its DID when it has none) with the same client, which here is one on this computer that
/// sign-in refuses, so it fails saying so. Either way nothing is published and the session
/// stays as it was.
#[cfg(unix)]
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn publishing_from_a_session_that_may_not_asks_to_sign_in_again(tc: TestCase) {
    let pds = MockPds::start();
    let fx = Env::from_template().with_directory(&pds.url);
    let handle = tc.draw(gs::booleans());
    let mut info = json!({ "client": { "kind": "hosted", "client_id": LOCAL_CLIENT } });
    if !handle {
        info["handle"] = Value::Null;
    }
    sign_in(&fx.store, &pds.url, &["atproto"], info);
    // Something to publish of My Settings.
    fx.ok(&["--yes", "settings", "set", "quantize=on"]);
    let before = fx.json(&["account", "status"]);
    let words: &[&str] = tc
        .draw(gs::sampled_from(&[&["settings", "publish"][..], &["profile", "publish", "A", "--no-analysis"][..]][..]));
    let yes = tc.draw(gs::booleans());
    let r = fx.typed_ahead(words, if yes { "y\n" } else { "n\n" });
    let said = format!("{words:?} answered {yes} exited {}\n{}\n{}", r.code, r.out, r.err);
    assert!(r.err.contains("Publishing needs your permission"), "{said}");
    assert!(r.err.contains("? Sign in again to allow publishing? [y/N] "), "{said}");
    assert!(r.out.contains("this session can't publish yet"), "{said}");
    match yes {
        true => {
            assert_eq!(r.code, 1, "{said}");
            assert!(r.err.contains(&format!("the client document at {LOCAL_CLIENT} can't be used")), "{said}");
        }
        false => assert_eq!(r.code, 3, "{said}"),
    }
    assert!(pds.calls("com.atproto.repo.putRecord").is_empty(), "{said}");
    assert_eq!(fx.json(&["account", "status"]), before, "{said}");
}

/// GitHub's bug report form, as `bug` prints it.
const FORM: &str = "https://github.com/cutenode/localnetwork/issues/new?template=locallibrary-bug.yml&";

/// Creating a record, as a post does.
const CREATE: &str = "com.atproto.repo.createRecord";

/// The space bug reports go to, a record of `DID`'s that Local Network's directory (the pretend
/// server) has.
fn space_on(pds: &MockPds) -> String {
    pds.put(DID, "app.userinput.space", "3lspace", json!({ "name": "locallibrary" }));
    format!("at://{DID}/app.userinput.space/3lspace")
}

/// `locallibrary ARGS` (a `bug` run) posting to `space` when there's one, with stdin a terminal
/// on which `typed` was typed ahead when there's that.
fn bug(fx: &Env, space: Option<&str>, args: &[&str], typed: Option<&str>) -> crate::support::Run {
    let mut c = fx.command();
    match space {
        Some(space) => c.env("LOCALLIBRARY_USERINPUT_SPACE", space),
        None => c.env_remove("LOCALLIBRARY_USERINPUT_SPACE"),
    };
    // Kept open until the run is over: closing it hangs the terminal up, losing what was typed.
    #[cfg(unix)]
    let _keyboard = typed.map(|typed| {
        use std::io::Write;
        let (mut keyboard, stdin) = crate::support::terminal();
        keyboard.write_all(typed.as_bytes()).unwrap();
        c.stdin(std::process::Stdio::from(stdin));
        keyboard
    });
    #[cfg(not(unix))]
    assert!(typed.is_none(), "only Unix has a terminal to type on here");
    let out = c.args(args).output().unwrap();
    crate::support::Run {
        out: String::from_utf8(out.stdout).unwrap(),
        err: String::from_utf8(out.stderr).unwrap(),
        code: out.status.code().unwrap(),
    }
}

/// The form's address in what a run printed: its JSON's `url`, or the address in its text.
fn form_in(out: &str, as_json: bool) -> String {
    let url = match as_json {
        true => serde_json::from_str::<Value>(out).unwrap()["url"].as_str().unwrap().to_string(),
        false => out[out.find(FORM).unwrap_or_else(|| panic!("no form in {out}"))..]
            .split_whitespace()
            .next()
            .unwrap()
            .to_string(),
    };
    assert!(url.starts_with(FORM), "{url}");
    url
}

/// Any `bug` run without a terminal (signed out, or in with or without leave to post reports; a
/// space named or not, gone, or not a space at all; a title or none, or one too long; `--yes` or
/// not; text or `--json`): a space that isn't one, or a title too long, is refused before
/// anything's sent; with an account allowed to post, a space and a title it shows the report and
/// asks, posting with `--yes` (or, when the space is gone or the server refuses it, giving
/// GitHub's form instead); anything else gives GitHub's form, filled in with what was typed.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn a_bug_report_is_posted_from_the_account_or_given_to_github(tc: TestCase) {
    let pds = MockPds::start();
    let good = space_on(&pds);
    let gone = format!("at://{DID}/app.userinput.space/3lgone");
    let fx = Env::new().with_directory(&pds.url);
    let reporting = ["atproto", REPORTING];
    let scopes: Option<&[&str]> = tc.draw(gs::sampled_from(vec![None, Some(&reporting[..]), Some(&PUBLISHING[..])]));
    if let Some(scopes) = scopes {
        sign_in(&fx.store, &pds.url, scopes, json!({}));
    }
    let spaces = vec![None, Some(good.as_str()), Some(good.as_str()), Some(gone.as_str()), Some("bugs please")];
    let space = tc.draw(gs::sampled_from(spaces));
    let refused = tc.draw(gs::weighted_booleans(0.3));
    if refused {
        pds.set_writes(Writes::RefuseRecord);
    }
    let title = tc.draw(gs::sampled_from(vec![None, Some("Sync stops at 99%".to_string()), Some("a".repeat(301))]));
    let message = tc.draw(gs::optional(gs::sampled_from(vec!["It stopped & said 100%", "line one\nline two"])));
    let (yes, as_json) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let mut args: Vec<&str> = Vec::new();
    args.extend(as_json.then_some("--json"));
    args.extend(yes.then_some("--yes"));
    args.extend(["bug", "--no-browser"]);
    if let Some(t) = &title {
        args.extend(["--title", t]);
    }
    args.extend(message.iter().flat_map(|m| ["-m", m]));
    let r = bug(&fx, space, &args, None);
    let said = format!("{scopes:?} {space:?} refused {refused} {args:?}: exit {}\n{}\n{}", r.code, r.out, r.err);
    let printed = format!("{}{}", r.out, r.err);
    let github = || {
        assert_eq!(r.code, 0, "{said}");
        let url = form_in(&r.out, as_json);
        assert_eq!(url.contains("&title="), title.is_some(), "{said}");
        assert_eq!(url.contains("&what-happened="), message.is_some(), "{said}");
    };
    let reports = scopes.is_some_and(|s| s.contains(&REPORTING));
    let sent = pds.calls(CREATE);
    match () {
        _ if space == Some("bugs please") => {
            assert_eq!(r.code, 1, "{said}");
            assert!(printed.contains("isn't a userinput.app space"), "{said}");
        }
        _ if space.is_none() || scopes.is_none() || title.is_none() => github(),
        _ if title.as_ref().is_some_and(|t| t.len() > 300) => {
            assert_eq!(r.code, 1, "{said}");
            assert!(printed.contains("the title is too long to post: 301 characters, 300 at most"), "{said}");
        }
        _ if !reports => {
            assert!(printed.contains("can't post bug reports yet"), "{said}");
            github();
        }
        _ if !yes => {
            assert_eq!(r.code, 3, "{said}");
            match as_json {
                true => assert_eq!(serde_json::from_str::<Value>(&r.out).unwrap()["outcome"], "not_confirmed"),
                false => assert!(r.out.contains("Bug report from @dj.example.com") && printed.contains(FORM), "{said}"),
            }
        }
        _ if space == Some(gone.as_str()) || refused => {
            assert!(printed.contains("couldn't post it"), "{said}");
            assert_eq!(sent.is_empty(), !refused || space == Some(gone.as_str()), "{said}");
            github();
            return;
        }
        _ => {
            assert_eq!(r.code, 0, "{said}");
            let link = format!("https://userinput.app/d/{DID}/");
            match as_json {
                true => {
                    let v: Value = serde_json::from_str(&r.out).unwrap();
                    assert_eq!(v["outcome"], "applied", "{said}");
                    assert!(v["report"]["url"].as_str().unwrap().starts_with(&link), "{said}");
                }
                false => assert!(r.out.contains("posted your bug report") && r.out.contains(&link), "{said}"),
            }
            let record = &sent.last().expect("posted").json()["record"];
            assert_eq!(record["title"], json!(title), "{said}");
            let body = record["body"].as_str().unwrap();
            assert!(body.starts_with(message.unwrap_or("Version: locallibrary ")), "{said}");
            return;
        }
    }
    assert!(sent.is_empty(), "nothing is sent: {said}");
}

/// `bug` in a terminal with no title given asks for one and for what happened (lines up to an
/// empty one), then: nothing typed for the title, GitHub's form with what was; from an account
/// allowed to post, the report shown and the question answered (posted, or declined); from one
/// that isn't, asked to sign in again, which declined or failed (its client is on this computer)
/// gives GitHub's form instead. Only a post that went ahead reaches the server.
#[cfg(unix)]
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn asked_in_a_terminal_bug_takes_what_was_typed(tc: TestCase) {
    let pds = MockPds::start();
    let space = space_on(&pds);
    let fx = Env::new().with_directory(&pds.url);
    let reports = tc.draw(gs::booleans());
    let scopes: &[&str] = if reports { &["atproto", REPORTING] } else { &PUBLISHING };
    let client = json!({ "client": { "kind": "hosted", "client_id": LOCAL_CLIENT } });
    sign_in(&fx.store, &pds.url, scopes, client);
    let title = tc.draw(gs::sampled_from(vec!["", "  The map is grey  "]));
    let lines: Vec<&str> =
        tc.draw(gs::vecs(gs::sampled_from(vec!["I added a venue", "and it stayed grey"])).max_size(2));
    let answer = tc.draw(gs::booleans());
    let typed = format!(
        "{title}\n{}\n{}\n",
        lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        if answer { "y" } else { "n" }
    );
    let r = bug(&fx, Some(&space), &["bug", "--no-browser"], Some(&typed));
    let said = format!("{scopes:?} typed {typed:?}: exit {}\n{}\n{}", r.code, r.out, r.err);
    assert!(r.err.contains("Title (a line saying what went wrong): "), "{said}");
    assert!(r.err.contains("What happened? What were you doing? (an empty line ends it)"), "{said}");
    let told = lines.join("\n");
    let posted = !title.trim().is_empty() && reports && answer;
    if title.trim().is_empty() {
        assert_eq!(r.code, 0, "{said}");
        let url = form_in(&r.out, false);
        assert!(!url.contains("&title=") && url.contains("&what-happened=") != lines.is_empty(), "{said}");
    } else if !reports {
        assert!(r.err.contains("? Sign in again to allow bug reports? [y/N] "), "{said}");
        assert_eq!(r.out.contains("couldn't sign in again"), answer, "{said}");
        assert_eq!(r.code, 0, "{said}");
        assert!(form_in(&r.out, false).contains("&title=The%20map%20is%20grey"), "{said}");
    } else {
        assert!(r.out.contains("  The map is grey\n"), "{said}");
        assert!(r.err.contains("? Post it as @dj.example.com? Anyone can read it [y/N] "), "{said}");
        assert_eq!(r.code, if answer { 0 } else { 3 }, "{said}");
    }
    let sent = pds.calls(CREATE);
    assert_eq!(!sent.is_empty(), posted, "{said}");
    if posted {
        let record = &sent.last().unwrap().json()["record"];
        assert_eq!(record["title"], "The map is grey", "{said}");
        let body = record["body"].as_str().unwrap();
        assert!(body.starts_with(&told) && body.contains("Version: locallibrary "), "{said}");
    }
}
