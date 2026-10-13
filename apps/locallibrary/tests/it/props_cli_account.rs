//! Property tests of `locallibrary account` and publishing (`settings publish`, `profile
//! publish`, `sets publish`, `settings import --from-account`) against a pretend data server on
//! 127.0.0.1, signed in as a sign-in with `--session-file` leaves it (never the keychain): what
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

/// What a session that asked for `scopes` may do, as `account status` says it.
fn allows(scopes: &[&str]) -> String {
    let scopes = granted(scopes);
    if scopes.iter().all(|s| *s == "atproto") {
        "knows who you are; can't post or change anything".into()
    } else if scopes.iter().all(|s| granted(&PUBLISHING).contains(s)) {
        "can publish your My Settings, profile and sets; can't post or change anything else".into()
    } else {
        format!("allowed: {}", scopes.join(" "))
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
    let scopes: Vec<&str> = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => vec!["atproto"],
        1 => PUBLISHING.to_vec(),
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
    let fx = Env::from_template();
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
        let refusal = refusals[tc.draw(gs::integers::<usize>().max_value(refusals.len() - 1))].clone();
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
                let now = pds.record(repo, collection, rkey).map(|r| r.0);
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
    // From the account signed in, or (signed out) one named, at its server.
    let import: Vec<&str> = match session {
        2 => vec!["settings", "import", "--from-handle", DID, "--pds", &pds.url],
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
    let fx = Env::from_template();
    let json = tc.draw(gs::booleans());
    let r = fx.run(&[if json { "--json" } else { "--quiet" }, "account", "login"]);
    assert_eq!(r.code, 1, "{}\n{}", r.out, r.err);
    assert!((r.out.clone() + &r.err).contains("which Atmosphere account?"), "{}\n{}", r.out, r.err);
    let r = fx.run(&["--json", "account", "login", &pds.url, "--no-browser", "--timeout", "1"]);
    assert_ne!(r.code, 0, "{}\n{}", r.out, r.err);
    assert!(fx.json(&["account", "status"])["account"].is_null());
}
