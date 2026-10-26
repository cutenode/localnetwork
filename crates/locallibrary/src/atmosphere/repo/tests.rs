use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use hegel::Generator;
use hegel::generators as gs;

use super::*;
use crate::atmosphere::Storage;

#[test]
fn a_session_without_permission_to_publish_is_stopped_before_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let record = crate::publish::settings_record(&[("quantize".into(), "off".into())].into(), chrono::Utc::now());
    let err = put_record(dir.path(), "did:plc:abc", lexicon::SETTINGS, "self", &record, None).unwrap_err();
    assert!(err.to_string().contains("not signed in"));
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing written");

    // Signed in to know who you are, nothing more. Its server refuses connections: nothing
    // may reach it.
    crate::atmosphere::tests::save_session(dir.path(), "did:plc:abc", "https://127.0.0.1:9", "atproto");
    let err = put_record(dir.path(), "did:plc:abc", lexicon::SETTINGS, "self", &record, None).unwrap_err();
    assert_eq!(err.code(), "account");
    assert!(err.to_string().contains("doesn't allow locallibrary to publish My Settings"));
    assert!(err.to_string().contains("sign in again, allowing publishing"));
    assert_eq!(err.reason(), Some(Reason::MissingScope));

    // A record that doesn't fit its Lexicon isn't sent either.
    let mut wrong = record.clone();
    wrong["player"]["quantize"] = 0.into();
    let err = put_record(dir.path(), "did:plc:abc", lexicon::SETTINGS, "self", &wrong, None).unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert_eq!(crate::atmosphere::status(dir.path()).unwrap().unwrap().storage, Storage::File);

    // Planned for another account than the one signed in now: not written to this one.
    let err = put_record(dir.path(), "did:plc:other", lexicon::SETTINGS, "self", &record, None).unwrap_err();
    assert_eq!((err.code(), err.reason()), ("conflict", Some(Reason::Changed)));
}

#[test]
fn a_did_is_taken_as_it_is_and_what_isnt_a_handle_isnt_looked_up() {
    let found = resolve(" @did:plc:abc ").unwrap();
    assert_eq!((found.did.as_str(), found.handle), ("did:plc:abc", None));
    assert!(resolve("not a handle").unwrap_err().to_string().contains("not a handle"));
}

/// However the account's server refuses a write, the error says what to do about it: sign
/// in again allowing publishing, sign in again, or what the server said.
#[test]
fn a_refused_write_says_how_to_go_on() {
    use std::sync::{Arc, Mutex};
    type Answer = (u16, String, Value);
    let next: Arc<Mutex<Answer>> = Arc::new(Mutex::new((200, String::new(), json!({}))));
    let answering = next.clone();
    let pds = crate::atmosphere::tests::serving(move |_, _| answering.lock().unwrap().clone());
    let dir = tempfile::tempdir().unwrap();
    crate::store::Store::open_or_init(dir.path()).unwrap();
    let scopes = crate::atmosphere::tests::mock_pds::granted(&crate::atmosphere::publish_scopes()).join(" ");
    crate::atmosphere::tests::save_session(dir.path(), "did:plc:abc", &pds, &scopes);
    let record = crate::publish::settings_record(&[("quantize".into(), "off".into())].into(), chrono::Utc::now());
    let write = |status: u16, headers: &str, body: Value| {
        *next.lock().unwrap() = (status, headers.to_string(), body);
        put_record(dir.path(), "did:plc:abc", lexicon::SETTINGS, "self", &record, None).unwrap_err()
    };

    for err in
        [write(403, "", json!({})), write(401, "www-authenticate: DPoP error=\"insufficient_scope\"\r\n", json!({}))]
    {
        assert_eq!(err.reason(), Some(Reason::MissingScope));
    }
    for err in [write(400, "", json!({ "error": "ExpiredToken" })), write(401, "", json!({ "error": "Unauthorized" }))]
    {
        assert_eq!(err.reason(), Some(Reason::SessionEnded));
    }
    let err = write(500, "", json!({ "error": "InternalServerError", "message": "boom" }));
    assert!(err.to_string().ends_with("refused to publish My Settings (InternalServerError: boom)"));
    let err = write(200, "", json!({ "nothing": "useful" }));
    assert!(err.to_string().contains("answered in a way locallibrary can't read"));

    let gone = tempfile::tempdir().unwrap();
    crate::store::Store::open_or_init(gone.path()).unwrap();
    crate::atmosphere::tests::save_session(gone.path(), "did:plc:abc", "http://127.0.0.1:9", &scopes);
    let err = put_record(gone.path(), "did:plc:abc", lexicon::SETTINGS, "self", &record, None).unwrap_err();
    assert!(err.to_string().starts_with("couldn't reach 127.0.0.1:9"));
}

