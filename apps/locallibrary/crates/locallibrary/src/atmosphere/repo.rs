//! Reading and writing records in an account's repository.
//!
//! Writes go through the signed-in session ([`put_record`]): jacquard sends them to the account's
//! data server (PDS) with the DPoP-bound access token, refreshing it when it has run out. Before
//! a write, the record is checked against its Lexicon and the session's scopes against what it
//! needs, so a session signed in without permission to publish fails here, not at the server.
//! Reads ([`get_record`]) need no session: anyone can read a repository, so they go straight to
//! the PDS, found from a handle or DID with [`resolve`]. What another account's servers answer is
//! read up to a limit, and shown on one line without control characters.

use std::path::Path;
use std::time::Duration;

use jacquard_api::com_atproto::repo::put_record::{PutRecord, PutRecordError};
use jacquard_common::error::{AuthError, ClientError, ClientErrorKind, HttpError};
use jacquard_common::types::did::Did;
use jacquard_common::types::handle::Handle;
use jacquard_common::types::value::Data;
use jacquard_common::types::value::from_json_value;
use jacquard_common::xrpc::{XrpcClient, XrpcError};
use jacquard_common::{SmolStr, ToSmolStr};
use jacquard_identity::resolver::IdentityResolver;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use super::{AccountInfo, block_on, lexicon, missing_scope, net, not_signed_in, resolver, restore, status, store};
use crate::error::{Action, Reason};
use crate::export::one_line;
use crate::{Error, Result};

/// `at://did/collection/rkey`.
pub fn at_uri(did: &str, collection: &str, rkey: &str) -> String {
    format!("at://{did}/{collection}/{rkey}")
}

fn host(url: &str) -> &str {
    url.split("://").nth(1).unwrap_or(url).trim_end_matches('/')
}

/// What publishing a record type is called, for messages.
fn what(collection: &str) -> &'static str {
    match collection {
        lexicon::SETTINGS => "My Settings",
        lexicon::PROFILE => "the profile",
        lexicon::SET => "sets",
        _ => "records",
    }
}

/// Signing in again, allowing publishing.
fn sign_in_to_publish() -> Action {
    Action::call("account.login.begin", serde_json::json!({ "publish": true }), "Sign in again, allowing publishing")
}

/// The error for a session that may not write `collection`.
pub fn scope_error(info: &AccountInfo, collection: &str) -> Error {
    Error::Account(format!(
        "{}'s session doesn't allow locallibrary to publish {}; sign in again, allowing publishing",
        info.name(),
        what(collection)
    ))
    .because(Reason::MissingScope)
    .action(sign_in_to_publish())
}

fn session_refused(info: &AccountInfo) -> Error {
    Error::Account(format!(
        "{} didn't accept the session for {} (it has ended or was revoked); sign in again, allowing publishing",
        host(&info.pds),
        info.name()
    ))
    .because(Reason::SessionEnded)
    .action(sign_in_to_publish())
}

/// A record written: where, and its content hash.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Written {
    pub uri: String,
    pub cid: String,
}

