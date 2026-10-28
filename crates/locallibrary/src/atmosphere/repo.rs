//! Reading and writing records in an account's repository.
//!
//! Writes go through the signed-in session ([`put_record`], [`create_record`]): jacquard sends
//! them to the account's data server (PDS) with the DPoP-bound access token, refreshing it when it
//! has run out. Before a write, the record is checked against its Lexicon and the session's
//! scopes against what it needs, so a session signed in without permission to publish fails here,
//! not at the server.
//! Reads need no session, and come from Local Network's directory ([`super::directory`]), never
//! the PDS: a write that replaces a record names the version the directory had ([`put_record`]'s
//! `swap`), so it only lands on what the DJ saw. [`resolve`] finds an account's DID from its
//! handle.

use std::path::Path;

use jacquard_api::com_atproto::repo::create_record::{CreateRecord, CreateRecordError};
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

use super::{
    AccountInfo, REPORT_SCOPE, block_on, kept_scopes, lexicon, may_create, missing_scope, not_signed_in, resolver,
    restore, status, store,
};
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
    let (_lock, info) = writer(root, did, collection, record, |info| missing_scope(info, collection).is_none())?;
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
    let (_lock, info) = writer(root, did, collection, record, |info| may_create(info, collection))?;
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

/// Before a write: `record` fits its Lexicon, the account signed in is `did` and its session
/// `may` write it. Holds the lock on the session until the write is done.
fn writer(
    root: &Path,
    did: &str,
    collection: &str,
    record: &Value,
    may: impl Fn(&AccountInfo) -> bool,
) -> Result<(store::AccountLock, AccountInfo)> {
    lexicon::validate(collection, record)?;
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
