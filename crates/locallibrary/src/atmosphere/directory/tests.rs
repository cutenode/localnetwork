use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use hegel::generators as gs;
use serde_json::json;

use super::*;
use crate::atmosphere::lexicon;
use crate::atmosphere::repo::tests::{Shared, shared, word};

/// Requests from this thread go to `url` as Local Network's directory and website.
fn at(url: &str) {
    crate::testing::directory_on_this_thread(Some(url.to_string()));
}

/// A server on 127.0.0.1 that answers each connection with the next of `answers`, and the
/// number of connections it got.
fn server(answers: Vec<Vec<u8>>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let got = Arc::new(AtomicUsize::new(0));
    let counted = got.clone();
    std::thread::spawn(move || {
        for (conn, answer) in listener.incoming().zip(answers) {
            let mut conn = conn.unwrap();
            counted.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(conn.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let _ = conn.write_all(&answer);
        }
    });
    (url, got)
}

fn answer(head: &str, body: &[u8]) -> Vec<u8> {
    let mut out =
        format!("HTTP/1.1 {head}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out
}

#[test]
fn the_directory_is_local_networks_unless_named() {
    crate::testing::directory_on_this_thread(None);
    assert_eq!(url("LOCALLIBRARY_TEST_NEVER_SET", HAPPYVIEW_URL), HAPPYVIEW_URL);
    at("http://127.0.0.1:2583/ ");
    assert_eq!((happyview(), website()), ("http://127.0.0.1:2583".to_string(), "http://127.0.0.1:2583".to_string()));
    crate::testing::directory_on_this_thread(None);
}

/// A website's service DID is did:web of its host, with its port when it has one.
#[test]
fn a_website_goes_by_its_hosts_did_web() {
    assert_eq!(service_did(WEBSITE_URL).as_deref(), Some("did:web:localnetwork.computer"));
    assert_eq!(service_did("http://127.0.0.1:5173/").as_deref(), Some("did:web:127.0.0.1%3A5173"));
    assert_eq!(service_did("https://example.com:443"), Some("did:web:example.com".into()), "the default port");
    assert_eq!(service_did("not a url"), None);
}

#[test]
fn reading_from_a_directory_that_isnt_there_says_so() {
    at("http://127.0.0.1:9");
    let err = get_record("did:plc:abc", lexicon::SETTINGS, "self").unwrap_err();
    assert_eq!(err.code(), "account");
    assert!(err.to_string().starts_with("couldn't reach Local Network's directory ("), "{err}");
}

#[test]
fn the_directory_is_read_with_care() {
    // A redirect to plain http, or to this computer, isn't followed.
    let (elsewhere, reached) = server(vec![answer("200 OK", b"{}")]);
    at(&server(vec![answer(&format!("302 Found\r\nlocation: {elsewhere}/x"), b"")]).0);
    let err = get_record("did:plc:abc", lexicon::SETTINGS, "self").unwrap_err();
    assert!(err.to_string().contains("isn't an https address"), "{err}");
    at(&server(vec![answer("302 Found\r\nlocation: https://127.0.0.1:9/x", b"")]).0);
    let err = get_record("did:plc:abc", lexicon::SETTINGS, "self").unwrap_err();
    assert!(err.to_string().contains("on this computer or its local network"), "{err}");
    assert_eq!(reached.load(Ordering::SeqCst), 0);

    // An answer that doesn't stop isn't read whole.
    at(&server(vec![answer("200 OK", &vec![b' '; net::MAX_BODY + 1])]).0);
    let err = get_record("did:plc:abc", lexicon::SETTINGS, "self").unwrap_err();
    assert!(err.to_string().contains("the answer is over 1 MB"), "{err}");

    // What it says goes to a terminal: no escape sequences.
    let uri = at_uri("did:plc:abc", lexicon::SETTINGS, "self");
    let records = json!({ "records": [{ "uri": uri, "cid": "bafy\u{1b}[2J", "value": { "n": 1 } }] });
    let refusal = json!({ "error": "Nope\u{1b}[2J", "message": "really\u{1b}]52;c;Zm9v\u{7}" });
    at(&server(vec![
        answer("200 OK", records.to_string().as_bytes()),
        answer("400 Bad Request", refusal.to_string().as_bytes()),
    ])
    .0);
    let fetched = get_record("did:plc:abc", lexicon::SETTINGS, "self").unwrap().unwrap();
    assert_eq!((fetched.uri, fetched.cid.as_str()), (uri, "bafy [2J"));
    let err = get_record("did:plc:abc", lexicon::SETTINGS, "self").unwrap_err().to_string();
    assert!(!err.chars().any(char::is_control) && err.contains("Nope [2J: really ]52;c;Zm9v "), "{err:?}");
}

/// A refusal to give a record says what it can: its error, else the status.
#[test]
fn a_refusal_to_read_says_its_error_or_status() {
    at(&server(vec![
        answer("400 Bad Request", br#"{"error":"InvalidRequest"}"#),
        answer("502 Bad Gateway", b"<html>"),
    ])
    .0);
    let err = get_record("did:plc:abc", lexicon::SETTINGS, "self").unwrap_err().to_string();
    assert!(err.ends_with("(InvalidRequest)"), "{err}");
    let err = get_record("did:plc:abc", lexicon::SETTINGS, "self").unwrap_err().to_string();
    assert!(err.ends_with("(502 Bad Gateway)"), "{err}");
}

/// Whatever the directory answers a read with, what's read is what it said: the record asked for
/// (with its CID on one line) when its records hold it with one, nothing when they don't, and otherwise an
/// error on one line giving its error and message, or its status.
#[hegel::test(test_cases = 40)]
#[hegel::explicit_test_case(status = 200u16, listed = true, uri = "at://did:plc:abc/computer.localnetwork.locallibrary.settings/self".to_string(), cid = Some("bafy".to_string()), value = json!({ "n": 1 }), error = None::<String>, message = None::<String>)]
#[hegel::explicit_test_case(status = 200u16, listed = true, uri = "at://did:plc:abc/computer.localnetwork.locallibrary.settings/other".to_string(), cid = Some("bafy".to_string()), value = json!({ "n": 1 }), error = None::<String>, message = None::<String>)]
#[hegel::explicit_test_case(status = 500u16, listed = false, uri = "at://did:plc:abc/computer.localnetwork.locallibrary.settings/self".to_string(), cid = None::<String>, value = Value::Null, error = Some("InternalServerError".to_string()), message = Some("down".to_string()))]
fn a_read_gives_what_the_directory_answered(tc: hegel::TestCase) {
    static SERVER: OnceLock<Shared> = OnceLock::new();
    let (directory, next, _) = shared(&SERVER);
    at(directory);
    let status = tc.draw(gs::sampled_from(vec![200u16, 400, 500, 502]));
    let listed = tc.draw(gs::booleans());
    let asked = at_uri("did:plc:abc", lexicon::SETTINGS, "self");
    let uris = vec![asked.clone(), at_uri("did:plc:abc", lexicon::SETTINGS, "other"), "at://\u{1b}]0;x".into()];
    let uri = tc.draw(gs::sampled_from(uris));
    let cid = tc.draw(gs::optional(word(vec!["bafy\u{1b}[2J"])));
    let values = vec![json!({ "$type": lexicon::SETTINGS, "n": 1 }), json!("x"), json!([]), Value::Null];
    let value: Value = tc.draw(gs::sampled_from(values));
    let said = || gs::optional(word(vec!["InvalidRequest", "Nope\u{1b}[2J"]));
    let error = tc.draw(said());
    let message = tc.draw(said());
    let mut body = serde_json::Map::new();
    if listed {
        body.insert("records".into(), json!([{ "uri": uri, "cid": cid, "value": value }]));
    }
    for (field, v) in [("error", error), ("message", message)] {
        body.extend(v.map(|v| (field.to_string(), Value::from(v))));
    }
    let body = Value::Object(body);
    *next.lock().unwrap() = (status, String::new(), body.clone());

    let answered = status == 200 && listed;
    let is_record = answered && uri == asked && value.is_object() && cid.is_some();
    match get_record("did:plc:abc", lexicon::SETTINGS, "self") {
        Ok(Some(f)) => {
            assert!(is_record, "{body}");
            assert_eq!((f.uri, Some(f.cid), f.value), (asked, cid.as_deref().map(one_line), value));
        }
        Ok(None) => assert!(answered && !is_record, "{body}"),
        Err(e) => {
            assert!(!answered, "{body}");
            let why = match (body["error"].as_str(), body["message"].as_str()) {
                (Some(e), Some(m)) => format!("{e}: {m}"),
                (Some(e), None) => e.to_string(),
                _ => reqwest::StatusCode::from_u16(status).unwrap().to_string(),
            };
            assert_eq!(e.code(), "account");
            let host = host(directory);
            assert_eq!(
                e.to_string(),
                one_line(&format!("Local Network's directory at {host} couldn't give {asked} ({why})"))
            );
        }
    }
}

/// A read from a directory nobody answers at fails saying so on one line, whatever was asked for
/// and however its address ends.
#[hegel::test(test_cases = 20)]
fn a_read_from_a_directory_nobody_answers_at_says_so(tc: hegel::TestCase) {
    // A port that was just free: nothing listens there now.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let slashes = "/".repeat(tc.draw(gs::integers::<usize>().max_value(2)));
    at(&format!("http://127.0.0.1:{port}{slashes}"));
    let did = tc.draw(word(vec!["did:plc:abc", "did:web:dj.example.com"]));
    let collection = tc.draw(gs::sampled_from(lexicon::COLLECTIONS.to_vec()));
    let rkey = tc.draw(word(vec!["self", "3m2abcdefgh22"]));
    let err = get_record(&did, collection, &rkey).unwrap_err();
    assert_eq!(err.code(), "account");
    let text = err.to_string();
    assert!(text.starts_with("couldn't reach Local Network's directory ("), "{text}");
    assert!(!text.contains('\n'), "{text}");
}

/// Asked once it's done, or for a session that may not ask, nothing goes out: the second says why
/// in a note for the plan.
#[test]
fn the_directory_is_only_asked_when_it_can_be_and_hasnt_been() {
    let dir = tempfile::tempdir().unwrap();
    let (website, got) = server(vec![]);
    at(&website);
    crate::atmosphere::tests::save_session(dir.path(), "did:plc:abc", &website, "atproto");
    let info = crate::atmosphere::status(dir.path()).unwrap().unwrap();
    let note = request_index(dir.path(), &info).unwrap().unwrap();
    assert!(note.ends_with("(the session doesn't allow it: sign in again, allowing publishing)"), "{note}");

    let mut file = AccountFile::load(dir.path()).unwrap();
    file.indexed.push("did:plc:abc".into());
    file.save(dir.path()).unwrap();
    assert_eq!(request_index(dir.path(), &info).unwrap(), None);
    assert_eq!(got.load(Ordering::SeqCst), 0);

    // A session that may ask, with a website that isn't a URL, or an account's server nobody
    // answers at: the note says which.
    let other = tempfile::tempdir().unwrap();
    crate::store::Store::open_or_init(other.path()).unwrap();
    let scopes = crate::atmosphere::tests::mock_pds::granted(&crate::atmosphere::publish_scopes()).join(" ");
    crate::atmosphere::tests::save_session(other.path(), "did:plc:abc", "http://127.0.0.1:9", &scopes);
    let info = crate::atmosphere::status(other.path()).unwrap().unwrap();
    at("not a url");
    assert!(request_index(other.path(), &info).unwrap().unwrap().ends_with("(\"not a url\" isn't a URL)"));
    at("http://127.0.0.1:9");
    let note = request_index(other.path(), &info).unwrap().unwrap();
    assert!(note.contains("(the account's server: "), "{note}");
}

/// What the account's server and the website answer, keyed by which of them is asked.
type Answers = Arc<std::sync::Mutex<(Answer, Answer)>>;
type Answer = (u16, String, Value);

/// However the account's server and the website answer, asking the directory to read the account
/// again ends one way: remembered (and not asked again) once the website says the directory has
/// read it, otherwise a note saying why, on one line: the account's server refused a token, the
/// website's error or status, or the directory is still reading.
#[hegel::test(test_cases = 30)]
fn asking_the_directory_ends_as_the_servers_said(tc: hegel::TestCase) {
    static SERVER: OnceLock<(String, Answers)> = OnceLock::new();
    let (url, answers) = SERVER.get_or_init(|| {
        let answers: Answers = Arc::new(std::sync::Mutex::new(Default::default()));
        let answering = answers.clone();
        let url = crate::atmosphere::tests::serving(move |_, target| {
            let (token, website) = answering.lock().unwrap().clone();
            if target.contains("getServiceAuth") { token } else { website }
        });
        (url, answers)
    });
    at(url);
    let token = tc.draw(gs::sampled_from(vec![
        (200u16, json!({ "token": "a.b.c" })),
        (400, json!({ "error": "InvalidRequest", "message": "Bad token scope\u{1b}[2J" })),
    ]));
    let website = tc.draw(gs::sampled_from(vec![
        (200u16, json!({ "indexed": true })),
        (200, json!({ "indexed": false })),
        (401, json!({ "error": "AuthenticationRequired", "message": "Invalid service auth" })),
        (502, json!("bad gateway")),
    ]));
    *answers.lock().unwrap() =
        ((token.0, String::new(), token.1.clone()), (website.0, String::new(), website.1.clone()));

    let dir = tempfile::tempdir().unwrap();
    crate::store::Store::open_or_init(dir.path()).unwrap();
    let scopes = crate::atmosphere::tests::mock_pds::granted(&crate::atmosphere::publish_scopes()).join(" ");
    crate::atmosphere::tests::save_session(dir.path(), "did:plc:abc", url, &scopes);
    let info = crate::atmosphere::status(dir.path()).unwrap().unwrap();
    let note = request_index(dir.path(), &info).unwrap();
    let host = host(url);
    let why = match (token.0, website.0, &website.1["indexed"]) {
        (400, ..) => Some("the account's server".to_string()),
        (_, 200, Value::Bool(true)) => None,
        (_, 200, _) => Some("it's still reading them".to_string()),
        (_, 401, _) => Some(format!("{host} answered AuthenticationRequired")),
        _ => Some(format!("{host} answered 502")),
    };
    match (&why, &note) {
        (None, None) => {
            assert_eq!(AccountFile::load(dir.path()).unwrap().indexed, ["did:plc:abc"]);
            assert_eq!(request_index(dir.path(), &info).unwrap(), None, "asked once");
        }
        (Some(why), Some(note)) => {
            let start = "Local Network's directory couldn't be asked to read this account's earlier records (";
            assert!(note.starts_with(&format!("{start}{why}")), "{note}");
            assert!(!note.chars().any(char::is_control), "{note:?}");
            assert!(AccountFile::load(dir.path()).unwrap().indexed.is_empty());
        }
        _ => panic!("{why:?} but {note:?}"),
    }
}
