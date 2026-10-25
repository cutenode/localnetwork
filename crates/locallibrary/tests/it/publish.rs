//! Publishing to an Atmosphere account, against a pretend data server on 127.0.0.1
//! (`support/mock_pds.rs`), which stands in for Local Network's directory and website too: the
//! requests' shapes, DPoP-bound writes, reads that only go to the directory, and what a refusal
//! says. Nothing here signs in to a real account or touches the network.

#![cfg(feature = "account")]

mod signing_in;

use crate::common;
use crate::mock_pds;
use crate::session::{DID, session_json, sign_in_as};

use std::path::Path;

use chrono::{NaiveDate, Utc};
use locallibrary::Track;
use locallibrary::api::{self, ImportPublishedSettings, ProfileRequest, SetPublishRequest, SetSettings};
use locallibrary::atmosphere;
use locallibrary::atmosphere::lexicon::{self, PROFILE, SET, SETTINGS};
use locallibrary::error::Reason;
use locallibrary::mix::{Transition, report};
use locallibrary::profile::PrefsUpdate;
use locallibrary::progress::Ctx;
use locallibrary::publish;
use locallibrary::sets::{EntryOrigin, MusicBrainzIds, Partner, Recording, Set, SetBook, SetEntry};
use locallibrary::store::Store;
use loclib_plugin::StrongRef;
use mock_pds::{MockPds, Writes, base64url};
use serde_json::{Value, json};
use sha2::Digest;
use uuid::Uuid;

const PUT: &str = "com.atproto.repo.putRecord";
/// How a record is read: from the directory, never from a data server.
const GET: &str = "computer.localnetwork.directory.getRecords";
const PDS_GET: &str = "com.atproto.repo.getRecord";
const REQUEST_INDEX: &str = "computer.localnetwork.directory.requestIndex";
const PUBLISHING: &str = "atproto repo:computer.localnetwork.locallibrary.settings \
                          repo:computer.localnetwork.dj.profile repo:computer.localnetwork.dj.set \
                          rpc:computer.localnetwork.directory.requestIndex?aud=*";

/// A pretend data server, which Local Network's directory and website are at too for this test.
fn network() -> MockPds {
    let pds = MockPds::start();
    locallibrary::testing::directory_on_this_thread(Some(pds.url.clone()));
    pds
}

