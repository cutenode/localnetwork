//! Records a plugin writes in the Atmosphere account (the `atmosphere` feature,
//! `atmosphere/put_record`), of the record types its `atmosphere:write:<nsid>` capabilities name.
//! Before the user is asked anything, the record is checked against its Lexicon (locallibrary's
//! own for the types it knows, else the one the manifest ships), its key and the version it
//! replaces are checked, and the session must be allowed to write the type. Then the front end
//! shows the user the record and asks (see [`super::AskAccess::write_record`]), one at a time, and
//! it's written with the session (`atmosphere::repo`) only once they confirm; nobody to
//! ask, it isn't. A record is replaced only where the plugin names the version it replaces. The
//! plugin never sees the session, and its errors say how to sign in again allowing the type.

#[cfg(feature = "account")]
use std::sync::Mutex;

#[cfg(feature = "account")]
use jacquard_common::SmolStr;
#[cfg(feature = "account")]
use jacquard_common::types::cid::Cid;
#[cfg(feature = "account")]
use jacquard_common::types::recordkey::Rkey;
#[cfg(feature = "account")]
use loclib_plugin::{Capability, ErrorData, Manifest, PutRecord, RpcError, StrongRef, params};
use serde_json::Value;

#[cfg(feature = "account")]
use super::callbacks::{for_plugin, known_roots};
#[cfg(feature = "account")]
use super::process::{Live, Plugin};
#[cfg(feature = "account")]
use crate::api::RpcFailure;
#[cfg(feature = "account")]
use crate::atmosphere::{self, lexicon, repo};
#[cfg(feature = "account")]
use crate::error::Reason;
#[cfg(feature = "account")]
use crate::store::Store;
#[cfg(feature = "account")]
use crate::{Error, lock};

/// A record a plugin asks to write in the Atmosphere account, for the user to confirm.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordWrite {
    /// The plugin, as it's shown.
    pub plugin: String,
    /// The account signed in: `@handle`, or its DID.
    pub account: String,
    /// The record type, by NSID.
    pub collection: String,
    /// Its record key; `None`: one the account's server picks.
    pub rkey: Option<String>,
    /// The CID of the record it replaces; `None`: it creates one.
    pub swap: Option<String>,
    /// The record, with its `$type`, as it's written.
    pub record: Value,
}

impl RecordWrite {
    /// What's asked, in a line.
    pub fn summary(&self) -> String {
        let (c, account) = (&self.collection, &self.account);
        let what = match (&self.rkey, &self.swap) {
            (Some(rkey), Some(_)) => format!("replace its {c} record {rkey}"),
            (Some(rkey), None) => format!("create the {c} record {rkey}"),
            (None, _) => format!("create a {c} record"),
        };
        format!("{} wants to {what} in the Atmosphere account {account}, where anyone can read it", self.plugin)
    }

