//! The player's keys and the account (`api::keybinds_*`), against a pretend data server on
//! 127.0.0.1 (`support/mock_pds.rs`) that stands in for Local Network's directory and website too:
//! publishing them, loading them on another computer after signing in, and importing someone's.

#![cfg(feature = "account")]

use std::path::Path;

use locallibrary::api::{self, AfterSignIn};
use locallibrary::atmosphere::{self, KEYBINDS_SCOPE, PUBLISH_SET, lexicon};
use locallibrary::error::Reason;
use locallibrary::keybinds::{self, Keybinds};
use serde_json::{Value, json};

use crate::mock_pds::MockPds;
use crate::session::{DID, session_json, sign_in_as};

const KEYBINDS: &str = lexicon::KEYBINDS;
const PUT: &str = "com.atproto.repo.putRecord";
const GET: &str = "computer.localnetwork.directory.getRecords";
const PDS_GET: &str = "com.atproto.repo.getRecord";

fn network() -> MockPds {
    let pds = MockPds::start();
    locallibrary::testing::directory_on_this_thread(Some(pds.url.clone()));
    pds
}

/// Sign `DID` in at `root`, as the player's sign-in does: allowed to publish keys and nothing else.
fn sign_in(root: &Path, pds: &MockPds) {
    let scopes = atmosphere::player_scopes().join(" ");
    sign_in_as(root, session_json(DID, "s1", &pds.url, &scopes, Some("refresh-1"), "2099-01-01T00:00:00.000Z"));
}

fn keys(pairs: &[(&str, &[&str])]) -> Keybinds {
    let mut out = Keybinds::default();
    for (id, keys) in pairs {
        out.keys.insert(id.to_string(), keys.iter().map(|k| k.to_string()).collect());
    }
    out
}

fn bindings(record: &Value) -> Value {
    record["bindings"].clone()
}

#[test]
fn publishing_keybinds_writes_the_record() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds);
    let mut mine = keys(&[("playback.next", &["]"]), ("player.jobs", &[]), ("library.search", &["Ctrl+F"])]);
    mine.from = Some("did:plc:someoneelse".into());
    keybinds::save(&root, &mine).unwrap();

    let published = api::keybinds_publish(&root).unwrap();
    assert!(published.written);
    assert_eq!(published.uri, format!("at://{DID}/{KEYBINDS}/self"));
    let (stored, cid) = pds.record(DID, KEYBINDS, "self").unwrap();
    assert_eq!(published.cid, cid);
    lexicon::validate(KEYBINDS, &stored).unwrap();
    assert_eq!(
        bindings(&stored),
        json!([
            { "action": "library.search", "keys": ["ctrl-f"] },
            { "action": "playback.next", "keys": ["]"] },
            { "action": "player.jobs", "keys": [] }
        ])
    );
    assert_eq!(stored["from"], "did:plc:someoneelse");
    // A first publish: nothing may be there by the time it's written.
    let put = pds.calls(PUT).last().unwrap().json();
    assert_eq!((put["collection"].as_str(), put["rkey"].as_str()), (Some(KEYBINDS), Some("self")));
    assert_eq!(put.get("swapRecord"), Some(&Value::Null));

    // Unchanged, nothing's written; changed, it replaces the version the directory has.
    let writes = pds.calls(PUT).len();
    assert!(!api::keybinds_publish(&root).unwrap().written);
    assert_eq!(pds.calls(PUT).len(), writes);
    keybinds::save(&root, &keys(&[("playback.next", &["n", "]"])])).unwrap();
    assert!(api::keybinds_publish(&root).unwrap().written);
    assert_eq!(pds.calls(PUT).last().unwrap().json()["swapRecord"], cid.as_str());
    assert_eq!(
        bindings(&pds.record(DID, KEYBINDS, "self").unwrap().0),
        json!([{ "action": "playback.next", "keys": ["n", "]"] }])
    );
    // Read only from the directory, without the session.
    assert!(pds.calls(PDS_GET).is_empty());
    assert!(pds.calls(GET).iter().all(|r| r.header("authorization").is_none()));
}

/// A session that may not publish keys is told to sign in again allowing just that, keeping what
/// it may do; signed out, nothing is published.
#[test]
fn publishing_keybinds_needs_its_scope() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    keybinds::save(&root, &keys(&[("playback.next", &["]"])])).unwrap();
    let e = api::keybinds_publish(&root).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::NotSignedIn), "{e}");

    let scopes = atmosphere::DEFAULT_SCOPES.join(" ");
    sign_in_as(&root, session_json(DID, "s1", &pds.url, &scopes, None, "2099-01-01T00:00:00.000Z"));
    let e = api::keybinds_publish(&root).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::MissingScope), "{e}");
    assert!(e.to_string().contains("doesn't allow locallibrary to publish your keys"), "{e}");
    let again = &e.detail().actions[0];
    assert_eq!(again.label, "Sign in again, allowing publishing your keys");
    let asked: Vec<String> = serde_json::from_value(again.params["scopes"].clone()).unwrap();
    assert_eq!(asked, atmosphere::player_scopes());
    assert!(!asked.contains(&format!("include:{PUBLISH_SET}")) && asked.contains(&KEYBINDS_SCOPE.to_string()));
    assert!(pds.calls(PUT).is_empty());
}

