use std::sync::OnceLock;

use chrono::TimeDelta;
use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::atmosphere::tests::mock_pds::MockPds;
use crate::atmosphere::tests::{save_session, session};
use crate::atmosphere::{Client, FLAGS_SCOPE, Storage, logout};
use crate::flags::{FRESH, FlagReport, Freshness, Gates, LAST_KNOWN_GOOD, RETRY_AFTER_FAILURE, all_at, check_at, keys};

const DID: &str = "did:plc:abc";
const OTHER: &str = "did:plc:other";
const FLAG: &str = keys::PARITY_CANARY;
const GET_SERVICE_AUTH: &str = "com.atproto.server.getServiceAuth";

const NOTHING_ON: Gates = Gates { enabled: true, signed_out: false, everyone: false, percentage: 0, targeted: false };

/// A store, and a pretend data server that's Local Network's website too.
struct World {
    pds: MockPds,
    dir: TempDir,
}

fn world() -> World {
    let pds = MockPds::start();
    crate::testing::directory_on_this_thread(Some(pds.url.clone()));
    let dir = new_store();
    World { pds, dir }
}

fn new_store() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    crate::store::Store::open_or_init(dir.path()).unwrap();
    dir
}

/// The website's answer, with `gates`, for `did`, `age` ago.
fn cached(did: Option<&str>, age: TimeDelta, gates: Gates, now: DateTime<Utc>) -> Cache {
    Cache {
        did: did.map(String::from),
        fetched_at: Some(now - age),
        last_attempt_at: Some(now - age),
        flags: vec![Flag { key: FLAG.into(), gates }],
    }
}

fn with_scope() -> String {
    format!("atproto {FLAGS_SCOPE}")
}

/// Sign `DID` in with a session that ran out, which nothing can refresh, and that's allowed to ask.
fn sign_in_ended(root: &Path, pds: &str) {
    let mut s = serde_json::to_value(session(DID, "refresh-1")).unwrap();
    for field in ["host_url", "authserver_url", "iss", "aud"] {
        s[field] = pds.into();
    }
    s["authserver_token_endpoint"] = format!("{pds}/oauth/token").into();
    s["authserver_revocation_endpoint"] = format!("{pds}/oauth/revoke").into();
    s["scopes"] = with_scope().into();
    s["scope"] = with_scope().into();
    s["expires_at"] = "2026-01-01T00:00:00.000Z".into();
    s["refresh_token"] = Value::Null;
    let callback = "http://127.0.0.1:1/callback".to_string();
    let store = store::LocalStore::new(root, Storage::File, Client::Loopback, callback, vec![]);
    let upsert = jacquard_oauth::authstore::ClientAuthStore::upsert_session(&store, serde_json::from_value(s).unwrap());
    block_on(upsert).unwrap();
}

impl World {
    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn sign_in(&self, scopes: &str) {
        save_session(self.root(), DID, &self.pds.url, scopes);
    }

    fn asked(&self) -> Vec<bool> {
        self.pds.calls(GET_FLAGS).iter().map(|r| r.header("authorization").is_some()).collect()
    }

    fn tokens(&self) -> usize {
        self.pds.calls(GET_SERVICE_AUTH).len()
    }
}

/// Whether `report` has the flag on.
fn shown(report: &FlagReport) -> bool {
    report.flags.iter().any(|flag| flag.key == FLAG && flag.on)
}

/// A check on a fresh cache goes nowhere, whoever's signed in.
#[test]
fn a_fresh_answer_asks_nobody() {
    let w = world();
    w.sign_in(&with_scope());
    let now = Utc::now();
    let gates = Gates { targeted: true, ..NOTHING_ON };
    save(w.root(), &cached(Some(DID), FRESH - TimeDelta::seconds(1), gates, now));
    assert!(check_at(w.root(), FLAG, now));
    assert!(w.pds.requests().is_empty());
}