#[test]
fn a_missing_scope_names_what_couldnt_be_published() {
    let info = crate::atmosphere::tests::signed_in_with(&["atproto"]);
    let said = |collection| scope_error(&info, collection).to_string();
    assert!(said(lexicon::PROFILE).contains("publish the profile;"));
    assert!(said(lexicon::SET).contains("publish sets;"));
    assert!(said("app.example.thing").contains("publish records;"));
}

/// A blob as a data server answers an upload.
pub(crate) fn blob() -> Value {
    let cid = "bafkreibme22gw2h7y2h7tg2fhqotaqjucnbc24deqo72b6mkl2egezxhvy";
    json!({ "$type": "blob", "ref": { "$link": cid }, "mimeType": "audio/mp4", "size": 1000 })
}

pub(crate) type Answer = (u16, String, Value);
/// A server's base URL, what it answers every request with, and how many requests it got.
pub(crate) type Shared = (String, Arc<Mutex<Answer>>, Arc<AtomicUsize>);

/// One server for all of a test's cases (a server per case would leave a thread behind each).
pub(crate) fn shared(server: &'static OnceLock<Shared>) -> &'static Shared {
    server.get_or_init(|| {
        let next = Arc::new(Mutex::new((200, String::new(), json!({}))));
        let got = Arc::new(AtomicUsize::new(0));
        let (answer, count) = (next.clone(), got.clone());
        let url = crate::atmosphere::tests::serving(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            answer.lock().unwrap().clone()
        });
        (url, next, got)
    })
}

/// Short text, sometimes one of `words`.
pub(crate) fn word(words: Vec<&'static str>) -> impl hegel::PrintableGenerator<String> {
    hegel::one_of!(gs::sampled_from(words).map(String::from), gs::text().max_size(8))
}

/// An at:// URI built from a DID, a record type and a record key parses back into those
/// three, and fits the Lexicons' `at-uri` format.
#[hegel::test(test_cases = 300)]
fn at_uris_parse_back_into_their_parts(tc: hegel::TestCase) {
    let did: String = tc.draw(hegel::one_of!(
        gs::from_regex("did:plc:[a-z2-7]{24}").fullmatch(true),
        gs::from_regex("did:web:[a-z][a-z0-9]{0,9}(\\.[a-z][a-z0-9]{0,9}){1,2}").fullmatch(true),
    ));
    let collection: String = tc.draw(hegel::one_of!(
        gs::sampled_from(lexicon::COLLECTIONS.to_vec()).map(String::from),
        gs::from_regex("[a-z][a-z0-9]{0,7}\\.[a-z][a-z0-9]{0,7}\\.[a-z][a-zA-Z0-9]{0,7}").fullmatch(true),
    ));
    let rkey: String = tc.draw(hegel::one_of!(
        gs::just("self".to_string()),
        gs::from_regex("[2-7a-j][2-7a-z]{12}").fullmatch(true),
        gs::from_regex("[a-zA-Z0-9_~-][a-zA-Z0-9._:~-]{0,20}").fullmatch(true),
    ));
    let uri = at_uri(&did, &collection, &rkey);
    let parsed = jacquard_common::types::aturi::AtUri::<SmolStr>::new_owned(&uri).unwrap();
    assert_eq!(parsed.authority().as_str(), did);
    assert_eq!(parsed.collection().unwrap().as_str(), collection);
    assert_eq!(parsed.rkey().unwrap().as_ref(), rkey);
    let set = json!({ "$type": lexicon::SET, "date": "2026-10-03", "tracks": [], "event": uri, "createdAt": "2026-10-04T09:30:00.000Z" });
    lexicon::validate(lexicon::SET, &set).unwrap();
}

