//! A pretend Atmosphere data server (PDS) on 127.0.0.1, for tests that publish without the
//! network: it keeps records in memory, answers `com.atproto.repo.getRecord`, `putRecord`
//! (`swapRecord: null` meaning there must be no record yet), `createRecord` (under a key of its
//! own, numbered from 1) and `uploadBlob` (up to a limit, [`MockPds::set_blob_limit`]),
//! `com.atproto.server.getSession`, `getServiceAuth` and token revocation the way a PDS does, and
//! records every request it gets. [`granted`] is what
//! its sign-in would grant: it doesn't sign anyone in, since that needs a browser.
//!
//! It also stands in for Local Network's directory and website, at the same address: HappyView's
//! `computer.localnetwork.directory.getRecords` answers from an index of its own, which every
//! record written here reaches as the firehose would bring it ([`MockPds::put_unindexed`] makes
//! one it missed, [`MockPds::set_firehose`] holds writes back), and the website's
//! `computer.localnetwork.directory.requestIndex` copies the caller's locallibrary records into
//! it, as HappyView reading the account's repository again would. A `computer.localnetwork.dj.set`
//! is indexed as the directory's record script keeps it: without its `$type`, with fields of its
//! own.
//!
//! Writes and uploads must be DPoP-bound: an `Authorization: DPoP <token>` header and a `DPoP` proof, which
//! must carry the nonce the server hands out (the first write is told to use one, as servers
//! do). The proof's signature isn't checked; its key and claims are, by the tests.
//!
//! Shared by the library's and the CLI's tests (`#[path]`), so it only uses what both have.

#![allow(dead_code)]

use indexmap::IndexMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// A request the server got.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// Without the query.
    pub path: String,
    pub query: IndexMap<String, String>,
    /// Every query parameter, repeated ones too, in order.
    pub params: Vec<(String, String)>,
    /// Lowercase names.
    pub headers: IndexMap<String, String>,
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

/// `(repo, collection, rkey)` → (record, CID).
type Records = IndexMap<(String, String, String), (Value, String)>;

/// The record types the website has the directory read again (its `requestIndex`).
const LOCALLIBRARY_COLLECTIONS: [&str; 4] = [
    "computer.localnetwork.locallibrary.settings",
    "computer.localnetwork.dj.profile",
    "computer.localnetwork.dj.set",
    "computer.localnetwork.dj.recording",
];

/// The account `getServiceAuth` signs for until [`MockPds::set_account`] names one: the tests'
/// own (`session::DID`).
const TESTS_ACCOUNT: &str = "did:plc:testdj";

#[derive(Debug)]
struct State {
    records: Records,
    /// What the directory has, as its record scripts keep it.
    indexed: Records,
    /// Whether writes reach the directory as they're made.
    firehose: bool,
    /// What `requestIndex` answers: whether the directory finished reading in time.
    finishes: bool,
    writes: Writes,
    nonce: String,
    /// The account `getSession` answers for: `(did, handle)`.
    account: Option<(String, String)>,
    /// What revoking a token answers.
    revocation: u16,
    /// The most an upload may be, in bytes.
    blob_limit: usize,
    /// The files uploaded, by CID: their type, as the upload said, and their bytes.
    blobs: IndexMap<String, (String, Vec<u8>)>,
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
            records: IndexMap::new(),
            indexed: IndexMap::new(),
            firehose: true,
            finishes: true,
            writes: Writes::Accept,
            nonce: "nonce-1".into(),
            account: None,
            revocation: 200,
            blob_limit: usize::MAX,
            blobs: IndexMap::new(),
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

    /// The DPoP nonce writes must carry from now on: a write with the one before is told to use
    /// this one, as when a server rotates its nonce.
    pub fn set_nonce(&self, nonce: &str) {
        self.state.lock().unwrap().nonce = nonce.into();
    }

    /// The account a session here is for, as `getSession` says (until then it refuses).
    pub fn set_account(&self, did: &str, handle: &str) {
        self.state.lock().unwrap().account = Some((did.into(), handle.into()));
    }

    /// The status revoking a token answers (200 until set, as RFC 7009 says).
    pub fn set_revocation(&self, status: u16) {
        self.state.lock().unwrap().revocation = status;
    }

    /// The most an upload may be from now on, in bytes: a bigger one is refused with a 413, as a
    /// PDS refuses one over its `PDS_BLOB_UPLOAD_LIMIT`.
    pub fn set_blob_limit(&self, bytes: usize) {
        self.state.lock().unwrap().blob_limit = bytes;
    }