/// Due and online: the public request, then one with a token for this method at this website
/// (the pretend website takes no other), and the answer is kept for the account.
#[test]
fn a_due_answer_is_asked_for_as_the_account() {
    let w = world();
    w.sign_in(&with_scope());
    w.pds.set_flags(&[(FLAG, false, 0), ("notInThisVersion", true, 100)], &[FLAG]);
    let now = Utc::now();
    assert!(check_at(w.root(), FLAG, now));
    assert_eq!(w.asked(), [false, true]);
    let kept = Cache::load(w.root());
    assert_eq!(kept.did.as_deref(), Some(DID));
    assert_eq!((kept.fetched_at, kept.last_attempt_at), (Some(now), Some(now)));
    assert_eq!(kept.flags.len(), 2, "a key this version doesn't know is kept: nothing asks about it");
    assert_eq!(kept.flags[0], Flag { key: FLAG.into(), gates: Gates { targeted: true, ..NOTHING_ON } });

    // Now it's fresh.
    assert!(check_at(w.root(), FLAG, now + TimeDelta::minutes(59)));
    assert_eq!(w.asked().len(), 2);
    // And an hour on it's due again.
    w.pds.set_flags(&[(FLAG, false, 0)], &[]);
    assert!(!check_at(w.root(), FLAG, now + FRESH));
    assert_eq!(w.asked().len(), 4);
}

/// A flag staff turned off is off for the account it targets too, and stays off from the cache.
#[test]
fn a_flag_turned_off_is_off_for_the_account_it_targets() {
    let w = world();
    w.sign_in(&with_scope());
    let off = |targeted| json!({ "flags": [{ "key": FLAG, "enabled": false, "everyone": true, "percentage": 100, "targeted": targeted }] });
    w.pds.set_flags_answers(Some((200, off(false))), Some((200, off(true))));
    let now = Utc::now();
    assert!(!check_at(w.root(), FLAG, now));
    assert_eq!(w.asked(), [false, true]);
    let gates = Gates { enabled: false, signed_out: false, everyone: true, percentage: 100, targeted: true };
    assert_eq!(Cache::load(w.root()).flags, [Flag { key: FLAG.into(), gates }]);
    assert!(!shown(&all_at(w.root(), false, now + TimeDelta::minutes(1))));
}

/// `check` and `all` read the clock themselves: on a fresh answer, signed out, they say what the
/// answer's public gates say and ask nobody.
#[hegel::test(test_cases = 50)]
fn a_check_now_reads_a_fresh_answer(tc: TestCase) {
    let w = world();
    let gates = Gates {
        enabled: tc.draw(gs::booleans()),
        signed_out: tc.draw(gs::booleans()),
        everyone: tc.draw(gs::booleans()),
        percentage: tc.draw(gs::sampled_from(vec![0u32, 100])),
        targeted: tc.draw(gs::booleans()),
    };
    save(w.root(), &cached(None, TimeDelta::zero(), gates, Utc::now()));
    let expected = gates.enabled && gates.signed_out && (gates.everyone || gates.percentage == 100);
    assert_eq!(crate::flags::check(w.root(), FLAG), expected);
    let report = crate::flags::all(w.root(), false);
    assert_eq!((shown(&report), report.freshness), (expected, Freshness::Fresh));
    assert!(w.pds.requests().is_empty());
}

/// With the server down, the last answer holds, and no second attempt goes out for 15 minutes.
#[test]
fn a_website_thats_down_leaves_the_last_answer_and_waits() {
    let w = world();
    let now = Utc::now();
    save(w.root(), &cached(None, TimeDelta::hours(2), Gates { everyone: true, signed_out: true, ..NOTHING_ON }, now));
    w.pds.set_flags_answers(Some((503, json!({ "error": "Down" }))), None);
    assert!(check_at(w.root(), FLAG, now));
    assert!(check_at(w.root(), FLAG, now + RETRY_AFTER_FAILURE - TimeDelta::seconds(1)));
    assert_eq!(w.asked().len(), 1);
    assert_eq!(Cache::load(w.root()).last_attempt_at, Some(now));
    assert!(check_at(w.root(), FLAG, now + RETRY_AFTER_FAILURE));
    assert_eq!(w.asked().len(), 2);

    // Nothing at all answers there, the same way.
    crate::testing::directory_on_this_thread(Some("http://127.0.0.1:9".into()));
    let later = now + RETRY_AFTER_FAILURE * 2;
    assert!(check_at(w.root(), FLAG, later));
    assert_eq!(Cache::load(w.root()).last_attempt_at, Some(later));
    // With no answer to keep, the back-off is kept all the same.
    let bare = new_store();
    assert!(!check_at(bare.path(), FLAG, later));
    assert_eq!(Cache::load(bare.path()).last_attempt_at, Some(later));
}