/// Sign `DID` in on this computer (the session in files, not the keychain) with a session for
/// `pds` that was granted `scopes`. Returns the session's DPoP key.
fn sign_in(root: &Path, pds: &str, scopes: &str, expires_at: &str, refresh: Option<&str>) -> Value {
    let session = session_json(DID, "s1", pds, scopes, refresh, expires_at);
    let key = session["dpop_key"].clone();
    sign_in_as(root, session);
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
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let key = sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    set(&root, &[("quantize", "off"), ("jog_mode", "cdj"), ("crossfader_curve", "constant-power")]);

    let plan = api::settings_publish_plan(&root).unwrap();
    let uri = format!("at://{DID}/{SETTINGS}/self");
    assert_eq!(plan.uri.as_deref(), Some(uri.as_str()));
    assert!(plan.current.is_none() && plan.changed && plan.missing_scope.is_none());
    assert!(plan.notes.is_empty(), "{:?}", plan.notes);
    // Reading needs no session: the directory's getRecords, and never the data server.
    let reads = pds.calls(GET);
    assert_eq!(reads.len(), 1);
    assert!(reads[0].header("authorization").is_none() && reads[0].header("dpop").is_none());
    assert_eq!(reads[0].params, [("uris".to_string(), uri.clone())]);
    assert!(pds.calls(PDS_GET).is_empty());

    let published = api::settings_publish_apply(&root).unwrap();
    assert!(published.written);
    assert_eq!(published.uri, uri);
    let (stored, cid) = pds.record(DID, SETTINGS, "self").unwrap();
    assert_eq!(published.cid, cid);
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
    pds.put(DID, SETTINGS, "self", json!({ "$type": SETTINGS, "updatedAt": "2026-10-05T00:00:00.000Z" }));
    let err = locallibrary::publish::apply(&root, &stale).unwrap_err();
    assert_eq!(err.code(), "conflict", "{err}");
    assert!(err.to_string().contains("isn't the version Local Network's directory has; run it again"), "{err}");
    // Applying the plan someone confirmed: it's stale.
    let err = stale.apply(None, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    // Planned again, it replaces what's there now.
    assert!(api::settings_publish_apply(&root).unwrap().written);

    // Another computer, signed out, imports them from the account's server.
    let other = dir.path().join("other");
    let from = ImportPublishedSettings { from: Some(DID.into()), ..Default::default() };
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
    // The directory was asked once to read what the account published before it kept it.
    assert_eq!(pds.calls(REQUEST_INDEX).len(), 1);
    assert!(pds.calls(PDS_GET).is_empty());
}

#[test]
fn refusals_say_how_to_go_on() {
    let pds = network();
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
    let pds = network();
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

/// The plyr.fm track a set's recording is, and its version.
const PLYR_TRACK: &str = "at://did:plc:dj/fm.plyr.track/3m2plyrtrack2";
const PLYR_TRACK_CID: &str = "bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm";

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
        link_name: Some("Mixcloud".into()),
        recording_record: Some(StrongRef { uri: PLYR_TRACK.into(), cid: PLYR_TRACK_CID.into() }),
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

/// A set of `n` tracks the recording `r` heard in turn, each blended in over the one before, with
/// every field filled: names `len` characters long (as long as the Lexicon takes an artist or
/// release), tagged with MusicBrainz IDs. Saved in the store at `root` with `r`.
fn save_heard_set(root: &Path, id: &str, n: usize, len: usize) {
    let store = Store::open_or_init(root).unwrap();
    let tracks: Vec<Track> = (0..n)
        .map(|i| Track {
            title: format!("{i:0len$}"),
            artist: format!("{i:a>len$}"),
            album: format!("{i:r>len$}"),
            isrc: "GBAHT1700123".into(),
            key: "12A".into(),
            bpm: 174.99,
            file_path: format!("/Contents/{i}.mp3"),
            ..Track::default()
        })
        .collect();
    let plays: Vec<(&Track, f64, f64)> = tracks.iter().enumerate().map(|(i, t)| (t, i as f64 * 180.0, 0.9)).collect();
    let mut r = crate::sets::report(&plays, None);
    r.transitions = (1..n)
        .map(|to| Transition {
            from: to - 1,
            to,
            at: r.plays[to].start,
            overlap: 20.0,
            harmony: Some("energy boost".into()),
            tempo_change: Some(-15.9),
        })
        .collect();
    let report = report::save(&store, &r).unwrap();
    let id_of = |i: usize| Uuid::from_u128(i as u128);
    let set = Set {
        id: id.into(),
        date: NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
        recording: Some(Recording { path: "/Users/dj/set.wav".into(), report: Some(report) }),
        tracks: r
            .plays
            .iter()
            .enumerate()
            .map(|(i, p)| SetEntry {
                start_seconds: Some(p.start_second()),
                musicbrainz: MusicBrainzIds {
                    recording: Some(id_of(i)),
                    release: Some(id_of(i)),
                    artists: vec![id_of(i)],
                },
                ..SetEntry::from_track(&p.track, EntryOrigin::Recording, None)
            })
            .collect(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        ..Default::default()
    };
    let mut book = SetBook::load(&store).unwrap();
    book.sets.push(set);
    book.save(&store).unwrap();
}

/// A set built from a recording says how each track came in, from the recording's report. As
/// many tracks as a set record holds fit a record and the request writing it, with every field
/// of every track filled as long as the Lexicon takes (ASCII); an older data server's 150 KB
/// doesn't, which the plan says. One track more is refused.
#[test]
fn a_set_says_how_tracks_came_in_and_holds_as_many_as_a_record_can() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    save_heard_set(&root, "3m2abcdefgh22", 2, 12);
    let plan = api::set_publish_plan(&root, &publish_set("3m2abcdefgh22")).unwrap();
    let tracks = plan.record["tracks"].as_array().unwrap();
    assert!(tracks[0].get("transitionIn").is_none(), "the first track");
    assert_eq!(
        tracks[1]["transitionIn"],
        json!({ "technique": "blend", "durationMs": 20000, "harmony": "energyBoost", "tempoChangePermille": -159 })
    );
    assert_eq!(tracks[1]["recordingMbId"], format!("mbid:{}", Uuid::from_u128(1)));
    assert!(plan.notes.is_empty(), "{:?}", plan.notes);

    let most = publish::SET_TRACK_LIMIT;
    save_heard_set(&root, "3m2abcdefgh23", most, 256);
    let plan = api::set_publish_plan(&root, &publish_set("3m2abcdefgh23")).unwrap();
    let r = &plan.record;
    assert_eq!(r["tracks"][most - 1]["title"].as_str().unwrap().len(), 256);
    assert!(r["tracks"][most - 1]["transitionIn"].is_object());
    let request = json!({ "repo": DID, "collection": SET, "rkey": plan.rkey, "record": r, "swapRecord": Value::Null });
    let size = request.to_string().len();
    assert!(size < 1_000_000, "{size} bytes");
    assert!(size > publish::OLDER_PDS_REQUEST_BYTES, "{size} bytes");
    let record = r.to_string().len();
    let note =
        format!("the record is {} KB: a data server not updated since May 2026 refuses one over 150 KB", record / 1024);
    assert_eq!(plan.notes, [note]);

    save_heard_set(&root, "3m2abcdefgh24", most + 1, 1);
    let err = api::set_publish_plan(&root, &publish_set("3m2abcdefgh24")).unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(
        err.to_string().contains(&format!("the set has {} tracks, more than a set record holds ({most})", most + 1)),
        "{err}"
    );
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
    assert_eq!(
        r["location"],
        json!({ "$type": lexicon::ADDRESS, "name": "Fabric", "locality": "London", "country": "GB" })
    );
    assert_eq!(r["event"], "at://did:plc:club/community.lexicon.calendar.event/3lte3c7x43l2e");
    assert_eq!(r["playedWith"], json!([{ "name": "Ayla", "did": "did:plc:ayla" }, { "name": "Sam" }]));
    assert_eq!(r["links"], json!([{ "uri": "https://www.mixcloud.com/dj/friday-warm-up/", "name": "Mixcloud" }]));
    assert_eq!(r["recording"], json!({ "uri": PLYR_TRACK, "cid": PLYR_TRACK_CID }));
    assert_eq!(r["sources"], json!([{ "kind": "history" }, { "kind": "mix" }, { "kind": "added" }]));
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
    assert!(err.contains("links[0].uri isn't a URI"), "{err}");
}

#[test]
fn a_set_and_the_profile_go_where_they_should() {
    let pds = network();
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
    assert_eq!(record["sets"]["total"]["count"], 1);
    assert!(record.get("signature").is_none());
}

const SESSION: &str = "0123456789abcdef0123456789abcdef";
const OLD_KEY: &str = "3lzzzzzzzzz22";

/// What `plays publish` left: the key it published the list `SESSION` under, remembered for `did`.
fn published_by_plays_publish(root: &Path, did: &str) {
    let keys = json!({ did: { SESSION: OLD_KEY } });
    std::fs::write(root.join(locallibrary::publish::SET_KEYS_FILE), keys.to_string()).unwrap();
}

#[test]
fn a_set_from_a_list_plays_publish_published_takes_its_key() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    published_by_plays_publish(&root, DID);
    let set = save_set(&root, "3m2abcdefgh22", &[SESSION]);

    let plan = api::set_publish_plan(&root, &publish_set(&set.id)).unwrap();
    assert_eq!(plan.rkey, OLD_KEY);
    assert!(plan.current.is_none() && !plan.destructive, "nothing of today's sets is replaced");
    let note = format!("takes the key `plays publish` gave session 01234567; the set's id becomes {OLD_KEY}");
    assert!(plan.notes.contains(&note), "{:?}", plan.notes);
    assert!(plan.apply(None, &Ctx::silent()).unwrap().written);
    let (record, _) = pds.record(DID, SET, OLD_KEY).unwrap();
    assert!(record.get("source").is_none() && record["sources"].is_array());
    assert!(pds.record(DID, SET, &set.id).is_none(), "not published twice");
    let store = Store::open(&root).unwrap();
    let book = SetBook::load(&store).unwrap();
    assert_eq!(book.sets[0].id, OLD_KEY, "the set took the key as its id");
    // From now on it's the set's own key.
    let plan = api::set_publish_plan(&root, &publish_set("last")).unwrap();
    assert_eq!((plan.rkey.as_str(), plan.noop), (OLD_KEY, true));
    assert!(plan.notes.iter().all(|n| !n.contains("plays publish")), "{:?}", plan.notes);

    // Another set from the same list doesn't take the key the first one has.
    let other = save_set(&root, "3m4bbbbbbbb22", &[SESSION]);
    assert_eq!(api::set_publish_plan(&root, &publish_set(&other.id)).unwrap().rkey, other.id);
}

#[test]
fn a_set_keeps_its_own_key_when_the_old_one_doesnt_fit() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    published_by_plays_publish(&root, DID);
    let key = |id: &str| {
        let plan = api::set_publish_plan(&root, &publish_set(id)).unwrap();
        assert!(plan.notes.iter().all(|n| !n.contains("plays publish")), "{:?}", plan.notes);
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
    published_by_plays_publish(&other, "did:plc:someoneelse");
    let set = save_set(&other, "3m2abcdefgh22", &[SESSION]);
    assert_eq!(api::set_publish_plan(&other, &publish_set(&set.id)).unwrap().rkey, set.id);
}

#[test]
fn a_first_publish_doesnt_replace_one_made_meanwhile() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    set(&root, &[("quantize", "off")]);
    let plan = api::settings_publish_plan(&root).unwrap();
    assert!(plan.current.is_none());
    // Another computer publishes first.
    let theirs = json!({ "$type": SETTINGS, "updatedAt": "2026-10-05T00:00:00.000Z" });
    pds.put(DID, SETTINGS, "self", theirs.clone());
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    assert_eq!(pds.record(DID, SETTINGS, "self").unwrap().0, theirs, "theirs stays");
}

