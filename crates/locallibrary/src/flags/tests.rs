use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};

use super::*;

const FLAG: &str = keys::PARITY_CANARY;

/// One flag, answered for `did`, `age` ago.
fn answered(did: Option<&str>, age: TimeDelta, gates: Gates, now: DateTime<Utc>) -> Cache {
    Cache {
        did: did.map(String::from),
        fetched_at: Some(now - age),
        last_attempt_at: Some(now - age),
        flags: vec![Flag { key: FLAG.into(), gates }],
    }
}

const NOTHING_ON: Gates = Gates { enabled: true, signed_out: false, everyone: false, percentage: 0, targeted: false };

/// Both the website and this code read these, so the two can't come to disagree.
#[test]
fn the_shared_vectors_hold() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../flags/vectors.json");
    let vectors: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    let cases = vectors["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        let (key, did) = (case["key"].as_str().unwrap(), case["did"].as_str());
        let gates: Gates = serde_json::from_value(case.clone()).unwrap();
        if let Some(did) = did {
            assert_eq!(Some(u64::from(bucket(key, did))), case["bucket"].as_u64(), "{case}");
        }
        assert_eq!(Some(decide(&gates, key, did)), case["on"].as_bool(), "{case}");
    }
}

/// Every file in the catalog has its constant, spelled from its key.
#[test]
fn the_keys_are_the_catalogs() {
    let catalog = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../flags/catalog");
    let mut files: Vec<String> = std::fs::read_dir(catalog)
        .unwrap()
        .map(|file| file.unwrap().path().file_stem().unwrap().to_str().unwrap().to_string())
        .collect();
    files.sort();
    assert_eq!(keys::ALL, files);
    assert_eq!(keys::PARITY_CANARY, "parityCanary");
}

#[cfg(feature = "account")]
#[test]
fn an_answer_lasts_as_long_as_a_session() {
    assert_eq!(LAST_KNOWN_GOOD, TimeDelta::days(crate::atmosphere::SESSION_LIMIT_DAYS));
}

/// What's left of a cache that doesn't read is no answer at all, never an error.
#[cfg(feature = "account")]
#[test]
fn a_damaged_cache_is_no_cache() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(CACHE_FILE);
    assert_eq!(Cache::load(dir.path()), Cache::default(), "no file");
    for damaged in ["", "{", "[]", r#"{"flags":"none"}"#, r#"{"flags":[{"key":"x"}]}"#] {
        std::fs::write(&file, damaged).unwrap();
        assert_eq!(Cache::load(dir.path()), Cache::default(), "{damaged}");
    }
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    assert_eq!(Cache::load(dir.path()), Cache::default(), "a folder in its place");
}

/// A key the website didn't list, or this version doesn't know, is off whatever else is on.
#[test]
fn an_unknown_key_is_off() {
    let now = Utc::now();
    let gates = Gates { enabled: true, signed_out: true, everyone: true, percentage: 100, targeted: true };
    let cache = answered(Some("did:plc:abc"), TimeDelta::zero(), gates, now);
    assert!(cache.on(FLAG, Some("did:plc:abc"), now));
    assert!(!cache.on("notAFlag", Some("did:plc:abc"), now));
    assert!(!Cache::default().on(FLAG, Some("did:plc:abc"), now));
}

/// With no account there's nothing to ask with: every flag is off, even with an answer a build
/// with the account left in the same store, and nothing is written.
#[cfg(not(feature = "account"))]
#[test]
fn a_build_without_the_account_has_every_flag_off() {
    let dir = tempfile::tempdir().unwrap();
    let off = |root: &std::path::Path| {
        assert!(!check(root, FLAG));
        let report = all(root, true);
        assert!(report.flags.iter().all(|flag| !flag.on), "{report:?}");
        assert_eq!((report.freshness, report.fetched_at, report.needs_sign_in), (Freshness::None, None, false));
    };
    off(dir.path());
    assert!(!dir.path().join(CACHE_FILE).exists());

    let gates = Gates { enabled: true, signed_out: true, everyone: true, percentage: 100, targeted: true };
    let left = answered(None, TimeDelta::zero(), gates, Utc::now());
    crate::fsutil::write_json_atomic(&dir.path().join(CACHE_FILE), &left).unwrap();
    off(dir.path());
}