/// Write `record` (with its `$type`) as `collection/rkey` in the repository of `did`, which has to
/// be the signed-in account: replacing the record with the CID `swap`, or with `None` creating it.
/// Anything else is a conflict: someone published there in between, or another account signed in.
pub fn put_record(
    root: &Path,
    did: &str,
    collection: &str,
    rkey: &str,
    record: &Value,
    swap: Option<&str>,
) -> Result<Written> {
    lexicon::validate(collection, record)?;
    // Nothing to write with: don't create the folder for a lock.
    status(root)?.ok_or_else(not_signed_in)?;
    let _lock = store::lock(root)?;
    let info = status(root)?.ok_or_else(not_signed_in)?;
    if info.did != did {
        return Err(Error::conflict(
            Reason::Changed,
            format!("{} is signed in now, not the account this was planned for; run it again", info.name()),
        ));
    }
    if missing_scope(&info, collection).is_some() {
        return Err(scope_error(&info, collection));
    }
    let session = restore(root)?;
    let body =
        json!({ "repo": info.did, "collection": collection, "rkey": rkey, "record": record, "swapRecord": swap });
    let mut request = from_json_value::<PutRecord>(body)
        .map_err(|e| Error::invalid(format!("the {} record: {e}", lexicon::short_name(collection))))?;
    // `swapRecord: null` is how a write says there must be no record yet; jacquard's type can't
    // say null, so it goes alongside.
    if swap.is_none() {
        request.extra_data = Some([("swapRecord".to_smolstr(), Data::Null)].into());
    }
    let response = block_on(session.send(request)).map_err(|e| client_error(&info, collection, e))?;
    let status = response.status();
    match response.into_output() {
        Ok(out) => Ok(Written { uri: out.uri.to_string(), cid: out.cid.to_string() }),
        Err(XrpcError::Xrpc(PutRecordError::InvalidSwap(_))) => Err(Error::conflict(
            Reason::Changed,
            format!(
                "{} changed on {} while locallibrary was publishing; run it again to see what's there now",
                what(collection),
                host(&info.pds)
            ),
        )),
        Err(XrpcError::Xrpc(PutRecordError::Other { error, message })) => {
            Err(refusal(&info, collection, status.as_u16(), &error, message.as_deref().unwrap_or_default()))
        }
        Err(XrpcError::Generic(g)) => {
            Err(refusal(&info, collection, status.as_u16(), &g.error, g.message.as_deref().unwrap_or_default()))
        }
        Err(XrpcError::Auth(_)) => Err(session_refused(&info)),
        Err(e) => Err(Error::Account(one_line(&format!(
            "{} answered in a way locallibrary can't read ({e})",
            host(&info.pds)
        )))),
    }
}

/// An error the server explained: `error` and `message` from its answer.
fn refusal(info: &AccountInfo, collection: &str, status: u16, error: &str, message: &str) -> Error {
    if error.to_ascii_lowercase().contains("scope") || message.to_ascii_lowercase().contains("scope") {
        return scope_error(info, collection);
    }
    if status == 401 || matches!(error, "AuthRequired" | "InvalidToken" | "ExpiredToken") {
        return session_refused(info);
    }
    let why = if message.is_empty() { error.to_string() } else { format!("{error}: {message}") };
    Error::Account(format!("{} refused to publish {} ({})", host(&info.pds), what(collection), one_line(&why)))
}

fn client_error(info: &AccountInfo, collection: &str, e: ClientError) -> Error {
    match e.kind() {
        ClientErrorKind::Transport => Error::Account(one_line(&format!("couldn't reach {} ({e})", host(&info.pds)))),
        ClientErrorKind::Auth(AuthError::Other(header))
            if header.to_str().is_ok_and(|h| h.contains("insufficient_scope")) =>
        {
            scope_error(info, collection)
        }
        ClientErrorKind::Auth(_) => session_refused(info),
        ClientErrorKind::Http { status } => {
            let body: Value = e
                .source_err()
                .and_then(|s| s.downcast_ref::<HttpError>())
                .and_then(|h| h.body.as_ref())
                .and_then(|b| serde_json::from_slice(b).ok())
                .unwrap_or_default();
            let error = body["error"].as_str().unwrap_or_default();
            let message = body["message"].as_str().unwrap_or_default();
            if status.as_u16() == 403 && error.is_empty() && message.is_empty() {
                // Forbidden without a word: what a server says to a token without the scope.
                return scope_error(info, collection);
            }
            let error = if error.is_empty() { status.as_str() } else { error };
            refusal(info, collection, status.as_u16(), error, message)
        }
        _ => Error::Account(one_line(&format!("publishing to {}: {e}", host(&info.pds)))),
    }
}

