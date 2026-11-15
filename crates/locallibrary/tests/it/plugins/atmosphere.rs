//! Records a plugin writes in the Atmosphere account (`atmosphere/put_record`), against a pretend
//! data server (`support/mock_pds.rs`): only of a type its capabilities name, fitting its Lexicon
//! (the manifest's, or locallibrary's for Local Network's types), once the user confirms it, with
//! a session allowed to write it. Otherwise nothing reaches the server, and the error says why
//! and, passed on, how to sign in again.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::atmosphere::lexicon::SET;
use locallibrary::error::Reason;
use locallibrary::plugin::{AccessQuestion, AskAccess, RecordWrite};
use loclib_plugin::StrongRef;

use super::*;
use crate::mock_pds::MockPds;
use crate::session::{DID, session_json, sign_in_as};

const TRACK: &str = "fm.plyr.track";
const PUT: &str = "com.atproto.repo.putRecord";
const CREATE: &str = "com.atproto.repo.createRecord";

/// A front end answering `answer` about every record (`None`: it can't ask), keeping what it was
/// asked.
#[derive(Default)]
struct Confirming {
    answer: Mutex<Option<bool>>,
    asked: Mutex<Vec<RecordWrite>>,
}

impl AskAccess for Confirming {
    fn allow(&self, _: &AccessQuestion) -> Option<bool> {
        None
    }

    fn sign_in(&self, _: &str) {}

    fn write_record(&self, write: &RecordWrite) -> Option<bool> {
        self.asked.lock().unwrap().push(write.clone());
        *self.answer.lock().unwrap()
    }
}

/// The echo plugin writing `fm.plyr.track` records (a title of 30 bytes at most, its Lexicon in
/// the manifest) and Local Network's sets (locallibrary's Lexicon).
fn writing() -> String {
    let lexicon = "\n[[lexicons]]\nlexicon = 1\nid = \"fm.plyr.track\"\n[lexicons.defs.main]\ntype = \"record\"\n\
                   key = \"tid\"\n[lexicons.defs.main.record]\ntype = \"object\"\nrequired = [\"title\"]\n\
                   properties = { title = { type = \"string\", maxLength = 30 } }\n";
    granting(&["atmosphere:write:fm.plyr.track", "atmosphere:write:computer.localnetwork.dj.set"]) + lexicon
}

/// Echo writing records (see [`writing`]), the user answering as `confirming` says.
fn writer(confirming: &Arc<Confirming>) -> Fixture {
    let ask = Some(confirming.clone() as Arc<dyn AskAccess>);
    Fixture::new(|dir| vec![spec(dir, &writing(), &[])], Options { ask, ..Options::default() })
}

/// [`writer`], signed in at a pretend data server granted `scopes`.
fn signed_in(scopes: &str, confirming: &Arc<Confirming>) -> (Fixture, MockPds) {
    let f = writer(confirming);
    let pds = MockPds::start();
    let session = session_json(DID, "s1", &pds.url, scopes, Some("refresh-1"), "2099-01-01T00:00:00.000Z");
    sign_in_as(f.store.root(), session);
    (f, pds)
}

/// What echo's `atmosphere/put_record` call with `params` answered: the record's URI and CID, or
/// its error (as JSON), with its kind.
fn put(f: &Fixture, params: Value) -> std::result::Result<StrongRef, (String, Value)> {
    let answer = f.echo_params(json!({ "call": "atmosphere/put_record", "params": params })).unwrap();
    match answer.get("error") {
        Some(e) => Err((e["data"]["kind"].as_str().unwrap_or_default().to_string(), e.clone())),
        None => Ok(serde_json::from_value(answer["result"].clone()).unwrap()),
    }
}

fn track(title: &str) -> Value {
    json!({ "$type": TRACK, "title": title })
}

/// Writes that reached the server.
fn writes(pds: &MockPds) -> usize {
    pds.calls(PUT).len() + pds.calls(CREATE).len()
}