#[hegel::test(test_cases = 200)]
#[hegel::explicit_test_case(key = "parityCanary".to_string(), did = "did:plc:z72i7hdynmk6r22z27h6tvur".to_string())]
fn prop_a_bucket_is_under_100(tc: TestCase) {
    let key = tc.draw(gs::text());
    let did = tc.draw(gs::text());
    assert!(bucket(&key, &did) < 100);
}

/// Raising a percentage only adds accounts: 0 is nobody and 100 everybody.
#[hegel::test(test_cases = 200)]
fn prop_raising_a_percentage_only_adds_accounts(tc: TestCase) {
    let key = tc.draw(gs::text());
    let did = tc.draw(gs::text());
    let low = tc.draw(gs::integers::<u32>().max_value(100));
    let high = tc.draw(gs::integers::<u32>().min_value(low).max_value(100));
    let at = |percentage| decide(&Gates { percentage, ..NOTHING_ON }, &key, Some(&did));
    assert!(!at(low) || at(high), "{low} {high}");
    assert_eq!(at(low), bucket(&key, &did) < low);
    assert!(!at(0) && at(100));
}

/// Signed out there's no DID: a percentage can't pick anyone and nothing cached can target
/// anyone, so a flag is on only for signed-out callers and once every account has it, as on the
/// website.
#[hegel::test(test_cases = 100)]
fn prop_signed_out_only_flags_on_for_them_and_every_account_are_on(tc: TestCase) {
    let now = Utc::now();
    let gates = Gates {
        enabled: tc.draw(gs::booleans()),
        signed_out: tc.draw(gs::booleans()),
        everyone: tc.draw(gs::booleans()),
        percentage: tc.draw(gs::integers::<u32>().max_value(100)),
        targeted: tc.draw(gs::booleans()),
    };
    let did = tc.draw(gs::optional(gs::just("did:plc:abc")));
    let on = gates.enabled && gates.signed_out && (gates.everyone || gates.percentage == 100);
    assert_eq!(answered(did, TimeDelta::zero(), gates, now).on(FLAG, None, now), on);
    assert_eq!(decide(&Gates { targeted: false, ..gates }, FLAG, None), on);
}

/// Signed in, the signed-out setting changes nothing.
#[hegel::test(test_cases = 100)]
fn prop_signed_out_changes_nothing_for_an_account(tc: TestCase) {
    let gates = Gates {
        enabled: tc.draw(gs::booleans()),
        signed_out: tc.draw(gs::booleans()),
        everyone: tc.draw(gs::booleans()),
        percentage: tc.draw(gs::integers::<u32>().max_value(100)),
        targeted: tc.draw(gs::booleans()),
    };
    let did = tc.draw(gs::text());
    assert_eq!(
        decide(&gates, FLAG, Some(&did)),
        decide(&Gates { signed_out: !gates.signed_out, ..gates }, FLAG, Some(&did))
    );
}

/// A flag staff turned off is off for everyone, whoever it targets or however far it's rolled out,
/// and turning it back on brings back exactly what it was.
#[hegel::test(test_cases = 200)]
fn prop_a_flag_turned_off_is_off_for_everyone(tc: TestCase) {
    let now = Utc::now();
    let on = Gates {
        enabled: true,
        signed_out: tc.draw(gs::booleans()),
        everyone: tc.draw(gs::booleans()),
        percentage: tc.draw(gs::integers::<u32>().max_value(100)),
        targeted: tc.draw(gs::booleans()),
    };
    let off = Gates { enabled: false, ..on };
    let did = tc.draw(gs::optional(gs::sampled_from(vec!["did:plc:abc", "did:plc:other"])));
    let said_of = tc.draw(gs::optional(gs::just("did:plc:abc")));
    assert!(!decide(&off, FLAG, did));
    assert!(!answered(said_of, TimeDelta::zero(), off, now).on(FLAG, did, now));
    let expected = match did {
        Some(did) => on.everyone || on.targeted || bucket(FLAG, did) < on.percentage,
        None => on.signed_out && (on.everyone || on.percentage == 100),
    };
    assert_eq!(decide(&on, FLAG, did), expected);
}

