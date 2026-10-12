//! Requests to Atmosphere servers, through clients from here.
//!
//! Where a request goes is often someone else's say-so: a DID document names the account's data
//! server, that server's metadata names its authorization server and endpoints, and any of them
//! can answer with a redirect. Those requests only go to public https addresses ([`public`]):
//! never plain http, this computer or its local network. So do signing in and a client document
//! the user names. The signed-in account's own servers, and a data server named to import
//! settings from (which may be on this computer, for testing), are taken as they are, but their
//! redirects too only go to public https addresses. Answers are read up to [`MAX_BODY`].
//!
//! What a name resolves to isn't checked: reqwest resolves a proxy's name the same way, and
//! proxies are usually on the local network. A name that leads to a private address gets no
//! request: the server there would need a certificate for that name first.

use std::net::IpAddr;

use jacquard_common::SmolStr;
use jacquard_common::http_client::HttpClient;
use jacquard_oauth::scopes::Scopes;

use crate::export::one_line;

/// The most of an answer read: far more than any record, DID document or server metadata.
pub(crate) const MAX_BODY: usize = 1 << 20;

/// The most redirects followed.
const MAX_REDIRECTS: usize = 5;

/// Why a request wasn't sent, or its answer not read.
#[derive(Debug, thiserror::Error)]
pub(crate) enum NetError {
    #[error("{0}")]
    Refused(String),
    #[error("the answer is over {} MB", MAX_BODY >> 20)]
    TooBig,
    #[error("{}", with_causes(.0))]
    Http(#[from] reqwest::Error),
}

/// An error and what caused it, each cause once: reqwest's own message leaves the cause out,
/// and so do some of jacquard's, while others name it.
pub(crate) fn with_causes(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        let said = c.to_string();
        if !out.contains(&said) {
            out = format!("{out}: {said}");
        }
        cause = c.source();
    }
    out
}

/// `Ok` when `url` is a public https address; otherwise why not.
pub(crate) fn public(url: &str) -> Result<(), String> {
    let shown = one_line(url);
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{shown} isn't a web address"))?;
    if parsed.scheme() != "https" {
        return Err(format!("{shown} isn't an https address"));
    }
    let host = parsed.host_str().unwrap_or_default();
    if host.is_empty() || this_computer(&parsed) || ip(&parsed).is_some_and(local) {
        return Err(format!("{shown} is on this computer or its local network"));
    }
    Ok(())
}

/// Whether `url` names this computer: `localhost` or a name below it, or a loopback address.
pub(crate) fn this_computer(url: &reqwest::Url) -> bool {
    let host = url.host_str().unwrap_or_default().trim_end_matches('.');
    host == "localhost" || host.ends_with(".localhost") || ip(url).is_some_and(|ip| ip.to_canonical().is_loopback())
}

/// The address `url` names, when it's given as one.
fn ip(url: &reqwest::Url) -> Option<IpAddr> {
    url.host_str()?.trim_start_matches('[').trim_end_matches(']').parse().ok()
}

/// This computer, a private network, or a link's own addresses.
fn local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.to_ipv4_mapped().is_some_and(|v4| local(IpAddr::V4(v4)))
        }
    }
}

/// A client builder whose redirects only go to public https addresses, at most five.
pub(crate) fn client() -> reqwest::ClientBuilder {
    let redirects = reqwest::redirect::Policy::custom(|attempt| {
        match redirect_refused(attempt.previous().len(), attempt.url().as_str()) {
            Some(why) => attempt.error(why),
            None => attempt.follow(),
        }
    });
    reqwest::Client::builder().redirect(redirects)
}

/// Why a redirect to `url` isn't followed, if it isn't, after `previous` requests in its chain
/// (reqwest counts the first one, which wasn't a redirect).
fn redirect_refused(previous: usize, url: &str) -> Option<String> {
    if previous > MAX_REDIRECTS {
        return Some("too many redirects".into());
    }
    public(url).err().map(|why| format!("redirected to an address that isn't public: {why}"))
}

