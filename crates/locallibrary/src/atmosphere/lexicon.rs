//! The record types locallibrary publishes to an Atmosphere account, and a check that a record
//! fits its type.
//!
//! Each type is a Lexicon document in the repo's `lexicons/`, in the folder its NSID names.
//! [`SETTINGS`] (My Settings, one per account) is locallibrary's own, under the authority
//! `locallibrary.localnetwork.computer`. [`PROFILE`] (the public profile, one per account), [`SET`]
//! (a set, one per set) and [`RECORDING`] (a set's recording) are shared by Local Network's tools,
//! under the authority `dj.localnetwork.computer`, with the definitions they share in [`DEFS`]. The documents others
//! maintain are copied in unchanged, in their own authority's folder: [`ADDRESS`], Lexicon
//! Community's, where a set was played; [`DISCUSSION`], userinput.app's record type, which a bug
//! report is; and [`STRONG_REF`] (a record's at:// URI and CID), which it uses.
//!
//! [`validate`] checks a record against its document before it's written. It's strict, so a field
//! the document doesn't define is an error, and it follows the AT Protocol's data model, which has
//! no fractional numbers. It covers the parts of Lexicon these documents use: objects, refs (within
//! a document and to another one), unions, strings, integers, booleans, arrays and blobs, and the
//! string formats `datetime`, `at-uri`, `did`, `cid` and `uri`. The AT Protocol library's parsers
//! check DIDs, at:// URIs and CIDs and chrono checks times; a `uri` only gets the light check the
//! protocol's reference implementation gives one (a scheme, then no spaces).

use rustc_hash::FxHashMap;
use std::sync::LazyLock;

use jacquard_common::SmolStr;
use jacquard_common::types::aturi::AtUri;
use jacquard_common::types::cid::Cid;
use jacquard_common::types::did::validate_did;
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

use crate::{Error, Result};

/// A DJ's My Settings (record key `self`).
pub const SETTINGS: &str = "computer.localnetwork.locallibrary.settings";
/// A DJ's public profile (record key `self`).
pub const PROFILE: &str = "computer.localnetwork.dj.profile";
/// A set: where and when it was played, and its tracklist (record key: a TID).
pub const SET: &str = "computer.localnetwork.dj.set";
/// A set's recording, uploaded to the account (record key: the set's).
pub const RECORDING: &str = "computer.localnetwork.dj.recording";
/// What [`PROFILE`] and [`SET`] share (tracks, keys, partners, shares, counts). Not a record type.
pub const DEFS: &str = "computer.localnetwork.dj.defs";
/// A street address, from Lexicon Community: where a set was played. Not a record type.
pub const ADDRESS: &str = "community.lexicon.location.address";
/// A bug report (or any feedback) posted to a space on userinput.app (record key: a TID).
pub const DISCUSSION: &str = "app.userinput.discussion";
/// A record's at:// URI and CID, from the AT Protocol: the space a discussion is posted to. Not a
/// record type.
pub const STRONG_REF: &str = "com.atproto.repo.strongRef";

/// Every record type locallibrary publishes.
pub const COLLECTIONS: [&str; 4] = [SETTINGS, PROFILE, SET, RECORDING];

/// The Lexicon documents locallibrary writes records of, by NSID, and the ones they share.
pub const DOCUMENTS: [(&str, &str); 8] = [
    (SETTINGS, include_str!("../../../../lexicons/computer/localnetwork/locallibrary/settings.json")),
    (PROFILE, include_str!("../../../../lexicons/computer/localnetwork/dj/profile.json")),
    (SET, include_str!("../../../../lexicons/computer/localnetwork/dj/set.json")),
    (RECORDING, include_str!("../../../../lexicons/computer/localnetwork/dj/recording.json")),
    (DEFS, include_str!("../../../../lexicons/computer/localnetwork/dj/defs.json")),
    (ADDRESS, include_str!("../../../../lexicons/community/lexicon/location/address.json")),
    (DISCUSSION, include_str!("../../../../lexicons/app/userinput/discussion.json")),
    (STRONG_REF, include_str!("../../../../lexicons/com/atproto/repo/strongRef.json")),
];

static PARSED: LazyLock<FxHashMap<&'static str, Value>> = LazyLock::new(|| {
    DOCUMENTS
        .iter()
        .map(|(id, text)| (*id, serde_json::from_str(text).expect("the Lexicon documents are JSON")))
        .collect()
});

/// The Lexicon document for `nsid`, one of [`DOCUMENTS`].
pub fn document(nsid: &str) -> Option<&'static Value> {
    PARSED.get(nsid)
}

/// The definition `name` in the document `doc`. `r` is the ref that named it, and `path` where in
/// the record it was met.
fn resolve(doc: &str, name: &str, r: &str, path: &str) -> std::result::Result<&'static Value, String> {
    def(doc, name).ok_or_else(|| format!("{}: unknown ref {r}", what(path)))
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

/// Check `v` against `schema`, a definition in the document `nsid` (local refs resolve there).
fn check(nsid: &str, schema: &Value, v: &Value, path: &str) -> std::result::Result<(), String> {
    let num = |key: &str| schema[key].as_i64();
    match schema["type"].as_str().unwrap_or_default() {
        "ref" => {
            let r = schema["ref"].as_str().unwrap_or_default();
            let (doc, name) = target(nsid, r);
            let found = resolve(doc, name, r, path)?;
            check(doc, found, v, path)
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
            check(doc, found, &Value::Object(rest), path)
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
                check(nsid, property, x, &at)?;
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
                "cid" => (Cid::str(s).is_valid(), "a CID"),
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
                check(nsid, &schema["items"], x, &format!("{}[{i}]", what(path)))?;
            }
            Ok(())
        }
        // A file the record links to: its media type, its size, and the CID of its bytes.
        "blob" => {
            let mime = v["mimeType"].as_str().unwrap_or_default();
            let size = v["size"].as_i64();
            let linked = v["ref"]["$link"].as_str().is_some_and(|l| Cid::str(l).is_valid());
            let Some(size) = size.filter(|_| v["$type"] == "blob" && linked && !mime.is_empty()) else {
                return Err(format!("{} isn't a blob", what(path)));
            };
            let takes = |a: &str| {
                a == "*/*" || a == mime || a.strip_suffix("/*").is_some_and(|t| mime.split('/').next() == Some(t))
            };
            if schema["accept"].as_array().is_some_and(|a| !a.iter().filter_map(Value::as_str).any(takes)) {
                return Err(format!("{} is {mime}, which the Lexicon doesn't take", what(path)));
            }
            if num("maxSize").is_some_and(|m| size > m) {
                return Err(format!("{} is {size} bytes, more than the Lexicon takes", what(path)));
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
