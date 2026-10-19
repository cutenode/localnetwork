//! Bug reports, posted from the signed-in Atmosphere account to Local Network's space on
//! userinput.app: a [`lexicon::DISCUSSION`] record in the account's own repository that points at
//! the space (its at:// URI and the CID of the version it was posted to) and is filed under
//! [`TAGS`]. Creating it needs [`REPORT_SCOPE`](super::REPORT_SCOPE); userinput.app shows it at
//! [`link`].

use std::path::Path;

use chrono::{DateTime, Utc};
use jacquard_common::SmolStr;
use jacquard_common::types::aturi::AtUri;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;

use super::repo::{self, Written};
use super::{AccountInfo, lexicon};
use crate::export::one_line;
use crate::{Error, Result};

/// Local Network's space on userinput.app, where bug reports are posted, as
/// `at://<owner DID>/app.userinput.space/<record key>`. `None` while there's none: reports then go
/// to GitHub. [`SPACE_ENV`] names another.
pub const SPACE: Option<&str> = None;

/// The environment variable that names another space.
pub const SPACE_ENV: &str = "LOCALLIBRARY_USERINPUT_SPACE";

/// A space's record type.
pub const SPACE_COLLECTION: &str = "app.userinput.space";

/// What a bug report is filed under: values from the space's own list of tags.
pub const TAGS: [&str; 2] = ["bug", "locallibrary"];

/// A space on userinput.app: a record in its owner's repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Space {
    pub did: String,
    pub rkey: String,
}

impl Space {
    /// `at://<DID>/app.userinput.space/<record key>`, and nothing else.
    pub fn parse(uri: &str) -> Result<Space> {
        let wrong = || {
            Error::invalid(format!(
                "{:?} isn't a userinput.app space: give at://<DID>/{SPACE_COLLECTION}/<record key>",
                one_line(uri)
            ))
        };
        let parsed = AtUri::<SmolStr>::new_owned(uri.trim()).map_err(|_| wrong())?;
        let did = parsed.authority().as_str().to_string();
        let is_space = parsed.collection().is_some_and(|c| c.as_str() == SPACE_COLLECTION);
        match parsed.rkey() {
            Some(rkey) if did.starts_with("did:") && is_space => Ok(Space { did, rkey: rkey.as_ref().to_string() }),
            _ => Err(wrong()),
        }
    }

    pub fn uri(&self) -> String {
        repo::at_uri(&self.did, SPACE_COLLECTION, &self.rkey)
    }
}

/// The space bug reports go to: [`SPACE_ENV`]'s, else [`SPACE`]; `None` when there's none. An
/// error when the one named isn't a space's at:// URI.
pub fn space() -> Result<Option<Space>> {
    space_from(std::env::var(SPACE_ENV).ok().as_deref())
}

fn space_from(env: Option<&str>) -> Result<Option<Space>> {
    env.filter(|v| !v.trim().is_empty()).or(SPACE).map(Space::parse).transpose()
}

/// `Ok` when `title` and `body` fit a discussion, otherwise what's too long. `body` counts as
/// posted, with anything added to what was typed.
pub fn check(title: &str, body: &str) -> Result<()> {
    if title.trim().is_empty() {
        return Err(Error::invalid("a bug report needs a title"));
    }
    fits("title", "title", title)?;
    fits("body", "description", body)
}

/// `text` within the discussion Lexicon's limits for `field`: characters as readers see them
/// (graphemes), and bytes.
fn fits(field: &str, what: &str, text: &str) -> Result<()> {
    let limits =
        &lexicon::document(lexicon::DISCUSSION).expect("embedded")["defs"]["main"]["record"]["properties"][field];
    let (most, bytes) = (limits["maxGraphemes"].as_u64().unwrap_or(u64::MAX), limits["maxLength"].as_u64());
    let n = text.graphemes(true).count() as u64;
    if n > most {
        return Err(Error::invalid(format!("the {what} is too long to post: {n} characters, {most} at most")));
    }
    match bytes {
        Some(max) if text.len() as u64 > max => Err(Error::invalid(format!(
            "the {what} is too long to post: {} bytes of text, {max} at most (some characters take several)",
            text.len()
        ))),
        _ => Ok(()),
    }
}

/// The record a report is: posted to `space` (the version with `cid`) at `now`.
pub fn record(space: &Space, cid: &str, title: &str, body: &str, now: DateTime<Utc>) -> Value {
    let mut r = json!({
        "$type": lexicon::DISCUSSION,
        "space": { "uri": space.uri(), "cid": cid },
        "title": title,
        "tags": TAGS,
        "createdAt": crate::profile::timestamp(now),
    });
    if !body.is_empty() {
        r["body"] = body.into();
    }
    r
}

/// A report posted: the record, and where userinput.app shows it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Posted {
    pub uri: String,
    pub cid: String,
    pub url: String,
}

/// Where userinput.app shows the discussion `did` posted with the key `rkey`.
pub fn link(did: &str, rkey: &str) -> String {
    format!("https://userinput.app/d/{did}/{rkey}")
}

/// Post a report titled `title` saying `body` to `space`, from `account`, which has to be the
/// account signed in.
pub fn post(root: &Path, account: &AccountInfo, space: &Space, title: &str, body: &str) -> Result<Posted> {
    check(title, body)?;
    let cid = space_cid(account, space)?;
    let record = record(space, &cid, title, body, Utc::now());
    let Written { uri, cid } = repo::create_record(root, &account.did, lexicon::DISCUSSION, &record)?;
    let rkey = uri.rsplit('/').next().unwrap_or_default();
    Ok(Posted { url: link(&account.did, rkey), uri, cid })
}

/// The CID of `space` as its owner's data server has it now: the version a report points at.
/// The signed-in account's data server is known; anyone else's is found from their DID, and
/// only at a public https address.
fn space_cid(account: &AccountInfo, space: &Space) -> Result<String> {
    let pds = if space.did == account.did { account.pds.clone() } else { repo::resolve(&space.did)?.pds };
    let found = repo::get_record(&pds, &space.did, SPACE_COLLECTION, &space.rkey)?
        .ok_or_else(|| Error::NotFound(format!("the space bug reports go to ({})", space.uri())))?;
    found
        .cid
        .ok_or_else(|| Error::Account(format!("the space's server didn't say which version it has ({})", space.uri())))
}

#[cfg(test)]
mod tests;
