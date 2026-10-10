//! The record types locallibrary publishes to an Atmosphere account, and a check that a record
//! fits its type.
//!
//! Each type is a Lexicon document in the repo's `lexicons/`, in the folder its NSID names.
//! [`SETTINGS`] (My Settings, one per account) is locallibrary's own, under the authority
//! `locallibrary.localnetwork.computer`. [`PROFILE`] (the public profile, one per account) and
//! [`SET`] (a set, one per set) are shared by Local Network's tools, under the authority
//! `dj.localnetwork.computer`, with the definitions they share in [`DEFS`]. Documents they refer
//! to that others maintain are copied in unchanged, in their own authority's folder: [`ADDRESS`]
//! (where a set was played).
//!
//! [`validate`] checks a record against its document before it's written. It's strict, so a field
//! the document doesn't define is an error, and it follows the AT Protocol's data model, which has
//! no fractional numbers. It covers the parts of Lexicon these documents use: objects, refs (within
//! a document and to another one), unions, strings, integers, booleans and arrays, and the string
//! formats `datetime`, `at-uri`, `did` and `uri`. The AT Protocol library's parsers check DIDs and
//! at:// URIs and chrono checks times; a `uri` only gets the light check the protocol's reference
//! implementation gives one (a scheme, then no spaces).

use rustc_hash::FxHashMap;
use std::sync::LazyLock;

use jacquard_common::SmolStr;
use jacquard_common::types::aturi::AtUri;
use jacquard_common::types::did::validate_did;
use serde_json::Value;

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

/// The Lexicon documents, by NSID: the ones locallibrary writes and the ones they refer to.
pub const DOCUMENTS: [(&str, &str); 5] = [
    (SETTINGS, include_str!("../../../../lexicons/computer/localnetwork/locallibrary/settings.json")),
    (PROFILE, include_str!("../../../../lexicons/computer/localnetwork/dj/profile.json")),
    (SET, include_str!("../../../../lexicons/computer/localnetwork/dj/set.json")),
    (DEFS, include_str!("../../../../lexicons/computer/localnetwork/dj/defs.json")),
    (ADDRESS, include_str!("../../../../lexicons/community/lexicon/location/address.json")),
];

static PARSED: LazyLock<FxHashMap<&'static str, Value>> = LazyLock::new(|| {
    DOCUMENTS
        .iter()
        .map(|(id, text)| (*id, serde_json::from_str(text).expect("the Lexicon documents are JSON")))
        .collect()
});

