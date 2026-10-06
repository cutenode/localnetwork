//! The record types locallibrary publishes to an Atmosphere account, and a check that a record
//! fits its type.
//!
//! Each type is a Lexicon document in `lexicons/computer/localnetwork/locallibrary/` at the top of
//! the repository, under the authority `locallibrary.localnetwork.computer`: [`SETTINGS`] (My
//! Settings, one per account), [`PROFILE`] (the public profile, one per account) and [`SET`] (a
//! set's tracklist, one per set). [`validate`] checks a record against its document before it's
//! written. It's strict, so a field the document doesn't define is an error, and it follows the
//! AT Protocol's data model, which has no fractional numbers. It covers the parts of Lexicon these
//! documents use: objects, refs, strings, integers, booleans and arrays.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde_json::Value;

use crate::{Error, Result};

/// A DJ's My Settings (record key `self`).
pub const SETTINGS: &str = "computer.localnetwork.locallibrary.settings";
/// A DJ's public profile (record key `self`).
pub const PROFILE: &str = "computer.localnetwork.locallibrary.profile";
/// A set's tracklist (record key: a TID).
pub const SET: &str = "computer.localnetwork.locallibrary.set";

/// Every record type locallibrary writes.
pub const COLLECTIONS: [&str; 3] = [SETTINGS, PROFILE, SET];

/// The Lexicon documents, by NSID.
pub const DOCUMENTS: [(&str, &str); 3] = [
    (SETTINGS, include_str!("../../../../lexicons/computer/localnetwork/locallibrary/settings.json")),
    (PROFILE, include_str!("../../../../lexicons/computer/localnetwork/locallibrary/profile.json")),
    (SET, include_str!("../../../../lexicons/computer/localnetwork/locallibrary/set.json")),
];

static PARSED: LazyLock<HashMap<&'static str, Value>> = LazyLock::new(|| {
    DOCUMENTS
        .iter()
        .map(|(id, text)| (*id, serde_json::from_str(text).expect("the Lexicon documents are JSON")))
        .collect()
});

/// The Lexicon document for `nsid`, if it's one of locallibrary's.
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
            if schema["format"] == "datetime" && chrono::DateTime::parse_from_rfc3339(s).is_err() {
                return Err(format!("{} isn't a date and time: {s:?}", what(path)));
            }
            Ok(())
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
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::settings::{Category, SCHEMA};

    /// Every definition reachable from `v`, checked for what [`check`] understands.
    fn walk(nsid: &str, v: &Value, at: &str, problems: &mut Vec<String>) {
        let known = ["record", "object", "ref", "string", "integer", "boolean", "array", "unknown"];
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
            "ref" => {
                let r = v["ref"].as_str().unwrap_or_default();
                let (doc, name) = target(nsid, r);
                if def(doc, name).is_none() {
                    problems.push(format!("{nsid} {at}: ref {r} doesn't resolve"));
                }
            }
            "string"
                if v.get("knownValues")
                    .is_some_and(|k| !k.as_array().is_some_and(|a| a.iter().all(Value::is_string))) =>
            {
                problems.push(format!("{nsid} {at}: knownValues"));
            }
            _ => {}
        }
    }

    #[test]
    fn every_lexicon_parses_and_its_refs_resolve() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lexicons");
        let mut problems = Vec::new();
        for (nsid, text) in DOCUMENTS {
            // The file is where the NSID says, so tools that publish a folder of Lexicons find it.
            let path = root.join(nsid.replace('.', "/")).with_extension("json");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "{}", path.display());
            let doc: Value = serde_json::from_str(text).unwrap();
            assert_eq!(doc["lexicon"], 1, "{nsid}");
            assert_eq!(doc["id"], nsid);
            assert!(nsid.starts_with("computer.localnetwork.locallibrary."));
            let main = &doc["defs"]["main"];
            assert_eq!(main["type"], "record", "{nsid}");
            let key = if nsid == SET { "tid" } else { "literal:self" };
            assert_eq!(main["key"], key, "{nsid}");
            for (name, d) in doc["defs"].as_object().unwrap() {
                walk(nsid, d, name, &mut problems);
            }
            // Written for people: no long dashes.
            assert!(!text.contains(['\u{2013}', '\u{2014}']), "{nsid}");
        }
        assert!(problems.is_empty(), "{problems:#?}");
        let files: BTreeSet<String> = std::fs::read_dir(root.join("computer/localnetwork/locallibrary"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files, ["profile.json", "set.json", "settings.json"].map(String::from).into());
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
}