/// Older than 14 days an answer counts for nothing, and every flag is off.
#[test]
fn an_answer_over_two_weeks_old_is_off() {
    let w = world();
    let now = Utc::now();
    let gates = Gates { everyone: true, signed_out: true, ..NOTHING_ON };
    let mut cache = cached(None, LAST_KNOWN_GOOD, gates, now);
    cache.last_attempt_at = Some(now);
    save(w.root(), &cache);
    assert!(check_at(w.root(), FLAG, now));
    assert_eq!(all_at(w.root(), false, now).freshness, Freshness::Stale);
    let later = now + TimeDelta::seconds(1);
    assert!(!check_at(w.root(), FLAG, later));
    let report = all_at(w.root(), false, later);
    assert_eq!(report.freshness, Freshness::None);
    assert_eq!(report.fetched_at, cache.fetched_at);
    assert!(w.pds.requests().is_empty(), "still backing off");
}

/// Another account signed in: what was said of the last one goes at once, the public gates stay.
#[test]
fn targeting_goes_with_the_account_it_was_said_of() {
    let w = world();
    w.sign_in(&with_scope());
    let now = Utc::now();
    let targeted = Gates { targeted: true, ..NOTHING_ON };
    save(w.root(), &cached(Some(OTHER), TimeDelta::zero(), targeted, now));
    assert!(!check_at(w.root(), FLAG, now));
    save(w.root(), &cached(Some(OTHER), TimeDelta::zero(), Gates { percentage: 100, ..targeted }, now));
    assert!(check_at(w.root(), FLAG, now), "the percentage holds for whoever's signed in");
    assert!(w.pds.requests().is_empty());
}

/// Signed out: the public request alone, and only what's on for every account.
#[test]
fn signed_out_only_the_public_answer_counts() {
    let w = world();
    let now = Utc::now();
    w.pds.set_flags(&[(FLAG, false, 50)], &[FLAG]);
    assert!(!check_at(w.root(), FLAG, now));
    assert_eq!(w.asked(), [false]);
    assert_eq!(w.tokens(), 0);
    assert_eq!(Cache::load(w.root()).did, None);
    assert!(!all_at(w.root(), false, now).needs_sign_in, "nobody's signed in");

    w.pds.set_flags(&[(FLAG, false, 100)], &[]);
    assert!(check_at(w.root(), FLAG, now + FRESH));
}

/// A session from before the scope gets the public gates for its own DID, never asks its data
/// server for a token, and `account flags` says what it's missing.
#[test]
fn without_the_scope_only_the_public_gates_count() {
    let w = world();
    w.sign_in("atproto");
    let now = Utc::now();
    w.pds.set_flags(&[(FLAG, false, 0)], &[FLAG]);
    assert!(!check_at(w.root(), FLAG, now), "targeting it isn't seen");
    assert_eq!(w.asked(), [false]);
    assert_eq!(w.tokens(), 0);

    w.pds.set_flags(&[(FLAG, false, 100)], &[]);
    let report = all_at(w.root(), true, now);
    assert!(shown(&report) && report.needs_sign_in, "{report:?}");
    assert_eq!(w.tokens(), 0);
}

/// Signing in with the scope doesn't wait out a fresh public answer: the next check asks as the
/// account, once the last attempt is 15 minutes old.
#[test]
fn a_new_sign_in_asks_again_for_what_a_public_answer_left_out() {
    let w = world();
    let now = Utc::now();
    w.pds.set_flags(&[(FLAG, false, 0)], &[FLAG]);
    assert!(!check_at(w.root(), FLAG, now), "signed out");
    assert_eq!(w.asked(), [false]);

    w.sign_in(&with_scope());
    assert!(!check_at(w.root(), FLAG, now + RETRY_AFTER_FAILURE - TimeDelta::seconds(1)), "backing off");
    assert_eq!(w.asked(), [false]);
    assert!(check_at(w.root(), FLAG, now + RETRY_AFTER_FAILURE));
    assert_eq!(w.asked(), [false, false, true]);
    assert_eq!(Cache::load(w.root()).did.as_deref(), Some(DID));

    // Answered for this account, it's fresh for an hour again.
    assert!(check_at(w.root(), FLAG, now + RETRY_AFTER_FAILURE * 2));
    assert_eq!(w.asked().len(), 3);
}

