use hegel::TestCase;
use hegel::generators as gs;
use hickory_resolver::proto::op::ResponseCode;

use super::*;
use crate::atmosphere::identity::tests::{Records, dns};

/// What's said when the permission set's DNS name has `txts` (each record its strings).
fn with_records(txts: &[&[&str]]) -> Availability {
    let records = Records::default();
    let txts = txts.iter().map(|strings| strings.iter().map(|s| s.to_string()).collect()).collect();
    records.lock().unwrap().insert("_lexicon.localnetwork.computer".into(), txts);
    asked(Some(dns(records, ResponseCode::ServFail)))
}

#[test]
fn a_record_naming_an_account_opens_publishing() {
    assert_eq!(with_records(&[&["v=spf1 -all"], &["other", "did=did:plc:abc"]]), Availability::Open);
}

#[test]
fn records_naming_no_account_dont_open_publishing() {
    assert_eq!(with_records(&[&["v=spf1 -all"], &["did:plc:abc", "a did=did:plc:abc"]]), Availability::NotOpen);
}

#[test]
fn a_name_that_doesnt_exist_isnt_open() {
    assert_eq!(asked(Some(dns(Records::default(), ResponseCode::NXDomain))), Availability::NotOpen);
}

#[test]
fn it_isnt_known_without_dns_to_ask() {
    assert_eq!(asked(None), Availability::Unknown);
}

/// Whatever a server fails with, it hasn't said there's no record.
#[hegel::test(test_cases = 10)]
#[hegel::explicit_test_case(code = ResponseCode::ServFail)]
fn prop_it_isnt_known_when_the_server_fails(tc: TestCase) {
    use ResponseCode::{FormErr, NotAuth, NotImp, Refused, ServFail};
    let code = tc.draw(gs::sampled_from(vec![ServFail, Refused, NotImp, FormErr, NotAuth]));
    assert_eq!(asked(Some(dns(Records::default(), code))), Availability::Unknown);
}

#[test]
fn the_dns_name_is_the_authority_backwards() {
    assert_eq!(lexicon_dns_name(PUBLISH_SET), "_lexicon.localnetwork.computer");
    assert_eq!(lexicon_dns_name("computer.localnetwork.dj.profile"), "_lexicon.dj.localnetwork.computer");
}

#[test]
fn only_the_permission_sets_refusal_counts() {
    let theirs = format!("Unable to resolve Lexicon schema {PUBLISH_SET}");
    assert!(refused("invalid_scope", &theirs));
    assert!(!refused("invalid_scope", "Unable to resolve Lexicon schema app.example.other"));
    assert!(!refused("invalid_request", &theirs));
}

#[test]
fn only_not_open_says_why() {
    assert_eq!(Availability::NotOpen.why_not(), Some(NOT_OPEN));
    assert_eq!(Availability::Open.why_not(), None);
    assert_eq!(Availability::Unknown.why_not(), None, "not known isn't closed");
}