/// A server is named by its URL without the scheme or trailing slashes.
#[hegel::test(test_cases = 200)]
fn a_server_is_named_by_its_url_without_scheme_or_slashes(tc: hegel::TestCase) {
    let name: String =
        tc.draw(gs::from_regex("[a-z0-9][a-z0-9.-]{0,11}(:[0-9]{1,5})?(/[a-z]{1,5}){0,2}").fullmatch(true));
    let scheme = tc.draw(gs::sampled_from(vec!["http", "https", "at"]));
    let slashes = "/".repeat(tc.draw(gs::integers::<usize>().max_value(3)));
    assert_eq!(host(&format!("{scheme}://{name}{slashes}")), name);
    assert_eq!(host(&format!("{name}{slashes}")), name);
}

/// However a server explains a refused write, the error says how to go on: a word about
/// scopes means signing in again allowing publishing, a refused session means signing in
/// again, and anything else says on one line what the server said and what wasn't published.
#[hegel::test(test_cases = 400)]
fn an_explained_refusal_says_how_to_go_on(tc: hegel::TestCase) {
    let mut info = crate::atmosphere::tests::signed_in_with(&["atproto"]);
    info.pds = tc.draw(gs::sampled_from(vec!["https://pds.example", "http://127.0.0.1:2583/"])).into();
    let collection =
        tc.draw(gs::sampled_from(vec![lexicon::SETTINGS, lexicon::PROFILE, lexicon::SET, "app.example.thing"]));
    let status = tc.draw(gs::sampled_from(vec![400u16, 401, 403, 409, 413, 500, 502]));
    let error =
        tc.draw(word(vec!["", "InvalidRequest", "AuthRequired", "InvalidToken", "ExpiredToken", "ScopeMissingError"]));
    let message = tc.draw(word(vec!["", "Missing required SCOPE", "Record is too big", "boom"]));
    let err = refusal(&info, collection, status, &error, &message);
    let scope = [&error, &message].iter().any(|s| s.to_ascii_lowercase().contains("scope"));
    let session = status == 401 || ["AuthRequired", "InvalidToken", "ExpiredToken"].contains(&error.as_str());
    let text = err.to_string();
    assert_eq!(err.code(), "account");
    assert!(!text.chars().any(char::is_control), "{text:?}");
    match (scope, session) {
        (true, _) => assert_eq!(err.reason(), Some(Reason::MissingScope), "{text}"),
        (false, true) => assert_eq!(err.reason(), Some(Reason::SessionEnded), "{text}"),
        (false, false) => {
            assert_eq!(err.reason(), None, "{text}");
            let why = if message.is_empty() { error.clone() } else { format!("{error}: {message}") };
            let host = host(&info.pds);
            assert_eq!(text, format!("{host} refused to publish {} ({})", what(collection), one_line(&why)));
        }
    }
    if scope || session {
        let again = &err.detail().actions[0];
        assert_eq!((again.method.as_deref(), &again.params["publish"]), (Some("account.login.begin"), &json!(true)));
    }
}

