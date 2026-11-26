//! A mock OAuth service on 127.0.0.1, for tests that sign in to accounts without the network:
//! [`Server`] answers each request as a route says and keeps what it was asked, and [`service`]
//! plays an account's service (tokens, revocation, who signed in), handing out tokens `at-<n>` and
//! `rt-<n>`, and [`account_plugin`] is the manifest of an account plugin signing in there. The
//! browser isn't played: a test comes back to the sign-in itself.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use oauth2::{PkceCodeChallenge, PkceCodeVerifier};
use serde_json::{Value, json};

/// A request the server got.
#[derive(Debug, Clone, Default)]
pub struct Got {
    pub method: String,
    /// With its query.
    pub target: String,
    /// By name, in lowercase.
    pub headers: IndexMap<String, String>,
    pub body: String,
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

pub fn ok(body: impl Into<Vec<u8>>) -> Reply {
    Reply { status: 200, headers: Vec::new(), body: body.into() }
}

/// A listener on this computer, whose connections `serve` takes one at a time on a thread of its
/// own (with whether it has stopped), until it answers false. Dropped, it stops: it's closed, and a
/// panic of its thread fails the test that dropped it.
pub struct Listening {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Listening {
    pub fn new(mut serve: impl FnMut(TcpStream, &AtomicBool) -> bool + Send + 'static) -> Listening {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                let stream = match stream {
                    // Windows says so of a client that has gone by now.
                    Err(e) if matches!(e.kind(), ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset) => continue,
                    stream => stream.unwrap(),
                };
                if !serve(stream, &stopped) {
                    break;
                }
            }
        });
        Listening { addr, stop, thread: Some(thread) }
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let Some(thread) = self.thread.take() else { return };
        // Wakes the thread waiting for a connection, to see it's stopped. Once it has ended, nothing
        // listens, which Windows takes 2 s to say. A process out of files can't connect until it has
        // closed some.
        let woken = (0..100).any(|_| {
            thread.is_finished() || TcpStream::connect(self.addr).is_ok() || {
                std::thread::sleep(Duration::from_millis(50));
                false
            }
        });
        if !woken {
            assert!(std::thread::panicking(), "can't connect to {} to stop it", self.addr);
            return;
        }
        if let Err(panic) = thread.join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(panic);
        }
    }
}

/// A server on this computer answering each request as `route` says (given its own address and
/// the request), keeping what it was asked. Dropped, it stops, as [`Listening`] does: a route's
/// assertion fails the test that dropped it.
pub struct Server {
    /// `http://127.0.0.1:<port>`.
    pub url: String,
    got: Arc<Mutex<Vec<Got>>>,
    listening: Listening,
}

impl Server {
    pub fn new(route: impl Fn(&str, &Got) -> Reply + Send + 'static) -> Server {
        let got: Arc<Mutex<Vec<Got>>> = Arc::default();
        let log = got.clone();
        let listening = Listening::new(move |stream, stopped| {
            // A client that hangs up before its request is whole, or hasn't sent it all when the
            // server is dropped, is answered nothing.
            let Ok(mut connection) = Connection::new(&stream, stopped) else { return true };
            let Ok(Some(got)) = read(&mut connection) else { return true };
            log.lock().unwrap().push(got.clone());
            let base = format!("http://{}", stream.local_addr().unwrap());
            let Reply { status, headers, body } = route(&base, &got);
            // A reply giving its own length keeps it: one longer than its body is cut short.
            let sized = headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
            let length = if sized { String::new() } else { format!("Content-Length: {}\r\n", body.len()) };
            let mut head = format!("HTTP/1.1 {status} X\r\n{length}Connection: close\r\n");
            for (name, value) in headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            let _ = connection.write_all(format!("{head}\r\n").as_bytes()).and_then(|()| connection.write_all(&body));
            true
        });
        Server { url: format!("http://{}", listening.addr), got, listening }
    }

    pub fn got(&self) -> Vec<Got> {
        self.got.lock().unwrap().clone()
    }
}

/// A connection, read and written until the server has `stopped`: a client that's slow is waited for,
/// and one that reads nothing for 10 s when it's written to isn't.
pub struct Connection<'a> {
    stream: &'a TcpStream,
    stopped: &'a AtomicBool,
}

