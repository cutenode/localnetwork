//! The record types locallibrary publishes to an Atmosphere account, and a check that a record
//! fits its type.
//!
//! Each type is a Lexicon document in the repo's `lexicons/`, in the folder its NSID names.
//! [`SETTINGS`] (My Settings, one per account) is locallibrary's own, under the authority
//! `locallibrary.localnetwork.computer`. [`PROFILE`] (the public profile, one per account) and
//! [`SET`] (a set, one per set) are shared by Local Network's tools, under the authority
//! `dj.localnetwork.computer`, with the definitions they share in [`DEFS`]. Documents they refer
//! to that others maintain, [`REFERENCED`] ([`ADDRESS`], where a set was played), are fetched from
//! their authority the first time a record needs one, and kept for the rest of the run.
//!
//! [`validate`] checks a record against its document before it's written. It's strict, so a field
//! the document doesn't define is an error, and it follows the AT Protocol's data model, which has
//! no fractional numbers. It covers the parts of Lexicon these documents use: objects, refs (within
//! a document and to another one), unions, strings, integers, booleans and arrays, and the string
//! formats `datetime`, `at-uri`, `did` and `uri`. The AT Protocol library's parsers check DIDs and
//! at:// URIs and chrono checks times; a `uri` only gets the light check the protocol's reference
//! implementation gives one (a scheme, then no spaces).

use rustc_hash::FxHashMap;
use std::sync::{LazyLock, PoisonError, RwLock};

use jacquard_common::SmolStr;
use jacquard_common::types::aturi::AtUri;
use jacquard_common::types::did::validate_did;
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

use crate::error::IoContext;
use crate::{Error, Result};

/// A DJ's My Settings (record key `self`).
pub const SETTINGS: &str = "computer.localnetwork.locallibrary.settings";
/// A DJ's public profile (record key `self`).
pub const PROFILE: &str = "computer.localnetwork.dj.profile";
/// A set: where and when it was played, and its tracklist (record key: a TID).
pub const SET: &str = "computer.localnetwork.dj.set";
/// What [`PROFILE`] and [`SET`] share (tracks, keys, partners, shares, counts). Not a record type.
pub const DEFS: &str = "computer.localnetwork.dj.defs";
/// A street address, from Lexicon Community: where a set was played. Not a record type.
pub const ADDRESS: &str = "community.lexicon.location.address";

/// Every record type locallibrary writes.
pub const COLLECTIONS: [&str; 3] = [SETTINGS, PROFILE, SET];

/// The Lexicon documents locallibrary writes records of, by NSID, and the ones they share.
pub const DOCUMENTS: [(&str, &str); 4] = [
    (SETTINGS, include_str!("../../../../lexicons/computer/localnetwork/locallibrary/settings.json")),
    (PROFILE, include_str!("../../../../lexicons/computer/localnetwork/dj/profile.json")),
    (SET, include_str!("../../../../lexicons/computer/localnetwork/dj/set.json")),
    (DEFS, include_str!("../../../../lexicons/computer/localnetwork/dj/defs.json")),
];

/// The documents others maintain that [`DOCUMENTS`] refer to, fetched as they publish them.
pub const REFERENCED: [&str; 1] = [ADDRESS];

static PARSED: LazyLock<FxHashMap<&'static str, Value>> = LazyLock::new(|| {
    DOCUMENTS
        .iter()
        .map(|(id, text)| (*id, serde_json::from_str(text).expect("the Lexicon documents are JSON")))
        .collect()
});

/// The [`REFERENCED`] documents fetched so far this run. Each is fetched once and kept for good,
/// so what's handed out lives as long as the process.
static FETCHED: LazyLock<RwLock<FxHashMap<&'static str, &'static Value>>> = LazyLock::new(Default::default);

/// The Lexicon document for `nsid`: one of [`DOCUMENTS`], or one of [`REFERENCED`] fetched already.
pub fn document(nsid: &str) -> Option<&'static Value> {
    PARSED.get(nsid).or_else(|| FETCHED.read().unwrap_or_else(PoisonError::into_inner).get(nsid).copied())
}