/// A website or a cache from before flags could be turned off says nothing about it: the flag is on.
#[test]
fn no_word_on_enabled_is_enabled() {
    let enabled = |flag: Value| serde_json::from_value::<Flag>(flag).unwrap().gates.enabled;
    assert!(enabled(json!({ "key": FLAG, "everyone": true, "percentage": 0, "targeted": false })));
    assert!(!enabled(json!({ "key": FLAG, "enabled": false, "everyone": true, "percentage": 0, "targeted": false })));
}

/// A website or a cache from before signed-out visitors had their own setting says nothing about
/// it: signed out, the flag is off, and the cache keeps the field's name as the website sends it.
#[test]
fn no_word_on_signed_out_is_off() {
    let signed_out = |flag: Value| serde_json::from_value::<Flag>(flag).unwrap().gates.signed_out;
    assert!(!signed_out(json!({ "key": FLAG, "everyone": true, "percentage": 100, "targeted": false })));
    assert!(signed_out(
        json!({ "key": FLAG, "signedOut": true, "everyone": true, "percentage": 0, "targeted": false })
    ));
    let gates = Gates { signed_out: true, ..NOTHING_ON };
    let kept = serde_json::to_value(Flag { key: FLAG.into(), gates }).unwrap();
    assert_eq!(kept["signedOut"], json!(true));
}

/// An answer is fresh for an hour, stale up to 14 days and then no answer; it's asked for again
/// once it isn't fresh and the last attempt is 15 minutes old.
#[cfg(feature = "account")]
#[hegel::test(test_cases = 200)]
#[hegel::explicit_test_case(minutes = 59i64, attempted = 0i64)]
#[hegel::explicit_test_case(minutes = 60i64, attempted = 14i64)]
#[hegel::explicit_test_case(minutes = 60i64, attempted = 15i64)]
#[hegel::explicit_test_case(minutes = 20160i64, attempted = 0i64)]
#[hegel::explicit_test_case(minutes = 20161i64, attempted = 0i64)]
fn prop_an_answer_ages_as_the_design_says(tc: TestCase) {
    let now = Utc::now();
    let minutes = tc.draw(gs::integers::<i64>().min_value(0).max_value(40_000));
    let attempted = tc.draw(gs::integers::<i64>().min_value(0).max_value(40));
    let gates = Gates { everyone: true, signed_out: true, ..NOTHING_ON };
    let mut cache = answered(None, TimeDelta::minutes(minutes), gates, now);
    cache.last_attempt_at = Some(now - TimeDelta::minutes(attempted));

    let expected = match minutes {
        0..60 => Freshness::Fresh,
        60..=20_160 => Freshness::Stale,
        _ => Freshness::None,
    };
    assert_eq!(cache.freshness(now), expected);
    assert_eq!(cache.due(&Caller::default(), now), minutes >= 60 && attempted >= 15);
    assert_eq!(cache.on(FLAG, None, now), expected != Freshness::None, "an answer past its time is off");
}

/// `targeted` only counts for the account it was said of; the public gates don't care who asks.
#[hegel::test(test_cases = 100)]
fn prop_targeted_only_counts_for_who_it_was_said_of(tc: TestCase) {
    let now = Utc::now();
    let gates = Gates { targeted: true, ..NOTHING_ON };
    let said_of = tc.draw(gs::optional(gs::sampled_from(vec!["did:plc:abc", "did:plc:other"])));
    let asking = tc.draw(gs::optional(gs::sampled_from(vec!["did:plc:abc", "did:plc:other"])));
    let cache = answered(said_of, TimeDelta::zero(), gates, now);
    assert_eq!(cache.on(FLAG, asking, now), asking.is_some() && said_of == asking);
    let public = answered(said_of, TimeDelta::zero(), Gates { percentage: 100, ..NOTHING_ON }, now);
    assert_eq!(public.on(FLAG, asking, now), asking.is_some());
}