/// The Lexicon document for `nsid`, if it's one of [`DOCUMENTS`].
pub fn document(nsid: &str) -> Option<&'static Value> {
    PARSED.get(nsid)
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
            let found = def(doc, name).ok_or_else(|| format!("{}: unknown ref {r}", what(path)))?;
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
            let found = def(doc, name).ok_or_else(|| format!("{}: unknown ref {r}", what(path)))?;
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
            // Characters, which are never fewer than graphemes: text within the limit in
            // characters is within it in graphemes.
            if num("maxGraphemes").is_some_and(|m| s.chars().count() as i64 > m) {
                return Err(format!("{} is longer than {} characters", what(path), num("maxGraphemes").unwrap_or(0)));
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
                check(nsid, &schema["items"], x, &format!("{}[{i}]", what(path)))?;
            }
            Ok(())
        }
        "unknown" if v.is_object() => Ok(()),
        "unknown" => Err(format!("{} should be an object", what(path))),
        other => Err(format!("{}: the check doesn't know the Lexicon type {other:?}", what(path))),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::*;
    use crate::settings::{Category, SCHEMA};

    /// Every definition reachable from `v`, checked for what [`check`] understands.
    fn walk(nsid: &str, v: &Value, at: &str, problems: &mut Vec<String>) {
        let known = ["record", "object", "ref", "union", "string", "integer", "boolean", "array", "unknown"];
        let ty = v["type"].as_str().unwrap_or_default();
        if !known.contains(&ty) {
            problems.push(format!("{nsid} {at}: type {ty:?}"));
        }
        match ty {
            "record" => walk(nsid, &v["record"], &format!("{at}.record"), problems),
            "object" => {
                let props = v["properties"].as_object().cloned().unwrap_or_default();
                for r in v["required"].as_array().into_iter().flatten() {
                    if !props.contains_key(r.as_str().unwrap_or_default()) {
                        problems.push(format!("{nsid} {at}: required {r} isn't a property"));
                    }
                }
                for (k, p) in &props {
                    walk(nsid, p, &format!("{at}.{k}"), problems);
                }
            }
            "array" => walk(nsid, &v["items"], &format!("{at}[]"), problems),
            "ref" | "union" => {
                let refs = v["ref"]
                    .as_str()
                    .into_iter()
                    .chain(v["refs"].as_array().into_iter().flatten().map(|r| r.as_str().unwrap_or_default()));
                for r in refs {
                    let (doc, name) = target(nsid, r);
                    if def(doc, name).is_none() {
                        problems.push(format!("{nsid} {at}: ref {r} doesn't resolve"));
                    }
                }
            }
            "string"
                if v.get("knownValues")
                    .is_some_and(|k| !k.as_array().is_some_and(|a| a.iter().all(Value::is_string))) =>
            {
                problems.push(format!("{nsid} {at}: knownValues"));
            }
            "string"
                if v.get("format").is_some_and(|f| {
                    let bare = json!({ "type": "string", "format": f });
                    check(nsid, &bare, &json!(""), at).is_err_and(|e| e.contains("doesn't know the format"))
                }) =>
            {
                problems.push(format!("{nsid} {at}: format {}", v["format"]));
            }
            _ => {}
        }
    }

    #[test]
    fn every_lexicon_parses_and_its_refs_resolve() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lexicons");
        // The file is where the NSID says, so tools that publish a folder of Lexicons find it.
        let path = |nsid: &str| root.join(nsid.replace('.', "/")).with_extension("json");
        let mut problems = Vec::new();
        for (nsid, text) in DOCUMENTS {
            assert_eq!(std::fs::read_to_string(path(nsid)).unwrap(), text, "{nsid}");
            let doc: Value = serde_json::from_str(text).unwrap();
            assert_eq!(doc["lexicon"], 1, "{nsid}");
            assert_eq!(doc["id"], nsid);
            for (name, d) in doc["defs"].as_object().unwrap() {
                walk(nsid, d, name, &mut problems);
            }
            // Written for people: no long dashes.
            assert!(!text.contains(['\u{2013}', '\u{2014}']), "{nsid}");
        }
        assert!(problems.is_empty(), "{problems:#?}");
        for nsid in COLLECTIONS {
            // locallibrary's own settings, and the DJ records Local Network's tools share.
            let group =
                if nsid == SETTINGS { "computer.localnetwork.locallibrary." } else { "computer.localnetwork.dj." };
            assert!(nsid.starts_with(group), "{nsid}");
            let main = &document(nsid).unwrap()["defs"]["main"];
            assert_eq!(main["type"], "record", "{nsid}");
            let key = if nsid == SET { "tid" } else { "literal:self" };
            assert_eq!(main["key"], key, "{nsid}");
        }
        // Every file in the folders locallibrary owns is one of the documents. The rest of the
        // repo's `lexicons/` belongs to the other tools.
        fn files(dir: &Path, found: &mut BTreeSet<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    files(&p, found)
                } else {
                    found.insert(p);
                }
            }
        }
        let ours = ["computer/localnetwork/locallibrary", "computer/localnetwork/dj"].map(|d| root.join(d));
        let mut found = BTreeSet::new();
        for dir in &ours {
            files(dir, &mut found);
        }
        let documents = DOCUMENTS.iter().map(|(nsid, _)| path(nsid)).filter(|p| ours.iter().any(|d| p.starts_with(d)));
        assert_eq!(found, documents.collect());
        assert_eq!(found.len(), 4, "settings, and the DJ profile, set and defs");
    }

    #[test]
    fn the_settings_lexicon_lists_every_setting_with_its_choices() {
        let defs = &document(SETTINGS).unwrap()["defs"];
        let mut listed = 0;
        for def in SCHEMA {
            let group = match def.category {
                Category::Player => "player",
                Category::Display => "display",
                Category::Mixer => "mixer",
            };
            let field = crate::publish::camel(def.key);
            let property = &defs[group]["properties"][&field];
            let ids: Vec<&str> = def.choices.iter().map(|c| c.id).collect();
            assert_eq!(property["knownValues"], json!(ids), "{group}.{field}");
            listed += 1;
        }
        let total: usize =
            ["player", "display", "mixer"].iter().map(|g| defs[g]["properties"].as_object().unwrap().len()).sum();
        assert_eq!(total, listed, "the Lexicon lists settings the schema doesn't have");
    }

    #[test]
    fn records_are_checked_strictly() {
        let ok = json!({
            "$type": SET,
            "date": "2026-10-03",
            "tracks": [{ "title": "Glue", "artist": "Bicep", "bpmCentis": 12500, "key": "8A" }, { "unidentified": true }],
            "createdAt": "2026-10-04T09:30:00.000Z",
        });
        validate(SET, &ok).unwrap();
        let broken = |f: &dyn Fn(&mut Value)| {
            let mut v = ok.clone();
            f(&mut v);
            validate(SET, &v).unwrap_err().to_string()
        };
        assert!(broken(&|v| v["$type"] = json!(PROFILE)).contains("$type"));
        assert!(broken(&|v| v.as_object_mut().unwrap().remove("date").map(drop).unwrap()).contains("missing date"));
        assert!(broken(&|v| v["mood"] = json!("dark")).contains("mood isn't in the Lexicon"));
        assert!(broken(&|v| v["tracks"][0]["bpmCentis"] = json!(125.5)).contains("whole number"));
        assert!(broken(&|v| v["tracks"][0]["bpmCentis"] = json!(-1)).contains("outside"));
        assert!(broken(&|v| v["tracks"][1]["unidentified"] = json!("yes")).contains("true or false"));
        assert!(broken(&|v| v["date"] = json!("3 Oct")).contains("bytes long"));
        assert!(broken(&|v| v["createdAt"] = json!("yesterday")).contains("date and time"));
        assert!(broken(&|v| v["title"] = json!("x".repeat(101))).contains("longer than 100"));
        assert!(broken(&|v| v["tracks"] = json!(vec![json!({}); 1001])).contains("1001 entries"));
        assert!(validate("app.example.thing", &ok).is_err());
    }

    /// The set's lists hold objects, so each entry can gain fields later.
    #[test]
    fn a_set_lists_objects_it_can_grow() {
        let mut set = json!({
            "$type": SET,
            "date": "2026-10-03",
            "playedWith": [{ "name": "Ayla", "did": "did:plc:ayla" }, { "name": "Sam" }],
            "links": [
                { "uri": "https://www.mixcloud.com/dj/friday-warm-up/", "name": "Mixcloud" },
                { "uri": "https://station.example/archive/2026-10-03" },
            ],
            "sources": [{ "kind": "history" }, { "kind": "mix" }],
            "tracks": [{ "title": "Glue", "recordingMbId": "c1a3e5c0-7b5e-4d51-9d0e-3c8b1f1f8a2b", "key": "8A" }],
            "createdAt": "2026-10-04T09:30:00.000Z",
        });
        validate(SET, &set).unwrap();
        let broken = |f: &dyn Fn(&mut Value)| {
            let mut v = set.clone();
            f(&mut v);
            validate(SET, &v).unwrap_err().to_string()
        };
        // The fields they replace are gone: `with` (a keyword in most languages) and the one `link`.
        assert!(broken(&|v| v["with"] = json!([{ "name": "Ayla" }])).contains("with isn't in the Lexicon"));
        assert!(broken(&|v| v["link"] = json!("https://www.mixcloud.com/dj/")).contains("link isn't in the Lexicon"));
        assert!(broken(&|v| v["sources"] = json!(["history"])).contains("sources[0] should be an object"));
        assert!(
            broken(&|v| v["links"] = json!(["https://www.mixcloud.com/dj/"])).contains("links[0] should be an object")
        );
        assert!(broken(&|v| v["links"][1] = json!({ "name": "Archive" })).contains("links[1] is missing uri"));
        assert!(broken(&|v| v["playedWith"][1] = json!({})).contains("playedWith[1] is missing name"));
        // A source this version doesn't know is still a source: known values aren't a closed list.
        set["sources"][1]["kind"] = json!("stream");
        validate(SET, &set).unwrap();
    }

    /// A ref names a definition in this document (`#name`), another document's (`nsid#name`) or
    /// another document's `main` (`nsid`), and that definition's own refs resolve in its document.
    #[test]
    fn refs_to_other_documents_are_checked_there() {
        // The set's location is an open union: an address is checked against the address
        // document, and a kind of location the set's document doesn't list passes as it is.
        let at = |location: Value| json!({ "$type": SET, "date": "2026-10-03", "tracks": [], "createdAt": "2026-10-04T09:30:00.000Z", "location": location });
        let address = |fields: Value| {
            let mut v = json!({ "$type": ADDRESS });
            v.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
            v
        };
        validate(SET, &at(address(json!({ "name": "Fabric", "locality": "London", "country": "GB" })))).unwrap();
        validate(
            SET,
            &at(json!({ "$type": "community.lexicon.location.geo", "latitude": "51.52", "longitude": "-0.10" })),
        )
        .unwrap();
        let err = |location: Value| validate(SET, &at(location)).unwrap_err().to_string();
        assert!(err(address(json!({ "name": "Fabric" }))).contains("location is missing country"));
        assert!(err(address(json!({ "country": "GB", "zip": "EC1M" }))).contains("location.zip isn't in the Lexicon"));
        assert!(err(address(json!({ "country": "G" }))).contains("location.country is 1 bytes long"));
        assert!(err(json!({ "country": "GB" })).contains("location is missing $type"));
        assert!(err(json!("London")).contains("location should be an object"));

        // From another document, the shared track, whose `#camelotKey` ref resolves in the shared
        // document (the settings have no #camelotKey), and the profile's collection, whose counts
        // are the shared document's.
        let track = json!({ "type": "ref", "ref": "computer.localnetwork.dj.defs#track" });
        check(SETTINGS, &track, &json!({ "title": "Glue", "key": "8A" }), "track").unwrap();
        assert!(check(SETTINGS, &track, &json!({ "key": "8A9" }), "track").is_ok(), "a key it doesn't know");
        assert!(
            check(SETTINGS, &track, &json!({ "key": "10AB" }), "track").unwrap_err().contains("track.key is 4 bytes")
        );
        let partner = json!({ "type": "ref", "ref": "computer.localnetwork.dj.defs#partner" });
        check(PROFILE, &partner, &json!({ "name": "Ayla" }), "playedWith").unwrap();
        assert!(check(PROFILE, &partner, &json!({}), "playedWith").unwrap_err().contains("playedWith is missing name"));
        let collection = json!({ "type": "ref", "ref": "computer.localnetwork.dj.profile#collection" });
        let counts = json!({ "tracks": { "count": 3500 }, "playlists": { "atLeast": 5 } });
        check(SET, &collection, &counts, "collection").unwrap();
        let bad = json!({ "tracks": { "count": -1 }, "playlists": {} });
        assert!(check(SET, &collection, &bad, "collection").unwrap_err().contains("collection.tracks.count is -1"));
        let nowhere = json!({ "type": "ref", "ref": "community.lexicon.nowhere#place" });
        assert!(
            check(SET, &nowhere, &json!({}), "x").unwrap_err().contains("unknown ref community.lexicon.nowhere#place")
        );

        // The address is a document records refer to, not a record type to write.
        let address = json!({ "$type": ADDRESS, "country": "GB" });
        assert!(
            validate(ADDRESS, &address).unwrap_err().to_string().contains("isn't one of locallibrary's record types")
        );
    }

    #[test]
    fn string_formats_are_checked() {
        let fits = |format: &str, s: &str| check(SET, &json!({ "type": "string", "format": format }), &json!(s), "x");
        let cases = [
            (
                "datetime",
                vec!["2026-10-04T09:30:00.000Z", "2026-10-04T09:30:00+01:00"],
                vec!["yesterday", "2026-10-04"],
            ),
            (
                "at-uri",
                vec![
                    "at://did:plc:ewvi7nxzyoun6zhxrhs64oiz/community.lexicon.calendar.event/3lte3c7x43l2e",
                    "at://dj.example.com",
                ],
                vec!["https://example.com/event", "did:plc:abc", "at://", "at://did:plc:abc/not a collection"],
            ),
            (
                "did",
                vec!["did:plc:ewvi7nxzyoun6zhxrhs64oiz", "did:web:dj.example.com"],
                vec!["dj.example.com", "did:plc", "DID:plc:abc", "at://did:plc:abc", "did:plc:abc:", ""],
            ),
            (
                "uri",
                vec!["https://www.mixcloud.com/dj/friday-warm-up/", "at://did:plc:abc/app.bsky.feed.post/3l2k"],
                vec!["mixcloud.com/dj/friday", "https://exa mple.com", ""],
            ),
        ];
        for (format, good, bad) in cases {
            for s in good {
                assert_eq!(fits(format, s), Ok(()), "{format} {s}");
            }
            for s in bad {
                let err = fits(format, s).unwrap_err();
                assert!(err.starts_with("x isn't "), "{format} {s}: {err}");
            }
        }
        assert!(fits("handle", "dj.example.com").unwrap_err().contains("doesn't know the format \"handle\""));
    }

    /// The parts of Lexicon today's documents don't use are checked as the spec says, ready for
    /// a document that does.
    #[test]
    fn nullable_fields_closed_enums_and_unknowns_are_checked() {
        let object = json!({
            "type": "object",
            "nullable": ["note"],
            "properties": {
                "note": { "type": "string" },
                "mood": { "type": "string", "enum": ["dark", "light"] },
                "extra": { "type": "unknown" },
                "blob": { "type": "blob" },
            },
        });
        let checked = |v: Value| check(SET, &object, &v, "");
        checked(json!({ "note": null, "mood": "dark", "extra": { "any": 1 } })).unwrap();
        assert_eq!(checked(json!({ "mood": "grey" })).unwrap_err(), "mood can't be \"grey\"");
        assert_eq!(checked(json!({ "extra": 1 })).unwrap_err(), "extra should be an object");
        assert!(checked(json!({ "blob": {} })).unwrap_err().contains("doesn't know the Lexicon type \"blob\""));
        // Only the fields named nullable may be null.
        assert_eq!(checked(json!({ "mood": null })).unwrap_err(), "mood should be text");

        // A closed union takes only the types it lists, and a local ref's type is `nsid#name`.
        let closed = json!({ "type": "union", "refs": ["#link", ADDRESS], "closed": true });
        let checked = |v: Value| check(SET, &closed, &v, "place");
        checked(json!({ "$type": format!("{SET}#link"), "uri": "https://station.example/" })).unwrap();
        checked(json!({ "$type": ADDRESS, "country": "GB" })).unwrap();
        assert_eq!(
            checked(json!({ "$type": "#link", "uri": "https://station.example/" })).unwrap_err(),
            "place can't be a #link"
        );
        assert_eq!(checked(json!({ "$type": "app.example.place" })).unwrap_err(), "place can't be a app.example.place");
        assert_eq!(checked(json!({ "$type": format!("{SET}#link") })).unwrap_err(), "place is missing uri");
        let nowhere = json!({ "type": "union", "refs": ["#nowhere"] });
        let err = check(SET, &nowhere, &json!({ "$type": format!("{SET}#nowhere") }), "place").unwrap_err();
        assert_eq!(err, "place: unknown ref #nowhere");
    }
}