/// After signing in: with no keys published, the ones set here are (or nothing, with none); with
/// none set here, the account's are written here; the same keys are left alone; and different
/// ones are handed back to choose from. Records published before the directory kept them are
/// read again first.
#[test]
fn after_sign_in_loads_the_published_keys() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let computer = |name: &str| {
        let root = dir.path().join(name);
        sign_in(&root, &pds);
        root
    };

    let blank = computer("blank");
    assert_eq!(api::keybinds_after_sign_in(&blank).unwrap(), AfterSignIn::NonePublished { published_here: false });
    assert!(pds.record(DID, KEYBINDS, "self").is_none());

    let first = computer("first");
    let mine = keys(&[("playback.next", &["]"]), ("deck.hot-cue", &["&", "é"])]);
    keybinds::save(&first, &mine).unwrap();
    assert_eq!(api::keybinds_after_sign_in(&first).unwrap(), AfterSignIn::NonePublished { published_here: true });
    assert!(pds.record(DID, KEYBINDS, "self").is_some());

    let second = computer("second");
    let AfterSignIn::Loaded(loaded) = api::keybinds_after_sign_in(&second).unwrap() else { panic!("not loaded") };
    assert_eq!(loaded.keys, mine.keys);
    assert_eq!(keybinds::load(&second).unwrap(), loaded, "written to keybinds.json");
    assert_eq!(api::keybinds_after_sign_in(&second).unwrap(), AfterSignIn::Same);

    let third = computer("third");
    let spelled_otherwise = keys(&[("playback.next", &[" ] "]), ("deck.hot-cue", &["&", "é"])]);
    keybinds::save(&third, &spelled_otherwise).unwrap();
    assert_eq!(api::keybinds_after_sign_in(&third).unwrap(), AfterSignIn::Same);
    let differs = keys(&[("playback.next", &["n"])]);
    keybinds::save(&third, &differs).unwrap();
    let AfterSignIn::Differs(fetched) = api::keybinds_after_sign_in(&third).unwrap() else { panic!("not differs") };
    assert_eq!(fetched.from.did, DID);
    assert_eq!(fetched.uri, format!("at://{DID}/{KEYBINDS}/self"));
    assert_eq!(fetched.theirs.keys, mine.keys);
    assert_eq!(keybinds::load(&third).unwrap(), differs, "the user chooses: nothing's written");

    // Published before the directory kept keys: it reads the account again first.
    let earlier = json!({
        "$type": KEYBINDS,
        "bindings": [{ "action": "player.quit", "keys": ["ctrl-x"] }],
        "updatedAt": "2026-11-01T00:00:00.000Z"
    });
    pds.put_unindexed(DID, KEYBINDS, "self", earlier);
    let fourth = computer("fourth");
    let AfterSignIn::Loaded(loaded) = api::keybinds_after_sign_in(&fourth).unwrap() else { panic!("not loaded") };
    assert_eq!(loaded.keys, keys(&[("player.quit", &["ctrl-x"])]).keys);

    let signed_out = dir.path().join("signed-out");
    let e = api::keybinds_after_sign_in(&signed_out).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::NotSignedIn), "{e}");
    assert!(pds.calls(PDS_GET).is_empty());
}

/// Someone's keys come from the directory, by their DID or handle, never from their data server
/// and without a session; what this version can't read is said, not taken.
#[test]
fn import_reads_through_the_directory() {
    let pds = network();
    let theirs = "did:plc:someoneelse";
    pds.put(
        theirs,
        KEYBINDS,
        "self",
        json!({
            "$type": KEYBINDS,
            "bindings": [
                { "action": "playback.next", "keys": ["]"] },
                { "action": "visuals.sparkles", "keys": ["f6", "hyper-s"] }
            ],
            "updatedAt": "2026-11-20T18:00:00.000Z"
        }),
    );
    let fetched = api::keybinds_fetch(&format!(" @{theirs} ")).unwrap();
    assert_eq!((fetched.from.did.as_str(), fetched.from.handle.as_deref()), (theirs, None));
    assert_eq!(fetched.uri, format!("at://{theirs}/{KEYBINDS}/self"));
    assert_eq!(fetched.theirs.keys, keys(&[("playback.next", &["]"]), ("visuals.sparkles", &["f6"])]).keys);
    assert_eq!(fetched.skipped, ["visuals.sparkles: \"hyper-s\""]);
    let reads = pds.calls(GET);
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].params, [("uris".to_string(), fetched.uri.clone())]);
    assert!(reads[0].header("authorization").is_none() && pds.calls(PDS_GET).is_empty());

    let e = api::keybinds_fetch("did:plc:nobody").unwrap_err();
    assert_eq!(e.code(), "not_found", "{e}");
    assert!(e.to_string().contains("keys published by did:plc:nobody"), "{e}");
    pds.put(theirs, KEYBINDS, "self", json!({ "$type": lexicon::SETTINGS, "updatedAt": "2026-11-20T18:00:00.000Z" }));
    assert!(api::keybinds_fetch(theirs).unwrap_err().to_string().contains("isn't a keybinds record"));
    assert!(api::keybinds_fetch("not a handle!").is_err());
}
