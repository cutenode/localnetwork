use super::*;

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