/// A record the user confirms is written as the plugin gave it, at its key or one the server
/// picks, and answered as its URI and CID; it replaces a record only that's the version the
/// plugin names. A Local Network record type is checked against locallibrary's Lexicon. The user
/// is shown each one first.
#[test]
fn a_record_the_user_confirms_is_written() {
    let confirming = Arc::new(Confirming { answer: Mutex::new(Some(true)), ..Confirming::default() });
    let (f, pds) = signed_in("atproto repo:fm.plyr.track repo:computer.localnetwork.dj.set", &confirming);
    let rkey = "3lzy2ji4nms2z";
    let first = put(&f, json!({ "collection": TRACK, "rkey": rkey, "record": track("Friday") })).unwrap();
    assert_eq!(first.uri, format!("at://{DID}/{TRACK}/{rkey}"));
    assert_eq!(pds.record(DID, TRACK, rkey), Some((track("Friday"), first.cid.clone())));
    let asked = confirming.asked.lock().unwrap().clone();
    let shown = RecordWrite {
        plugin: "Echo".into(),
        account: DID.into(),
        collection: TRACK.into(),
        rkey: Some(rkey.into()),
        swap: None,
        record: track("Friday"),
    };
    assert_eq!(asked, [shown]);
    assert!(asked[0].summary().starts_with("Echo wants to create the fm.plyr.track record 3lzy2ji4nms2z in"));

    // Created there already: only a write naming the version there replaces it.
    let (kind, _) = put(&f, json!({ "collection": TRACK, "rkey": rkey, "record": track("Again") })).unwrap_err();
    assert_eq!(kind, "conflict");
    let swap = json!({ "collection": TRACK, "rkey": rkey, "record": track("Saturday"), "swap": first.cid });
    let second = put(&f, swap.clone()).unwrap();
    assert_eq!(pds.record(DID, TRACK, rkey), Some((track("Saturday"), second.cid.clone())));
    assert_eq!(put(&f, swap).unwrap_err().0, "conflict", "the version it named isn't there any more");
    assert_eq!(pds.record(DID, TRACK, rkey).unwrap().1, second.cid);

    let picked = put(&f, json!({ "collection": TRACK, "record": track("Sunday") })).unwrap();
    let key = picked.uri.rsplit('/').next().unwrap();
    assert_eq!(pds.record(DID, TRACK, key), Some((track("Sunday"), picked.cid)));
    let set = json!({ "$type": SET, "date": "2026-10-03", "tracks": [], "createdAt": "2026-10-04T09:30:00.000Z" });
    let published = put(&f, json!({ "collection": SET, "record": set })).unwrap();
    assert!(published.uri.starts_with(&format!("at://{DID}/{SET}/")), "{}", published.uri);
    assert_eq!(confirming.asked.lock().unwrap().len(), 6, "each write was asked about");
}

/// Nothing reaches the server, and the user isn't asked, for a record type the plugin doesn't
/// declare, a record that doesn't fit its Lexicon (the error says where), a record key that isn't
/// one or a version that isn't a CID; and nothing does when the user declines or can't be asked.
/// A plugin without the `atmosphere` feature isn't run at all.
#[test]
fn nothing_is_written_unless_declared_fitting_and_confirmed() {
    let confirming = Arc::new(Confirming { answer: Mutex::new(Some(true)), ..Confirming::default() });
    let (f, pds) = signed_in("atproto transition:generic", &confirming);
    let album = json!({ "$type": "fm.plyr.album", "title": "A" });
    let (kind, e) = put(&f, json!({ "collection": "fm.plyr.album", "rkey": "self", "record": album })).unwrap_err();
    assert_eq!(kind, "denied");
    assert!(e["message"].as_str().unwrap().contains("needs the atmosphere:write:fm.plyr.album capability"), "{e}");
    let mut extra = track("Friday");
    extra["mood"] = "dark".into();
    let (kind, e) = put(&f, json!({ "collection": TRACK, "record": extra })).unwrap_err();
    assert_eq!(kind, "invalid");
    assert_eq!(e["message"], "the track record doesn't fit its Lexicon: mood isn't in the Lexicon");
    assert_eq!(e["data"]["fields"]["collection"], TRACK);
    let long = track(&"a".repeat(31));
    assert_eq!(put(&f, json!({ "collection": TRACK, "record": long })).unwrap_err().0, "invalid");
    let wrong_type = json!({ "$type": "fm.plyr.album", "title": "A" });
    assert_eq!(put(&f, json!({ "collection": TRACK, "record": wrong_type })).unwrap_err().0, "invalid");
    for bad in [
        json!({ "collection": TRACK, "rkey": "..", "record": track("A") }),
        json!({ "collection": TRACK, "rkey": "a b", "record": track("A") }),
        json!({ "collection": TRACK, "record": track("A"), "swap": "bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm" }),
        json!({ "collection": TRACK, "rkey": "self", "record": track("A"), "swap": "nope" }),
    ] {
        assert_eq!(put(&f, bad.clone()).unwrap_err().0, "invalid_params", "{bad}");
    }
    assert!(confirming.asked.lock().unwrap().is_empty(), "nothing was asked");

    *confirming.answer.lock().unwrap() = Some(false);
    assert_eq!(put(&f, json!({ "collection": TRACK, "record": track("A") })).unwrap_err().0, "cancelled");
    *confirming.answer.lock().unwrap() = None;
    let (kind, e) = put(&f, json!({ "collection": TRACK, "record": track("A") })).unwrap_err();
    assert_eq!(kind, "invalid");
    assert!(e["message"].as_str().unwrap().contains("can't be asked here"), "{e}");
    assert_eq!(confirming.asked.lock().unwrap().len(), 2);
    assert_eq!(writes(&pds), 0, "nothing reached the server");

    let lacking = Fixture::new(|dir| vec![spec(dir, &writing(), &["--without", "atmosphere"])], Options::default());
    let e = lacking.echo_params(json!({ "n": 1 })).unwrap_err();
    assert_eq!(
        failure(e),
        "the Echo plugin failed: it says it doesn't have the atmosphere feature, which its manifest needs"
    );
}