/// A record read from a repository.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Fetched {
    pub uri: String,
    /// The record's content hash (servers always send it).
    pub cid: Option<String>,
    pub value: Value,
}

/// Read `collection/rkey` from `repo` (a DID or handle) on the PDS at `pds`, without signing in.
/// `None` when there's no such record. `pds` is the signed-in account's, one the user named, or
/// one [`resolve`] found; redirects only go to public https addresses.
pub fn get_record(pds: &str, repo: &str, collection: &str, rkey: &str) -> Result<Option<Fetched>> {
    let base = pds.trim_end_matches('/');
    let unreachable = |e: net::NetError| Error::Account(one_line(&format!("couldn't reach {} ({e})", host(base))));
    block_on(async {
        let http = net::client().timeout(Duration::from_secs(20)).build().map_err(|e| unreachable(e.into()))?;
        let response = http
            .get(format!("{base}/xrpc/com.atproto.repo.getRecord"))
            .query(&[("repo", repo), ("collection", collection), ("rkey", rkey)])
            .send()
            .await
            .map_err(|e| unreachable(e.into()))?;
        let status = response.status();
        let bytes = net::body(response).await.map_err(unreachable)?;
        let body: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        if status.is_success() && body["value"].is_object() {
            return Ok(Some(Fetched {
                uri: body["uri"].as_str().map_or_else(|| at_uri(repo, collection, rkey), one_line),
                cid: body["cid"].as_str().map(one_line),
                value: body["value"].clone(),
            }));
        }
        if body["error"] == "RecordNotFound" {
            return Ok(None);
        }
        let why = match (body["error"].as_str(), body["message"].as_str()) {
            (Some(e), Some(m)) => format!("{e}: {m}"),
            (Some(e), None) => e.to_string(),
            _ => status.to_string(),
        };
        Err(Error::Account(one_line(&format!(
            "{} couldn't give {} ({why})",
            host(base),
            at_uri(repo, collection, rkey)
        ))))
    })
}

/// An account found from its handle or DID.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Resolved {
    pub did: String,
    /// When it was found by handle.
    pub handle: Option<String>,
    /// Its data server.
    pub pds: String,
}

impl Resolved {
    /// `@handle`, or the DID.
    pub fn name(&self) -> String {
        self.handle.as_ref().map_or_else(|| self.did.clone(), |h| format!("@{h}"))
    }
}