/// An answer's body, up to [`MAX_BODY`].
pub(crate) async fn body(mut response: reqwest::Response) -> Result<Vec<u8>, NetError> {
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if out.len() + chunk.len() > MAX_BODY {
            return Err(NetError::TooBig);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// The HTTP client jacquard's identity resolution, OAuth and XRPC go through: [`client`]'s, and
/// with `public_only` (servers found from someone's handle or DID) only to [`public`] addresses.
#[derive(Clone)]
pub(crate) struct Http {
    client: reqwest::Client,
    public_only: bool,
}

impl Http {
    pub(crate) fn new(public_only: bool) -> crate::Result<Http> {
        let client = client().build().map_err(|e| crate::Error::Account(format!("the network: {e}")))?;
        Ok(Http { client, public_only })
    }
}

impl HttpClient for Http {
    type Error = NetError;

    async fn send_http(&self, request: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>, NetError> {
        let url = request.uri().to_string();
        if self.public_only {
            public(&url).map_err(NetError::Refused)?;
        }
        let (parts, sent) = request.into_parts();
        let response = self.client.request(parts.method, &url).headers(parts.headers).body(sent).send().await?;
        let mut answer = http::Response::builder().status(response.status());
        if let Some(headers) = answer.headers_mut() {
            headers.extend(response.headers().clone());
        }
        let body = body(response).await?;
        readable_scopes(&body)?;
        answer.body(body).map_err(|e| NetError::Refused(e.to_string()))
    }
}

/// jacquard-oauth 0.12.2 panics on a token answer that grants a scope it can't parse
/// (`client.rs:344-346`); refusing such an answer here makes signing in fail instead.
fn readable_scopes(body: &[u8]) -> Result<(), NetError> {
    let Ok(answer) = serde_json::from_slice::<serde_json::Value>(body) else { return Ok(()) };
    match answer["scope"].as_str() {
        Some(scope) if answer["access_token"].is_string() && Scopes::new(SmolStr::new(scope)).is_err() => {
            Err(NetError::Refused(format!(
                "the server granted scopes this version of locallibrary can't read ({})",
                one_line(scope)
            )))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_https_addresses_are_public() {
        for ok in
            ["https://bsky.social/xrpc", "https://pds.example.com:8443/", "https://8.8.8.8/", "https://[2606:4700::1]/"]
        {
            assert_eq!(public(ok), Ok(()), "{ok}");
        }
        for refused in [
            "http://bsky.social/",
            "ftp://bsky.social/",
            "file:///etc/hosts",
            "https://localhost/",
            "https://pds.localhost./",
            "https://127.0.0.1:2583/",
            "https://0x7f.1/",
            "https://10.0.0.1/",
            "https://192.168.1.1/",
            "https://172.16.0.1/",
            "https://169.254.169.254/latest/meta-data",
            "https://0.0.0.0/",
            "https://[::1]/",
            "https://[fd00::1]/",
            "https://[fe80::1]/",
            "https://[::ffff:192.168.1.1]/",
            "not a url",
        ] {
            assert!(public(refused).is_err(), "{refused}");
        }
        assert!(!public("https://a\u{1b}]0;x/").unwrap_err().chars().any(char::is_control));
    }

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

    use hegel::generators as gs;
    use hegel::{Generator, TestCase};

    /// An IPv4 address, often from the ranges `public` treats specially.
    #[hegel::test_helper]
    fn ipv4(tc: &TestCase) -> [u8; 4] {
        let first = tc.draw(hegel::one_of!(
            gs::sampled_from(vec![0u8, 1, 8, 10, 100, 127, 169, 172, 192, 224, 255]),
            gs::integers::<u8>()
        ));
        let second =
            tc.draw(hegel::one_of!(gs::sampled_from(vec![0u8, 15, 16, 31, 32, 168, 254]), gs::integers::<u8>()));
        [first, second, tc.draw(gs::integers::<u8>().max_value(3)), tc.draw(gs::integers::<u8>())]
    }

    /// This computer, a private network (RFC 1918), a link's own (169.254/16), or no address.
    fn local_v4([a, b, c, d]: [u8; 4]) -> bool {
        a == 127
            || a == 10
            || (a == 172 && (16..32).contains(&b))
            || (a, b) == (192, 168)
            || (a, b) == (169, 254)
            || [a, b, c, d] == [0; 4]
    }

    /// `v4` the ways a URL may spell it: dotted (with or without a final dot), one decimal
    /// number, or hex.
    #[hegel::test_helper]
    fn spelled_v4(tc: &TestCase, v4: [u8; 4]) -> String {
        let n = u32::from_be_bytes(v4);
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => format!("{}.{}.{}.{}", v4[0], v4[1], v4[2], v4[3]),
            1 => format!("{}.{}.{}.{}.", v4[0], v4[1], v4[2], v4[3]),
            2 => n.to_string(),
            _ => format!("0x{n:x}"),
        }
    }

    #[hegel::test_helper]
    fn port_and_path(tc: &TestCase) -> String {
        let port = tc.draw(gs::optional(gs::integers::<u16>().min_value(1)));
        let path = tc.draw(gs::sampled_from(vec!["/", "/xrpc/com.atproto.server.getSession", "/oauth/token?x=1", ""]));
        format!("{}{path}", port.map(|p| format!(":{p}")).unwrap_or_default())
    }

    /// An https address naming an IPv4 address, however it's spelled, is public unless the address
    /// is this computer's, a private network's, a link's own or none; it names this computer
    /// only when it's a loopback address; and any other scheme is refused.
    #[hegel::test(test_cases = 400)]
    fn ipv4_addresses_are_public_unless_theyre_local(tc: TestCase) {
        let v4 = ipv4(&tc);
        let url = format!("https://{}{}", spelled_v4(&tc, v4), port_and_path(&tc));
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert_eq!(public(&url).is_ok(), !local_v4(v4), "{url}");
        if let Err(why) = public(&url) {
            assert!(why.ends_with("is on this computer or its local network"), "{why}");
        }
        assert_eq!(this_computer(&parsed), v4[0] == 127, "{url}");
        let scheme = tc.draw(gs::sampled_from(vec!["http", "wss", "ftp"]));
        let other = url.replacen("https", scheme, 1);
        assert!(public(&other).unwrap_err().contains("isn't an https address"), "{other}");
    }

    /// The same for IPv6: the loopback and unspecified addresses, unique local (fc00::/7) and
    /// link-local (fe80::/10) ones, and IPv4 ones mapped into IPv6 that are local as IPv4.
    #[hegel::test(test_cases = 400)]
    fn ipv6_addresses_are_public_unless_theyre_local(tc: TestCase) {
        let low = tc.draw(gs::integers::<u128>().max_value(u128::from(u32::MAX)));
        let v6: u128 = match tc.draw(gs::integers::<u8>().max_value(6)) {
            0 => tc.draw(gs::sampled_from(vec![0u128, 1])),
            1 => (0xfc00 | u128::from(tc.draw(gs::integers::<u16>().max_value(0x1ff)))) << 112 | low,
            2 => (0xfe80 | u128::from(tc.draw(gs::integers::<u16>().max_value(0x3f)))) << 112 | low,
            3 => 0xffff << 32 | u128::from(u32::from_be_bytes(ipv4(&tc))),
            4 => 0x2606_4700 << 96 | low,
            _ => tc.draw(gs::integers::<u128>()),
        };
        let addr = std::net::Ipv6Addr::from(v6);
        let segment = (v6 >> 112) as u16;
        let mapped = addr.to_ipv4_mapped().map(|v4| v4.octets());
        let local = v6 <= 1 || segment & 0xfe00 == 0xfc00 || segment & 0xffc0 == 0xfe80 || mapped.is_some_and(local_v4);
        let url = format!("https://[{addr}]{}", port_and_path(&tc));
        assert_eq!(public(&url).is_ok(), !local, "{url}");
        let loopback = v6 == 1 || mapped.is_some_and(|v4| v4[0] == 127);
        assert_eq!(this_computer(&reqwest::Url::parse(&url).unwrap()), loopback, "{url}");
    }

    /// A name is public unless it's `localhost` or below it (in any case, with or without the
    /// final dot), and then it names this computer. Whatever's refused is said on one line.
    #[hegel::test(test_cases = 300)]
    fn names_are_public_unless_theyre_localhost(tc: TestCase) {
        let label = || gs::from_regex("[a-zA-Z0-9]([a-zA-Z0-9-]{0,8}[a-zA-Z0-9])?").fullmatch(true);
        let labels: Vec<String> = tc.draw(gs::vecs(label()).max_size(3));
        let last = tc.draw(hegel::one_of!(
            gs::from_regex("[a-zA-Z]{2,6}").fullmatch(true),
            gs::sampled_from(vec!["localhost".to_string(), "LocalHost".into()])
        ));
        let dot = tc.draw(gs::sampled_from(vec!["", "."]));
        let host = labels.iter().chain([&last]).cloned().collect::<Vec<_>>().join(".");
        let url = format!("https://{host}{dot}{}", port_and_path(&tc));
        let is_localhost = last.eq_ignore_ascii_case("localhost");
        assert_eq!(public(&url).is_ok(), !is_localhost, "{url}");
        assert_eq!(this_computer(&reqwest::Url::parse(&url).unwrap()), is_localhost, "{url}");

        let anything = tc.draw(gs::text().max_size(20));
        for url in [anything.clone(), format!("https://{anything}/")] {
            if let Err(why) = public(&url) {
                assert!(!why.chars().any(char::is_control), "{why:?}");
            }
        }
    }

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

    /// An error and its causes, outermost first.
    #[derive(Debug)]
    struct Caused(String, Option<Box<Caused>>);

    impl std::fmt::Display for Caused {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl std::error::Error for Caused {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1.as_deref().map(|c| c as _)
        }
    }

    /// An error is said with each of its causes after it, in order, leaving out any already
    /// said.
    #[hegel::test(test_cases = 300)]
    fn errors_say_each_cause_once(tc: TestCase) {
        let words = gs::sampled_from(vec!["storage error", "disk full", "storage error: disk full", "timed out", "x"]);
        let said: Vec<String> =
            tc.draw(gs::vecs(hegel::one_of!(words.map(String::from), gs::text().max_size(6))).min_size(1).max_size(5));
        let chain = said.iter().rev().fold(None, |cause, s| Some(Box::new(Caused(s.clone(), cause)))).unwrap();
        let mut expected = said[0].clone();
        for s in &said[1..] {
            if !expected.contains(s.as_str()) {
                expected = format!("{expected}: {s}");
            }
        }
        assert_eq!(with_causes(chain.as_ref()), expected);
        assert_eq!(with_causes(&Caused(said[0].clone(), None)), said[0], "no causes");
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
}