#[test]
fn a_plan_is_written_only_to_the_account_it_was_for() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    set(&root, &[("quantize", "off")]);
    let plan = api::settings_publish_plan(&root).unwrap();
    assert_eq!(plan.account.as_ref().unwrap().did, DID);
    // Someone signs in with another account (in another window, say) before it's confirmed.
    sign_in_as(&root, session_json("did:plc:someoneelse", "s1", &pds.url, PUBLISHING, Some("refresh-2"), LATER));
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    assert!(err.to_string().contains("did:plc:someoneelse is signed in now"), "{err}");
    assert!(pds.calls(PUT).is_empty() && pds.record("did:plc:someoneelse", SETTINGS, "self").is_none());
}

#[test]
fn checking_asks_the_server_and_says_when_it_refuses() {
    let pds = network();
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
    let pds = network();
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
    let pds = network();
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
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    set(&root, &[("quantize", "off"), ("jog_mode", "cdj")]);
    let mut unknown: serde_json::Map<String, Value> =
        (0..30).map(|i| (format!("newSetting{i}"), Value::from("on"))).collect();
    unknown.insert("quantize".into(), "on".into());
    unknown.insert("evil\u{1b}]0;owned\u{7}".into(), "on".into());
    let theirs = json!({ "$type": SETTINGS, "player": unknown, "updatedAt": "2026-10-05T00:00:00.000Z" });
    pds.put(DID, SETTINGS, "self", theirs);
    let from = ImportPublishedSettings { from: Some(DID.into()), replace: false };

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

/// What the account published before Local Network's directory kept it: the first plan has the
/// directory read the account again, with a token the account's server signed for that one method
/// at the website, and sees it; from then on the directory isn't asked again.
#[test]
fn what_was_published_before_the_directory_kept_it_is_read_again_once() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    set(&root, &[("quantize", "off")]);
    let earlier = json!({ "$type": SETTINGS, "player": { "quantize": "on" }, "updatedAt": "2026-10-01T00:00:00.000Z" });
    let cid = pds.put_unindexed(DID, SETTINGS, "self", earlier.clone());
    pds.put_unindexed("did:plc:someoneelse", SETTINGS, "self", earlier.clone());

    let plan = api::settings_publish_plan(&root).unwrap();
    assert_eq!(plan.current.as_ref().map(|c| (&c.value, c.cid.as_str())), Some((&earlier, cid.as_str())));
    assert!(plan.notes.is_empty(), "{:?}", plan.notes);
    let asked = pds.calls("com.atproto.server.getServiceAuth");
    assert_eq!(asked.len(), 1);
    let website = format!("did:web:{}", pds.url.trim_start_matches("http://").replace(':', "%3A"));
    assert_eq!((asked[0].query["aud"].as_str(), asked[0].query["lxm"].as_str()), (website.as_str(), REQUEST_INDEX));
    assert_eq!(asked[0].header("authorization"), Some("DPoP access-token"));
    let index = pds.calls(REQUEST_INDEX);
    assert_eq!(index.len(), 1);
    assert!(index[0].method == "POST" && index[0].header("authorization").unwrap().starts_with("Bearer "));
    assert!(pds.indexed("did:plc:someoneelse", SETTINGS, "self").is_none(), "only the caller's own");

    // Replacing it names the version the directory had.
    assert!(plan.apply(None, &Ctx::silent()).unwrap().written);
    assert_eq!(pds.calls(PUT).last().unwrap().json()["swapRecord"], cid.as_str());
    api::settings_publish_plan(&root).unwrap();
    assert_eq!(pds.calls(REQUEST_INDEX).len(), 1, "asked once");
    assert_eq!(atmosphere::store::AccountFile::load(&root).unwrap().indexed, [DID]);
    assert!(pds.calls(PDS_GET).is_empty());
}