    /// The file uploaded with this CID: its type, and its bytes.
    pub fn blob(&self, cid: &str) -> Option<(String, Vec<u8>)> {
        self.state.lock().unwrap().blobs.get(cid).cloned()
    }

    /// What's stored, and its CID.
    pub fn record(&self, repo: &str, collection: &str, rkey: &str) -> Option<(Value, String)> {
        self.state.lock().unwrap().records.get(&(repo.into(), collection.into(), rkey.into())).cloned()
    }

    /// Store a record as if another app had written it.
    pub fn put(&self, repo: &str, collection: &str, rkey: &str, record: Value) -> String {
        let cid = self.put_unindexed(repo, collection, rkey, record);
        let mut s = self.state.lock().unwrap();
        if s.firehose {
            index(&mut s, &(repo.into(), collection.into(), rkey.into()));
        }
        cid
    }

    /// Store a record as if it had been written before the directory kept its record type: the
    /// directory doesn't have it until the account's repository is read again.
    pub fn put_unindexed(&self, repo: &str, collection: &str, rkey: &str, record: Value) -> String {
        let cid = cid(&record);
        self.state.lock().unwrap().records.insert((repo.into(), collection.into(), rkey.into()), (record, cid.clone()));
        cid
    }

    /// What the directory has as `collection/rkey` in `repo`, and its CID.
    pub fn indexed(&self, repo: &str, collection: &str, rkey: &str) -> Option<(Value, String)> {
        self.state.lock().unwrap().indexed.get(&(repo.into(), collection.into(), rkey.into())).cloned()
    }

    /// Whether writes reach the directory as they're made (they do until this says otherwise). Set
    /// back on, the directory catches up with what it missed.
    pub fn set_firehose(&self, on: bool) {
        let mut s = self.state.lock().unwrap();
        s.firehose = on;
        if on {
            let all: Vec<_> = s.records.keys().cloned().collect();
            all.iter().for_each(|id| index(&mut s, id));
        }
    }

    /// Whether the directory finishes reading an account again within `requestIndex` (it does
    /// until this says otherwise; otherwise it answers before reading anything).
    pub fn set_index_finishes(&self, finishes: bool) {
        self.state.lock().unwrap().finishes = finishes;
    }
}

/// Have the directory keep `id`'s record as its record script would, or forget it once it's gone.
fn index(s: &mut State, id: &(String, String, String)) {
    let Some((record, cid)) = s.records.get(id).cloned() else {
        s.indexed.shift_remove(id);
        return;
    };
    let mut kept = record;
    if id.1 == "computer.localnetwork.dj.set"
        && let Some(set) = kept.as_object_mut()
    {
        set.remove("$type");
        let tracks = set.get("tracks").and_then(Value::as_array).map_or(0, Vec::len);
        set.insert("trackCount".into(), tracks.into());
        set.insert("namedTracks".into(), json!([]));
    }
    s.indexed.insert(id.clone(), (kept, cid));
}