impl<'a> Connection<'a> {
    pub fn new(stream: &'a TcpStream, stopped: &'a AtomicBool) -> std::io::Result<Connection<'a>> {
        // How often it's seen whether the server has stopped.
        stream.set_read_timeout(Some(Duration::from_millis(50)))?;
        // Not tried again, as a read is: Windows doesn't say how much of one that timed out it sent.
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        Ok(Connection { stream, stopped })
    }
}

impl Read for Connection<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            match self.stream.read(buf) {
                Err(e)
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
                        && !self.stopped.load(Ordering::SeqCst) => {}
                read => return read,
            }
        }
    }
}

impl Write for Connection<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // One that times out having sent some of it isn't an error, so the next would wait again.
        if self.stopped.load(Ordering::SeqCst) {
            return Err(ErrorKind::TimedOut.into());
        }
        self.stream.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The request `connection` sends, or none when it hangs up before its headers end.
fn read(connection: &mut Connection) -> std::io::Result<Option<Got>> {
    let mut reader = BufReader::new(connection);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if !line.ends_with('\n') {
        return Ok(None);
    }
    let mut words = line.split(' ');
    let (method, target) = (words.next().unwrap().to_string(), words.next().unwrap().to_string());
    let mut headers = IndexMap::new();
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        if !line.ends_with('\n') {
            return Ok(None);
        }
        let Some((name, value)) = line.trim_end().split_once(": ") else { break };
        headers.insert(name.to_lowercase(), value.to_string());
    }
    let length = headers.get("content-length").map_or(0, |n| n.parse().unwrap());
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(Got { method, target, headers, body: String::from_utf8_lossy(&body).into() }))
}

/// What the mock service hands out: access tokens `at-<n>` and refresh tokens `rt-<n>`, each
/// valid `expires_in` seconds, and the code challenge the browser was sent with.
#[derive(Default)]
pub struct Service {
    pub expires_in: Option<u64>,
    /// How many seconds a refresh token it answers lasts, which it says when it's given.
    pub refresh_expires_in: Option<u64>,
    /// Whether a refresh answers a new refresh token.
    pub rotates: bool,
    /// It never answers a refresh token, so what's signed in to there isn't refreshed.
    pub refreshless: bool,
    /// An error the token endpoint answers instead (`invalid_grant`), with its description.
    pub refuses: Option<(String, String)>,
    /// The token endpoint answers what can't be read as tokens instead.
    pub garbles: bool,
    pub challenge: String,
    pub issued: usize,
    /// Who the browser signs in as next: what `/userinfo` answers the tokens its code trades for.
    pub signing_in_as: Value,
    /// What `/userinfo` answers each access token `at-<n>` with, by `n`: a refresh's are its
    /// refresh token's.
    pub who: IndexMap<usize, Value>,
    /// A refresh token is refused once it has refreshed.
    pub single_use: bool,
    /// The refresh tokens that have refreshed.
    pub spent: Vec<String>,
    /// The token endpoint doesn't say what it granted.
    pub unscoped: bool,
    /// What the token endpoint says it granted, when it isn't `read write`.
    pub grants: Option<Vec<String>>,
    /// What it answers the requests it doesn't serve itself, in turn, before answering what
    /// they were sent.
    pub script: VecDeque<Reply>,
}

/// The form a request sent, by name.
pub fn form(got: &Got) -> IndexMap<String, String> {
    form_urlencoded::parse(got.body.as_bytes()).into_owned().collect()
}

/// What a token request sent, by name: a form to a token endpoint, a JSON object to Local
/// Network's token exchange.
pub fn fields(got: &Got) -> IndexMap<String, String> {
    match got.target.starts_with(EXCHANGE) {
        true => serde_json::from_str(&got.body).unwrap_or_default(),
        false => form(got),
    }
}

/// Where the mock service plays Local Network's token exchange for itself.
pub const EXCHANGE: &str = "/api/token-exchange/mock";

pub fn json_reply(status: u16, body: Value) -> Reply {
    Reply { status, headers: vec![("Content-Type", "application/json".into())], body: body.to_string().into_bytes() }
}