/// A directory that hasn't finished reading, or a session that may not ask it to: the plan says
/// so and goes on with what the directory has, and it's asked again next time. Replacing a record
/// the directory doesn't have yet is refused at the data server, saying to run it again.
#[test]
fn a_directory_that_hasnt_read_the_account_yet_is_asked_again() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    set(&root, &[("quantize", "off")]);
    pds.put_unindexed(DID, SETTINGS, "self", json!({ "$type": SETTINGS, "updatedAt": "2026-10-01T00:00:00.000Z" }));
    pds.set_index_finishes(false);
    let plan = api::settings_publish_plan(&root).unwrap();
    assert!(plan.current.is_none());
    assert_eq!(
        plan.notes,
        [
            "Local Network's directory couldn't be asked to read this account's earlier records (it's still reading them)"
        ]
    );
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    assert!(err.to_string().contains("run it again in a minute, once the directory has caught up"), "{err}");

    pds.set_index_finishes(true);
    let plan = api::settings_publish_plan(&root).unwrap();
    assert!(plan.current.is_some() && plan.notes.is_empty(), "{:?}", plan.notes);
    assert_eq!(pds.calls(REQUEST_INDEX).len(), 2);

    // Signed in before publishing's permission set let it ask: it can't, and says how to.
    let other = dir.path().join("other");
    let without_asking = PUBLISHING.replace(" rpc:computer.localnetwork.directory.requestIndex?aud=*", "");
    sign_in(&other, &pds.url, &without_asking, LATER, Some("refresh-2"));
    set(&other, &[("quantize", "off")]);
    let plan = api::settings_publish_plan(&other).unwrap();
    assert!(
        plan.notes[0].ends_with("(the session doesn't allow it: sign in again, allowing publishing)"),
        "{:?}",
        plan.notes
    );
    assert_eq!(pds.calls(REQUEST_INDEX).len(), 2, "nothing asked");
}

