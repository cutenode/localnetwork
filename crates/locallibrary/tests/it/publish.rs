//! Publishing to an Atmosphere account, against a pretend data server on 127.0.0.1
//! (`support/mock_pds.rs`): the requests' shapes, DPoP-bound writes, and what a refusal says.
//! Nothing here signs in to a real account or touches the network.

#![cfg(feature = "account")]

mod signing_in;

use crate::common;
use crate::mock_pds;

use std::path::Path;

use chrono::{NaiveDate, Utc};
use jacquard_oauth::authstore::ClientAuthStore;
use locallibrary::api::{self, ImportPublishedSettings, ProfileRequest, SetPublishRequest, SetSettings};
use locallibrary::atmosphere::lexicon::{self, PROFILE, SET, SETTINGS};
use locallibrary::atmosphere::store::LocalStore;
use locallibrary::atmosphere::{self, Client, Storage};
use locallibrary::error::Reason;
use locallibrary::profile::PrefsUpdate;
use locallibrary::progress::Ctx;
use locallibrary::sets::{EntryOrigin, Partner, Recording, Set, SetBook, SetEntry};
use locallibrary::store::Store;
use mock_pds::{MockPds, Writes, base64url};
use serde_json::{Value, json};
use sha2::Digest;

const DID: &str = "did:plc:testdj";
const PUT: &str = "com.atproto.repo.putRecord";
const GET: &str = "com.atproto.repo.getRecord";
const PUBLISHING: &str = "atproto repo:computer.localnetwork.locallibrary.settings \
                          repo:computer.localnetwork.locallibrary.profile repo:computer.localnetwork.locallibrary.set";

/// Sign `DID` in on this computer (the session in files, not the keychain) with a session for
/// `pds` that was granted `scopes`. Returns the session's DPoP key.
fn sign_in(root: &Path, pds: &str, scopes: &str, expires_at: &str, refresh: Option<&str>) -> Value {
    sign_in_as(root, DID, pds, scopes, expires_at, refresh)
}

/// [`sign_in`], as `did`.
fn sign_in_as(root: &Path, did: &str, pds: &str, scopes: &str, expires_at: &str, refresh: Option<&str>) -> Value {
    let key = serde_json::to_value(jacquard_oauth::utils::generate_key(&["ES256"]).unwrap()).unwrap();
    let session = serde_json::from_value(json!({
        "account_did": did,
        "session_id": "s1",
        "host_url": pds,
        "authserver_url": pds,
        "authserver_token_endpoint": format!("{pds}/oauth/token"),
        "authserver_revocation_endpoint": format!("{pds}/oauth/revoke"),
        "scopes": scopes,
        "dpop_key": key,
        "dpop_authserver_nonce": "",
        "dpop_host_nonce": "",
        "iss": pds,
        "sub": did,
        "aud": pds,
        "scope": scopes,
        "refresh_token": refresh,
        "access_token": "access-token",
        "token_type": "DPoP",
        "expires_at": expires_at,
    }))
    .unwrap();
    // Signing in makes the store folder a store first.
    Store::open_or_init(root).unwrap();
    let store = LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    rt.block_on(store.upsert_session(session)).unwrap();
    key
}

const LATER: &str = "2099-01-01T00:00:00.000Z";

fn set(root: &Path, values: &[(&str, &str)]) {
    let store = Store::open_or_init(root).unwrap();
    let values = values.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    api::settings_set(&store, &SetSettings { values, drive: None }).unwrap();
}

fn without(mut v: Value, field: &str) -> Value {
    v.as_object_mut().unwrap().remove(field);
    v
}

