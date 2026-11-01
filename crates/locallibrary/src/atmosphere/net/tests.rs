use super::*;

/// Regression: the fifth redirect was refused, as reqwest counts the first request among the
/// ones before it.
#[test]
fn five_redirects_are_followed_and_no_more() {
    let url = "https://pds.example/";
    assert_eq!(redirect_refused(5, url), None, "the fifth");
    assert!(redirect_refused(6, url).is_some(), "the sixth");
    assert!(redirect_refused(1, "https://127.0.0.1/").is_some());
}

#[test]
fn an_unreadable_granted_scope_is_refused_before_jacquard_reads_it() {
    let answer = |scope: &str| serde_json::json!({ "access_token": "a", "scope": scope }).to_string();
    assert!(readable_scopes(answer("atproto repo:computer.localnetwork.locallibrary.settings").as_bytes()).is_ok());
    let err = readable_scopes(answer("atproto foo:bar").as_bytes()).unwrap_err();
    assert!(err.to_string().contains("can't read (atproto foo:bar)"), "{err}");
    // Anything else passes through untouched.
    assert!(readable_scopes(br#"{"scope":"foo:bar"}"#).is_ok());
    assert!(readable_scopes(b"<html>").is_ok());
    // jacquard would panic on it.
    assert!(Scopes::new(SmolStr::new("atproto foo:bar")).is_err());

    // Every answer jacquard gets goes through the check.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        use std::io::{BufRead, Write};
        let (mut conn, _) = listener.accept().unwrap();
        let mut reader = std::io::BufReader::new(conn.try_clone().unwrap());
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
            line.clear();
        }
        let body = answer("atproto foo:bar");
        let _ = write!(conn, "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{body}", body.len());
    });
    let http = Http::new(false).unwrap();
    let request = http::Request::post(url).body(Vec::new()).unwrap();
    let err = crate::atmosphere::block_on(http.send_http(request)).unwrap_err();
    assert!(err.to_string().contains("can't read"), "{err}");
}

use hegel::TestCase;
use hegel::generators as gs;

/// Redirects are followed to public addresses for the first five, and none after.
#[hegel::test(test_cases = 200)]
fn redirects_go_only_to_public_addresses_five_times(tc: TestCase) {
    let previous = tc.draw(gs::integers::<usize>().max_value(10));
    let url = tc.draw(gs::sampled_from(vec![
        "https://pds.example/",
        "https://bsky.social/xrpc/x",
        "http://pds.example/",
        "https://127.0.0.1/",
        "https://[fd00::1]/",
        "https://localhost./",
    ]));
    match redirect_refused(previous, url) {
        None => assert!(previous <= MAX_REDIRECTS && public(url).is_ok(), "{previous} {url}"),
        Some(why) if previous > MAX_REDIRECTS => assert_eq!(why, "too many redirects"),
        Some(why) => {
            assert!(why.starts_with("redirected to an address that isn't public: "), "{why}");
            assert!(why.ends_with(&public(url).unwrap_err()), "{why}");
        }
    }
}

/// A scope list jacquard can read: `(scope, readable)`.
#[hegel::test_helper]
fn scope_list(tc: &TestCase) -> (String, bool) {
    let atoms = vec![
        ("atproto", true),
        ("transition:generic", true),
        ("repo:computer.localnetwork.locallibrary.settings?action=create&action=update", true),
        ("repo:*", true),
        ("foo:bar", false),
        ("repo:app.example.thing?action=publish", false),
    ];
    let picked = tc.draw(gs::vecs(gs::sampled_from(atoms)).min_size(1).max_size(4));
    let scope = picked.iter().map(|(s, _)| *s).collect::<Vec<_>>().join(" ");
    (scope, picked.iter().all(|(_, ok)| *ok))
}

