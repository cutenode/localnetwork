//! Feature `atmosphere`: writing a record in the Atmosphere account signed in to the host, of a
//! record type (a collection, by NSID) the plugin's `atmosphere:write:<nsid>` capabilities grant.
//! The host checks the record against its Lexicon (its own for the record types it knows, else
//! the one the manifest ships in `lexicons`), shows it to the user and writes it only once they
//! confirm, with the session's scopes. The plugin never sees the session. Uploading files to the
//! account isn't part of it.

use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::invalid;
use crate::{Capability, Feature, Manifest, ManifestError};

/// Plugin to host (feature `atmosphere`): [`PutRecord`] -> [`crate::StrongRef`].
pub const ATMOSPHERE_PUT_RECORD: &str = "atmosphere/put_record";

/// Local Network's authority: its record types are locallibrary's own, which only plugins it
/// ships may write.
const LOCAL_NETWORK: &str = "computer.localnetwork.";

/// The AT Protocol's own record types (a published Lexicon, `com.atproto.lexicon.schema`), which
/// no plugin writes.
const PROTOCOL: &str = "com.atproto.";

// Every plugin written with the SDK has it: what's written is the host's to check and the user's
// to confirm.
pub(crate) const FEATURE: Feature =
    Feature { name: "atmosphere", methods: &[], sends: &[ATMOSPHERE_PUT_RECORD], contributed: writes };

/// Whether the manifest asks to write records.
fn writes(m: &Manifest) -> bool {
    m.capabilities.iter().any(|c| matches!(c, Capability::AtmosphereWrite(_)))
}

/// `atmosphere/put_record`: write `record` (with its `$type`, the collection) in the account
/// signed in to the host. With `rkey`, at that record key: replacing the version whose CID is
/// `swap`, or without one creating it (it fails when there's a record there). Without `rkey`,
/// under a key the account's server picks. The host answers the record's at:// URI and CID once
/// the user confirms; it fails with kind `denied` for a collection the manifest doesn't grant,
/// `invalid` for a record that doesn't fit its Lexicon (the message says where) or a write that
/// can't be confirmed here, `cancelled` when the user declines, `conflict` when `swap` isn't the
/// version there, and `account` when nobody's signed in or the session may not write the
/// collection (passed on, it says how to sign in again allowing it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PutRecord {
    /// The record type, by NSID (`fm.plyr.track`).
    pub collection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rkey: Option<String>,
    pub record: Value,
    /// The CID of the record replaced, with `rkey`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swap: Option<String>,
}

/// Whether `s` is an NSID (`fm.plyr.track`), as the AT Protocol's spec has one: a domain
/// authority of two segments or more, the first starting with a letter, each 1 to 63 letters,
/// digits and `-` that don't start or end with `-`; then a name of 1 to 63 letters and digits
/// starting with a letter; 317 characters at most. The authority is in lowercase, as it's
/// compared: a capability can't name one record type two ways.
pub fn is_nsid(s: &str) -> bool {
    let Some((authority, name)) = s.rsplit_once('.') else { return false };
    let segment = |seg: &str| {
        (1..=63).contains(&seg.len())
            && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !seg.starts_with('-')
            && !seg.ends_with('-')
    };
    s.len() <= 317
        && authority.starts_with(|c: char| c.is_ascii_lowercase())
        && authority.contains('.')
        && authority.split('.').all(segment)
        && name.len() <= 63
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric())
}

/// `atmosphere:write:` and `lexicons`: no plugin writes the AT Protocol's own record types, only
/// plugins locallibrary ships write Local Network's (with locallibrary's Lexicons), and any other
/// has its Lexicon in `lexicons`, a record type. Each Lexicon there is a Lexicon document (`lexicon
/// = 1`, an NSID for its `id`, `defs`), each id once.
pub(crate) fn validate(m: &Manifest, built_in: bool) -> Result<(), ManifestError> {
    let mut ids = FxHashSet::default();
    for (i, doc) in m.lexicons.iter().enumerate() {
        let at = format!("lexicons[{i}]");
        let id = doc["id"].as_str().unwrap_or_default();
        if doc["lexicon"] != 1 || !doc["defs"].is_object() {
            return Err(invalid(at, "isn't a Lexicon document: lexicon = 1, an id and defs"));
        }
        if !is_nsid(id) {
            return Err(invalid(format!("{at}.id"), format!("{id:?} isn't an NSID, like \"fm.plyr.track\"")));
        }
        if !ids.insert(id) {
            return Err(invalid(format!("{at}.id"), format!("{id:?} is there twice")));
        }
    }
    for (i, c) in m.capabilities.iter().enumerate() {
        let Capability::AtmosphereWrite(nsid) = c else { continue };
        let at = format!("capabilities[{i}]");
        if nsid.starts_with(PROTOCOL) {
            return Err(invalid(at, "the AT Protocol's own records aren't a plugin's to write"));
        }
        if nsid.starts_with(LOCAL_NETWORK) {
            if !built_in {
                return Err(invalid(at, "only plugins locallibrary ships write Local Network's records"));
            }
            continue;
        }
        if m.lexicon(nsid).is_none_or(|doc| doc["defs"]["main"]["type"] != "record") {
            return Err(invalid(at, format!("{c} needs its record type's Lexicon in lexicons")));
        }
    }
    Ok(())
}