/// A session not allowed to write the record type, or nobody signed in, fails before the user is
/// asked, as `account` naming the type; passed on, it's an account error offering to sign in
/// again allowing that type (the scope `repo:<nsid>`, keeping what the session may do), or to sign
/// in. An `account` error about a type the plugin doesn't write, or about none, is the plugin
/// failing, as any other `account` error it passes on is.
#[test]
fn a_session_that_may_not_write_it_says_how_to_sign_in_again() {
    let confirming = Arc::new(Confirming { answer: Mutex::new(Some(true)), ..Confirming::default() });
    let (f, pds) = signed_in("atproto", &confirming);
    let (kind, e) = put(&f, json!({ "collection": TRACK, "rkey": "self", "record": track("A") })).unwrap_err();
    assert_eq!(
        (kind.as_str(), &e["data"]["reason"], &e["data"]["fields"]["collection"]),
        ("account", &json!("missing_scope"), &json!(TRACK))
    );
    assert!(confirming.asked.lock().unwrap().is_empty());
    assert_eq!(writes(&pds), 0);

    let passed_on = |e: &Value| {
        let fail = json!({ "fail": "account", "message": e["message"], "reason": e["data"]["reason"], "fields": e["data"]["fields"] });
        f.echo_params(fail).unwrap_err()
    };
    let error = passed_on(&e);
    assert_eq!((error.code(), error.reason()), ("account", Some(Reason::MissingScope)), "{error}");
    let again = &error.detail().actions[0];
    assert_eq!(again.method.as_deref(), Some("account.login.begin"));
    assert_eq!(again.params["scopes"], json!(["atproto", "repo:fm.plyr.track?action=create&action=update"]));
    assert_eq!(again.label, "Sign in again, allowing fm.plyr.track records");

    let mut other = e.clone();
    other["data"]["fields"]["collection"] = "app.bsky.feed.post".into();
    let error = passed_on(&other);
    assert_eq!((error.code(), error.reason()), ("external", None), "{error}");
    assert!(error.detail().actions.is_empty(), "{error}");
    other["data"]["fields"] = json!({});
    assert_eq!(passed_on(&other).code(), "external");

    let nobody = writer(&confirming);
    let (kind, e) = put(&nobody, json!({ "collection": TRACK, "record": track("A") })).unwrap_err();
    assert_eq!((kind.as_str(), &e["data"]["reason"]), ("account", &json!("not_signed_in")));
    let fail =
        json!({ "fail": "account", "message": e["message"], "reason": "not_signed_in", "fields": e["data"]["fields"] });
    let error = nobody.echo_params(fail).unwrap_err();
    assert_eq!(error.reason(), Some(Reason::NotSignedIn));
    let actions = error.detail().actions;
    let offered: Vec<(Option<&str>, usize)> = actions.iter().map(|a| (a.method.as_deref(), a.params.len())).collect();
    assert_eq!(offered, [(Some("account.login.begin"), 0)], "{error}");
    assert!(confirming.asked.lock().unwrap().is_empty());
}

/// For any session's scopes and a record with a key or without: a write without a key needs the
/// session to create the type, one with a key to create and replace it (`transition:generic`
/// allows both); allowed, it's written once confirmed, and otherwise it fails as `account`
/// (`missing_scope`) with nothing asked or sent.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_write_needs_the_scope_for_it(tc: TestCase) {
    let scopes = tc.draw(gs::sampled_from(vec![
        "atproto",
        "atproto repo:fm.plyr.track?action=create",
        "atproto repo:fm.plyr.track?action=create&action=update",
        "atproto repo:fm.plyr.track",
        "atproto repo:fm.plyr.album",
        "atproto transition:generic",
    ]));
    let keyed: bool = tc.draw(gs::booleans());
    let confirming = Arc::new(Confirming { answer: Mutex::new(Some(true)), ..Confirming::default() });
    let (f, pds) = signed_in(scopes, &confirming);
    let mut params = json!({ "collection": TRACK, "record": track("A") });
    if keyed {
        params["rkey"] = "self".into();
    }
    let creates =
        scopes.contains("transition:generic") || scopes.split(' ').any(|s| s.starts_with("repo:fm.plyr.track"));
    let replaces = scopes.contains("transition:generic")
        || scopes.ends_with("repo:fm.plyr.track")
        || scopes.contains("action=update");
    let allowed = if keyed { creates && replaces } else { creates };
    match put(&f, params) {
        Ok(written) => {
            assert!(allowed, "{scopes} {keyed}");
            assert_eq!(confirming.asked.lock().unwrap().len(), 1);
            // Written once, however many requests that took (the first is told to use a DPoP nonce).
            assert!(written.uri.starts_with(&format!("at://{DID}/{TRACK}/")));
            let key = written.uri.rsplit('/').next().unwrap();
            assert_eq!(pds.record(DID, TRACK, key), Some((track("A"), written.cid)));
        }
        Err((kind, e)) => {
            assert!(!allowed, "{scopes} {keyed}: {e}");
            assert_eq!((kind.as_str(), &e["data"]["reason"]), ("account", &json!("missing_scope")));
            assert!(confirming.asked.lock().unwrap().is_empty());
            assert_eq!(writes(&pds), 0);
        }
    }
}
