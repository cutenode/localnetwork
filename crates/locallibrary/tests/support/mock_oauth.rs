//! A mock OAuth service on 127.0.0.1, for tests that sign in to accounts without the network:
//! [`Server`] answers each request as a route says and keeps what it was asked, and [`service`]
//! plays an account's service (tokens, revocation, who signed in), handing out tokens `at-<n>` and
//! `rt-<n>`, and [`account_plugin`] is the manifest of an account plugin signing in there. The
//! browser isn't played: a test comes back to the sign-in itself.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

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

/// A server on this computer answering each request as `route` says (given its own address and
/// the request), keeping what it was asked.
pub struct Server {
    /// `http://127.0.0.1:<port>`.
    pub url: String,
    got: Arc<Mutex<Vec<Got>>>,
}

impl Server {
    pub fn new(route: impl Fn(&str, &Got) -> Reply + Send + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let got: Arc<Mutex<Vec<Got>>> = Arc::default();
        let (base, log) = (url.clone(), got.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut words = line.split(' ');
                let (method, target) = (words.next().unwrap().to_string(), words.next().unwrap().to_string());
                let mut headers = IndexMap::new();
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    let Some((name, value)) = line.trim_end().split_once(": ") else { break };
                    headers.insert(name.to_lowercase(), value.to_string());
                }
                let length = headers.get("content-length").map_or(0, |n| n.parse().unwrap());
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let got = Got { method, target, headers, body: String::from_utf8_lossy(&body).into() };
                log.lock().unwrap().push(got.clone());
                let Reply { status, headers, body } = route(&base, &got);
                let mut head =
                    format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                for (name, value) in headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                let _ = stream.write_all(format!("{head}\r\n").as_bytes()).and_then(|()| stream.write_all(&body));
            }
        });
        Server { url, got }
    }

    pub fn got(&self) -> Vec<Got> {
        self.got.lock().unwrap().clone()
    }
}

/// What the mock service hands out: access tokens `at-<n>` and refresh tokens `rt-<n>`, each
/// valid `expires_in` seconds, and the code challenge the browser was sent with.
#[derive(Default)]
pub struct Service {
    pub expires_in: Option<u64>,
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