#[test]
fn my_settings_go_out_in_a_dpop_bound_put_record_and_come_back() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let key = sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    set(&root, &[("quantize", "off"), ("jog_mode", "cdj"), ("crossfader_curve", "constant-power")]);

    let plan = api::settings_publish_plan(&root).unwrap();
    let uri = format!("at://{DID}/{SETTINGS}/self");
    assert_eq!(plan.uri.as_deref(), Some(uri.as_str()));
    assert!(plan.current.is_none() && plan.changed && plan.missing_scope.is_none());
    // Reading needs no session: a plain getRecord.
    let reads = pds.calls(GET);
    assert_eq!(reads.len(), 1);
    assert!(reads[0].header("authorization").is_none() && reads[0].header("dpop").is_none());
    assert_eq!(
        (reads[0].query["repo"].as_str(), reads[0].query["collection"].as_str(), reads[0].query["rkey"].as_str()),
        (DID, SETTINGS, "self")
    );

    let published = api::settings_publish_apply(&root).unwrap();
    assert!(published.written);
    assert_eq!(published.uri, uri);
    let (stored, cid) = pds.record(DID, SETTINGS, "self").unwrap();
    assert_eq!(published.cid.as_deref(), Some(cid.as_str()));
    assert_eq!(stored["player"], json!({ "quantize": "off", "jogMode": "cdj" }));

    // The server asked for its DPoP nonce once; the write went again with it.
    let writes = pds.calls(PUT);
    assert_eq!(writes.len(), 2);
    let w = &writes[1];
    let body = w.json();
    assert_eq!(
        (body["repo"].as_str(), body["collection"].as_str(), body["rkey"].as_str()),
        (Some(DID), Some(SETTINGS), Some("self"))
    );
    // Nothing to replace yet, and nothing may be there by the time it's written.
    assert_eq!(body.get("swapRecord"), Some(&Value::Null), "nothing to replace yet");
    assert_eq!(without(body["record"].clone(), "updatedAt"), without(plan.record.clone(), "updatedAt"));
    assert_eq!(w.header("authorization"), Some("DPoP access-token"));
    let (header, claims) = w.dpop().unwrap();
    assert_eq!((header["typ"].as_str(), header["alg"].as_str()), (Some("dpop+jwt"), Some("ES256")));
    assert_eq!((&header["jwk"]["x"], &header["jwk"]["y"]), (&key["x"], &key["y"]), "bound to the session's key");
    assert!(header["jwk"].get("d").is_none(), "the private key stays home");
    assert_eq!(claims["htm"], "POST");
    assert!(claims["htu"].as_str().unwrap().starts_with(&format!("{}/xrpc/{PUT}", pds.url)));
    assert_eq!(claims["ath"], base64url(&sha2::Sha256::digest(b"access-token")));
    assert_eq!(claims["nonce"], "nonce-1");
    assert!(claims["jti"].is_string() && claims["iat"].is_number());
    assert_ne!(writes[0].dpop().unwrap().1["nonce"], "nonce-1");

    // Unchanged: nothing is written.
    let again = api::settings_publish_plan(&root).unwrap();
    assert!(!again.changed && again.current.is_some());
    assert!(!api::settings_publish_apply(&root).unwrap().written);
    assert_eq!(pds.calls(PUT).len(), 2);

    // A change replaces the record, but only the one that was read.
    set(&root, &[("quantize", "on")]);
    let plan = api::settings_publish_plan(&root).unwrap();
    assert!(plan.changed);
    assert!(api::settings_publish_apply(&root).unwrap().written);
    assert_eq!(pds.calls(PUT).last().unwrap().json()["swapRecord"], cid.as_str());

    // Published from another computer since the plan read it: a conflict, not lost.
    set(&root, &[("language", "german")]);
    let stale = api::settings_publish_plan(&root).unwrap();
    pds.put(DID, SETTINGS, "self", json!({ "$type": SETTINGS, "version": 1, "updatedAt": "2026-10-05T00:00:00.000Z" }));
    let err = locallibrary::publish::apply(&root, &stale).unwrap_err();
    assert_eq!(err.code(), "conflict", "{err}");
    // Applying the plan someone confirmed: it's stale.
    let err = stale.apply(None, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    // Planned again, it replaces what's there now.
    assert!(api::settings_publish_apply(&root).unwrap().written);

    // Another computer, signed out, imports them from the account's server.
    let other = dir.path().join("other");
    let from = ImportPublishedSettings { from: Some(DID.into()), pds: Some(pds.url.clone()), ..Default::default() };
    let plan = api::settings_import_published_plan(&other, &from).unwrap();
    assert_eq!(plan.from.did, DID);
    assert!(plan.changes.iter().all(|c| c.from.is_none()));
    assert_eq!(plan.changes.len(), 4);
    let profile = api::settings_import_published_apply(&other, &from).unwrap();
    assert_eq!(profile.base, Store::open(&root).unwrap().settings_profile().unwrap().base);
    // Signed in, it's the account's own by default.
    assert!(
        api::settings_import_published_plan(&root, &ImportPublishedSettings::default()).unwrap().changes.is_empty()
    );
}