/// A write that can't go anywhere stops here, with nothing sent: a record that doesn't fit its
/// Lexicon, nobody signed in, another account signed in than the one the write was for, or a
/// session that may not write that kind of record, or a record key no repository takes, checked
/// in that order. Any other write goes to the server.
#[hegel::test(test_cases = 8)]
#[hegel::explicit_test_case(collection = lexicon::SETTINGS, fits = true, scopes = "atproto".to_string(), signed_in = Some("did:plc:abc"), unreachable = false, rkey = "self")]
#[hegel::explicit_test_case(collection = lexicon::PROFILE, fits = true, scopes = "atproto transition:generic".to_string(), signed_in = Some("did:plc:other"), unreachable = false, rkey = "self")]
#[hegel::explicit_test_case(collection = lexicon::SET, fits = true, scopes = "atproto transition:generic".to_string(), signed_in = Some("did:plc:abc"), unreachable = true, rkey = "self")]
#[hegel::explicit_test_case(collection = lexicon::SET, fits = true, scopes = "atproto transition:generic".to_string(), signed_in = Some("did:plc:abc"), unreachable = true, rkey = "a b")]
fn a_write_that_cant_go_stops_before_the_server(tc: hegel::TestCase) {
    static SERVER: OnceLock<Shared> = OnceLock::new();
    let (pds, next, got) = shared(&SERVER);
    *next.lock().unwrap() = (500, String::new(), json!({ "error": "InternalServerError" }));
    let collection = tc.draw(gs::sampled_from(lexicon::COLLECTIONS.to_vec()));
    let mut record = match collection {
        lexicon::SETTINGS => {
            crate::publish::settings_record(&[("sync".into(), "on".into())].into(), chrono::Utc::now())
        }
        lexicon::PROFILE => {
            json!({ "$type": collection, "generatedAt": "2026-10-04T09:30:00.000Z", "precision": "exact", "basis": "plays" })
        }
        lexicon::RECORDING => crate::publish::recording::record(blob(), 3600, chrono::Utc::now()),
        _ => {
            json!({ "$type": collection, "date": "2026-10-03", "tracks": [], "createdAt": "2026-10-04T09:30:00.000Z" })
        }
    };
    let fits: bool = tc.draw(gs::booleans());
    if !fits {
        record["mood"] = "dark".into();
    }
    let all = crate::atmosphere::tests::mock_pds::granted(&crate::atmosphere::publish_scopes()).join(" ");
    let one = |c: &str| format!("atproto repo:{c}?action=create&action=update");
    let scopes: String = tc.draw(gs::sampled_from(vec![
        "atproto".to_string(),
        all,
        one(lexicon::SETTINGS),
        one(lexicon::SET),
        "atproto transition:generic".into(),
    ]));
    let signed_in = tc.draw(gs::optional(gs::sampled_from(vec!["did:plc:abc", "did:plc:other"])));
    // Now and then the account's server is one that refuses connections.
    let unreachable: bool = tc.draw(gs::weighted_booleans(0.2));
    // Now and then a record key no repository takes.
    let rkey = tc.draw(gs::sampled_from(vec!["self", "self", "a b", ".."]));
    let dir = tempfile::tempdir().unwrap();
    if let Some(did) = signed_in {
        crate::store::Store::open_or_init(dir.path()).unwrap();
        let server = if unreachable { "http://127.0.0.1:9" } else { pds.as_str() };
        crate::atmosphere::tests::save_session(dir.path(), did, server, &scopes);
    }

    let before = got.load(Ordering::SeqCst);
    let err = put_record(dir.path(), "did:plc:abc", collection, rkey, &record, None).unwrap_err();
    let may_write = scopes.contains("transition:generic") || scopes.contains(&format!("repo:{collection}?"));
    let sent = got.load(Ordering::SeqCst) > before;
    match signed_in {
        _ if !fits => assert_eq!(err.code(), "invalid"),
        None => assert!(err.to_string().contains("not signed in")),
        Some("did:plc:other") => {
            assert_eq!((err.code(), err.reason()), ("conflict", Some(Reason::Changed)))
        }
        Some(_) if !may_write => assert_eq!(err.reason(), Some(Reason::MissingScope)),
        Some(_) if rkey != "self" => {
            assert_eq!(err.code(), "invalid");
            assert!(err.to_string().starts_with(&format!("the {} record: ", lexicon::short_name(collection))));
        }
        Some(_) if unreachable => assert!(err.to_string().starts_with("couldn't reach 127.0.0.1:9")),
        Some(_) => {
            assert!(sent);
            assert!(
                err.to_string().ends_with(&format!("refused to publish {} (InternalServerError)", what(collection)))
            );
            return;
        }
    }
    assert!(!sent);
    if signed_in.is_none() {
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing written");
    }
}