impl Drop for MockPds {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// The scopes a sign-in that asks for `requested` is granted, as a PDS's token answer lists them:
/// each `include:<nsid>` replaced by the permissions its permission set (the repo's Lexicon for
/// `nsid`) lists, then the rest. That's `@atproto/oauth-provider`'s `buildTokenScope`
/// (`lexicon/lexicon-manager.js`) with `@atproto/oauth-scopes`'s `IncludeScope`
/// (`scopes/include-scope.js`) for `repo` and `rpc` permissions: one is dropped when a collection
/// or method isn't under the set's NSID group, a `repo` one has a field `repo:` scopes don't, or an
/// `rpc` one names an audience other than `*`; one with several collections is written
/// `repo?collection=…`.
pub fn granted<S: AsRef<str>>(requested: &[S]) -> Vec<String> {
    let (sets, rest): (Vec<&str>, Vec<&str>) =
        requested.iter().map(AsRef::as_ref).partition(|s| s.starts_with("include:"));
    sets.iter().flat_map(|s| permission_set(&s["include:".len()..])).chain(rest.into_iter().map(String::from)).collect()
}

fn permission_set(nsid: &str) -> Vec<String> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().find(|p| p.join("lexicons").is_dir()).unwrap();
    let file = repo.join("lexicons").join(nsid.replace('.', "/")).with_extension("json");
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let doc: Value = serde_json::from_str(&text).unwrap();
    let set = &doc["defs"]["main"];
    assert_eq!((&doc["id"], &set["type"]), (&json!(nsid), &json!("permission-set")), "{}", file.display());
    let group = &nsid[..=nsid.rfind('.').unwrap()];
    let all = ["create", "update", "delete"];
    let under_group =
        |nsids: &[&str]| !nsids.is_empty() && nsids.iter().all(|c| c.starts_with(group) && c.len() > group.len());
    let rpc_scope = |p: &Value| -> Option<String> {
        let mut lxm: Vec<&str> = p["lxm"].as_array()?.iter().map(Value::as_str).collect::<Option<_>>()?;
        if !under_group(&lxm) || p["aud"] != "*" {
            return None;
        }
        lxm.sort_unstable();
        lxm.dedup();
        Some(match lxm[..] {
            [one] => format!("rpc:{one}?aud=*"),
            _ => format!("rpc?{}&aud=*", lxm.iter().map(|m| format!("lxm={m}")).collect::<Vec<_>>().join("&")),
        })
    };
    let permission = |p: &Value| -> Option<String> {
        if p["resource"] == "rpc" {
            return rpc_scope(p);
        }
        assert_eq!(p["resource"], "repo", "the mock only grants repo and rpc permissions");
        if p.as_object()?.keys().any(|k| !["type", "resource", "collection", "action"].contains(&k.as_str())) {
            return None;
        }
        let mut collections: Vec<&str> =
            p["collection"].as_array()?.iter().map(Value::as_str).collect::<Option<_>>()?;
        if !under_group(&collections) {
            return None;
        }
        collections.sort_unstable();
        collections.dedup();
        let actions: Vec<&str> = match p.get("action") {
            None => all.to_vec(),
            Some(a) => {
                let a: Vec<&str> = a.as_array()?.iter().map(Value::as_str).collect::<Option<_>>()?;
                if a.is_empty() || !a.iter().all(|x| all.contains(x)) {
                    return None;
                }
                all.into_iter().filter(|x| a.contains(x)).collect()
            }
        };
        let mut params: Vec<String> = Vec::new();
        let mut scope = "repo".to_string();
        match collections[..] {
            [one] => scope += &format!(":{one}"),
            _ => params.extend(collections.iter().map(|c| format!("collection={c}"))),
        }
        if actions.len() < all.len() {
            params.extend(actions.iter().map(|a| format!("action={a}")));
        }
        if !params.is_empty() {
            scope += &format!("?{}", params.join("&"));
        }
        Some(scope)
    };
    set["permissions"].as_array().unwrap().iter().filter_map(permission).collect()
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
    let mut headers = IndexMap::new();
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
    let params: Vec<(String, String)> =
        query.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), percent_decode(v))).collect();
    let query = params.iter().cloned().collect();
    Some(Request { method, path: path.to_string(), query, params, headers, body })
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