/// A token that keeps being refused leaves a public answer for someone who may ask, so the check
/// asks again, but every 15 minutes and not on every run.
#[test]
fn a_token_that_keeps_being_refused_is_tried_every_15_minutes() {
    let w = world();
    w.sign_in(&with_scope());
    w.pds.set_flags(&[(FLAG, false, 0)], &[FLAG]);
    w.pds.set_flags_answers(None, Some((401, json!({ "error": "AuthenticationRequired" }))));
    let now = Utc::now();
    assert!(!check_at(w.root(), FLAG, now));
    assert_eq!(w.asked().len(), 2);
    assert!(!check_at(w.root(), FLAG, now + RETRY_AFTER_FAILURE - TimeDelta::seconds(1)));
    assert_eq!(w.asked().len(), 2);
    assert!(!check_at(w.root(), FLAG, now + RETRY_AFTER_FAILURE));
    assert_eq!(w.asked().len(), 4);
}

/// A session that can't hear about targeting has nothing to gain from asking early.
#[test]
fn without_the_scope_a_fresh_answer_asks_nobody() {
    let w = world();
    w.sign_in("atproto");
    let now = Utc::now();
    save(w.root(), &cached(None, FRESH - TimeDelta::seconds(1), NOTHING_ON, now));
    assert!(!check_at(w.root(), FLAG, now));
    assert!(w.pds.requests().is_empty());
}

/// A website that doesn't take the token leaves the public answer and nothing targeted, even for
/// what it said before.
#[test]
fn a_refused_token_leaves_the_public_answer() {
    let w = world();
    w.sign_in(&with_scope());
    let now = Utc::now();
    save(w.root(), &cached(Some(DID), TimeDelta::hours(2), Gates { targeted: true, ..NOTHING_ON }, now));
    w.pds.set_flags(&[(FLAG, false, 0)], &[FLAG]);
    let refusal = json!({ "error": "AuthenticationRequired" });
    w.pds.set_flags_answers(None, Some((401, refusal)));
    assert!(!check_at(w.root(), FLAG, now));
    assert_eq!(w.asked(), [false, true]);
    let kept = Cache::load(w.root());
    assert_eq!((kept.did, kept.fetched_at), (None, Some(now)));
}

/// An answer that doesn't read leaves the last one standing, and counts as a failed attempt.
#[test]
fn a_bad_answer_keeps_the_last_one() {
    let bad_flag = |flag: Value| json!({ "flags": [flag] });
    let whole = json!({ "key": FLAG, "everyone": true, "percentage": 0, "targeted": false });
    let answers = [
        json!("not the answer"),
        json!({ "flags": "none" }),
        json!({}),
        bad_flag(json!({ "key": FLAG, "everyone": true })),
        bad_flag(json!({ "key": FLAG, "everyone": true, "percentage": 101, "targeted": false })),
        bad_flag(json!({ "key": FLAG, "everyone": true, "percentage": -1, "targeted": false })),
        bad_flag(json!({ "key": FLAG, "everyone": true, "percentage": 12.5, "targeted": false })),
        bad_flag(json!({ "key": FLAG, "enabled": "no", "everyone": true, "percentage": 0, "targeted": false })),
        json!({ "flags": [whole, { "key": "other", "everyone": false, "percentage": 500, "targeted": false }] }),
    ];
    for answer in answers {
        let w = world();
        let now = Utc::now();
        let last = cached(None, TimeDelta::hours(2), Gates { percentage: 100, ..NOTHING_ON }, now);
        save(w.root(), &last);
        w.pds.set_flags_answers(Some((200, answer.clone())), None);
        assert!(!check_at(w.root(), FLAG, now), "{answer}: signed out, and not on for everyone");
        let kept = Cache::load(w.root());
        assert_eq!(kept, Cache { last_attempt_at: Some(now), ..last }, "{answer}");
    }
}