#[test]
fn refusals_say_how_to_go_on() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    set(&root, &[("quantize", "off")]);

    pds.set_writes(Writes::RefuseScope);
    let err = api::settings_publish_apply(&root).unwrap_err();
    assert_eq!(err.code(), "account");
    assert!(err.to_string().contains("doesn't allow locallibrary to publish My Settings"), "{err}");
    assert!(err.to_string().contains("sign in again, allowing publishing"), "{err}");
    assert_eq!(err.reason(), Some(Reason::MissingScope));
    let sign_in_again = &err.detail().actions[0];
    assert_eq!(
        (sign_in_again.method.as_deref(), &sign_in_again.params["publish"]),
        (Some("account.login.begin"), &json!(true))
    );

    // A token the server no longer takes: jacquard tries again, then it's time to sign in.
    pds.set_writes(Writes::RefuseToken);
    let before = pds.calls(PUT).len();
    let err = api::settings_publish_apply(&root).unwrap_err();
    assert!(err.to_string().contains("didn't accept the session for did:plc:testdj"), "{err}");
    assert_eq!(err.reason(), Some(Reason::SessionEnded), "{err}");
    let refused = pds.calls(PUT)[before..].iter().filter(|r| r.dpop().unwrap().1["nonce"] == "nonce-1").count();
    assert_eq!(refused, 2, "tried again before giving up");

    pds.set_writes(Writes::RefuseRecord);
    let err = api::settings_publish_apply(&root).unwrap_err();
    assert!(err.to_string().contains("refused to publish My Settings (InvalidRequest: Record is too big)"), "{err}");
    assert!(pds.record(DID, SETTINGS, "self").is_none());

    // Signed in only to say who you are: the plan says so, and nothing is sent.
    sign_in(&root, &pds.url, "atproto", LATER, Some("refresh-2"));
    pds.set_writes(Writes::Accept);
    let plan = api::settings_publish_plan(&root).unwrap();
    assert_eq!(plan.missing_scope, Some(format!("repo:{SETTINGS}?action=create&action=update")));
    let before = pds.calls(PUT).len();
    let err = api::settings_publish_apply(&root).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::MissingScope), "{err}");
    assert_eq!(pds.calls(PUT).len(), before);
}

#[test]
fn a_session_that_ran_out_and_cant_be_refreshed_has_ended() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, "2026-01-01T00:00:00.000Z", None);
    set(&root, &[("quantize", "off")]);
    let err = api::settings_publish_apply(&root).unwrap_err();
    assert_eq!(err.code(), "account");
    assert!(err.to_string().contains("has ended"), "{err}");
    assert_eq!(err.reason(), Some(Reason::SessionEnded), "{err}");
    assert_eq!(err.detail().actions[0].method.as_deref(), Some("account.login.begin"));
    // jacquard looked up the authorization server to refresh; nothing was written.
    assert!(pds.requests().iter().any(|r| r.path == "/.well-known/oauth-authorization-server"));
    assert!(pds.calls(PUT).is_empty());
    // The session is gone from this computer.
    assert!(atmosphere::status(&root).unwrap().is_none());
    assert!(!root.join(atmosphere::store::SECRETS_FILE).exists());
}

