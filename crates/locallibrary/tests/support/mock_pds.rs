//! A pretend Atmosphere data server (PDS) on 127.0.0.1, for tests that publish without the
//! network: it keeps records in memory, answers `com.atproto.repo.getRecord` and `putRecord`
//! (`swapRecord: null` meaning there must be no record yet), `com.atproto.server.getSession` and
//! token revocation the way a PDS does, and records every request it gets.
//!
//! Writes must be DPoP-bound: an `Authorization: DPoP <token>` header and a `DPoP` proof, which
//! must carry the nonce the server hands out (the first write is told to use one, as servers
//! do). The proof's signature isn't checked; its key and claims are, by the tests.
//!
//! Shared by the library's and the CLI's tests (`#[path]`), so it only uses what both have.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// A request the server got.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// Without the query.
    pub path: String,
    pub query: BTreeMap<String, String>,
    /// Lowercase names.
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_default()
    }

    /// The DPoP proof's header and claims (not checking its signature).
    pub fn dpop(&self) -> Option<(Value, Value)> {
        let jwt = self.header("dpop")?;
        let mut parts = jwt.split('.');
        let (header, claims) = (parts.next()?, parts.next()?);
        Some((decode_json(header)?, decode_json(claims)?))
    }
}

/// How the server answers writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Writes {
    /// Like a PDS: stores the record.
    Accept,
    /// The token doesn't allow it (403, as for a missing scope).
    RefuseScope,
    /// The token is no good any more (401 `invalid_token`).
    RefuseToken,
    /// The record isn't acceptable (400 InvalidRequest).
    RefuseRecord,
}

#[derive(Debug)]
struct State {
    /// `(repo, collection, rkey)` → (record, CID).
    records: BTreeMap<(String, String, String), (Value, String)>,
    writes: Writes,
    nonce: String,
    /// The account `getSession` answers for: `(did, handle)`.
    account: Option<(String, String)>,
    /// What revoking a token answers.
    revocation: u16,
    requests: Vec<Request>,
}

pub struct MockPds {
    pub url: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    port: u16,
}