/// A cache that doesn't read is no cache: the next check asks, and writes a good one over it.
#[test]
fn a_damaged_cache_is_replaced() {
    let w = world();
    let file = w.root().join(CACHE_FILE);
    std::fs::write(&file, "{ not json").unwrap();
    w.pds.set_flags(&[(FLAG, true, 0)], &[]);
    let now = Utc::now();
    assert!(check_at(w.root(), FLAG, now));
    assert_eq!(Cache::load(w.root()).fetched_at, Some(now));

    // And with nothing to ask, it's still only a flag that's off.
    std::fs::write(&file, "{ not json").unwrap();
    w.pds.set_flags_answers(Some((500, json!({}))), None);
    assert!(!check_at(w.root(), FLAG, now));
}

/// While something else has the account's lock the session isn't touched: the public answer comes
/// with the targeting last said of this account, or none when it was said of another.
#[test]
fn a_taken_lock_keeps_what_was_said_of_the_account() {
    let w = world();
    w.sign_in(&with_scope());
    let now = Utc::now();
    w.pds.set_flags(&[(FLAG, false, 0)], &[]);
    let lock = store::lock(w.root()).unwrap();

    save(w.root(), &cached(Some(DID), TimeDelta::hours(2), Gates { targeted: true, ..NOTHING_ON }, now));
    assert!(check_at(w.root(), FLAG, now));
    assert_eq!(Cache::load(w.root()).did.as_deref(), Some(DID));

    save(w.root(), &cached(Some(OTHER), TimeDelta::hours(2), Gates { targeted: true, ..NOTHING_ON }, now));
    assert!(!check_at(w.root(), FLAG, now));
    assert_eq!(Cache::load(w.root()).did, None);

    assert_eq!(w.asked(), [false, false]);
    assert_eq!(w.tokens(), 0);
    drop(lock);
}

/// When the website or the account's server fails on the way to the targeted answer, that says
/// nothing about the account: the public gates are news, and targeting stays as it was.
#[test]
fn a_failure_after_the_public_answer_keeps_what_was_said_of_the_account() {
    let w = world();
    w.sign_in(&with_scope());
    let now = Utc::now();
    let last = cached(Some(DID), TimeDelta::hours(2), Gates { targeted: true, ..NOTHING_ON }, now);
    w.pds.set_flags(&[(FLAG, false, 0)], &[]);

    save(w.root(), &last);
    w.pds.set_flags_answers(None, Some((503, json!({ "error": "Down" }))));
    assert!(check_at(w.root(), FLAG, now));
    assert_eq!(w.asked(), [false, true]);

    save(w.root(), &last);
    w.pds.set_flags_answers(None, Some((200, json!("garbage"))));
    assert!(check_at(w.root(), FLAG, now));
    assert_eq!(w.asked().len(), 4);

    // The account's server can't be reached, while the website can.
    let elsewhere = new_store();
    save_session(elsewhere.path(), DID, "http://127.0.0.1:9", &with_scope());
    save(elsewhere.path(), &last);
    w.pds.set_flags_answers(None, None);
    assert!(check_at(elsewhere.path(), FLAG, now));
    assert_eq!(Cache::load(elsewhere.path()).did.as_deref(), Some(DID));
}

/// A session that has ended isn't an error here: it goes, as it does anywhere, and the check
/// answers as signed out.
#[test]
fn a_session_that_has_ended_falls_back_to_the_public_answer() {
    let w = world();
    sign_in_ended(w.root(), &w.pds.url);
    let now = Utc::now();
    save(w.root(), &cached(Some(DID), TimeDelta::hours(2), Gates { targeted: true, ..NOTHING_ON }, now));
    w.pds.set_flags(&[(FLAG, true, 0)], &[]);
    assert!(check_at(w.root(), FLAG, now));
    assert_eq!(status(w.root()).unwrap(), None);
    assert_eq!(Cache::load(w.root()).did, None);
    assert_eq!(w.tokens(), 0);
}