/// The `n` of the access token `at-<n>` a request was sent, in its `Authorization` header (after
/// `Bearer`) or its query (as `access_token`).
pub fn sent_at(got: &Got) -> Option<usize> {
    let query = got.target.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let mut query = form_urlencoded::parse(query.as_bytes());
    let token = got.headers.get("authorization").and_then(|a| a.strip_prefix("Bearer ")).map(String::from);
    let token = token.or_else(|| query.find(|(k, _)| k == "access_token").map(|(_, v)| v.into_owned()));
    token?.strip_prefix("at-")?.parse().ok()
}

/// Whether `text` holds a token `at-<n>` or `rt-<n>`, one of the first `issued` the service handed
/// out. Whole tokens only: `at-1` also turns up inside the random state and PKCE challenge of a
/// sign-in link, which are base64url.
pub fn holds_token(text: &str, issued: usize) -> bool {
    let in_a_word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_');
    let whole = |token: String| {
        text.match_indices(&token)
            .any(|(at, _)| !text[..at].ends_with(in_a_word) && !text[at + token.len()..].starts_with(in_a_word))
    };
    (1..=issued).any(|n| whole(format!("at-{n}")) || whole(format!("rt-{n}")))
}

/// A mock OAuth service: `/token` trades a code (checked against the PKCE challenge) or a refresh
/// token for tokens, and so does Local Network's token exchange for it, taking the same as JSON;
/// `/revoke` takes one back; `/userinfo` says who an access token is for, and
/// `/userinfo/moved` sends there; `/home` sends to `/here`, and `/r/go/<path>` to `/<path>`; and
/// anything else answers its [`Service::script`]'s next reply, else what it was sent.
pub fn service(state: Arc<Mutex<Service>>) -> Server {
    Server::new(move |base, got| {
        let mut s = state.lock().unwrap();
        let path = got.target.split('?').next().unwrap_or_default();
        match got.target.as_str() {
            "/token" | EXCHANGE => {
                let json = got.headers.get("content-type").is_some_and(|t| t == "application/json");
                if json != (got.target == EXCHANGE) {
                    return json_reply(415, json!({ "error": "invalid_request" }));
                }
                let f = fields(got);
                if let Some((error, description)) = &s.refuses {
                    return json_reply(400, json!({ "error": error, "error_description": description }));
                }
                if s.garbles {
                    return json_reply(200, json!({ "token_type": "bearer" }));
                }
                let pkce_holds = |v: &String| {
                    PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(v.clone())).as_str()
                        == s.challenge
                };
                let fits = match f.get("grant_type").map(String::as_str) {
                    Some("authorization_code") => f.get("code_verifier").is_some_and(pkce_holds),
                    Some("refresh_token") => f.contains_key("refresh_token"),
                    _ => false,
                };
                if !fits || f.get("client_id").map(String::as_str) != Some("locallibrary-test") {
                    return json_reply(400, json!({ "error": "invalid_request" }));
                }
                let refresh = f.get("refresh_token").cloned();
                if let Some(refresh) = &refresh {
                    if s.single_use && s.spent.contains(refresh) {
                        return json_reply(400, json!({ "error": "invalid_grant" }));
                    }
                    s.spent.push(refresh.clone());
                }
                s.issued += 1;
                let n = s.issued;
                let refreshed = refresh.and_then(|r| r.strip_prefix("rt-")?.parse::<usize>().ok());
                let who = match refreshed {
                    Some(m) => s.who.get(&m).cloned().unwrap_or_default(),
                    None => s.signing_in_as.clone(),
                };
                s.who.insert(n, who);
                let granted = s.grants.as_ref().map_or("read write".into(), |g| g.join(" "));
                let mut answer = json!({ "access_token": format!("at-{n}"), "token_type": "bearer", "scope": granted });
                if s.unscoped {
                    answer.as_object_mut().unwrap().remove("scope");
                }
                if !s.refreshless && (f["grant_type"] == "authorization_code" || s.rotates) {
                    answer["refresh_token"] = format!("rt-{n}").into();
                    if let Some(secs) = s.refresh_expires_in {
                        answer["refresh_token_expires_in"] = secs.into();
                    }
                }
                if let Some(secs) = s.expires_in {
                    answer["expires_in"] = secs.into();
                }
                json_reply(200, answer)
            }
            "/revoke" => ok(""),
            _ if path == "/userinfo/moved" => {
                Reply { status: 307, headers: vec![("Location", format!("{base}/userinfo"))], body: Vec::new() }
            }
            _ if path == "/userinfo" => match sent_at(got).and_then(|n| s.who.get(&n)) {
                Some(who) => json_reply(200, who.clone()),
                None => json_reply(401, json!({ "error": "invalid_token" })),
            },
            // Elsewhere in its own origin.
            "/home" => Reply { status: 307, headers: vec![("Location", format!("{base}/here"))], body: Vec::new() },
            _ if path.starts_with("/r/go/") => {
                Reply { status: 307, headers: vec![("Location", format!("{base}/{}", &path[6..]))], body: Vec::new() }
            }
            _ => s.script.pop_front().unwrap_or_else(|| {
                ok(format!("{} {}", got.target, got.headers.get("authorization").map_or("-", String::as_str)))
            }),
        }
    })
}

