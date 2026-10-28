use super::*;
use crate::atmosphere::tests::mock_pds::{MockPds, Writes};
use crate::atmosphere::tests::{save_session, signed_in_with};
use crate::atmosphere::{REPORT_SCOPE, may_create, status};
use crate::error::Reason;

const SPACE_URI: &str = "at://did:plc:owner/app.userinput.space/3lspace";

#[test]
fn a_space_is_named_by_its_at_uri_and_nothing_else() {
    let space = Space::parse(SPACE_URI).unwrap();
    assert_eq!(space, Space { did: "did:plc:owner".into(), rkey: "3lspace".into() });
    assert_eq!(space.uri(), SPACE_URI);
    assert_eq!(Space::parse(&format!("  {SPACE_URI}\n")).unwrap(), space);
    for wrong in [
        "https://userinput.app/s/owner",
        "at://did:plc:owner/app.userinput.discussion/3lspace",
        "at://owner.example.com/app.userinput.space/3lspace",
        "at://did:plc:owner/app.userinput.space",
        "at://did:plc:owner",
        "",
    ] {
        let err = Space::parse(wrong).unwrap_err();
        assert_eq!(err.code(), "invalid", "{wrong}");
        assert!(err.to_string().contains("isn't a userinput.app space"), "{err}");
    }
}

#[test]
fn the_environment_names_the_space_or_the_built_in_one_is_used() {
    assert_eq!(space_from(Some(SPACE_URI)).unwrap().map(|s| s.uri()).as_deref(), Some(SPACE_URI));
    let built_in = SPACE.map(|s| Space::parse(s).unwrap());
    assert_eq!(space_from(None).unwrap(), built_in);
    assert_eq!(space_from(Some(" ")).unwrap(), built_in);
    assert!(space_from(Some("bugs please")).is_err());
}

#[test]
fn titles_and_descriptions_fit_the_lexicon_or_say_what_is_too_long() {
    check("Sync stops at 99%", "").unwrap();
    assert!(check("  ", "body").unwrap_err().to_string().contains("needs a title"));
    check(&"a".repeat(300), &"a".repeat(10_000)).unwrap();
    // Characters as people see them: an e and its accent are one.
    check(&"e\u{301}".repeat(200), "").unwrap();
    let err = check(&"a".repeat(301), "").unwrap_err().to_string();
    assert_eq!(err, "the title is too long to post: 301 characters, 300 at most");
    let err = check("t", &"a".repeat(10_001)).unwrap_err().to_string();
    assert_eq!(err, "the description is too long to post: 10001 characters, 10000 at most");
    // 300 characters of three bytes each are more than a title's 600 bytes.
    let err = check(&"日".repeat(300), "").unwrap_err().to_string();
    assert!(err.starts_with("the title is too long to post: 900 bytes of text, 600 at most"), "{err}");
}

#[test]
fn a_report_is_a_discussion_in_the_space_filed_as_a_bug() {
    let space = Space::parse(SPACE_URI).unwrap();
    let cid = "bafyreidfayvfuwqa7qlnopdjiqrxzs6blmoeu4rujcjtnci5beludirz2a";
    let now = DateTime::parse_from_rfc3339("2026-10-17T09:30:00Z").unwrap().to_utc();
    let r = record(&space, cid, "Sync stops", "It stopped.", now);
    lexicon::validate(lexicon::DISCUSSION, &r).unwrap();
    assert_eq!(
        r,
        json!({
            "$type": "app.userinput.discussion",
            "space": { "uri": SPACE_URI, "cid": cid },
            "title": "Sync stops",
            "body": "It stopped.",
            "tags": ["bug", "locallibrary"],
            "createdAt": "2026-10-17T09:30:00.000Z",
        })
    );
    assert!(record(&space, cid, "Sync stops", "", now).get("body").is_none());
    // A space's version is a real CID.
    assert!(lexicon::validate(lexicon::DISCUSSION, &record(&space, "not-a-cid", "t", "", now)).is_err());
    assert_eq!(link("did:plc:abc", "3lpost"), "https://userinput.app/d/did:plc:abc/3lpost");
}

#[test]
fn reporting_needs_only_the_report_scope() {
    assert!(!may_create(&signed_in_with(&["atproto"]), lexicon::DISCUSSION));
    assert!(may_create(&signed_in_with(&["atproto", REPORT_SCOPE]), lexicon::DISCUSSION));
    assert!(may_create(&signed_in_with(&["atproto", "transition:generic"]), lexicon::DISCUSSION));
    assert!(!may_create(&signed_in_with(&["atproto", REPORT_SCOPE]), lexicon::SETTINGS));
}