/// `--refresh` asks whatever the cache's age and the back-off say.
#[test]
fn a_refresh_ignores_the_back_off() {
    let w = world();
    let now = Utc::now();
    save(w.root(), &cached(None, TimeDelta::zero(), NOTHING_ON, now));
    w.pds.set_flags(&[(FLAG, true, 0)], &[]);
    assert!(!shown(&all_at(w.root(), false, now)));
    assert!(w.asked().is_empty());
    let report = all_at(w.root(), true, now);
    assert!(shown(&report) && report.freshness == Freshness::Fresh, "{report:?}");
    assert_eq!(w.asked().len(), 1);
}

/// Nothing about the account stays behind once it's signed out.
#[test]
fn signing_out_forgets_the_answer() {
    let w = world();
    w.sign_in(&with_scope());
    save(w.root(), &cached(Some(DID), TimeDelta::zero(), Gates { targeted: true, ..NOTHING_ON }, Utc::now()));
    assert!(w.root().join(CACHE_FILE).exists());
    logout(w.root()).unwrap();
    assert!(!w.root().join(CACHE_FILE).exists());
}

/// Where the session stands when a check starts.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Session {
    SignedOut,
    WithoutScope,
    WithScope,
    /// With the scope, but nothing can refresh it.
    Ended,
}

/// How the website answers a request.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Website {
    Lists,
    Failing,
    Garbled,
    Refusing,
}

impl Website {
    fn instead(self) -> Option<(u16, Value)> {
        match self {
            Website::Lists => None,
            Website::Failing => Some((503, json!({ "error": "Down" }))),
            Website::Garbled => Some((200, json!({ "flags": [{ "key": FLAG, "percentage": 900 }] }))),
            Website::Refusing => Some((401, json!({ "error": "AuthenticationRequired" }))),
        }
    }
}