/// The account plugin's id.
pub const WITH: &str = "mock-accounts";

/// An account plugin providing the `mock` account type at `server`, asking `whoami` who signed
/// in, with the type's keys `keys` besides: where its token goes (`token = { query =
/// "access_token" }`; `Bearer` in the `Authorization` header when they don't say), or the port the
/// browser comes back to. Its scopes: `read` covers GETs under `/r/`, `write` POSTs under `/w/`,
/// and `peek`, which asks the service for nothing, GETs under `/p/`.
pub fn account_plugin(server: &str, whoami: &str, keys: &str) -> String {
    format!(
        r#"id = "{WITH}"
name = "Mock accounts"
version = "0.1.0"
protocol = "^0.2"

[[contributes.account_types]]
type = "mock"
title = "Mock"
authorize_url = "{server}/authorize"
token_url = "{server}/token"
client_id = "locallibrary-test"
revoke_url = "{server}/revoke"
always = ["openid"]
whoami = {{ url = "{whoami}", subject = "/sub", name = "/email" }}
one_account = {{ login_hint = "{{subject}}" }}
another_account = {{ prompt = "select_account" }}
{keys}
[contributes.account_types.scopes.read]
title = "read"
asks = ["read"]
requests = [{{ method = "GET", url = "{server}/r/*" }}]

[contributes.account_types.scopes.write]
title = "write"
asks = ["write", "read"]
requests = [{{ method = "POST", url = "{server}/w/*" }}]

[contributes.account_types.scopes.peek]
title = "peek"
requests = [{{ method = "GET", url = "{server}/p/*" }}]
"#
    )
}

/// An account plugin providing the `mock` account type at `server`, signed in to with an `access`
/// and a `secret` key going in the `Authorization` header as `LOW <access>:<secret>`, asking
/// `whoami` (an address) who they're for when it's given. Its scopes: `read` covers GETs under
/// `/r/`, and `write` POSTs under `/w/`.
pub fn keys_account_plugin(server: &str, whoami: Option<&str>) -> String {
    let whoami = whoami.map(|w| format!("whoami = {{ url = \"{w}\", subject = \"/id\", name = \"/name\" }}\n"));
    format!(
        r#"id = "{WITH}"
name = "Mock accounts"
version = "0.1.0"
protocol = "^0.2"

[[contributes.account_types]]
type = "mock"
title = "Mock"
kind = "keys"
keys_url = "{server}/keys"
authorization = "LOW {{access}}:{{secret}}"
{whoami}
[contributes.account_types.keys.access]
title = "Access key"

[contributes.account_types.keys.secret]
title = "Secret key"

[contributes.account_types.scopes.read]
title = "read"
requests = [{{ method = "GET", url = "{server}/r/*" }}]

[contributes.account_types.scopes.write]
title = "write"
requests = [{{ method = "POST", url = "{server}/w/*" }}]
"#,
        whoami = whoami.unwrap_or_default()
    )
}

