use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use hegel::Generator;
use hegel::generators as gs;
use serde_json::json;

use super::*;
use crate::settings::{Category, SCHEMA};

/// Every definition reachable from `v`, checked for what [`check`] understands.
fn walk(nsid: &str, v: &Value, at: &str, problems: &mut Vec<String>) {
    let known = ["record", "object", "ref", "union", "string", "integer", "boolean", "array", "blob", "unknown"];
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
            if v.get("knownValues").is_some_and(|k| !k.as_array().is_some_and(|a| a.iter().all(Value::is_string))) =>
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
        let group = if nsid == SETTINGS { "computer.localnetwork.locallibrary." } else { "computer.localnetwork.dj." };
        assert!(nsid.starts_with(group), "{nsid}");
        let main = &document(nsid).unwrap()["defs"]["main"];
        assert_eq!(main["type"], "record", "{nsid}");
        let key = if [SET, RECORDING].contains(&nsid) { "tid" } else { "literal:self" };
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
    assert_eq!(found.len(), 5, "settings, and the DJ profile, set, recording and defs");
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
    assert!(broken(&|v| v["links"] = json!(["https://www.mixcloud.com/dj/"])).contains("links[0] should be an object"));
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
    validate(SET, &at(json!({ "$type": "community.lexicon.location.geo", "latitude": "51.52", "longitude": "-0.10" })))
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
    assert!(check(SETTINGS, &track, &json!({ "key": "10AB" }), "track").unwrap_err().contains("track.key is 4 bytes"));
    let partner = json!({ "type": "ref", "ref": "computer.localnetwork.dj.defs#partner" });
    check(PROFILE, &partner, &json!({ "name": "Ayla" }), "playedWith").unwrap();
    assert!(check(PROFILE, &partner, &json!({}), "playedWith").unwrap_err().contains("playedWith is missing name"));
    let collection = json!({ "type": "ref", "ref": "computer.localnetwork.dj.profile#collection" });
    let counts = json!({ "tracks": { "count": 3500 }, "playlists": { "atLeast": 5 } });
    check(SET, &collection, &counts, "collection").unwrap();
    let bad = json!({ "tracks": { "count": -1 }, "playlists": {} });
    assert!(check(SET, &collection, &bad, "collection").unwrap_err().contains("collection.tracks.count is -1"));
    let nowhere = json!({ "type": "ref", "ref": "community.lexicon.nowhere#place" });
    assert!(check(SET, &nowhere, &json!({}), "x").unwrap_err().contains("unknown ref community.lexicon.nowhere#place"));

    // The address is a document records refer to, not a record type to write.
    let address = json!({ "$type": ADDRESS, "country": "GB" });
    assert!(validate(ADDRESS, &address).unwrap_err().to_string().contains("isn't one of locallibrary's record types"));
}