/// A flag check, whatever the cache holds and however the session, the lock and the website stand,
/// ends where the design's table says: what's on, who it's kept for, and which requests went out.
#[hegel::test(test_cases = 120, suppress_health_check = [hegel::HealthCheck::TooSlow])]
#[hegel::explicit_test_case(age = None::<i64>, attempted = 600i64, said_of = None::<&str>, was = (false, 0u32, false), now_is = (false, 100u32, true), session = Session::WithScope, public = Website::Lists, with_token = Website::Lists, busy = false)]
#[hegel::explicit_test_case(age = Some(120i64), attempted = 120i64, said_of = Some(DID), was = (false, 0u32, true), now_is = (false, 0u32, true), session = Session::WithScope, public = Website::Lists, with_token = Website::Refusing, busy = false)]
#[hegel::explicit_test_case(age = Some(120i64), attempted = 5i64, said_of = Some(DID), was = (true, 0u32, false), now_is = (false, 0u32, false), session = Session::Ended, public = Website::Failing, with_token = Website::Lists, busy = true)]
#[hegel::explicit_test_case(age = Some(30000i64), attempted = 5i64, said_of = Some(DID), was = (true, 0u32, false), now_is = (false, 0u32, false), session = Session::WithoutScope, public = Website::Failing, with_token = Website::Lists, busy = false)]
#[hegel::explicit_test_case(age = Some(120i64), attempted = 120i64, said_of = Some(DID), was = (false, 0u32, true), now_is = (false, 0u32, false), session = Session::WithScope, public = Website::Lists, with_token = Website::Failing, busy = false)]
fn a_check_ends_as_the_cache_and_the_servers_say(tc: TestCase) {
    static PDS: OnceLock<MockPds> = OnceLock::new();
    let pds = PDS.get_or_init(MockPds::start);
    crate::testing::directory_on_this_thread(Some(pds.url.clone()));
    let gates = || gs::tuples!(gs::booleans(), gs::sampled_from(vec![0u32, 100]), gs::booleans());
    let age = tc.draw(gs::optional(gs::sampled_from(vec![0i64, 59, 60, 120, 20_160, 20_161, 30_000])));
    let attempted = tc.draw(gs::sampled_from(vec![0i64, 14, 15, 120, 600]));
    let said_of = tc.draw(gs::optional(gs::sampled_from(vec![DID, OTHER])));
    let was = tc.draw(gates());
    let now_is = tc.draw(gates());
    let sessions = vec![Session::SignedOut, Session::WithoutScope, Session::WithScope, Session::Ended];
    let session = tc.draw(gs::sampled_from(sessions));
    let public = tc.draw(gs::sampled_from(vec![Website::Lists, Website::Failing, Website::Garbled]));
    let with_token =
        tc.draw(gs::sampled_from(vec![Website::Lists, Website::Failing, Website::Garbled, Website::Refusing]));
    let busy = tc.draw(gs::booleans());

    let dir = new_store();
    let root = dir.path();
    let now = Utc::now();
    match session {
        Session::SignedOut => {}
        Session::WithoutScope => save_session(root, DID, &pds.url, "atproto"),
        Session::WithScope => save_session(root, DID, &pds.url, &with_scope()),
        Session::Ended => sign_in_ended(root, &pds.url),
    }
    let last = age.map(|age| {
        // Signed-out callers have it once every account does, as the website answers for an ops flag like this one.
        let gates = Gates {
            enabled: true,
            signed_out: was.0 || was.1 == 100,
            everyone: was.0,
            percentage: was.1,
            targeted: was.2,
        };
        let mut cache = cached(said_of, TimeDelta::minutes(age), gates, now);
        cache.last_attempt_at = Some(now - TimeDelta::minutes(attempted));
        cache
    });
    if let Some(last) = &last {
        save(root, last);
    }
    let targeted_now: &[&str] = if now_is.2 { &[FLAG] } else { &[] };
    pds.set_flags(&[(FLAG, now_is.0, now_is.1)], targeted_now);
    pds.set_flags_answers(public.instead(), with_token.instead());
    let lock = busy.then(|| store::lock(root).unwrap());
    let (asked_before, tokens_before) = (pds.calls(GET_FLAGS).len(), pds.calls(GET_SERVICE_AUTH).len());

    let on = check_at(root, FLAG, now);

    let fresh = age.is_some_and(|age| age < 60);
    let may_ask = matches!(session, Session::WithScope | Session::Ended);
    let for_someone_else = may_ask && (age.is_none() || said_of != Some(DID));
    let due = (!fresh || for_someone_else) && (age.is_none() || attempted >= 15);
    let answered = due && public == Website::Lists;
    let reaches_session = answered && !busy && matches!(session, Session::WithScope | Session::Ended);
    let ended = reaches_session && session == Session::Ended;
    let signed_in = session != Session::SignedOut && !ended;
    let kept = said_of == Some(DID) && was.2 && age.is_some();
    let (everyone, percentage, targeted) = if answered {
        let targeted = match session {
            Session::SignedOut | Session::WithoutScope => false,
            Session::Ended | Session::WithScope if busy => kept,
            Session::Ended => false,
            Session::WithScope => match with_token {
                Website::Lists => now_is.2,
                Website::Refusing => false,
                Website::Failing | Website::Garbled => kept,
            },
        };
        (now_is.0, now_is.1, targeted)
    } else if age.is_some_and(|age| age <= 20_160) {
        (was.0, was.1, kept)
    } else {
        (false, 0, false)
    };
    let expected = everyone || percentage == 100 || (signed_in && targeted);
    let said =
        format!("{age:?} {attempted} {said_of:?} {was:?} {now_is:?} {session:?} {public:?} {with_token:?} {busy}");
    assert_eq!(on, expected, "{said}");

    let with_a_token = usize::from(reaches_session && session == Session::WithScope);
    assert_eq!(pds.calls(GET_FLAGS).len() - asked_before, usize::from(due) + with_a_token, "{said}");
    assert_eq!(pds.calls(GET_SERVICE_AUTH).len() - tokens_before, with_a_token, "{said}");
    let kept_now = Cache::load(root);
    if due {
        assert_eq!(kept_now.last_attempt_at, Some(now), "{said}");
    } else {
        assert_eq!(Some(kept_now), last, "{said}");
    }

    // `account flags` says the same, and has nothing to ask now.
    let report = all_at(root, false, now);
    assert_eq!(shown(&report), on, "{said}");
    assert_eq!(report.needs_sign_in, session == Session::WithoutScope, "{said}");
    assert_eq!(pds.calls(GET_FLAGS).len() - asked_before, usize::from(due) + with_a_token, "{said}");
    drop(lock);
}