/// How a write or an upload is refused, when it is: without a DPoP-bound token, without the nonce
/// the server hands out, or as [`Writes`] says.
fn refused(req: &Request, s: &State) -> Option<Answer> {
    let auth = req.header("authorization").unwrap_or_default();
    if !auth.starts_with("DPoP ") || req.dpop().is_none() {
        return Some((401, vec![], json!({ "error": "AuthRequired", "message": "a DPoP-bound token is required" })));
    }
    let (_, claims) = req.dpop().unwrap();
    if claims["nonce"] != s.nonce.as_str() {
        let headers =
            vec![("WWW-Authenticate", r#"DPoP error="use_dpop_nonce""#.into()), ("DPoP-Nonce", s.nonce.clone())];
        return Some((401, headers, json!({ "error": "use_dpop_nonce" })));
    }
    match s.writes {
        Writes::Accept => None,
        Writes::RefuseScope => Some((
            403,
            vec![],
            json!({ "error": "ScopeMissingError", "message": "Missing required scope \"repo:...\"" }),
        )),
        Writes::RefuseToken => Some((
            401,
            vec![("WWW-Authenticate", r#"DPoP error="invalid_token", error_description="revoked""#.into())],
            json!({ "error": "InvalidToken" }),
        )),
        Writes::RefuseRecord => {
            Some((400, vec![], json!({ "error": "InvalidRequest", "message": "Record is too big" })))
        }
    }
}

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
        ("POST", "/xrpc/com.atproto.repo.uploadBlob") => {
            if let Some(refused) = refused(req, &s) {
                return refused;
            }
            if req.body.len() > s.blob_limit {
                return (413, vec![], json!({ "error": "PayloadTooLarge", "message": "request entity too large" }));
            }
            let mime = req.header("content-type").unwrap_or("*/*").to_string();
            let blob_cid = cid(&Value::from(String::from_utf8_lossy(&req.body)));
            let size = req.body.len();
            s.blobs.insert(blob_cid.clone(), (mime.clone(), req.body.clone()));
            let blob = json!({ "$type": "blob", "ref": { "$link": blob_cid }, "mimeType": mime, "size": size });
            (200, vec![], json!({ "blob": blob }))
        }
        ("POST", path @ ("/xrpc/com.atproto.repo.putRecord" | "/xrpc/com.atproto.repo.createRecord")) => {
            if let Some(refused) = refused(req, &s) {
                return refused;
            }
            let body = req.json();
            let key = |k: &str| body[k].as_str().unwrap_or_default().to_string();
            let created = path.ends_with("createRecord");
            let rkey = if created { format!("3mockrecord{:02}", s.records.len() + 1) } else { key("rkey") };
            let id = (key("repo"), key("collection"), rkey);
            // A CID: what's there must be that record. `null`: there must be nothing there.
            let swap = body.get("swapRecord").map(|s| s.as_str());
            if let Some(swap) = swap
                && s.records.get(&id).map(|r| r.1.as_str()) != swap
            {
                return (400, vec![], json!({ "error": "InvalidSwap", "message": "Record was at another CID" }));
            }
            let record_cid = cid(&body["record"]);
            s.records.insert(id.clone(), (body["record"].clone(), record_cid.clone()));
            if s.firehose {
                index(&mut s, &id);
            }
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
        ("GET", "/xrpc/com.atproto.server.getServiceAuth") => {
            let signed = req.header("authorization").is_some_and(|a| a.starts_with("DPoP ")) && req.dpop().is_some();
            if !signed || s.writes == Writes::RefuseToken {
                return (401, vec![], json!({ "error": "InvalidToken", "message": "Token has been revoked" }));
            }
            let iss = s.account.as_ref().map_or(TESTS_ACCOUNT, |(did, _)| did.as_str());
            let claims =
                json!({ "iss": iss, "aud": req.query.get("aud"), "lxm": req.query.get("lxm"), "exp": 4102444800u64 });
            let token = format!(
                "{}.{}.mock-signature",
                base64url(br#"{"typ":"JWT","alg":"ES256K"}"#),
                base64url(claims.to_string().as_bytes())
            );
            (200, vec![], json!({ "token": token }))
        }
        ("GET", "/xrpc/computer.localnetwork.directory.getRecords") => {
            let records: Vec<Value> = req
                .params
                .iter()
                .filter(|(k, _)| k == "uris")
                .filter_map(|(_, uri)| {
                    let mut parts = uri.strip_prefix("at://")?.splitn(3, '/').map(String::from);
                    let id = (parts.next()?, parts.next()?, parts.next()?);
                    let (value, cid) = s.indexed.get(&id)?;
                    Some(json!({ "uri": uri, "cid": cid, "value": value }))
                })
                .collect();
            (200, vec![], json!({ "records": records }))
        }
        ("POST", "/xrpc/computer.localnetwork.directory.requestIndex") => {
            // A token this server's getServiceAuth signed for this method, at this website.
            let audience = format!("did:web:{}", base.trim_start_matches("http://").replace(':', "%3A"));
            let claims = req
                .header("authorization")
                .and_then(|a| a.strip_prefix("Bearer "))
                .and_then(|t| t.split('.').nth(1))
                .and_then(decode_json)
                .filter(|c| {
                    c["aud"] == audience.as_str() && c["lxm"] == "computer.localnetwork.directory.requestIndex"
                });
            let Some(claims) = claims else {
                return (401, vec![], json!({ "error": "AuthenticationRequired", "message": "Invalid service auth" }));
            };
            if !s.finishes {
                return (200, vec![], json!({ "indexed": false }));
            }
            let did = claims["iss"].as_str().unwrap_or_default();
            let theirs: Vec<_> = s
                .records
                .keys()
                .filter(|(repo, collection, _)| repo == did && LOCALLIBRARY_COLLECTIONS.contains(&collection.as_str()))
                .cloned()
                .collect();
            theirs.iter().for_each(|id| index(&mut s, id));
            (200, vec![], json!({ "indexed": true }))
        }
        ("POST", "/oauth/revoke") => (s.revocation, vec![], json!({})),
        ("POST", "/oauth/token") => {
            (400, vec![], json!({ "error": "invalid_grant", "error_description": "refresh token expired" }))
        }
        _ => (404, vec![], json!({ "error": "NotFound", "message": format!("no {} {}", req.method, req.path) })),
    }
}