/// A set with every field, built from `sessions`, saved in the store at `root`.
fn save_set(root: &Path, id: &str, sessions: &[&str]) -> Set {
    let at = Utc::now();
    let entry = |text: &str, origin: EntryOrigin, start: u32| SetEntry {
        file: Some(format!("/Contents/{text}.mp3")),
        start_seconds: Some(start),
        session: sessions.first().map(|s| s.to_string()),
        origin,
        ..SetEntry::added(text)
    };
    let mut glue = entry("Bicep - Glue", EntryOrigin::Recording, 0);
    (glue.isrc, glue.bpm, glue.key) = ("GB-AHT-17-00123".into(), 129.98, "Am".into());
    let set = Set {
        id: id.into(),
        title: Some("Friday warm-up".into()),
        date: NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
        kind: Some("club".into()),
        venue: Some("Fabric".into()),
        city: Some("London".into()),
        country: Some("GB".into()),
        event: Some("at://did:plc:club/community.lexicon.calendar.event/3lte3c7x43l2e".into()),
        with: vec![
            Partner { name: "Ayla".into(), did: Some("did:plc:ayla".into()) },
            Partner { name: "Sam".into(), did: None },
        ],
        link: Some("https://www.mixcloud.com/dj/friday-warm-up/".into()),
        recording: Some(Recording {
            path: "/Users/dj/Recordings/REC001.WAV".into(),
            report: Some("report-7f3a".into()),
        }),
        notes: Some("PRIVATE: the monitors were too loud".into()),
        sessions: sessions.iter().map(|s| s.to_string()).collect(),
        tracks: vec![
            glue,
            SetEntry { unnamed: true, ..entry("Unreleased - Promo", EntryOrigin::Recording, 300) },
            entry("Floorplan - Never Grow Old", EntryOrigin::Session, 600),
            entry("Calvin Harris feat. Rihanna - This Is What You Came For", EntryOrigin::Session, 900),
            entry("Four Tet - Baby", EntryOrigin::Added, 1200),
            entry("Overmono - So U Kno", EntryOrigin::Session, 1500),
        ],
        created_at: at,
        updated_at: at,
    };
    let store = Store::open_or_init(root).unwrap();
    let mut book = SetBook::load(&store).unwrap();
    book.sets.push(set.clone());
    book.save(&store).unwrap();
    set
}

fn publish_set(set: &str) -> SetPublishRequest {
    SetPublishRequest { set: set.into(), hide: vec![] }
}

