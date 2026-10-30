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

use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// An IPv4 address, often from the ranges `public` treats specially.
#[hegel::test_helper]
fn ipv4(tc: &TestCase) -> [u8; 4] {
    let first = tc.draw(hegel::one_of!(
        gs::sampled_from(vec![0u8, 1, 8, 10, 100, 127, 169, 172, 192, 224, 255]),
        gs::integers::<u8>()
    ));
    let second = tc.draw(hegel::one_of!(gs::sampled_from(vec![0u8, 15, 16, 31, 32, 168, 254]), gs::integers::<u8>()));
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

/// A body that's endless zeros, saying when it's read and when it's let go of.
struct Endless {
    said: std::sync::mpsc::Sender<&'static str>,
}

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let _ = self.said.send("read");
        buf.fill(0);
        Ok(buf.len())
    }
}

impl Drop for Endless {
    fn drop(&mut self) {
        let _ = self.said.send("dropped");
    }
}

/// A request sending to a server that never reads stops once it's cancelled, and lets go of its
/// body (a file it sends), though it has filled what the connection holds by then: hyper alone
/// would hold it until it could send the rest.
#[test]
fn a_request_cancelled_lets_go_of_its_body_at_once() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    // Taken and held, never read.
    let held = std::thread::spawn(move || listener.accept().unwrap());
    let (tx, said) = std::sync::mpsc::channel();
    let request = reqwest::Client::new().put(url).body(streamed(Endless { said: tx }));
    let cancel = CancelToken::new();
    let cancelling = cancel.clone();
    // Its connection goes with the runtime, once the request has given up.
    let sent = std::thread::spawn(move || Requests::new().unwrap().run(|| request.send(), &cancelling).is_none());
    assert_eq!(said.recv_timeout(Duration::from_secs(10)), Ok("read"));
    cancel.cancel();
    // Generous: it notices in 50 ms.
    let bound = Duration::from_secs(5);
    let dropped = std::iter::from_fn(|| said.recv_timeout(bound).ok()).find(|s| *s == "dropped");
    assert_eq!(dropped, Some("dropped"), "the body is still held");
    assert!(sent.join().unwrap(), "cancelled");
    drop(held);
}
