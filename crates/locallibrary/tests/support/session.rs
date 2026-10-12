//! Signed in to an Atmosphere account without the network: a session as jacquard hands one over
//! after a sign-in, kept as locallibrary keeps it (in files, never the keychain).
//!
//! Shared by the library's and the CLI's tests (`#[path]`), so it only uses what both have.

#![allow(dead_code)]

use std::path::Path;

use jacquard_oauth::authstore::ClientAuthStore;
use locallibrary::atmosphere::store::LocalStore;
use locallibrary::atmosphere::{Client, Storage};
use serde_json::{Value, json};

/// The account the tests sign in as.
pub const DID: &str = "did:plc:testdj";

/// A session for `did`, numbered `session_id` and granted `scopes`, with a fresh DPoP key and the
/// refresh token `refresh` (or none), its servers at `pds`, its access token running out at
/// `expires_at`.
pub fn session_json(
    did: &str,
    session_id: &str,
    pds: &str,
    scopes: &str,
    refresh: Option<&str>,
    expires_at: &str,
) -> Value {
    let key = jacquard_oauth::utils::generate_key(&["ES256"]).unwrap();
    json!({
        "account_did": did,
        "session_id": session_id,
        "host_url": pds,
        "authserver_url": pds,
        "authserver_token_endpoint": format!("{pds}/oauth/token"),
        "authserver_revocation_endpoint": format!("{pds}/oauth/revoke"),
        "scopes": scopes,
        "dpop_key": key,
        "dpop_authserver_nonce": "",
        "dpop_host_nonce": "",
        "iss": pds,
        "sub": did,
        "aud": pds,
        "scope": scopes,
        "refresh_token": refresh,
        "access_token": "access-token",
        "token_type": "DPoP",
        "expires_at": expires_at,
    })
}

/// Sign `session`'s account in on this computer, with the store at `root` (signing in makes the
/// folder a store first), as jacquard does at the end of a sign-in.
pub fn sign_in_as(root: &Path, session: Value) {
    locallibrary::Store::open_or_init(root).unwrap();
    let store = LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    rt.block_on(store.upsert_session(serde_json::from_value(session).unwrap())).unwrap();
}