#[test]
fn a_report_is_posted_from_the_account_to_the_spaces_current_version() {
    let pds = MockPds::start();
    crate::testing::directory_on_this_thread(Some(pds.url.clone()));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    crate::store::Store::open_or_init(root).unwrap();
    let did = "did:plc:abc";
    save_session(root, did, &pds.url, &format!("atproto {REPORT_SCOPE}"));
    let account = status(root).unwrap().unwrap();
    // The space's version is the one Local Network's directory has, never read from a data server.
    let space = Space::parse("at://did:plc:owner/app.userinput.space/3lspace").unwrap();
    pds.put_unindexed("did:plc:owner", SPACE_COLLECTION, "3lspace", json!({ "name": "locallibrary" }));
    let err = post(root, &account, &space, "Sync stops", "It stopped.").unwrap_err();
    assert_eq!(err.code(), "not_found", "{err}");
    assert!(err.to_string().ends_with("Local Network's directory doesn't have it yet)"), "{err}");
    let space_cid = pds.put("did:plc:owner", SPACE_COLLECTION, "3lspace", json!({ "name": "locallibrary" }));
    assert!(pds.calls("com.atproto.repo.getRecord").is_empty());

    let posted = post(root, &account, &space, "Sync stops", "It stopped.").unwrap();
    let rkey = posted.uri.rsplit('/').next().unwrap();
    assert_eq!(posted.url, format!("https://userinput.app/d/{did}/{rkey}"));
    let (stored, cid) = pds.record(did, lexicon::DISCUSSION, rkey).unwrap();
    assert_eq!(posted.cid, cid);
    assert_eq!(stored["space"], json!({ "uri": space.uri(), "cid": space_cid }));
    assert_eq!((&stored["title"], &stored["body"]), (&json!("Sync stops"), &json!("It stopped.")));
    let create = pds.calls("com.atproto.repo.createRecord").pop().unwrap();
    assert_eq!(create.json()["collection"], lexicon::DISCUSSION);
    assert!(create.json().get("rkey").is_none(), "the server picks the key");

    // Too long: nothing is sent.
    let before = pds.requests().len();
    assert!(post(root, &account, &space, &"a".repeat(301), "").is_err());
    assert_eq!(pds.requests().len(), before);

    // The server refuses: it says so, and how to go on.
    pds.set_writes(Writes::RefuseScope);
    let err = post(root, &account, &space, "Sync stops", "").unwrap_err();
    assert_eq!(err.reason(), Some(Reason::MissingScope), "{err}");
    assert!(err.to_string().contains("sign in again, allowing bug reports"), "{err}");
}

#[test]
fn a_session_without_the_report_scope_is_stopped_before_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    crate::store::Store::open_or_init(root).unwrap();
    // Allowed to publish, not to report. Its server refuses connections: nothing may reach it.
    let publishing = crate::atmosphere::PUBLISH_SCOPES.join(" ");
    save_session(root, "did:plc:abc", "http://127.0.0.1:9", &format!("atproto {publishing}"));
    let account = status(root).unwrap().unwrap();
    let space = Space::parse(SPACE_URI).unwrap();
    let r = record(&space, "bafyreidfayvfuwqa7qlnopdjiqrxzs6blmoeu4rujcjtnci5beludirz2a", "t", "", Utc::now());
    let err = repo::create_record(root, &account.did, lexicon::DISCUSSION, &r).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::MissingScope), "{err}");
    assert!(err.to_string().contains("publish bug reports; sign in again, allowing bug reports"), "{err}");
}

#[test]
fn signing_in_again_to_report_keeps_publishing() {
    let publishing = crate::atmosphere::tests::mock_pds::granted(&crate::atmosphere::publish_scopes());
    let info = signed_in_with(&publishing.iter().map(String::as_str).collect::<Vec<_>>());
    let action = &repo::scope_error(&info, lexicon::DISCUSSION).detail().actions[0];
    let asked: Vec<&str> = action.params["scopes"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!(
        asked,
        ["atproto", "include:computer.localnetwork.locallibraryPublishing", "blob:audio/*", REPORT_SCOPE]
    );
    assert_eq!(action.label, "Sign in again, allowing bug reports");
}