/// However the account's server refuses a write (other than refusing the session itself),
/// the error says how to go on: InvalidSwap is a conflict, a token the server calls invalid
/// or expired means signing in again, a word about scopes or a bare 403 means signing in
/// again allowing publishing, an answer that isn't XRPC can't be read, and anything else
/// says what the server said, or its status.
#[hegel::test(test_cases = 1)]
#[hegel::explicit_test_case(status = 200u16, error = String::new(), message = String::new(), challenge = 0usize)]
#[hegel::explicit_test_case(status = 400u16, error = "InvalidToken".to_string(), message = String::new(), challenge = 0usize)]
#[hegel::explicit_test_case(status = 400u16, error = "InvalidSwap".to_string(), message = "at another CID".to_string(), challenge = 0usize)]
#[hegel::explicit_test_case(status = 400u16, error = "InvalidRequest".to_string(), message = "Record is too big".to_string(), challenge = 0usize)]
#[hegel::explicit_test_case(status = 403u16, error = String::new(), message = String::new(), challenge = 0usize)]
#[hegel::explicit_test_case(status = 502u16, error = String::new(), message = String::new(), challenge = 0usize)]
#[hegel::explicit_test_case(status = 401u16, error = String::new(), message = String::new(), challenge = 1usize)]
#[hegel::explicit_test_case(status = 401u16, error = String::new(), message = String::new(), challenge = 2usize)]
#[hegel::explicit_test_case(status = 401u16, error = "Unauthorized".to_string(), message = String::new(), challenge = 0usize)]
fn a_refused_write_says_how_to_go_on_whatever_the_server_says(tc: hegel::TestCase) {
    static SERVER: OnceLock<Shared> = OnceLock::new();
    let (pds, next, _) = shared(&SERVER);
    let status = tc.draw(gs::sampled_from(vec![200u16, 400, 401, 403, 409, 413, 500, 502]));
    let error = tc.draw(word(vec![
        "",
        "InvalidSwap",
        "InvalidRequest",
        "InvalidToken",
        "ExpiredToken",
        "ScopeMissingError",
        "AuthRequired",
    ]));
    let message = tc.draw(word(vec!["", "Missing required scope", "Record is too big"]));
    // What a 401 says in its WWW-Authenticate header: nothing, that the token lacks a scope,
    // or that the token is no good.
    let challenge = tc.draw(gs::integers::<usize>().max_value(2));
    let header = match (status, challenge) {
        (401, 1) => "www-authenticate: DPoP error=\"insufficient_scope\"\r\n",
        (401, 2) => "www-authenticate: DPoP error=\"invalid_token\"\r\n",
        _ => "",
    };
    let mut body = serde_json::Map::new();
    for (k, v) in [("error", &error), ("message", &message)] {
        if !v.is_empty() {
            body.insert(k.into(), v.as_str().into());
        }
    }
    *next.lock().unwrap() = (status, header.to_string(), Value::Object(body));
    let dir = tempfile::tempdir().unwrap();
    crate::store::Store::open_or_init(dir.path()).unwrap();
    crate::atmosphere::tests::save_session(
        dir.path(),
        "did:plc:abc",
        pds,
        &crate::atmosphere::tests::mock_pds::granted(&crate::atmosphere::publish_scopes()).join(" "),
    );
    let record = crate::publish::settings_record(&[("sync".into(), "on".into())].into(), chrono::Utc::now());
    let err = put_record(dir.path(), "did:plc:abc", lexicon::SETTINGS, "self", &record, None).unwrap_err();
    let text = err.to_string();
    assert!(!text.chars().any(char::is_control), "{text:?}");

    let scope = [&error, &message].iter().any(|s| s.to_ascii_lowercase().contains("scope"));
    let host = host(pds);
    let refused = |e: &str| {
        let why = if message.is_empty() { e.to_string() } else { format!("{e}: {message}") };
        format!("{host} refused to publish My Settings ({})", one_line(&why))
    };
    let shown = (text.as_str(), err.reason());
    let token = ["InvalidToken", "ExpiredToken"].contains(&error.as_str());
    match status {
        401 if challenge == 1 => assert_eq!(err.reason(), Some(Reason::MissingScope), "{text}"),
        401 if challenge == 2 => assert_eq!(err.reason(), Some(Reason::SessionEnded), "{text}"),
        200 => assert!(text.contains("answered in a way locallibrary can't read"), "{text}"),
        400 | 401 if error.is_empty() => {
            assert!(text.contains("answered in a way locallibrary can't read"), "{text}")
        }
        400 | 401 if token => assert_eq!(err.reason(), Some(Reason::SessionEnded), "{text}"),
        400 if error == "InvalidSwap" => {
            assert_eq!((err.code(), err.reason()), ("conflict", Some(Reason::Changed)))
        }
        _ if scope => assert_eq!(err.reason(), Some(Reason::MissingScope), "{text}"),
        401 => assert_eq!(err.reason(), Some(Reason::SessionEnded), "{text}"),
        403 if error.is_empty() && message.is_empty() => assert_eq!(err.reason(), Some(Reason::MissingScope)),
        _ if ["AuthRequired", "InvalidToken", "ExpiredToken"].contains(&error.as_str()) => {
            assert_eq!(err.reason(), Some(Reason::SessionEnded), "{text}")
        }
        400 => assert_eq!(shown, (refused(&error).as_str(), None)),
        _ if error.is_empty() => assert_eq!(shown, (refused(&status.to_string()).as_str(), None)),
        _ => assert_eq!(shown, (refused(&error).as_str(), None)),
    }
}