/// A token answer granting scopes jacquard can't read is refused, naming them on one line;
/// anything else (not a token answer, not JSON) passes through.
#[hegel::test(test_cases = 300)]
fn only_token_answers_with_unreadable_scopes_are_refused(tc: TestCase) {
    let (scope, readable) = scope_list(&tc);
    let token = tc.draw(gs::booleans());
    let mut answer = serde_json::json!({ "scope": scope });
    if token {
        answer["access_token"] = "a".into();
    }
    let (body, refused) = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => (b"<html>not json</html>".to_vec(), false),
        1 => (serde_json::json!({ "access_token": "a", "scope": 7 }).to_string().into_bytes(), false),
        _ => (answer.to_string().into_bytes(), token && !readable),
    };
    match readable_scopes(&body) {
        Ok(()) => assert!(!refused, "{scope}"),
        Err(e) => {
            assert!(refused, "{scope}: {e}");
            let said = format!("the server granted scopes this version of locallibrary can't read ({scope})");
            assert_eq!(e.to_string(), said);
        }
    }
}

/// Answers come back whole, with their status and headers, up to `MAX_BODY` and refused
/// past it; a redirect to this computer isn't followed; a client for public servers only
/// sends nothing to this computer.
#[hegel::test(test_cases = 6)]
fn answers_are_read_whole_up_to_the_limit(tc: TestCase) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, OnceLock};
    // Building a client loads the system's certificates: one of each, and one server, for
    // every case.
    static SERVER: OnceLock<(String, Arc<AtomicUsize>, Http, Http)> = OnceLock::new();
    let (base, asked, public_only, any) = SERVER.get_or_init(|| {
        let asked = Arc::new(AtomicUsize::new(0));
        let counted = asked.clone();
        // `GET /<status>/<length>`: a JSON string `length` bytes long, quotes and all; a
        // redirect is to this server again.
        let base = crate::atmosphere::tests::serving(move |base, target| {
            counted.fetch_add(1, Ordering::SeqCst);
            let mut parts = target.trim_start_matches("GET /").split('/');
            let status: u16 = parts.next().unwrap().parse().unwrap();
            let length: usize = parts.next().unwrap().parse().unwrap();
            let location = if status == 302 { format!("location: {base}/200/{length}\r\n") } else { String::new() };
            (status, format!("x-said: hello\r\n{location}"), serde_json::Value::String("a".repeat(length - 2)))
        });
        (base, asked, Http::new(true).unwrap(), Http::new(false).unwrap())
    });
    // A short answer, and one about as long as may be read.
    let short = tc.draw(gs::integers::<usize>().min_value(2).max_value(5000));
    let long = tc.draw(gs::integers::<usize>().min_value(MAX_BODY - 3).max_value(MAX_BODY + 3));
    let status = tc.draw(gs::sampled_from(vec![200u16, 400, 404, 500]));
    for (status, length) in [(status, short), (status, long), (302, short)] {
        let url = format!("{base}/{status}/{length}");
        let request = || http::Request::get(&url).body(Vec::new()).unwrap();

        let before = asked.load(Ordering::SeqCst);
        let refused = block(public_only.send_http(request())).unwrap_err();
        assert!(matches!(&refused, NetError::Refused(why) if why.contains("isn't an https address")), "{refused}");
        assert_eq!(asked.load(Ordering::SeqCst), before, "nothing was sent");

        match block(any.send_http(request())) {
            Err(e) if status == 302 => {
                assert!(e.to_string().contains("redirected to an address that isn't public"), "{e}");
                assert_eq!(asked.load(Ordering::SeqCst), before + 1, "the redirect wasn't followed");
            }
            Ok(answer) => {
                assert!(length <= MAX_BODY && status != 302, "{length}");
                assert_eq!(answer.status().as_u16(), status);
                assert_eq!(answer.headers()["x-said"], "hello");
                assert_eq!(answer.body().len(), length);
            }
            Err(e) => {
                assert!(length > MAX_BODY, "{length}: {e}");
                assert!(matches!(e, NetError::TooBig), "{e}");
                assert_eq!(e.to_string(), "the answer is over 1 MB");
            }
        }
    }
}

fn block<F: Future>(f: F) -> F::Output {
    crate::atmosphere::block_on(f)
}