/// Find an account's DID and data server from its handle (`dj.example.com`, `@` optional) or DID.
/// Its DID document is someone else's say-so: the data server has to be a public https address.
pub fn resolve(input: &str) -> Result<Resolved> {
    let input = input.trim().trim_start_matches('@');
    let not_found = |why: String| Error::NotFound(one_line(&format!("the Atmosphere account {input:?} ({why})")));
    let resolver = resolver(true)?;
    block_on(async {
        let (did, handle) = if input.starts_with("did:") {
            (Did::<SmolStr>::new_owned(input).map_err(|e| not_found(e.to_string()))?, None)
        } else {
            let h: Handle = Handle::new_owned(input).map_err(|e| not_found(format!("not a handle: {e}")))?;
            (resolver.resolve_handle(&h).await.map_err(|e| not_found(e.to_string()))?, Some(input.to_lowercase()))
        };
        let pds = resolver.pds_for_did(&did).await.map_err(|e| not_found(e.to_string()))?;
        net::public(pds.as_str()).map_err(|why| not_found(format!("its data server can't be used: {why}")))?;
        Ok(Resolved { did: did.as_str().to_string(), handle, pds: pds.as_str().trim_end_matches('/').to_string() })
    })
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    use super::*;
    use crate::atmosphere::Storage;

    #[test]
    fn a_session_without_permission_to_publish_is_stopped_before_the_server() {
        let dir = tempfile::tempdir().unwrap();
        let record = crate::publish::settings_record(&[("quantize".into(), "off".into())].into(), chrono::Utc::now());
        let err = put_record(dir.path(), "did:plc:abc", lexicon::SETTINGS, "self", &record, None).unwrap_err();
        assert!(err.to_string().contains("not signed in"), "{err}");
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing written");

        // Signed in to know who you are, nothing more. Its server refuses connections: nothing
        // may reach it.
        crate::atmosphere::tests::save_session(dir.path(), "did:plc:abc", "https://127.0.0.1:9", "atproto");
        let err = put_record(dir.path(), "did:plc:abc", lexicon::SETTINGS, "self", &record, None).unwrap_err();
        assert_eq!(err.code(), "account");
        assert!(err.to_string().contains("doesn't allow locallibrary to publish My Settings"), "{err}");
        assert!(err.to_string().contains("sign in again, allowing publishing"), "{err}");
        assert_eq!(err.reason(), Some(Reason::MissingScope));

        // A record that doesn't fit its Lexicon isn't sent either.
        let mut wrong = record.clone();
        wrong["player"]["quantize"] = 0.into();
        let err = put_record(dir.path(), "did:plc:abc", lexicon::SETTINGS, "self", &wrong, None).unwrap_err();
        assert_eq!(err.code(), "invalid");
        assert_eq!(crate::atmosphere::status(dir.path()).unwrap().unwrap().storage, Storage::File);

        // Planned for another account than the one signed in now: not written to this one.
        let err = put_record(dir.path(), "did:plc:other", lexicon::SETTINGS, "self", &record, None).unwrap_err();
        assert_eq!((err.code(), err.reason()), ("conflict", Some(Reason::Changed)), "{err}");
    }

    #[test]
    fn reading_from_a_server_that_isnt_there_says_so() {
        let err = get_record("http://127.0.0.1:9", "did:plc:abc", lexicon::SETTINGS, "self").unwrap_err();
        assert!(err.to_string().contains("couldn't reach 127.0.0.1:9"), "{err}");
        assert!(resolve("not a handle").unwrap_err().to_string().contains("not a handle"));
    }

    /// A server on 127.0.0.1 that answers each connection with the next of `answers`, and the
    /// number of connections it got.
    fn server(answers: Vec<Vec<u8>>) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let got = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = got.clone();
        std::thread::spawn(move || {
            for (conn, answer) in listener.incoming().zip(answers) {
                let mut conn = conn.unwrap();
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
    fn someone_elses_server_is_read_with_care() {
        // A redirect to plain http, or to this computer, isn't followed.
        let (elsewhere, reached) = server(vec![answer("200 OK", b"{}")]);
        let (pds, _) = server(vec![answer(&format!("302 Found\r\nlocation: {elsewhere}/x"), b"")]);
        let err = get_record(&pds, "did:plc:abc", lexicon::SETTINGS, "self").unwrap_err();
        assert!(err.to_string().contains("isn't an https address"), "{err}");
        let (pds, _) = server(vec![answer("302 Found\r\nlocation: https://127.0.0.1:9/x", b"")]);
        let err = get_record(&pds, "did:plc:abc", lexicon::SETTINGS, "self").unwrap_err();
        assert!(err.to_string().contains("on this computer or its local network"), "{err}");
        assert_eq!(reached.load(std::sync::atomic::Ordering::SeqCst), 0);

        // An answer that doesn't stop isn't read whole.
        let (pds, _) = server(vec![answer("200 OK", &vec![b' '; net::MAX_BODY + 1])]);
        let err = get_record(&pds, "did:plc:abc", lexicon::SETTINGS, "self").unwrap_err();
        assert!(err.to_string().contains("the answer is over 1 MB"), "{err}");

        // What it says goes to a terminal: no escape sequences.
        let record = json!({
            "uri": "at://did:plc:abc/x/self\u{1b}]0;owned\u{7}",
            "cid": "bafy\u{1b}[2J",
            "value": { "version": 1 },
        });
        let refusal = json!({ "error": "Nope\u{1b}[2J", "message": "really\u{1b}]52;c;Zm9v\u{7}" });
        let (pds, _) = server(vec![
            answer("200 OK", record.to_string().as_bytes()),
            answer("400 Bad Request", refusal.to_string().as_bytes()),
        ]);
        let fetched = get_record(&pds, "did:plc:abc", lexicon::SETTINGS, "self").unwrap().unwrap();
        assert!(!fetched.uri.chars().chain(fetched.cid.unwrap().chars()).any(char::is_control), "{}", fetched.uri);
        let err = get_record(&pds, "did:plc:abc", lexicon::SETTINGS, "self").unwrap_err().to_string();
        assert!(!err.chars().any(char::is_control) && err.contains("Nope [2J: really ]52;c;Zm9v "), "{err:?}");
    }

    /// A refusal to give a record says what it can: its error, else the status.
    #[test]
    fn a_refusal_to_read_says_its_error_or_status() {
        let (pds, _) = server(vec![
            answer("400 Bad Request", br#"{"error":"RepoDeactivated"}"#),
            answer("502 Bad Gateway", b"<html>"),
        ]);
        let err = get_record(&pds, "did:plc:abc", lexicon::SETTINGS, "self").unwrap_err().to_string();
        assert!(err.ends_with("(RepoDeactivated)"), "{err}");
        let err = get_record(&pds, "did:plc:abc", lexicon::SETTINGS, "self").unwrap_err().to_string();
        assert!(err.ends_with("(502 Bad Gateway)"), "{err}");
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
        let scopes = crate::atmosphere::publish_scopes().join(" ");
        crate::atmosphere::tests::save_session(dir.path(), "did:plc:abc", &pds, &scopes);
        let record = crate::publish::settings_record(&[("quantize".into(), "off".into())].into(), chrono::Utc::now());
        let write = |status: u16, headers: &str, body: Value| {
            *next.lock().unwrap() = (status, headers.to_string(), body);
            put_record(dir.path(), "did:plc:abc", lexicon::SETTINGS, "self", &record, None).unwrap_err()
        };

        for err in [
            write(403, "", json!({})),
            write(401, "www-authenticate: DPoP error=\"insufficient_scope\"\r\n", json!({})),
        ] {
            assert_eq!(err.reason(), Some(Reason::MissingScope), "{err}");
        }
        for err in
            [write(400, "", json!({ "error": "ExpiredToken" })), write(401, "", json!({ "error": "Unauthorized" }))]
        {
            assert_eq!(err.reason(), Some(Reason::SessionEnded), "{err}");
        }
        let err = write(500, "", json!({ "error": "InternalServerError", "message": "boom" }));
        assert!(err.to_string().ends_with("refused to publish My Settings (InternalServerError: boom)"), "{err}");
        let err = write(200, "", json!({ "nothing": "useful" }));
        assert!(err.to_string().contains("answered in a way locallibrary can't read"), "{err}");

        let gone = tempfile::tempdir().unwrap();
        crate::store::Store::open_or_init(gone.path()).unwrap();
        crate::atmosphere::tests::save_session(gone.path(), "did:plc:abc", "http://127.0.0.1:9", &scopes);
        let err = put_record(gone.path(), "did:plc:abc", lexicon::SETTINGS, "self", &record, None).unwrap_err();
        assert!(err.to_string().starts_with("couldn't reach 127.0.0.1:9"), "{err}");
    }

    #[test]
    fn a_missing_scope_names_what_couldnt_be_published() {
        let info = crate::atmosphere::tests::signed_in_with(&["atproto"]);
        let said = |collection| scope_error(&info, collection).to_string();
        assert!(said(lexicon::PROFILE).contains("publish the profile;"));
        assert!(said(lexicon::SET).contains("publish sets;"));
        assert!(said("app.example.thing").contains("publish records;"));
    }
}
