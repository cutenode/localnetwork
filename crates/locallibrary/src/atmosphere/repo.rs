//! Reading and writing records in an account's repository.
//!
//! Writes go through the signed-in session ([`put_record`], [`create_record`], [`delete_record`]):
//! jacquard sends them to the account's data server (PDS) with the DPoP-bound access token,
//! refreshing it when it has run out. Before a write, the record is checked against its Lexicon
//! and the session's scopes against what it needs, so a session signed in without permission to
//! publish fails here, not at the server. [`upload_blob`] sends a file the same way, read from
//! disk as it goes.
//! Reads need no session, and come from Local Network's directory ([`super::directory`]), never
//! the PDS: a write that replaces a record names the version the directory had ([`put_record`]'s
//! `swap`), so it only lands on what the DJ saw. [`resolve`] finds an account's DID from its
//! handle.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jacquard_api::com_atproto::repo::create_record::{CreateRecord, CreateRecordError};
use jacquard_api::com_atproto::repo::delete_record::{DeleteRecord, DeleteRecordError};
use jacquard_api::com_atproto::repo::put_record::{PutRecord, PutRecordError};
use jacquard_api::com_atproto::server::get_session::GetSession;
use jacquard_common::error::{AuthError, ClientError, ClientErrorKind, HttpError};
use jacquard_common::types::did::Did;
use jacquard_common::types::handle::Handle;
use jacquard_common::types::value::Data;
use jacquard_common::types::value::from_json_value;
use jacquard_common::xrpc::{XrpcClient, XrpcError};
use jacquard_common::{SmolStr, ToSmolStr};
use jacquard_identity::resolver::IdentityResolver;
use jacquard_oauth::dpop::build_dpop_proof;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    AccountInfo, REPORT_SCOPE, block_on, kept_scopes, lexicon, may_create, may_delete, may_upload, missing_scope, net,
    not_signed_in, resolver, restore, status, store,
};
use crate::error::{Action, IoContext, Reason};
use crate::export::one_line;
use crate::progress::{CancelToken, Ctx, Event, Phase};
use crate::{Error, Result};

/// The data server's method that takes a file.
const UPLOAD_BLOB: &str = "com.atproto.repo.uploadBlob";

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
        lexicon::RECORDING => "recordings",
        lexicon::DISCUSSION => "bug reports",
        _ => "records",
    }
}

/// Signing `info`'s account in again, allowing what writing `collection` needs: filing bug
/// reports (keeping what the session may do now), or publishing.
fn sign_in_again(info: &AccountInfo, collection: &str) -> Action {
    match collection {
        lexicon::DISCUSSION => {
            let mut scopes = kept_scopes(info);
            if !scopes.iter().any(|s| s == REPORT_SCOPE) {
                scopes.push(REPORT_SCOPE.into());
            }
            Action::call("account.login.begin", json!({ "scopes": scopes }), "Sign in again, allowing bug reports")
        }
        _ => Action::call("account.login.begin", json!({ "publish": true }), "Sign in again, allowing publishing"),
    }
}

/// The error for a session that may not write `collection`.
pub fn scope_error(info: &AccountInfo, collection: &str) -> Error {
    let again = sign_in_again(info, collection);
    Error::Account(format!(
        "{}'s session doesn't allow locallibrary to publish {}; {}",
        info.name(),
        what(collection),
        again.label.to_lowercase()
    ))
    .because(Reason::MissingScope)
    .action(again)
}

fn session_refused(info: &AccountInfo, collection: &str) -> Error {
    let again = sign_in_again(info, collection);
    Error::Account(format!(
        "{} didn't accept the session for {} (it has ended or was revoked); {}",
        host(&info.pds),
        info.name(),
        again.label.to_lowercase()
    ))
    .because(Reason::SessionEnded)
    .action(again)
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
    let (_lock, info) = writer(root, did, collection, |info| missing_scope(info, collection).is_none())?;
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
        // The swap is the version the directory had, which lags behind the PDS for a moment after
        // each write (and misses what was published before it kept the record type).
        Err(XrpcError::Xrpc(PutRecordError::InvalidSwap(_))) => Err(Error::conflict(
            Reason::Changed,
            format!(
                "{} on {} isn't the version Local Network's directory has; run it again in a minute, once the \
                 directory has caught up",
                what(collection),
                host(&info.pds)
            ),
        )),
        Err(XrpcError::Xrpc(PutRecordError::Other { error, message })) => {
            Err(refusal(&info, collection, status.as_u16(), &error, message.as_deref().unwrap_or_default()))
        }
        Err(e) => Err(answer_error(&info, collection, status.as_u16(), e)),
    }
}