#[test]
fn a_set_goes_out_as_its_record_with_nothing_private() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let set = save_set(&root, "3m2abcdefgh22", &["0123456789abcdef0123456789abcdef"]);
    let store = Store::open(&root).unwrap();
    let hide = PrefsUpdate {
        hide: vec!["floorplan - never grow old".into()],
        hide_artist: vec!["Rihanna".into()],
        ..Default::default()
    };
    api::profile_prefs_update(&store, &hide).unwrap();

    // Signed out, the record can be looked at (`--dry-run`), not published.
    let req = SetPublishRequest { set: "friday warm-up".into(), hide: vec![6] };
    let plan = api::set_publish_plan(&root, &req).unwrap();
    assert!(plan.account.is_none() && plan.uri.is_none() && plan.current.is_none());
    assert_eq!(plan.rkey, set.id);
    let r = &plan.record;
    lexicon::validate(SET, r).unwrap();
    assert_eq!(
        (&r["title"], &r["date"], &r["kind"], &r["venue"]),
        (&json!("Friday warm-up"), &json!("2026-10-03"), &json!("club"), &json!("Fabric"))
    );
    assert_eq!(r["location"], json!({ "name": "Fabric", "locality": "London", "country": "GB" }));
    assert_eq!(r["event"], "at://did:plc:club/community.lexicon.calendar.event/3lte3c7x43l2e");
    assert_eq!(r["with"], json!([{ "name": "Ayla", "did": "did:plc:ayla" }, { "name": "Sam" }]));
    assert_eq!(r["link"], "https://www.mixcloud.com/dj/friday-warm-up/");
    assert_eq!(r["sources"], json!(["history", "mix", "added"]));
    let tracks = r["tracks"].as_array().unwrap();
    assert_eq!(
        tracks[0],
        json!({ "title": "Glue", "artist": "Bicep", "isrc": "GBAHT1700123", "bpmCentis": 12998, "key": "8A", "startSeconds": 0 })
    );
    assert_eq!(tracks[1], json!({ "unidentified": true, "startSeconds": 300 }), "marked unnamed");
    assert_eq!(tracks[2], json!({ "unidentified": true, "startSeconds": 600 }), "the profile hides the track");
    assert_eq!(tracks[3], json!({ "unidentified": true, "startSeconds": 900 }), "the profile hides the artist");
    assert_eq!(tracks[4], json!({ "title": "Baby", "artist": "Four Tet", "startSeconds": 1200 }));
    assert_eq!(tracks[5], json!({ "unidentified": true, "startSeconds": 1500 }), "hidden as asked");
    assert_eq!(
        plan.notes,
        [
            "track 2 isn't named: marked unnamed (`sets edit --named`)",
            "track 6 isn't named: hidden as asked",
            "tracks 3, 4 aren't named: hidden by your profile (`profile unhide`)",
        ]
    );
    // The notes, the recording, the sessions, the files and the set's own bookkeeping stay here.
    let text = r.to_string();
    for private in
        ["PRIVATE", "REC001", "report-7f3a", "0123456789abcdef", "Contents", "updatedAt", "unnamed", "origin"]
    {
        assert!(!text.contains(private), "{private}: {text}");
    }
    assert_eq!(api::set_publish_apply(&root, &req).unwrap_err().code(), "account");

    let bad = |req: SetPublishRequest| api::set_publish_plan(&root, &req).unwrap_err().code();
    assert_eq!(bad(SetPublishRequest { hide: vec![7], ..req.clone() }), "invalid");
    assert_eq!(bad(SetPublishRequest { hide: vec![0], ..req.clone() }), "invalid");
    assert_eq!(bad(publish_set("Saturday")), "not_found");
    assert_eq!(api::set_publish_plan(&dir.path().join("nowhere"), &req).unwrap_err().code(), "not_found");
    // Without a country, there's no address to give the city in.
    let mut book = SetBook::load(&store).unwrap();
    book.sets[0].country = None;
    book.save(&store).unwrap();
    let plan = api::set_publish_plan(&root, &req).unwrap();
    assert!(plan.record.get("location").is_none() && plan.record["venue"] == "Fabric");
    assert_eq!(plan.notes.last().unwrap(), "the city is left out: it goes out with a country (`sets edit --country`)");
    // A set that doesn't fit the Lexicon isn't planned.
    book.sets[0].link = Some("mixcloud dot com".into());
    book.save(&store).unwrap();
    let err = api::set_publish_plan(&root, &req).unwrap_err().to_string();
    assert!(err.contains("link isn't a URI"), "{err}");
}

