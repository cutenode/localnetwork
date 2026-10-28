//! Local Network's directory: the HappyView AppView that every record locallibrary reads comes
//! from, never an account's data server or Bluesky's AppView. Writes still go to the account's
//! data server ([`super::repo`]).
//!
//! HappyView learns of records from the network's firehose as they're written. Records an account
//! published before HappyView kept their collection aren't there until it reads the account's
//! repository again, which only Local Network's website can have it do: [`request_index`] asks it
//! to, once per account, with a token the account's server signs for that one method (atproto
//! inter-service auth, which [`super::PUBLISH_SET`] allows).

use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

use jacquard_api::com_atproto::server::get_service_auth::GetServiceAuth;
use jacquard_common::SmolStr;
use jacquard_common::types::nsid::Nsid;
use jacquard_common::xrpc::XrpcClient;
use jacquard_oauth::scopes::Scope;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::store::{self, AccountFile};
use super::{AccountInfo, block_on, grants, net, restore};
use crate::atmosphere::repo::at_uri;
use crate::error::Reason;
use crate::export::one_line;
use crate::{Error, Result};

/// Local Network's HappyView, which serves the directory's queries.
pub const HAPPYVIEW_URL: &str = "https://happyview.localnetwork.computer";

/// The environment variable that names another HappyView (a development or test one).
pub const HAPPYVIEW_ENV: &str = "LOCALLIBRARY_HAPPYVIEW_URL";

/// Local Network's website, which has HappyView read an account's repository again.
pub const WEBSITE_URL: &str = "https://localnetwork.computer";

/// The environment variable that names another website (a development or test one).
pub const WEBSITE_ENV: &str = "LOCALLIBRARY_WEBSITE_URL";

/// The website's method that has HappyView read the caller's locallibrary records again.
pub const REQUEST_INDEX: &str = "computer.localnetwork.directory.requestIndex";

const GET_RECORDS: &str = "computer.localnetwork.directory.getRecords";

thread_local! {
    /// Where this thread finds both of them instead (`testing::directory_on_this_thread`).
    pub(crate) static THREAD_DIRECTORY: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// `env`'s URL, else `default`, unless this thread names its own.
fn url(env: &str, default: &str) -> String {
    let url = THREAD_DIRECTORY
        .with(|d| d.borrow().clone())
        .or_else(|| std::env::var(env).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| default.to_string());
    url.trim().trim_end_matches('/').to_string()
}

fn happyview() -> String {
    url(HAPPYVIEW_ENV, HAPPYVIEW_URL)
}

fn website() -> String {
    url(WEBSITE_ENV, WEBSITE_URL)
}

fn host(url: &str) -> &str {
    url.split("://").nth(1).unwrap_or(url)
}

/// A record as the directory has it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Fetched {
    pub uri: String,
    /// The version the directory has.
    pub cid: String,
    /// The record as its collection's record script kept it, which may leave fields out and add
    /// its own.
    pub value: Value,
}

/// `collection/rkey` in `did`'s repository, as Local Network's directory has it. `None` when it
/// has no such record: there's none, or it was published before the directory kept it and the
/// directory hasn't read it since.
pub fn get_record(did: &str, collection: &str, rkey: &str) -> Result<Option<Fetched>> {
    let base = happyview();
    let uri = at_uri(did, collection, rkey);
    let unreachable =
        |e: net::NetError| Error::Account(one_line(&format!("couldn't reach Local Network's directory ({e})")));
    block_on(async {
        let http = net::client().timeout(Duration::from_secs(20)).build().map_err(|e| unreachable(e.into()))?;
        let response = http
            .get(format!("{base}/xrpc/{GET_RECORDS}"))
            .query(&[("uris", &uri)])
            .send()
            .await
            .map_err(|e| unreachable(e.into()))?;
        let status = response.status();
        let bytes = net::body(response).await.map_err(unreachable)?;
        let body: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        if let Some(records) = body["records"].as_array().filter(|_| status.is_success()) {
            // A record without its version can't be replaced safely (a write names the version it
            // replaces), so it isn't one.
            let found = records.iter().find(|r| r["uri"] == uri.as_str() && r["value"].is_object());
            return Ok(found.and_then(|r| {
                Some(Fetched { uri: uri.clone(), cid: one_line(r["cid"].as_str()?), value: r["value"].clone() })
            }));
        }
        let why = match (body["error"].as_str(), body["message"].as_str()) {
            (Some(e), Some(m)) => format!("{e}: {m}"),
            (Some(e), None) => e.to_string(),
            _ => status.to_string(),
        };
        Err(Error::Account(one_line(&format!(
            "Local Network's directory at {} couldn't give {uri} ({why})",
            host(&base)
        ))))
    })
}