/// Create `record` (with its `$type`) in `collection` of the repository of `did`, which has to be
/// the signed-in account, under a key the server picks (a TID). The session only needs to be
/// allowed to create such records, not to replace them.
pub fn create_record(root: &Path, did: &str, collection: &str, record: &Value) -> Result<Written> {
    lexicon::validate(collection, record)?;
    let (_lock, info) = writer(root, did, collection, |info| may_create(info, collection))?;
    let session = restore(root)?;
    let request =
        from_json_value::<CreateRecord>(json!({ "repo": info.did, "collection": collection, "record": record }))
            .map_err(|e| Error::invalid(format!("the {} record: {e}", lexicon::short_name(collection))))?;
    let response = block_on(session.send(request)).map_err(|e| client_error(&info, collection, e))?;
    let status = response.status();
    match response.into_output() {
        Ok(out) => Ok(Written { uri: out.uri.to_string(), cid: out.cid.to_string() }),
        Err(XrpcError::Xrpc(CreateRecordError::Other { error, message })) => {
            Err(refusal(&info, collection, status.as_u16(), &error, message.as_deref().unwrap_or_default()))
        }
        Err(e) => Err(answer_error(&info, collection, status.as_u16(), e)),
    }
}

/// Delete `collection/rkey` from the repository of `did`, which has to be the signed-in account,
/// whatever version is there. A server answers a record that isn't there as deleted.
pub fn delete_record(root: &Path, did: &str, collection: &str, rkey: &str) -> Result<()> {
    let (_lock, info) = writer(root, did, collection, |info| may_delete(info, collection))?;
    let session = restore(root)?;
    let request = from_json_value::<DeleteRecord>(json!({ "repo": info.did, "collection": collection, "rkey": rkey }))
        .map_err(|e| Error::invalid(format!("deleting the {} record: {e}", lexicon::short_name(collection))))?;
    let response = block_on(session.send(request)).map_err(|e| client_error(&info, collection, e))?;
    let status = response.status();
    match response.into_output() {
        Ok(_) => Ok(()),
        Err(XrpcError::Xrpc(DeleteRecordError::Other { error, message })) => {
            Err(refusal(&info, collection, status.as_u16(), &error, message.as_deref().unwrap_or_default()))
        }
        Err(e) => Err(answer_error(&info, collection, status.as_u16(), e)),
    }
}

/// Before a write of `collection`: the account signed in is `did` and its session `may` write it.
/// Holds the lock on the session until the write is done.
fn writer(
    root: &Path,
    did: &str,
    collection: &str,
    may: impl Fn(&AccountInfo) -> bool,
) -> Result<(store::AccountLock, AccountInfo)> {
    // Nothing to write with: don't create the folder for a lock.
    status(root)?.ok_or_else(not_signed_in)?;
    let lock = store::lock(root)?;
    let info = status(root)?.ok_or_else(not_signed_in)?;
    if info.did != did {
        return Err(Error::conflict(
            Reason::Changed,
            format!("{} is signed in now, not the account this was planned for; run it again", info.name()),
        ));
    }
    if !may(&info) {
        return Err(scope_error(&info, collection));
    }
    Ok((lock, info))
}

/// A write's answer that isn't the record written, nor an error of the method's own.
fn answer_error<E: std::error::Error>(info: &AccountInfo, collection: &str, status: u16, e: XrpcError<E>) -> Error {
    match e {
        XrpcError::Generic(g) => refusal(info, collection, status, &g.error, g.message.as_deref().unwrap_or_default()),
        XrpcError::Auth(_) => session_refused(info, collection),
        e => Error::Account(one_line(&format!("{} answered in a way locallibrary can't read ({e})", host(&info.pds)))),
    }
}