/// Keeps `doc` as the [`REFERENCED`] document `nsid` for the rest of the run, unless one is kept
/// already, and returns the one kept.
pub(crate) fn keep(nsid: &'static str, doc: Value) -> &'static Value {
    let mut fetched = FETCHED.write().unwrap_or_else(PoisonError::into_inner);
    fetched.entry(nsid).or_insert_with(|| Box::leak(Box::new(doc)))
}

/// One of [`REFERENCED`] as its authority publishes it now; or, while `$LOCALLIBRARY_LEXICONS`
/// names a folder (a test switch, so tests don't reach the network), as `<nsid>.json` there has it.
fn fetch(nsid: &str) -> Result<Value> {
    let doc = match std::env::var_os("LOCALLIBRARY_LEXICONS") {
        None => super::fetch_lexicon(nsid)?,
        Some(dir) => {
            let path = std::path::Path::new(&dir).join(format!("{nsid}.json"));
            let text = std::fs::read_to_string(&path).at(&path)?;
            serde_json::from_str(&text).map_err(|e| Error::Parse { path, message: e.to_string() })?
        }
    };
    as_document(nsid, doc)
}

/// `doc`, fetched as the document `nsid`, if it is one: its `id` is `nsid`, and it has definitions.
fn as_document(nsid: &str, doc: Value) -> Result<Value> {
    if doc["id"] != nsid || !doc["defs"].is_object() {
        return Err(Error::invalid(format!("what was fetched as the Lexicon {nsid} isn't that document")));
    }
    Ok(doc)
}

/// The definition `name` in the document `doc`, fetching `doc` if it's one of [`REFERENCED`] not
/// fetched yet. `r` is the ref that named it, and `path` where in the record it was met.
fn resolve(doc: &str, name: &str, r: &str, path: &str) -> std::result::Result<&'static Value, String> {
    let fetched = match (document(doc), REFERENCED.iter().find(|id| **id == doc)) {
        (Some(found), _) => found,
        (None, Some(id)) => keep(id, fetch(id).map_err(|e| format!("{}: {e}", what(path)))?),
        (None, None) => return Err(format!("{}: unknown ref {r}", what(path))),
    };
    fetched["defs"].get(name).ok_or_else(|| format!("{}: unknown ref {r}", what(path)))
}

/// The last part of an NSID: `settings` for [`SETTINGS`].
pub fn short_name(nsid: &str) -> &str {
    nsid.rsplit('.').next().unwrap_or(nsid)
}

fn def(nsid: &str, name: &str) -> Option<&'static Value> {
    document(nsid)?.get("defs")?.get(name)
}

/// Check `record` (with its `$type`) against the record type `nsid`.
pub fn validate(nsid: &str, record: &Value) -> Result<()> {
    let bad = |why: String| Error::invalid(format!("the {} record doesn't fit its Lexicon: {why}", short_name(nsid)));
    let main = def(nsid, "main")
        .filter(|d| d["type"] == "record")
        .ok_or_else(|| Error::invalid(format!("{nsid} isn't one of locallibrary's record types")))?;
    let Value::Object(fields) = record else { return Err(bad("it isn't an object".into())) };
    if fields.get("$type").and_then(Value::as_str) != Some(nsid) {
        return Err(bad(format!("its $type isn't {nsid}")));
    }
    let mut rest = fields.clone();
    rest.remove("$type");
    check(nsid, &main["record"], &Value::Object(rest), "").map_err(bad)
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() { key.to_string() } else { format!("{path}.{key}") }
}

fn what(path: &str) -> &str {
    if path.is_empty() { "the record" } else { path }
}

/// The document and definition a ref in the document `nsid` points to: `#name` is local,
/// `other.nsid#name` another document's, and a bare NSID its `main`.
fn target<'a>(nsid: &'a str, r: &'a str) -> (&'a str, &'a str) {
    match r.split_once('#') {
        Some(("", name)) => (nsid, name),
        Some((doc, name)) => (doc, name),
        None => (r, "main"),
    }
}

/// A `uri`, checked as lightly as the AT Protocol's reference implementation checks one: a scheme
/// (letters, digits, `_`), `:`, maybe `//`, then text without spaces that doesn't start with `/`.
/// (An at:// URI naming a DID isn't a URI to a strict RFC 3986 parser: it reads `did:plc:...` as a
/// host and a port.)
fn is_uri(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once(':') else { return false };
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    !scheme.is_empty()
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && rest.chars().next().is_some_and(|c| c != '/')
        && !rest.contains(char::is_whitespace)
}

/// How many refs deep a value may be checked: far more than these documents nest, and a stop for
/// a fetched document whose refs go round in a circle.
const MAX_REFS: u32 = 32;

/// Check `v` against `schema`, a definition in the document `nsid` (local refs resolve there).
fn check(nsid: &str, schema: &Value, v: &Value, path: &str) -> std::result::Result<(), String> {
    walk(nsid, schema, v, path, 0)
}