#[test]
fn a_set_and_the_profile_go_where_they_should() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    let set = save_set(&root, "3m2abcdefgh22", &[]);

    let plan = api::set_publish_plan(&root, &publish_set("last")).unwrap();
    assert_eq!(plan.uri, Some(format!("at://{DID}/{SET}/{}", set.id)));
    assert!(plan.current.is_none() && !plan.destructive);
    let published = api::set_publish_apply(&root, &publish_set("last")).unwrap();
    assert_eq!(published.uri, format!("at://{DID}/{SET}/{}", set.id), "the key is the set's id");
    let (first, _) = pds.record(DID, SET, &set.id).unwrap();
    assert_eq!(without(first.clone(), "createdAt"), without(plan.record.clone(), "createdAt"));

    // Again: as published, so nothing to write. What's published is read once.
    let reads = pds.calls(GET).len();
    let plan = api::set_publish_plan(&root, &publish_set("last")).unwrap();
    assert_eq!(pds.calls(GET).len(), reads + 1);
    assert!(!plan.changed && plan.noop, "{:?}", plan.notes);
    assert!(!api::set_publish_apply(&root, &publish_set("last")).unwrap().written);
    // Edited, it replaces its record, keeping when it was first published.
    let store = Store::open(&root).unwrap();
    let mut book = SetBook::load(&store).unwrap();
    book.sets[0].title = Some("Friday closing".into());
    book.save(&store).unwrap();
    let plan = api::set_publish_plan(&root, &publish_set("last")).unwrap();
    assert!(plan.changed && plan.destructive);
    assert!(plan.apply(None, &Ctx::silent()).unwrap().written);
    let (second, _) = pds.record(DID, SET, &set.id).unwrap();
    assert_eq!((second["title"].as_str(), &second["createdAt"]), (Some("Friday closing"), &first["createdAt"]));
    assert!(pds.calls(PUT).last().unwrap().json()["swapRecord"].is_string());
    assert_eq!(SetBook::load(&store).unwrap().sets[0].id, set.id, "the set keeps its id");

    // The set changed since the plan read it: refused, nothing sent.
    let plan = api::set_publish_plan(&root, &SetPublishRequest { set: "last".into(), hide: vec![1] }).unwrap();
    let mut book = SetBook::load(&store).unwrap();
    book.sets[0].title = Some("Friday".into());
    book.save(&store).unwrap();
    let writes = pds.calls(PUT).len();
    assert_eq!(plan.apply(None, &Ctx::silent()).unwrap_err().code(), "stale_plan");
    assert_eq!(pds.calls(PUT).len(), writes);

    let drive = common::bigger_drive(dir.path(), "Main");
    common::add_history(&drive, "HISTORY 001", &[1, 2, 3, 4, 5, 6]);
    let profile = ProfileRequest { drives: vec![drive.display().to_string()], ..Default::default() };
    let published = api::profile_publish_apply(&root, &profile, &Ctx::silent()).unwrap();
    assert_eq!(published.uri, format!("at://{DID}/{PROFILE}/self"));
    let (record, _) = pds.record(DID, PROFILE, "self").unwrap();
    assert_eq!(record["sets"]["count"], 1);
    assert!(record.get("signature").is_none());
}

const SESSION: &str = "0123456789abcdef0123456789abcdef";
const OLD_KEY: &str = "3lzzzzzzzzz22";

/// What `plays publish` left: a set record under `OLD_KEY`, remembered for `SESSION` and `did`.
fn published_by_plays_publish(root: &Path, pds: &MockPds, did: &str) {
    let old = json!({
        "$type": SET,
        "date": "2026-10-03",
        "source": "history",
        "tracks": [{ "title": "Glue", "artist": "Bicep" }],
        "createdAt": "2026-10-04T09:30:00.000Z",
    });
    pds.put(did, SET, OLD_KEY, old);
    let keys = json!({ did: { SESSION: OLD_KEY } });
    std::fs::write(root.join(locallibrary::publish::SET_KEYS_FILE), keys.to_string()).unwrap();
}

#[test]
fn a_set_from_a_list_plays_publish_published_replaces_that_record() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    published_by_plays_publish(&root, &pds, DID);
    let set = save_set(&root, "3m2abcdefgh22", &[SESSION]);

    let plan = api::set_publish_plan(&root, &publish_set(&set.id)).unwrap();
    assert_eq!(plan.rkey, OLD_KEY);
    assert!(plan.current.is_some() && plan.destructive, "it replaces the old record");
    assert!(
        plan.notes.contains(&format!(
            "replaces what `plays publish` published from session 01234567; the set's id becomes {OLD_KEY}"
        )),
        "{:?}",
        plan.notes
    );
    assert!(plan.apply(None, &Ctx::silent()).unwrap().written);
    let (record, _) = pds.record(DID, SET, OLD_KEY).unwrap();
    assert!(record.get("source").is_none() && record["sources"].is_array());
    assert_eq!(record["createdAt"], "2026-10-04T09:30:00.000Z", "first published by plays publish");
    assert!(pds.record(DID, SET, &set.id).is_none(), "not published twice");
    let store = Store::open(&root).unwrap();
    let book = SetBook::load(&store).unwrap();
    assert_eq!(book.sets[0].id, OLD_KEY, "the set took the key as its id");
    // From now on it's the set's own key.
    let plan = api::set_publish_plan(&root, &publish_set("last")).unwrap();
    assert_eq!((plan.rkey.as_str(), plan.noop), (OLD_KEY, true));
    assert!(plan.notes.iter().all(|n| !n.contains("replaces")), "{:?}", plan.notes);

    // Another set from the same list doesn't take the key the first one has.
    let other = save_set(&root, "3m4bbbbbbbb22", &[SESSION]);
    assert_eq!(api::set_publish_plan(&root, &publish_set(&other.id)).unwrap().rkey, other.id);
}