/// An error the server explained: `error` and `message` from its answer.
fn refusal(info: &AccountInfo, collection: &str, status: u16, error: &str, message: &str) -> Error {
    if error.to_ascii_lowercase().contains("scope") || message.to_ascii_lowercase().contains("scope") {
        return scope_error(info, collection);
    }
    if status == 401 || matches!(error, "AuthRequired" | "InvalidToken" | "ExpiredToken") {
        return session_refused(info, collection);
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
        ClientErrorKind::Auth(_) => session_refused(info, collection),
        ClientErrorKind::Http { status } => {
            let body: Value = e
                .source_err()
                .and_then(|s| s.downcast_ref::<HttpError>())
                .and_then(|h| h.body.as_ref())
                .and_then(|b| serde_json::from_slice(b).ok())
                .unwrap_or_default();
            http_refusal(info, collection, status.as_u16(), &body)
        }
        _ => Error::Account(one_line(&format!("publishing to {}: {e}", host(&info.pds)))),
    }
}

/// An HTTP error answer: its status, and `body`'s `error` and `message`.
fn http_refusal(info: &AccountInfo, collection: &str, status: u16, body: &Value) -> Error {
    let error = body["error"].as_str().unwrap_or_default();
    let message = body["message"].as_str().unwrap_or_default();
    if status == 403 && error.is_empty() && message.is_empty() {
        // Forbidden without a word: what a server says to a token without the scope.
        return scope_error(info, collection);
    }
    let status_text = status.to_string();
    let error = if error.is_empty() { status_text.as_str() } else { error };
    refusal(info, collection, status, error, message)
}

/// Upload the file at `path` (of the type `mime`, `audio/mp4`) to the data server of `did`, which
/// has to be the signed-in account, reporting the bytes sent in the upload phase. Returns its blob,
/// as a record links to it: `$type`, `ref`, `mimeType` (what the server made of the file) and
/// `size`. The file is read as it's sent, never held whole: a set's recording runs to hundreds of
/// MB. jacquard would hold it (its upload takes the bytes, and its streaming upload keeps every
/// part sent in case the server asks for a DPoP nonce), so this request goes out by itself, with
/// the session's token and a DPoP proof of its own.
pub fn upload_blob(root: &Path, did: &str, path: &Path, mime: &str, ctx: &Ctx<'_>) -> Result<Value> {
    let collection = lexicon::RECORDING;
    let (_lock, info) = writer(root, did, collection, |info| may_upload(info, mime))?;
    let size = std::fs::metadata(path).at(path)?.len();
    let session = restore(root)?;
    // A small request through jacquard first: it refreshes the access token if it ran out and
    // learns the server's DPoP nonce, so the upload goes out with both.
    block_on(session.send(GetSession)).map_err(|e| client_error(&info, collection, e))?;
    // As long as sending it at 128 KiB/s would take, and half a minute more.
    let timeout = Duration::from_secs(30 + size / (128 * 1024));
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|e| Error::Account(format!("the network: {e}")))?;
    let mut retried = false;
    loop {
        let (url, token, proof) = block_on(async {
            let data = session.data.read().await;
            let url = format!("{}/xrpc/{UPLOAD_BLOB}", data.host_url.as_str().trim_end_matches('/'));
            let token = data.token_set.access_token.to_string();
            let ath = URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()));
            let nonce = Some(data.dpop_data.dpop_host_nonce.as_str()).filter(|n| !n.is_empty());
            let proof = build_dpop_proof(&data.dpop_data.dpop_key, "POST", &url, nonce, Some(&ath));
            (url, token, proof)
        });
        let proof = proof.map_err(|e| Error::Account(format!("signing the upload: {e}")))?;
        let request = client
            .post(&url)
            .header(AUTHORIZATION, format!("DPoP {token}"))
            .header("DPoP", proof.as_str())
            .header(CONTENT_TYPE, mime);
        let response = send_file(request, path, size, ctx)?;
        let status = response.status().as_u16();
        let header = |name| response.headers().get(name).and_then(|v| v.to_str().ok()).map(String::from);
        let (nonce, challenge) = (header("DPoP-Nonce"), header(WWW_AUTHENTICATE.as_str()).unwrap_or_default());
        let mut answer = Vec::new();
        let _ = response.take(net::MAX_BODY as u64).read_to_end(&mut answer);
        let body: Value = serde_json::from_slice(&answer).unwrap_or_default();
        match (status, nonce) {
            (200, _) if body["blob"].is_object() => return Ok(body["blob"].clone()),
            (200, _) => {
                return Err(Error::Account(one_line(&format!(
                    "{} answered the upload in a way locallibrary can't read",
                    host(&info.pds)
                ))));
            }
            // Its nonce changed since: once more with the new one.
            (401, Some(nonce))
                if !retried && (challenge.contains("use_dpop_nonce") || body["error"] == "use_dpop_nonce") =>
            {
                block_on(async { session.data.write().await.dpop_data.dpop_host_nonce = nonce.into() });
                retried = true;
            }
            (401, _) if challenge.contains("insufficient_scope") => return Err(scope_error(&info, collection)),
            _ if status == 413 || body["error"] == "PayloadTooLarge" => return Err(too_big(&info, size)),
            _ => return Err(http_refusal(&info, collection, status, &body)),
        }
    }
}