#[test]
fn string_formats_are_checked() {
    let fits = |format: &str, s: &str| check(SET, &json!({ "type": "string", "format": format }), &json!(s), "x");
    let cases = [
        ("datetime", vec!["2026-10-04T09:30:00.000Z", "2026-10-04T09:30:00+01:00"], vec!["yesterday", "2026-10-04"]),
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
    assert_eq!(checked(json!({ "blob": {} })).unwrap_err(), "blob isn't a blob");
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

/// An optional bound from 0 to `max`.
#[hegel::test_helper]
fn bound(tc: &hegel::TestCase, max: i64) -> Option<i64> {
    tc.draw(gs::optional(gs::integers::<i64>().min_value(0).max_value(max)))
}

/// Text fits a string schema exactly when its length in bytes is within `minLength` and
/// `maxLength`, its length in graphemes within `maxGraphemes` and it's one of `enum` (when
/// there is one), and the first of those it breaks is the one the error names.
#[hegel::test(test_cases = 400)]
fn text_fits_when_its_bytes_graphemes_and_value_do(tc: hegel::TestCase) {
    let words = vec!["dark", "light", "dämmerung", "夜"];
    // Accents, ZWJ and regional indicators, so graphemes of several code points come up often.
    let clusters = gs::text().alphabet("e\u{301}\u{200d}👩🇬🇧").max_size(12);
    let s: String =
        tc.draw(hegel::one_of!(gs::sampled_from(words.clone()).map(String::from), gs::text().max_size(12), clusters));
    let (min, max, graphemes) = (bound(&tc, 12), bound(&tc, 40), bound(&tc, 12));
    let closed = tc.draw(gs::booleans());
    let mut schema = json!({ "type": "string" });
    for (k, v) in [("minLength", min), ("maxLength", max), ("maxGraphemes", graphemes)] {
        if let Some(v) = v {
            schema[k] = v.into();
        }
    }
    if closed {
        schema["enum"] = json!(words);
    }
    let (bytes, count) = (s.len() as i64, s.graphemes(true).count() as i64);
    let bytes_fit = min.is_none_or(|m| bytes >= m) && max.is_none_or(|m| bytes <= m);
    let graphemes_fit = graphemes.is_none_or(|m| count <= m);
    let listed = !closed || words.contains(&s.as_str());
    match check(SET, &schema, &json!(s), "x") {
        Ok(()) => assert!(bytes_fit && graphemes_fit && listed, "{s:?} passed {schema}"),
        Err(e) if !bytes_fit => assert_eq!(e, format!("x is {bytes} bytes long, outside what the Lexicon allows")),
        Err(e) if !graphemes_fit => assert_eq!(e, format!("x is longer than {} graphemes", graphemes.unwrap())),
        Err(e) => assert_eq!((listed, e), (false, format!("x can't be {s:?}"))),
    }
}

/// `maxGraphemes` counts what a reader sees as one character, so a flag, a family emoji and a
/// letter with a combining accent are one each, though they're 9 code points between them.
#[test]
fn max_graphemes_counts_clusters_not_code_points() {
    let schema = json!({ "type": "string", "maxGraphemes": 3 });
    let checked = |s: &str| check(SET, &schema, &json!(s), "name");
    let three = "🇬🇧👩\u{200d}👩\u{200d}👧e\u{301}";
    assert_eq!(three.chars().count(), 9);
    checked(three).unwrap();
    assert_eq!(checked(&format!("{three}!")).unwrap_err(), "name is longer than 3 graphemes");
}

/// A number fits an integer schema exactly when it's a whole number (the AT Protocol has no
/// fractions, so `3.0` isn't one) that fits in 64 bits and is within the bounds.
#[hegel::test(test_cases = 400)]
fn integers_are_whole_numbers_within_their_bounds(tc: hegel::TestCase) {
    let v: Value = tc.draw(hegel::one_of!(
        gs::integers::<i64>().min_value(-60).max_value(60).map(Value::from),
        gs::integers::<i64>().map(Value::from),
        gs::integers::<u64>().min_value(i64::MAX as u64 + 1).map(Value::from),
        gs::floats::<f64>().allow_nan(false).allow_infinity(false).map(Value::from),
        gs::text().max_size(3).map(Value::from),
        gs::booleans().map(Value::from),
    ));
    let bound = || gs::optional(gs::integers::<i64>().min_value(-50).max_value(50));
    let (min, max) = (tc.draw(bound()), tc.draw(bound()));
    let mut schema = json!({ "type": "integer" });
    if let Some(m) = min {
        schema["minimum"] = m.into();
    }
    if let Some(m) = max {
        schema["maximum"] = m.into();
    }
    let checked = check(SET, &schema, &v, "n");
    let Some(i) = (if v.is_f64() { None } else { v.as_i64() }) else {
        assert_eq!(checked, Err(format!("n should be a whole number, not {v}")));
        return;
    };
    let within = min.is_none_or(|m| i >= m) && max.is_none_or(|m| i <= m);
    let outside = Err(format!("n is {i}, outside what the Lexicon allows"));
    assert_eq!(checked, if within { Ok(()) } else { outside }, "{schema}");
}

/// A list fits when its length is within the schema's bounds and every entry fits; when the
/// length is fine, the first entry that doesn't fit is the one named.
#[hegel::test(test_cases = 300)]
fn lists_check_their_length_then_each_entry(tc: hegel::TestCase) {
    let items: Vec<i64> = tc.draw(gs::vecs(gs::integers::<i64>().min_value(-2).max_value(9)).max_size(8));
    let (min, max) = (bound(&tc, 8), bound(&tc, 8));
    let mut schema = json!({ "type": "array", "items": { "type": "integer", "minimum": 0 } });
    if let Some(m) = min {
        schema["minLength"] = m.into();
    }
    if let Some(m) = max {
        schema["maxLength"] = m.into();
    }
    let n = items.len() as i64;
    let length_fits = min.is_none_or(|m| n >= m) && max.is_none_or(|m| n <= m);
    let first_bad = items.iter().position(|i| *i < 0);
    match check(SET, &schema, &json!(items), "list") {
        Ok(()) => assert!(length_fits && first_bad.is_none()),
        Err(e) if !length_fits => assert_eq!(e, format!("list has {n} entries, outside what the Lexicon allows")),
        Err(e) => {
            let i = first_bad.expect("a list that fits was refused");
            assert_eq!(e, format!("list[{i}] is {}, outside what the Lexicon allows", items[i]));
        }
    }
    assert_eq!(check(SET, &schema, &json!({ "0": 1 }), "list").unwrap_err(), "list should be a list");
}

/// An object fits when it has every required field, holds only fields the schema defines,
/// each fitting, and is null only where the schema allows; the first problem is the one named.
#[hegel::test(test_cases = 400)]
fn objects_need_what_is_required_and_hold_nothing_else(tc: hegel::TestCase) {
    let names = vec!["a", "b", "c", "d"];
    let subset = |of: Vec<&'static str>| -> BTreeSet<&'static str> {
        if of.is_empty() {
            BTreeSet::new()
        } else {
            tc.draw(gs::vecs(gs::sampled_from(of)).max_size(4)).into_iter().collect()
        }
    };
    let properties = subset(names.clone());
    let required = subset(properties.iter().copied().collect());
    let nullable = subset(properties.iter().copied().collect());
    let fields: Vec<(&str, Option<String>)> = tc.draw(gs::vecs(gs::tuples!(
        gs::sampled_from(vec!["a", "b", "c", "d", "e"]),
        gs::optional(gs::text().max_size(5))
    )));
    let schema = json!({
        "type": "object",
        "required": required,
        "nullable": nullable,
        "properties": properties.iter().map(|p| (p.to_string(), json!({ "type": "string", "maxLength": 3 }))).collect::<serde_json::Map<_, _>>(),
    });
    let value = Value::Object(fields.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect());
    let object = value.as_object().unwrap();
    // The problems in the order they're checked: required fields first, then each field.
    let missing = required.iter().find(|r| !object.contains_key(**r)).map(|r| format!("the record is missing {r}"));
    let wrong = object.iter().find_map(|(k, x)| match x.as_str() {
        _ if !properties.contains(k.as_str()) => Some(format!("{k} isn't in the Lexicon")),
        None if nullable.contains(k.as_str()) => None,
        None => Some(format!("{k} should be text")),
        Some(s) if s.len() > 3 => Some(format!("{k} is {} bytes long, outside what the Lexicon allows", s.len())),
        Some(_) => None,
    });
    assert_eq!(check(SET, &schema, &value, "").err(), missing.or(wrong), "{value} against {schema}");
}

/// A union's value names its type in `$type`: a type the union lists is checked as that type,
/// and one it doesn't list passes when the union is open and is refused when it's closed.
#[hegel::test(test_cases = 200)]
fn unions_check_listed_types_and_pass_others_unless_closed(tc: hegel::TestCase) {
    let link = format!("{SET}#link");
    let closed = tc.draw(gs::booleans());
    let schema = json!({ "type": "union", "refs": ["#link", ADDRESS], "closed": closed });
    let ty: Option<String> = tc.draw(gs::optional(gs::sampled_from(vec![
        link.clone(),
        ADDRESS.to_string(),
        "app.example.place".to_string(),
        "#link".to_string(),
        format!("{SET}#source"),
    ])));
    let fields = tc.draw(gs::sampled_from(vec![
        json!({}),
        json!({ "uri": "https://station.example/" }),
        json!({ "country": "GB" }),
        json!({ "uri": "station dot example" }),
    ]));
    let mut value = fields.clone();
    if let Some(ty) = &ty {
        value["$type"] = json!(ty);
    }
    let has = |k: &str| fields.get(k).is_some();
    let expected = match ty.as_deref() {
        None => Err("place is missing $type".to_string()),
        Some(t) if t == link => match fields.get("uri").and_then(Value::as_str) {
            None => Err("place is missing uri".into()),
            Some(u) if u.contains(' ') => Err(format!("place.uri isn't a URI: {u:?}")),
            Some(_) => Ok(()),
        },
        Some(ADDRESS) if !has("country") => Err("place is missing country".into()),
        Some(ADDRESS) => Ok(()),
        Some(t) if closed => Err(format!("place can't be a {t}")),
        Some(_) => Ok(()),
    };
    assert_eq!(check(SET, &schema, &value, "place"), expected, "{value}");
    assert_eq!(check(SET, &schema, &json!("Fabric"), "place").unwrap_err(), "place should be an object");
}

/// A ref names a definition in its own document (`#name`), another document's (`nsid#name`),
/// or another document's main definition (`nsid`).
#[hegel::test(test_cases = 200)]
fn refs_name_their_document_and_definition(tc: hegel::TestCase) {
    let nsid = || gs::from_regex("[a-z]{1,6}\\.[a-z]{1,6}\\.[a-z][a-zA-Z]{0,6}").fullmatch(true);
    let (here, there) = (tc.draw(nsid()), tc.draw(nsid()));
    let name: String = tc.draw(gs::from_regex("[a-z][a-zA-Z]{0,8}").fullmatch(true));
    assert_eq!(target(&here, &format!("#{name}")), (here.as_str(), name.as_str()));
    assert_eq!(target(&here, &format!("{there}#{name}")), (there.as_str(), name.as_str()));
    assert_eq!(target(&here, &there), (there.as_str(), "main"));
    assert_eq!(short_name(&there), there.rsplit('.').next().unwrap());
}

/// A `uri` is a scheme of letters, digits and `_`, a colon, maybe `//`, then text without
/// spaces that doesn't start with `/`: any whitespace in it, a scheme with other characters or
/// no scheme at all, and it isn't one.
#[hegel::test(test_cases = 400)]
fn uris_are_a_scheme_then_text_without_spaces(tc: hegel::TestCase) {
    let scheme: String = tc.draw(gs::from_regex("[A-Za-z0-9_]{1,8}").fullmatch(true));
    let text: String = tc.draw(gs::text().max_size(16));
    let rest: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let rest = if rest.is_empty() || rest.starts_with('/') { format!("x{rest}") } else { rest };
    let slashes = if tc.draw(gs::booleans()) { "//" } else { "" };
    let uri = format!("{scheme}:{slashes}{rest}");
    assert!(is_uri(&uri), "{uri:?}");
    assert_eq!(check(SET, &json!({ "type": "string", "format": "uri" }), &json!(uri), "x"), Ok(()));

    let space = tc.draw(gs::sampled_from(vec![' ', '\t', '\n', '\u{a0}', '\u{2003}', '\u{3000}']));
    let at = tc.draw(gs::integers::<usize>().max_value(rest.chars().count()));
    let spaced: String = rest.chars().take(at).chain([space]).chain(rest.chars().skip(at)).collect();
    assert!(!is_uri(&format!("{scheme}:{slashes}{spaced}")), "{spaced:?}");
    let odd = tc.draw(gs::sampled_from(vec!['-', '+', '.', 'é', ' ', '/']));
    let at = tc.draw(gs::integers::<usize>().max_value(scheme.len()));
    let (a, b) = scheme.split_at(at);
    assert!(!is_uri(&format!("{a}{odd}{b}:{slashes}{rest}")));
    assert!(!is_uri(&format!(":{slashes}{rest}")) && !is_uri(&format!("{scheme}:///{rest}")));
    assert!(!is_uri(&format!("{scheme}{rest}").replace(':', "")));
}

/// A boolean schema takes exactly `true` and `false`, an `unknown` one exactly objects, and a
/// type or string format the check doesn't know is an error, whatever the value.
#[hegel::test(test_cases = 200)]
fn booleans_unknowns_and_what_the_check_doesnt_know(tc: hegel::TestCase) {
    let v: Value = tc.draw(gs::sampled_from(vec![
        json!(true),
        json!(false),
        json!(0),
        json!("true"),
        Value::Null,
        json!([]),
        json!({}),
        json!({ "a": 1 }),
    ]));
    let boolean = check(SET, &json!({ "type": "boolean" }), &v, "b");
    assert_eq!(boolean, if v.is_boolean() { Ok(()) } else { Err("b should be true or false".into()) });
    let unknown = check(SET, &json!({ "type": "unknown" }), &v, "u");
    assert_eq!(unknown, if v.is_object() { Ok(()) } else { Err("u should be an object".into()) });
    let ty = tc.draw(gs::sampled_from(vec!["bytes", "cid-link", "token", "float"]));
    let err = check(SET, &json!({ "type": ty }), &v, "x").unwrap_err();
    assert_eq!(err, format!("x: the check doesn't know the Lexicon type {ty:?}"));
    let format = tc.draw(gs::sampled_from(vec!["handle", "nsid", "tid", "language", "record-key"]));
    let err = check(SET, &json!({ "type": "string", "format": format }), &json!("dj.example.com"), "s").unwrap_err();
    assert_eq!(err, format!("s: the check doesn't know the format {format:?}"));
}

/// A CID as the AT Protocol writes one (a dag-cbor record's).
const CID: &str = "bafyreidfayvfuwqa7qlnopdjiqrxzs6blmoeu4rujcjtnci5beludirz2a";

/// A CID is checked as one, and a blob (a file a record links to) for its shape, its media type
/// and its size.
#[test]
fn cids_and_blobs_are_checked() {
    let cid = json!({ "type": "string", "format": "cid" });
    check(SET, &cid, &json!(CID), "c").unwrap();
    assert_eq!(check(SET, &cid, &json!("bafy-not-one"), "c").unwrap_err(), "c isn't a CID: \"bafy-not-one\"");

    let schema = json!({ "type": "blob", "accept": ["image/*", "application/json"], "maxSize": 1000 });
    let blob =
        |mime: &str, size: i64| json!({ "$type": "blob", "ref": { "$link": CID }, "mimeType": mime, "size": size });
    check(SET, &schema, &blob("image/png", 1000), "b").unwrap();
    check(SET, &schema, &blob("application/json", 1), "b").unwrap();
    check(SET, &json!({ "type": "blob" }), &blob("video/mp4", 1 << 30), "b").unwrap();
    assert_eq!(
        check(SET, &schema, &blob("video/mp4", 10), "b").unwrap_err(),
        "b is video/mp4, which the Lexicon doesn't take"
    );
    assert_eq!(
        check(SET, &schema, &blob("image/png", 1001), "b").unwrap_err(),
        "b is 1001 bytes, more than the Lexicon takes"
    );
    for broken in ["$type", "ref", "mimeType", "size"] {
        let mut b = blob("image/png", 10);
        b.as_object_mut().unwrap().remove(broken);
        assert_eq!(check(SET, &schema, &b, "b").unwrap_err(), "b isn't a blob", "{broken}");
    }
    let mut b = blob("image/png", 10);
    b["ref"]["$link"] = "bafy-not-one".into();
    assert_eq!(check(SET, &schema, &b, "b").unwrap_err(), "b isn't a blob");
}