/// [`check`], `refs` refs deep already.
fn walk(nsid: &str, schema: &Value, v: &Value, path: &str, refs: u32) -> std::result::Result<(), String> {
    let num = |key: &str| schema[key].as_i64();
    let deeper = || {
        let refs = refs + 1;
        if refs > MAX_REFS { Err(format!("{}: refs nest more than {MAX_REFS} deep", what(path))) } else { Ok(refs) }
    };
    match schema["type"].as_str().unwrap_or_default() {
        "ref" => {
            let r = schema["ref"].as_str().unwrap_or_default();
            let (doc, name) = target(nsid, r);
            let found = resolve(doc, name, r, path)?;
            walk(doc, found, v, path, deeper()?)
        }
        // A union's value names its type in `$type`: `nsid` for a document's main definition,
        // `nsid#name` for another. A type the union doesn't list passes unless it's closed.
        "union" => {
            let Value::Object(fields) = v else { return Err(format!("{} should be an object", what(path))) };
            let Some(ty) = fields.get("$type").and_then(Value::as_str) else {
                return Err(format!("{} is missing $type", what(path)));
            };
            let listed = schema["refs"].as_array().into_iter().flatten().filter_map(Value::as_str).find(|r| {
                let (doc, name) = target(nsid, r);
                if name == "main" { ty == doc } else { ty.split_once('#') == Some((doc, name)) }
            });
            let Some(r) = listed else {
                return match schema["closed"].as_bool() {
                    Some(true) => Err(format!("{} can't be a {ty}", what(path))),
                    _ => Ok(()),
                };
            };
            let (doc, name) = target(nsid, r);
            let found = resolve(doc, name, r, path)?;
            let mut rest = fields.clone();
            rest.remove("$type");
            walk(doc, found, &Value::Object(rest), path, deeper()?)
        }
        "object" => {
            let Value::Object(fields) = v else { return Err(format!("{} should be an object", what(path))) };
            for key in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if !fields.contains_key(key) {
                    return Err(format!("{} is missing {key}", what(path)));
                }
            }
            let nullable = schema["nullable"].as_array();
            for (key, x) in fields {
                let at = join(path, key);
                let Some(property) = schema["properties"].get(key) else {
                    return Err(format!("{at} isn't in the Lexicon"));
                };
                if x.is_null() && nullable.is_some_and(|n| n.iter().any(|k| k == key.as_str())) {
                    continue;
                }
                walk(nsid, property, x, &at, refs)?;
            }
            Ok(())
        }
        "string" => {
            let Some(s) = v.as_str() else { return Err(format!("{} should be text", what(path))) };
            let bytes = s.len() as i64;
            if num("maxLength").is_some_and(|m| bytes > m) || num("minLength").is_some_and(|m| bytes < m) {
                return Err(format!("{} is {bytes} bytes long, outside what the Lexicon allows", what(path)));
            }
            // Lexicon counts extended grapheme clusters, what a reader sees as one character: a
            // flag or an emoji joined with ZWJ is one, though it's several code points.
            if let Some(max) = num("maxGraphemes")
                && s.graphemes(true).count() as i64 > max
            {
                return Err(format!("{} is longer than {max} graphemes", what(path)));
            }
            if let Some(allowed) = schema["enum"].as_array()
                && !allowed.iter().any(|a| a == s)
            {
                return Err(format!("{} can't be {s:?}", what(path)));
            }
            let Some(format) = schema["format"].as_str() else { return Ok(()) };
            let (fits, called) = match format {
                "datetime" => (chrono::DateTime::parse_from_rfc3339(s).is_ok(), "a date and time"),
                "at-uri" => (AtUri::<SmolStr>::new_owned(s).is_ok(), "an at:// URI"),
                "did" => (validate_did(s).is_ok(), "a DID"),
                "uri" => (is_uri(s), "a URI"),
                other => return Err(format!("{}: the check doesn't know the format {other:?}", what(path))),
            };
            match fits {
                true => Ok(()),
                false => Err(format!("{} isn't {called}: {s:?}", what(path))),
            }
        }
        "integer" => {
            let Some(i) = v.as_i64() else { return Err(format!("{} should be a whole number, not {v}", what(path))) };
            if num("minimum").is_some_and(|m| i < m) || num("maximum").is_some_and(|m| i > m) {
                return Err(format!("{} is {i}, outside what the Lexicon allows", what(path)));
            }
            Ok(())
        }
        "boolean" if v.is_boolean() => Ok(()),
        "boolean" => Err(format!("{} should be true or false", what(path))),
        "array" => {
            let Some(items) = v.as_array() else { return Err(format!("{} should be a list", what(path))) };
            let n = items.len() as i64;
            if num("maxLength").is_some_and(|m| n > m) || num("minLength").is_some_and(|m| n < m) {
                return Err(format!("{} has {n} entries, outside what the Lexicon allows", what(path)));
            }
            for (i, x) in items.iter().enumerate() {
                walk(nsid, &schema["items"], x, &format!("{}[{i}]", what(path)), refs)?;
            }
            Ok(())
        }
        "unknown" if v.is_object() => Ok(()),
        "unknown" => Err(format!("{} should be an object", what(path))),
        other => Err(format!("{}: the check doesn't know the Lexicon type {other:?}", what(path))),
    }
}

#[cfg(test)]
mod tests;