    /// The record as JSON over several lines, each control character and bidirectional text mark
    /// in it written as an escape (`\u001b`), so it can't move the cursor or reorder the text
    /// around it where it's shown.
    pub fn shown(&self) -> String {
        let json = serde_json::to_string_pretty(&self.record).expect("a record is JSON");
        let mut out = String::with_capacity(json.len());
        for c in json.chars() {
            let bidi = matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
            match c {
                '\n' => out.push('\n'),
                c if c.is_control() || bidi => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out
    }
}

/// Held while the user is asked about a record: one question at a time, whichever plugin asks.
#[cfg(feature = "account")]
static ASKING: Mutex<()> = Mutex::new(());

#[cfg(feature = "account")]
impl Plugin {
    /// Answers `atmosphere/put_record` (`asked`, a [`PutRecord`]) with the record's
    /// [`StrongRef`], as the module's docs say. An error about the account or the record says
    /// which record type (`fields.collection`).
    pub(crate) fn put_record(&self, live: &Live, asked: Value) -> Result<Value, RpcError> {
        let req: PutRecord = params(asked)?;
        let c = req.collection.as_str();
        if !self.manifest.capabilities.contains(&Capability::AtmosphereWrite(c.into())) {
            let id = &self.manifest.id;
            let e = format!(
                "writing {c} records needs the atmosphere:write:{c} capability, which the {id} plugin doesn't have"
            );
            return Err(RpcError::failed("denied", e));
        }
        if let Some(rkey) = req.rkey.as_deref().filter(|k| Rkey::<SmolStr>::new_owned(k).is_err()) {
            return Err(RpcError::invalid_params(format!("{rkey:?} isn't a record key")));
        }
        match (&req.rkey, &req.swap) {
            (None, Some(_)) => return Err(RpcError::invalid_params("a record replaced is named by its rkey")),
            (_, Some(swap)) if !Cid::str(swap).is_valid() => {
                return Err(RpcError::invalid_params(format!("{swap:?} isn't a CID")));
            }
            _ => {}
        }
        let roots = known_roots(live.store.as_ref(), &self.given());
        let failed = |e: Error| for_plugin(RpcFailure::from(e.with("collection", c)), &roots);
        lexicon::validate_with(&self.manifest.lexicons, c, &req.record).map_err(failed)?;
        let root = live.store.as_ref().map(Store::root).ok_or_else(|| failed(atmosphere::not_signed_in()))?;
        let info = atmosphere::status(root).map_err(failed)?.ok_or_else(|| failed(atmosphere::not_signed_in()))?;
        let may = match req.rkey {
            Some(_) => atmosphere::missing_scope(&info, c).is_none(),
            None => atmosphere::may_create(&info, c),
        };
        if !may {
            return Err(failed(repo::scope_error(&info, c)));
        }
        let write = RecordWrite {
            plugin: self.manifest.name.clone(),
            account: info.name(),
            collection: c.into(),
            rkey: req.rkey.clone(),
            swap: req.swap.clone(),
            record: req.record,
        };
        let answer = {
            let _one = lock(&ASKING);
            self.host.options.ask.as_ref().and_then(|ask| ask.write_record(&write))
        };
        match answer {
            Some(true) => {}
            Some(false) => return Err(RpcError::cancelled()),
            None => {
                let e = "writing a record in the Atmosphere account needs the user to confirm it, which can't be asked here";
                return Err(RpcError::failed("invalid", e));
            }
        }
        let written = match &write.rkey {
            Some(rkey) => repo::put_checked(root, &info.did, c, rkey, &write.record, write.swap.as_deref()),
            None => repo::create_checked(root, &info.did, c, &write.record),
        };
        let written = written.map_err(failed)?;
        Ok(serde_json::to_value(StrongRef { uri: written.uri, cid: written.cid }).expect("an answer is JSON"))
    }
}

/// The error a plugin's hook failed with when it passes on an `account` error `atmosphere/put_record`
/// answered (`data`) about a record type it writes (`fields.collection`): locallibrary's own,
/// saying it as `said`, offering to sign in again allowing that type, or to sign in. `None` for any
/// other `account` error (a sign-in that couldn't listen for the browser, say), which is the plugin
/// failing as before.
#[cfg(feature = "account")]
pub(super) fn account_error(
    manifest: &Manifest,
    store: Option<&Store>,
    data: Option<&ErrorData>,
    said: &str,
) -> Option<Error> {
    let data = data?;
    let reason = serde_json::from_value::<Reason>(data.reason.clone()?.into()).ok()?;
    if !matches!(reason, Reason::NotSignedIn | Reason::SessionEnded | Reason::MissingScope) {
        return None;
    }
    let c = data.fields.get("collection")?.as_str()?;
    if !manifest.capabilities.contains(&Capability::AtmosphereWrite(c.to_string())) {
        return None;
    }
    let again = match store.and_then(|s| atmosphere::status(s.root()).ok().flatten()) {
        Some(info) => repo::sign_in_again(&info, c),
        None => atmosphere::sign_in(),
    };
    Some(Error::Account(said.to_string()).because(reason).action(again))
}
