//! Publishing to an Atmosphere account, against a pretend data server on 127.0.0.1
//! (`support/mock_pds.rs`): the requests' shapes, DPoP-bound writes, and what a refusal says.
//! Nothing here signs in to a real account or touches the network.

#![cfg(feature = "account")]

use crate::common;
use crate::mock_pds;

use std::path::Path;

use jacquard_oauth::authstore::ClientAuthStore;
use locallibrary::api::{self, HistoryPublishRequest, ImportPublishedSettings, ProfileRequest, SetSettings};
use locallibrary::atmosphere::lexicon::{PROFILE, SET, SETTINGS};
use locallibrary::atmosphere::store::LocalStore;
use locallibrary::atmosphere::{self, Client, Storage};
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
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

#[test]
fn a_set_and_the_profile_go_where_they_should() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    let drive = common::bigger_drive(dir.path(), "Main");
    common::add_history(&drive, "HISTORY 001", &[1, 2, 3, 4, 5, 6]);

    let req = HistoryPublishRequest {
        drive: drive.display().to_string(),
        list: "HISTORY 001".into(),
        date: Some("2026-10-03".into()),
        ..Default::default()
    };
    let plan = api::history_publish_plan(&root, &req, &Ctx::silent()).unwrap();
    let published = api::history_publish_apply(&root, &req, &Ctx::silent()).unwrap();
    assert_eq!(published.uri, format!("at://{DID}/{SET}/{}", plan.rkey), "the key the preview showed");
    let (first, _) = pds.record(DID, SET, &plan.rkey).unwrap();
    assert_eq!(first["tracks"].as_array().unwrap().len(), 6);
    assert_eq!(first["date"], "2026-10-03");

    // Again, without a date: the same set, as published, so nothing to write. What's published
    // is read once, for its date and for the plan.
    let undated = HistoryPublishRequest { date: None, ..req.clone() };
    let reads = pds.calls(GET).len();
    let plan = api::history_publish_plan(&root, &undated, &Ctx::silent()).unwrap();
    assert_eq!(pds.calls(GET).len(), reads + 1);
    assert!(!plan.changed && plan.notes.is_empty(), "{:?}", plan.notes);
    // A title replaces it, keeping when it was first published.
    let titled = HistoryPublishRequest { title: Some("Friday warm-up".into()), ..undated };
    let published = api::history_publish_apply(&root, &titled, &Ctx::silent()).unwrap();
    assert!(published.written);
    let (second, _) = pds.record(DID, SET, &plan.rkey).unwrap();
    assert_eq!((second["title"].as_str(), &second["createdAt"]), (Some("Friday warm-up"), &first["createdAt"]));
    assert!(pds.calls(PUT).last().unwrap().json()["swapRecord"].is_string());

    let profile = ProfileRequest { drives: vec![drive.display().to_string()], ..Default::default() };
    let published = api::profile_publish_apply(&root, &profile, &Ctx::silent()).unwrap();
    assert_eq!(published.uri, format!("at://{DID}/{PROFILE}/self"));
    let (record, _) = pds.record(DID, PROFILE, "self").unwrap();
    assert_eq!(record["sets"]["count"], 1);
    assert!(record.get("signature").is_none());
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