/// The directory lags behind a write for a moment: a plan made meanwhile replaces nothing it
/// didn't see, and once the directory catches up, publishing goes on.
#[test]
fn a_write_the_directory_hasnt_caught_up_with_isnt_written_over() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    set(&root, &[("quantize", "off")]);
    assert!(api::settings_publish_apply(&root).unwrap().written);
    pds.set_firehose(false);
    set(&root, &[("quantize", "on")]);
    assert!(api::settings_publish_apply(&root).unwrap().written);
    let (newest, _) = pds.record(DID, SETTINGS, "self").unwrap();

    set(&root, &[("jog_mode", "cdj")]);
    let err = api::settings_publish_apply(&root).unwrap_err();
    assert_eq!((err.code(), err.reason()), ("conflict", Some(Reason::Changed)), "{err}");
    assert_eq!(pds.record(DID, SETTINGS, "self").unwrap().0, newest, "the newest stays");
    pds.set_firehose(true);
    assert!(api::settings_publish_apply(&root).unwrap().written);
    assert_eq!(pds.record(DID, SETTINGS, "self").unwrap().0["player"]["jogMode"], "cdj");
}

/// Someone else's settings the directory doesn't have: it says so, without asking it to read
/// their account (only the account itself can).
#[test]
fn someone_elses_settings_the_directory_doesnt_have_arent_found() {
    let pds = network();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    pds.put_unindexed("did:plc:someoneelse", SETTINGS, "self", json!({ "$type": SETTINGS }));
    let from = ImportPublishedSettings { from: Some("did:plc:someoneelse".into()), ..Default::default() };
    let err = api::settings_import_published_plan(&root, &from).unwrap_err();
    assert_eq!(err.code(), "not_found");
    assert_eq!(
        err.to_string(),
        "not found: My Settings published by did:plc:someoneelse (Local Network's directory doesn't have them yet)"
    );
    assert!(pds.calls(REQUEST_INDEX).is_empty() && pds.calls(PDS_GET).is_empty());
}