/// The did:web the website at `url` goes by (`did:web:localnetwork.computer`), which service
/// tokens for it name as their audience.
fn service_did(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("did:web:{host}%3A{port}"),
        None => format!("did:web:{host}"),
    })
}

/// Have Local Network's directory read what `info`'s account published before it kept
/// locallibrary's records, so a plan that reads them back sees them. Asked once per account
/// (remembered in `atmosphere.json`); once it's done, the firehose brings every later write.
/// `Ok(None)` once it's done; otherwise a note on why it couldn't be, for the plan, which reads
/// what the directory has anyway. A session that has ended is an error, as writing would be.
pub fn request_index(root: &Path, info: &AccountInfo) -> Result<Option<String>> {
    let note = |why: &str| {
        Ok(Some(format!("Local Network's directory couldn't be asked to read this account's earlier records ({why})")))
    };
    if AccountFile::load(root)?.indexed.contains(&info.did) {
        return Ok(None);
    }
    let website = website();
    let Some(aud) = service_did(&website) else { return note(&format!("{website:?} isn't a URL")) };
    if !grants(info, [Scope::rpc(REQUEST_INDEX)]) {
        return note("the session doesn't allow it: sign in again, allowing publishing");
    }
    let token = match service_token(root, &aud) {
        Ok(token) => token,
        Err(e) if e.reason() == Some(Reason::SessionEnded) => return Err(e),
        Err(e) => return note(&e.to_string()),
    };
    match ask(&website, &token) {
        Ok(true) => {}
        Ok(false) => return note("it's still reading them"),
        Err(why) => return note(&why),
    }
    let _lock = store::lock(root)?;
    let mut file = AccountFile::load(root)?;
    if !file.indexed.contains(&info.did) {
        file.indexed.push(info.did.clone());
    }
    file.save(root)?;
    Ok(None)
}

/// A token from the signed-in account's server for [`REQUEST_INDEX`] at the service `aud`.
fn service_token(root: &Path, aud: &str) -> Result<String> {
    let _lock = store::lock(root)?;
    let session = restore(root)?;
    let lxm = Nsid::new_static(REQUEST_INDEX).expect("REQUEST_INDEX is an NSID");
    let request = GetServiceAuth { aud: SmolStr::new(aud), exp: None, lxm: Some(lxm) };
    let refused = |e: &dyn std::fmt::Display| Error::Account(one_line(&format!("the account's server: {e}")));
    let response = block_on(session.send(request)).map_err(|e| refused(&e))?;
    Ok(response.into_output().map_err(|e| refused(&e))?.token.to_string())
}

/// Ask the website at `website` to have the directory read the account `token` is from again:
/// whether it has.
fn ask(website: &str, token: &str) -> std::result::Result<bool, String> {
    block_on(async {
        let http = net::client().timeout(Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
        let response = http
            .post(format!("{website}/xrpc/{REQUEST_INDEX}"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| net::with_causes(&e))?;
        let status = response.status();
        let bytes = net::body(response).await.map_err(|e| e.to_string())?;
        let body: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        if status.is_success() {
            return Ok(body["indexed"] == true);
        }
        Err(one_line(&format!("{} answered {}", host(website), body["error"].as_str().unwrap_or(status.as_str()))))
    })
}

#[cfg(test)]
mod tests;