impl MockPds {
    pub fn start() -> MockPds {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");
        let state = Arc::new(Mutex::new(State {
            records: BTreeMap::new(),
            writes: Writes::Accept,
            nonce: "nonce-1".into(),
            account: None,
            revocation: 200,
            requests: Vec::new(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st, base) = (state.clone(), stop.clone(), url.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if st.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(conn) = conn else { continue };
                let (s, base) = (s.clone(), base.clone());
                std::thread::spawn(move || serve(conn, &s, &base));
            }
        });
        MockPds { url, state, stop, port }
    }

    /// Every request so far, oldest first.
    pub fn requests(&self) -> Vec<Request> {
        self.state.lock().unwrap().requests.clone()
    }

    /// The requests to one XRPC method.
    pub fn calls(&self, nsid: &str) -> Vec<Request> {
        let path = format!("/xrpc/{nsid}");
        self.requests().into_iter().filter(|r| r.path == path).collect()
    }

    pub fn set_writes(&self, w: Writes) {
        self.state.lock().unwrap().writes = w;
    }

    /// The account a session here is for, as `getSession` says (until then it refuses).
    pub fn set_account(&self, did: &str, handle: &str) {
        self.state.lock().unwrap().account = Some((did.into(), handle.into()));
    }

    /// The status revoking a token answers (200 until set, as RFC 7009 says).
    pub fn set_revocation(&self, status: u16) {
        self.state.lock().unwrap().revocation = status;
    }

    /// What's stored, and its CID.
    pub fn record(&self, repo: &str, collection: &str, rkey: &str) -> Option<(Value, String)> {
        self.state.lock().unwrap().records.get(&(repo.into(), collection.into(), rkey.into())).cloned()
    }

    /// Store a record as if another app had written it.
    pub fn put(&self, repo: &str, collection: &str, rkey: &str, record: Value) -> String {
        let cid = cid(&record);
        self.state.lock().unwrap().records.insert((repo.into(), collection.into(), rkey.into()), (record, cid.clone()));
        cid
    }
}

impl Drop for MockPds {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn decode_json(part: &str) -> Option<Value> {
    serde_json::from_slice(&base64url_decode(part)?).ok()
}

/// base64url without padding (what JWTs use).
pub fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// base64url without padding.
pub fn base64url(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// A stand-in content hash, shaped like a real one (a CIDv1, dag-cbor, sha-256 in base32), and
/// different for different records.
fn cid(record: &Value) -> String {
    let mut bytes = vec![0x01, 0x71, 0x12, 0x20];
    let mut h: u64 = 0xcbf29ce484222325;
    for round in 0..4u8 {
        for b in record.to_string().bytes().chain([round]) {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        bytes.extend(h.to_be_bytes());
    }
    const ABC: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::from("b");
    let (mut buf, mut bits) = (0u64, 0);
    for b in bytes {
        buf = (buf << 8) | b as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ABC[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ABC[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn serve(mut conn: TcpStream, state: &Mutex<State>, base: &str) {
    let Some(req) = read_request(&mut conn) else { return };
    let (status, headers, body) = answer(&req, state, base);
    state.lock().unwrap().requests.push(req);
    let body = body.to_string();
    let mut head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = conn.write_all(head.as_bytes());
    let _ = conn.write_all(body.as_bytes());
}

fn read_request(conn: &mut TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(conn.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next()?.to_string(), parts.next()?.to_string());
    let mut headers = BTreeMap::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':')?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    let len: usize = headers.get("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
    let mut body = vec![0; len];
    reader.read_exact(&mut body).ok()?;
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let query =
        query.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), percent_decode(v))).collect();
    Some(Request { method, path: path.to_string(), query, headers, body })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Ok(b) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..(i + 3).min(bytes.len())]).unwrap_or(""), 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

type Answer = (u16, Vec<(&'static str, String)>, Value);

fn answer(req: &Request, state: &Mutex<State>, base: &str) -> Answer {
    let mut s = state.lock().unwrap();
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/xrpc/com.atproto.repo.getRecord") => {
            let q = |k: &str| req.query.get(k).cloned().unwrap_or_default();
            let (repo, collection, rkey) = (q("repo"), q("collection"), q("rkey"));
            match s.records.get(&(repo.clone(), collection.clone(), rkey.clone())) {
                Some((value, cid)) => (
                    200,
                    vec![],
                    json!({ "uri": format!("at://{repo}/{collection}/{rkey}"), "cid": cid, "value": value }),
                ),
                None => (400, vec![], json!({ "error": "RecordNotFound", "message": "Could not locate record" })),
            }
        }
        ("POST", "/xrpc/com.atproto.repo.putRecord") => {
            let auth = req.header("authorization").unwrap_or_default();
            if !auth.starts_with("DPoP ") || req.dpop().is_none() {
                return (401, vec![], json!({ "error": "AuthRequired", "message": "a DPoP-bound token is required" }));
            }
            let (_, claims) = req.dpop().unwrap();
            if claims["nonce"] != s.nonce.as_str() {
                return (
                    401,
                    vec![
                        ("WWW-Authenticate", r#"DPoP error="use_dpop_nonce""#.into()),
                        ("DPoP-Nonce", s.nonce.clone()),
                    ],
                    json!({ "error": "use_dpop_nonce" }),
                );
            }
            match s.writes {
                Writes::Accept => {}
                Writes::RefuseScope => {
                    return (
                        403,
                        vec![],
                        json!({ "error": "ScopeMissingError", "message": "Missing required scope \"repo:...\"" }),
                    );
                }
                Writes::RefuseToken => {
                    return (
                        401,
                        vec![("WWW-Authenticate", r#"DPoP error="invalid_token", error_description="revoked""#.into())],
                        json!({ "error": "InvalidToken" }),
                    );
                }
                Writes::RefuseRecord => {
                    return (400, vec![], json!({ "error": "InvalidRequest", "message": "Record is too big" }));
                }
            }
            let body = req.json();
            let key = |k: &str| body[k].as_str().unwrap_or_default().to_string();
            let id = (key("repo"), key("collection"), key("rkey"));
            // A CID: what's there must be that record. `null`: there must be nothing there.
            let swap = body.get("swapRecord").map(|s| s.as_str());
            if let Some(swap) = swap
                && s.records.get(&id).map(|r| r.1.as_str()) != swap
            {
                return (400, vec![], json!({ "error": "InvalidSwap", "message": "Record was at another CID" }));
            }
            let record_cid = cid(&body["record"]);
            s.records.insert(id.clone(), (body["record"].clone(), record_cid.clone()));
            (
                200,
                vec![],
                json!({
                    "uri": format!("at://{}/{}/{}", id.0, id.1, id.2),
                    "cid": record_cid,
                    "commit": { "cid": cid(&json!({ "commit": id.2 })), "rev": "3l3qo2vutsw2b" },
                    "validationStatus": "unknown",
                }),
            )
        }
        ("GET", "/.well-known/oauth-authorization-server") => (
            200,
            vec![],
            json!({
                "issuer": base,
                "authorization_endpoint": format!("{base}/oauth/authorize"),
                "token_endpoint": format!("{base}/oauth/token"),
                "pushed_authorization_request_endpoint": format!("{base}/oauth/par"),
                "revocation_endpoint": format!("{base}/oauth/revoke"),
                "scopes_supported": ["atproto", "transition:generic"],
                "response_types_supported": ["code"],
                "grant_types_supported": ["authorization_code", "refresh_token"],
                "code_challenge_methods_supported": ["S256"],
                "token_endpoint_auth_methods_supported": ["none", "private_key_jwt"],
                "dpop_signing_alg_values_supported": ["ES256"],
                "require_pushed_authorization_requests": true,
                "authorization_response_iss_parameter_supported": true,
                "client_id_metadata_document_supported": true,
            }),
        ),
        ("GET", "/xrpc/com.atproto.server.getSession") => {
            let signed = req.header("authorization").is_some_and(|a| a.starts_with("DPoP ")) && req.dpop().is_some();
            match &s.account {
                Some((did, handle)) if signed && s.writes != Writes::RefuseToken => {
                    (200, vec![], json!({ "did": did, "handle": handle }))
                }
                _ => (
                    401,
                    vec![("WWW-Authenticate", r#"DPoP error="invalid_token", error_description="revoked""#.into())],
                    json!({ "error": "InvalidToken", "message": "Token has been revoked" }),
                ),
            }
        }
        ("POST", "/oauth/revoke") => (s.revocation, vec![], json!({})),
        ("POST", "/oauth/token") => {
            (400, vec![], json!({ "error": "invalid_grant", "error_description": "refresh token expired" }))
        }
        _ => (404, vec![], json!({ "error": "NotFound", "message": format!("no {} {}", req.method, req.path) })),
    }
}