/// Starts a server with `start` and drops it, over and over: it closes its listener each time, so a
/// property starting one in each of its cases, shrinking for long, doesn't run the process out of
/// files. Each is used as `use_it` does, then dropped, after which nothing answers at the address
/// `start` gave with it.
pub fn stops_when_dropped<T>(start: impl Fn() -> (T, SocketAddr), use_it: impl Fn(&T, SocketAddr)) {
    // The files this process has open: nextest runs one test in it, where `cargo test` runs others
    // beside it.
    #[cfg(unix)]
    let open = || std::env::var_os("NEXTEST").map(|_| std::fs::read_dir("/dev/fd").unwrap().count());
    #[cfg(unix)]
    let before = open();
    let dropped = (0..20).map(|_| {
        let (server, addr) = start();
        use_it(&server, addr);
        let dropping = Instant::now();
        drop(server);
        // Generous: a server sees it's stopped in 50 ms.
        assert!(dropping.elapsed() < Duration::from_secs(10), "the drop was held up");
        addr
    });
    // The last one's alone: Windows takes 2 s to say nothing listens.
    let addr = dropped.last().unwrap();
    #[cfg(unix)]
    assert_eq!(open(), before, "a dropped server left files open");
    let e = TcpStream::connect(addr).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::ConnectionRefused, "{addr} still answers");
}

#[test]
fn a_dropped_server_stops_listening() {
    let start = || {
        let server = Server::new(|_, _| ok("hi"));
        let addr = server.listening.addr;
        (server, addr)
    };
    stops_when_dropped(start, |_, addr| {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(b"GET /x HTTP/1.1\r\n\r\n").unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        assert!(answer.ends_with("\r\n\r\nhi"), "{answer}");
    });
}

/// A client that sends part of its request and then nothing more, keeping the connection open,
/// doesn't keep its server from stopping.
#[test]
fn a_half_sent_request_doesnt_hold_up_the_drop() {
    let (told, said) = std::sync::mpsc::channel();
    let listening = Listening::new(move |stream, stopped| {
        told.send(None).unwrap();
        let got = Connection::new(&stream, stopped).and_then(|mut connection| read(&mut connection));
        told.send(Some(matches!(got, Ok(Some(_))))).unwrap();
        true
    });
    let mut stream = TcpStream::connect(listening.addr).unwrap();
    stream.write_all(b"POST /x HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc").unwrap();
    assert_eq!(said.recv_timeout(Duration::from_secs(20)), Ok(None), "it isn't read");
    drops_in_time(listening);
    assert_eq!(said.recv(), Ok(Some(false)), "a half-sent request was taken");
}

/// A client that doesn't read a reply too long to send at once doesn't keep its server from stopping.
#[test]
fn a_reply_nobody_reads_doesnt_hold_up_the_drop() {
    let server = Server::new(|_, _| ok(vec![0; 64 << 20]));
    let mut stream = TcpStream::connect(server.listening.addr).unwrap();
    stream.write_all(b"GET /x HTTP/1.1\r\n\r\n").unwrap();
    let asked = Instant::now();
    while server.got().is_empty() {
        assert!(asked.elapsed() < Duration::from_secs(20), "it isn't asked");
        std::thread::sleep(Duration::from_millis(10));
    }
    drops_in_time(server);
}

/// A client that hangs up in the middle of its request line or headers asked nothing.
#[test]
fn a_request_cut_short_in_its_head_isnt_one() {
    let server = Server::new(|_, _| ok("hi"));
    for sent in
        ["", "GET", "GET /x HTTP/1.1", "GET /x HTTP/1.1\r\n", "GET /x HTTP/1.1\r\nA: b", "GET /x HTTP/1.1\r\nA: b\r\n"]
    {
        let mut stream = TcpStream::connect(server.listening.addr).unwrap();
        stream.write_all(sent.as_bytes()).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
        assert!(answer.is_empty(), "{sent:?} was answered");
    }
    assert!(server.got().is_empty(), "{:?}", server.got());
}

/// Drops `server`, on a thread of its own: a drop that waited forever fails the test instead.
fn drops_in_time(server: impl Send + 'static) {
    let (dropped, done) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(server);
        let _ = dropped.send(());
    });
    // Generous: a reply nobody reads is given up on after 10 s of it being stuck, 15 s in all on macOS.
    assert!(done.recv_timeout(Duration::from_secs(30)).is_ok(), "dropping the server hung");
}

/// A route that panics fails the test once its server is dropped, saying why.
#[test]
#[should_panic(expected = "the route's own assertion")]
fn a_server_s_panic_fails_the_test_that_drops_it() {
    let server = Server::new(|_, _| panic!("the route's own assertion"));
    let mut stream = TcpStream::connect(server.url.strip_prefix("http://").unwrap()).unwrap();
    stream.write_all(b"GET /x HTTP/1.1\r\n\r\n").unwrap();
    let _ = stream.read_to_string(&mut String::new());
}