#[test]
fn a_set_keeps_its_own_key_when_the_old_one_doesnt_fit() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    published_by_plays_publish(&root, &pds, DID);
    let key = |id: &str| {
        let plan = api::set_publish_plan(&root, &publish_set(id)).unwrap();
        assert!(plan.notes.iter().all(|n| !n.contains("replaces")), "{:?}", plan.notes);
        plan.rkey.clone()
    };
    // Built from that list and another.
    let two = save_set(&root, "3m2aaaaaaaa22", &[SESSION, "fedcba9876543210fedcba9876543210"]);
    assert_eq!(key(&two.id), two.id);
    // Published under its own id already.
    let own = save_set(&root, "3m3aaaaaaaa22", &[SESSION]);
    pds.put(
        DID,
        SET,
        &own.id,
        json!({ "$type": SET, "date": "2026-10-03", "tracks": [], "createdAt": "2026-10-05T00:00:00.000Z" }),
    );
    assert_eq!(key(&own.id), own.id);
    // Remembered for another account.
    let other = dir.path().join("other");
    sign_in(&other, &pds.url, PUBLISHING, LATER, Some("refresh-2"));
    published_by_plays_publish(&other, &pds, "did:plc:someoneelse");
    let set = save_set(&other, "3m2abcdefgh22", &[SESSION]);
    assert_eq!(api::set_publish_plan(&other, &publish_set(&set.id)).unwrap().rkey, set.id);
}

#[test]
fn a_first_publish_doesnt_replace_one_made_meanwhile() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    set(&root, &[("quantize", "off")]);
    let plan = api::settings_publish_plan(&root).unwrap();
    assert!(plan.current.is_none());
    // Another computer publishes first.
    let theirs = json!({ "$type": SETTINGS, "version": 1, "updatedAt": "2026-10-05T00:00:00.000Z" });
    pds.put(DID, SETTINGS, "self", theirs.clone());
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    assert_eq!(pds.record(DID, SETTINGS, "self").unwrap().0, theirs, "theirs stays");
}

#[test]
fn a_plan_is_written_only_to_the_account_it_was_for() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    set(&root, &[("quantize", "off")]);
    let plan = api::settings_publish_plan(&root).unwrap();
    assert_eq!(plan.account.as_ref().unwrap().did, DID);
    // Someone signs in with another account (in another window, say) before it's confirmed.
    sign_in_as(&root, "did:plc:someoneelse", &pds.url, PUBLISHING, LATER, Some("refresh-2"));
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    assert!(err.to_string().contains("did:plc:someoneelse is signed in now"), "{err}");
    assert!(pds.calls(PUT).is_empty() && pds.record("did:plc:someoneelse", SETTINGS, "self").is_none());
}

#[test]
fn checking_asks_the_server_and_says_when_it_refuses() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, "atproto", LATER, Some("refresh-1"));
    pds.set_account(DID, "dj.example.com");
    let info = api::account_check(&root, &Ctx::silent()).unwrap();
    assert_eq!(info.handle.as_deref(), Some("dj.example.com"));
    let asked = pds.calls("com.atproto.server.getSession");
    assert_eq!(asked.last().unwrap().header("authorization"), Some("DPoP access-token"));

    // The DJ revoked locallibrary in their account's settings: the token is refused.
    pds.set_writes(Writes::RefuseToken);
    let err = api::account_check(&root, &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::SessionEnded), "{err}");
    assert!(err.to_string().contains("didn't accept the session"), "{err}");
}

