//! Signing in with an Atmosphere account (see [`crate::atmosphere`]).
//!
//! A GUI signs in in two steps: `account_login_begin` returns the URL to open in the browser,
//! then `account_login_finish` waits for the browser to come back (cancel it to stop waiting).
//! Account data lives in the store folder, which signing in creates if needed.

use std::path::Path;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::atmosphere::{self, LoginOptions};
pub use crate::atmosphere::{AccountInfo, Client, LoginStart, LogoutReport, Storage};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AccountLoginRequest {
    /// A handle (`alice.bsky.social`), a DID, or the account's server (`https://bsky.social`).
    /// Default: the handle last signed in with.
    #[serde(default)]
    pub handle: Option<String>,
    /// The URL of another client metadata document. Default: `$LOCALLIBRARY_OAUTH_CLIENT_ID`,
    /// else locallibrary's own (falling back to the development client while it isn't online).
    #[serde(default)]
    pub client_id: Option<String>,
    /// Sign in as the `http://localhost` development client.
    #[serde(default)]
    pub dev_client: bool,
    /// Keep the session in a file only you can read instead of the system keychain. Default:
    /// `$LOCALLIBRARY_SESSION_STORE=file`.
    #[serde(default)]
    pub session_file: Option<bool>,
    /// Scopes to ask for (default: `atproto`, which identifies you and nothing more).
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Also allow publishing: writing the My Settings, profile and set records to the account
    /// ([`atmosphere::PUBLISH_SCOPES`]), and nothing else.
    #[serde(default)]
    pub publish: bool,
}

/// Start signing in: returns the URL to open in the browser.
pub fn account_login_begin(store_root: &Path, req: &AccountLoginRequest) -> Result<LoginStart> {
    let store = Store::open_or_init(store_root)?;
    let handle = req
        .handle
        .clone()
        .filter(|h| !h.trim().is_empty())
        .or_else(|| atmosphere::last_handle(store.root()))
        .ok_or_else(|| {
            Error::invalid(
                "which Atmosphere account? Give a handle (alice.bsky.social) or a server (https://bsky.social)",
            )
        })?;
    let options = LoginOptions {
        client: match (&req.client_id, req.dev_client) {
            (_, true) => Some(Client::Loopback),
            (Some(id), false) if !id.trim().is_empty() => Some(Client::Hosted { client_id: id.trim().to_string() }),
            _ => Client::from_env(),
        },
        storage: match req.session_file {
            Some(true) => Storage::File,
            Some(false) => Storage::Keychain,
            None => Storage::from_env(),
        },
        scopes: if req.publish {
            let mut all = atmosphere::publish_scopes();
            for s in &req.scopes {
                if !all.contains(s) {
                    all.push(s.clone());
                }
            }
            all
        } else {
            req.scopes.clone()
        },
    };
    atmosphere::begin_login(store.root(), &handle, &options)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AccountLoginFinish {
    pub id: String,
    /// How long to wait for the browser (default 300).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Wait for the browser and finish signing in.
pub fn account_login_finish(req: &AccountLoginFinish, ctx: &Ctx<'_>) -> Result<AccountInfo> {
    let timeout = req.timeout_secs.map_or(atmosphere::LOGIN_TIMEOUT, Duration::from_secs);
    atmosphere::finish_login(&req.id, timeout, ctx)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AccountLoginPaste {
    pub id: String,
    /// The address the browser ended up on after approving.
    pub url: String,
}

/// Finish signing in with the redirect address pasted by hand (the browser ran elsewhere).
pub fn account_login_paste(req: &AccountLoginPaste) -> Result<AccountInfo> {
    atmosphere::finish_login_with_url(&req.id, &req.url)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AccountLoginCancel {
    pub id: String,
}

/// Forget a sign-in that won't be finished; false when there was none.
pub fn account_login_cancel(req: &AccountLoginCancel) -> bool {
    atmosphere::cancel_login(&req.id)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccountStatus {
    /// The signed-in account (`None`: not signed in).
    pub account: Option<AccountInfo>,
    /// The handle last signed in with, to offer next time.
    pub last_handle: Option<String>,
}

/// Who's signed in, as last saved (no network).
pub fn account_status(store_root: &Path) -> Result<AccountStatus> {
    Ok(AccountStatus { account: atmosphere::status(store_root)?, last_handle: atmosphere::last_handle(store_root) })
}

/// Ask the account's server who's signed in, refreshing the session if needed.
pub fn account_check(store_root: &Path, ctx: &Ctx<'_>) -> Result<AccountInfo> {
    atmosphere::check(store_root, ctx)
}

/// Sign out: revoke the session and remove it from this computer.
pub fn account_logout(store_root: &Path) -> Result<LogoutReport> {
    atmosphere::logout(store_root)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ClientMetadataRequest {
    /// Where the document will be hosted (an https:// URL; default: locallibrary's own,
    /// [`atmosphere::CLIENT_ID`]).
    #[serde(default = "published_client_id")]
    pub client_id: String,
    /// Scopes to allow besides `atproto` and the publish scopes, which every document allows.
    #[serde(default)]
    pub scopes: Vec<String>,
}

fn published_client_id() -> String {
    atmosphere::CLIENT_ID.to_string()
}

/// The client metadata document to host for a hosted client ID.
pub fn account_client_metadata(req: &ClientMetadataRequest) -> Result<serde_json::Value> {
    atmosphere::client_metadata(&req.client_id, &req.scopes)
}