/// A file the server wouldn't take for its size, which no server says up front.
fn too_big(info: &AccountInfo, size: u64) -> Error {
    Error::Account(format!(
        "{} refused the recording: {} is more than it takes. A data server takes 5 MiB unless whoever runs \
         it allows more (PDS_BLOB_UPLOAD_LIMIT; the official installer allows 300 MiB); `--max-mib` makes the \
         recording smaller",
        host(&info.pds),
        crate::op::bytes(size)
    ))
}

/// Send `request` with the file at `path` (`size` bytes) as its body, read as it's sent, with
/// what's sent reported in the upload phase.
fn send_file(
    request: reqwest::blocking::RequestBuilder,
    path: &Path,
    size: u64,
    ctx: &Ctx<'_>,
) -> Result<reqwest::blocking::Response> {
    let (sent, reported) = mpsc::channel();
    let body = Sending { file: File::open(path).at(path)?, sent, cancel: ctx.cancel.clone() };
    let request = request.body(reqwest::blocking::Body::sized(body, size));
    ctx.phase_start(Phase::Upload, 1, size);
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    ctx.emit(Event::Item { phase: Phase::Upload, path: name, bytes: size });
    // reqwest reads the body on the thread that sends it, so another one sends while this one
    // reports.
    let response = std::thread::scope(|s| {
        let sending = s.spawn(move || request.send());
        loop {
            match reported.recv_timeout(Duration::from_millis(100)) {
                Ok(bytes) => ctx.emit(Event::Bytes { phase: Phase::Upload, bytes }),
                Err(mpsc::RecvTimeoutError::Timeout) if !sending.is_finished() => {}
                Err(_) => break,
            }
        }
        sending.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    });
    ctx.phase_end(Phase::Upload);
    response.map_err(|e| match ctx.check() {
        Err(_) => Error::Cancelled,
        Ok(()) => Error::Account(one_line(&format!("sending {}: {}", path.display(), net::with_causes(&e)))),
    })
}

/// A file as it's sent: each read told to whoever reports it, and failing once the operation is
/// cancelled.
struct Sending {
    file: File,
    sent: mpsc::Sender<u64>,
    cancel: CancelToken,
}

impl Read for Sending {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(std::io::Error::other("cancelled"));
        }
        let n = self.file.read(buf)?;
        let _ = self.sent.send(n as u64);
        Ok(n)
    }
}

/// An account found from its handle or DID.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Resolved {
    pub did: String,
    /// When it was found by handle.
    pub handle: Option<String>,
}

impl Resolved {
    /// `@handle`, or the DID.
    pub fn name(&self) -> String {
        self.handle.as_ref().map_or_else(|| self.did.clone(), |h| format!("@{h}"))
    }
}

/// Find an account's DID from its handle (`dj.example.com`, `@` optional), or take a DID as it is.
/// The handle's DNS and web server are someone else's say-so: only public https addresses.
pub fn resolve(input: &str) -> Result<Resolved> {
    let input = input.trim().trim_start_matches('@');
    let not_found = |why: String| Error::NotFound(one_line(&format!("the Atmosphere account {input:?} ({why})")));
    if input.starts_with("did:") {
        let did = Did::<SmolStr>::new_owned(input).map_err(|e| not_found(e.to_string()))?;
        return Ok(Resolved { did: did.as_str().to_string(), handle: None });
    }
    let h: Handle = Handle::new_owned(input).map_err(|e| not_found(format!("not a handle: {e}")))?;
    let resolver = resolver(true)?;
    let did = block_on(resolver.resolve_handle(&h)).map_err(|e| not_found(e.to_string()))?;
    Ok(Resolved { did: did.as_str().to_string(), handle: Some(input.to_lowercase()) })
}

#[cfg(test)]
pub(crate) mod tests;