#[test]
fn signing_out_revokes_the_refresh_token_too() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, "atproto", LATER, Some("refresh-1"));
    let r = api::account_logout(&root).unwrap();
    assert!(r.revoked && r.left_behind.is_none());
    let revoked: Vec<String> = pds
        .requests()
        .iter()
        .filter(|r| r.path == "/oauth/revoke")
        .map(|r| String::from_utf8_lossy(&r.body).into_owned())
        .collect();
    assert_eq!(revoked.len(), 2, "{revoked:?}");
    assert!(revoked[0].contains("token=refresh-1") && revoked[1].contains("token=access-token"), "{revoked:?}");
    assert!(atmosphere::status(&root).unwrap().is_none());

    // A server that won't revoke them: signed out here, and said so.
    sign_in(&root, &pds.url, "atproto", LATER, Some("refresh-1"));
    pds.set_revocation(400);
    let r = api::account_logout(&root).unwrap();
    assert!(r.account.is_some() && !r.revoked);
    assert!(atmosphere::status(&root).unwrap().is_none());
}

#[test]
fn signing_out_of_a_session_that_has_ended_says_whose_it_was() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    // Ran out, with nothing to refresh it: finding that out removes it.
    sign_in(&root, &pds.url, "atproto", "2026-01-01T00:00:00.000Z", None);
    let r = api::account_logout(&root).unwrap();
    assert_eq!(r.account.map(|a| a.did).as_deref(), Some(DID));
    assert!(!r.revoked && r.left_behind.is_none());
    assert!(atmosphere::status(&root).unwrap().is_none());
}

#[test]
fn importing_adds_to_the_base_unless_asked_to_replace_it() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    set(&root, &[("quantize", "off"), ("jog_mode", "cdj")]);
    let mut unknown: serde_json::Map<String, Value> =
        (0..30).map(|i| (format!("newSetting{i}"), Value::from("on"))).collect();
    unknown.insert("quantize".into(), "on".into());
    unknown.insert("evil\u{1b}]0;owned\u{7}".into(), "on".into());
    let theirs = json!({ "$type": SETTINGS, "version": 1, "player": unknown, "updatedAt": "2026-10-05T00:00:00.000Z" });
    pds.put(DID, SETTINGS, "self", theirs);
    let from = ImportPublishedSettings { from: Some(DID.into()), pds: Some(pds.url.clone()), replace: false };

    // Theirs over yours; what they didn't publish stays.
    let plan = api::settings_import_published_plan(&root, &from).unwrap();
    assert_eq!(plan.changes.len(), 1, "{:?}", plan.changes);
    assert_eq!((plan.changes[0].from.as_deref(), plan.changes[0].to.as_deref()), (Some("off"), Some("on")));
    assert!(plan.summary[0].starts_with("add the 1 settings"), "{:?}", plan.summary);
    // A few of the settings this version doesn't know say enough, and none plays with the terminal.
    assert_eq!(plan.warnings.len(), 21, "{:?}", plan.warnings);
    assert!(plan.warnings.last().unwrap().contains("left out 11 more"), "{:?}", plan.warnings);
    assert!(plan.published.skipped.iter().all(|s| !s.chars().any(char::is_control)), "{:?}", plan.published.skipped);
    let profile = api::settings_import_published_apply(&root, &from).unwrap();
    assert_eq!(profile.base, [("jog_mode".to_string(), "cdj".to_string()), ("quantize".into(), "on".into())].into());

    // Asked to replace: the base becomes theirs.
    let replace = ImportPublishedSettings { replace: true, ..from };
    let plan = api::settings_import_published_plan(&root, &replace).unwrap();
    assert_eq!(plan.changes.len(), 1);
    assert_eq!((plan.changes[0].key.as_str(), plan.changes[0].to.as_deref()), ("jog_mode", None));
    let profile = api::settings_import_published_apply(&root, &replace).unwrap();
    assert_eq!(profile.base, [("quantize".to_string(), "on".to_string())].into());
}