/// What isn't a handle or a DID is never looked up: it isn't found, and the error names what
/// was asked for (without its `@` and blanks) on one line.
#[hegel::test(test_cases = 3)]
fn what_isnt_a_handle_or_did_isnt_looked_up(tc: hegel::TestCase) {
    let asked: String = if tc.draw(gs::booleans()) {
        let name: String = tc.draw(gs::from_regex("[a-z]{2,8}(\\.[a-z]{2,8}){1,2}").fullmatch(true));
        let odd = tc.draw(gs::sampled_from(vec![' ', '!', '/', '?', '#', '_', '*', '\u{1b}']));
        let at = tc.draw(gs::integers::<usize>().min_value(1).max_value(name.len() - 1));
        format!("{}{odd}{}", &name[..at], &name[at..])
    } else {
        tc.draw(gs::sampled_from(vec![
            "did:PLC:abc",
            "did:plc:",
            "did:plc:a b",
            "did:plc:abc:",
            "did:",
            "did:plc:\u{7}",
        ]))
        .to_string()
    };
    let pad = tc.draw(gs::sampled_from(vec!["", " ", "\t", "  "]));
    let at = if tc.draw(gs::booleans()) { "@" } else { "" };
    let err = resolve(&format!("{pad}{at}{asked}{pad}")).unwrap_err();
    assert_eq!(err.code(), "not_found");
    let text = err.to_string();
    assert!(text.contains(&one_line(&format!("the Atmosphere account {asked:?} ("))), "{text}");
    assert!(!text.chars().any(char::is_control), "{text:?}");
}
