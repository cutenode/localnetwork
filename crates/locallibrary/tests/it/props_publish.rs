//! Properties of publishing to an Atmosphere account (`publish`, `atmosphere::lexicon`,
//! `api::*_publish_*`): the records built from settings, profile documents and sets fit their
//! Lexicons, survive JSON and read back; the Lexicon check refuses what breaks them; and random
//! sequences of edits published to a pretend data server (`support/mock_pds.rs`) leave it holding
//! what a model of the records says. Nothing here touches the network.

use crate::common;
use crate::mock_pds::{MockPds, Writes};
use crate::session::{DID, session_json, sign_in_as};

use indexmap::IndexMap;
use std::collections::BTreeSet;
use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};
use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::api::{self, ImportPublishedSettings, ProfileRequest, SetPublishRequest, SetSettings};
use locallibrary::atmosphere::lexicon::{self, ADDRESS, PROFILE, SET, SETTINGS};
use locallibrary::atmosphere::repo::{self, Fetched};
use locallibrary::atmosphere::{self, AccountInfo, Client, Storage};
use locallibrary::error::Reason;
use locallibrary::profile::{
    self, ArcShape, Basis, Count, DocArc, DocCollection, DocEnergy, DocEra, DocKeys, DocMixing, DocPrep, DocSets,
    DocShare, DocTempo, DocTrack, Precision, PrefsUpdate, ProfileDocument, Section,
};
use locallibrary::progress::Ctx;
use locallibrary::publish;
use locallibrary::sets::{EntryOrigin, MusicBrainzIds, Partner, Recording, Set, SetBook, SetEntry};
use locallibrary::settings::{self, Category, SCHEMA, SettingDef};
use locallibrary::store::Store;
use loclib_model::test_util::keys;
use serde_json::{Map, Value, json};
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

const PUT: &str = "com.atproto.repo.putRecord";

/// Sign `DID` in on this computer (the session in files, not the keychain) with a session for
/// `pds` that was granted `scopes`.
fn sign_in(root: &Path, pds: &str, scopes: &str) {
    sign_in_as(root, session_json(DID, "s1", pds, scopes, Some("refresh-1"), "2099-01-01T00:00:00.000Z"));
}

/// What a sign-in that allows publishing is granted.
fn publishing() -> String {
    crate::mock_pds::granted(&atmosphere::publish_scopes()).join(" ")
}

/// `v` without the fields that only say when.
fn timeless(mut v: Value) -> Value {
    if let Some(m) = v.as_object_mut() {
        for f in ["updatedAt", "generatedAt", "createdAt"] {
            m.remove(f);
        }
    }
    v
}

fn group(def: &SettingDef) -> &'static str {
    match def.category {
        Category::Player => "player",
        Category::Display => "display",
        Category::Mixer => "mixer",
    }
}

/// Any time from 1970 to 2100, to the nanosecond (records keep milliseconds).
#[hegel::test_helper]
fn time(tc: &TestCase) -> DateTime<Utc> {
    let last = DateTime::from_timestamp(4_102_444_800, 999_999_999).unwrap();
    tc.draw(gs::default::<DateTime<Utc>>().min_value(DateTime::UNIX_EPOCH.naive_utc()).max_value(last.naive_utc()))
}

/// A setting and one of its values.
#[hegel::test_helper]
fn setting(tc: &TestCase) -> (&'static SettingDef, &'static str) {
    let def = tc.draw(gs::sampled_from(SCHEMA.iter().collect::<Vec<_>>()));
    let choice = tc.draw(gs::sampled_from(def.choices)).id;
    (def, choice)
}

/// What each key of a base means: the setting and its value, or `None` for nothing this version
/// knows.
type Meanings = IndexMap<String, Option<(&'static str, &'static str)>>;

/// Settings as a profile's base: setting keys with one of their values, now and then spelled
/// loosely, or a key or value this version doesn't know; and what each key means.
#[hegel::test_helper]
fn loose_base(tc: &TestCase) -> (IndexMap<String, String>, Meanings) {
    let (mut base, mut meaning) = (IndexMap::new(), IndexMap::new());
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(10)) {
        let (def, choice) = setting(tc);
        let (k, v, m) = match tc.draw(gs::integers::<u8>().max_value(5)) {
            0 => ("new_setting".to_string(), "on".to_string(), None),
            1 => (def.key.to_string(), "sideways".to_string(), None),
            2 => (
                format!(" {} ", def.key.to_uppercase().replace('_', "-")),
                choice.to_uppercase(),
                Some((def.key, choice)),
            ),
            _ => (def.key.to_string(), choice.to_string(), Some((def.key, choice))),
        };
        base.insert(k.clone(), v);
        meaning.insert(k, m);
    }
    (base, meaning)
}

/// Settings as `settings set` keeps them: each key once, with one of its values.
#[hegel::test_helper]
fn canonical_settings(tc: &TestCase, min: usize, max: usize) -> IndexMap<String, String> {
    let n = tc.draw(gs::integers::<usize>().min_value(min).max_value(max));
    (0..n).map(|_| setting(tc)).map(|(d, c)| (d.key.to_string(), c.to_string())).collect()
}

/// My Settings built from any base fit the settings Lexicon, survive JSON, and read back as the
/// settings the base sets (however loosely spelled), with nothing skipped and the time as
/// records carry it; groups with nothing in them stay out. Breaking a value's type, adding a
/// field the Lexicon doesn't have or dropping the time is refused.
#[hegel::test(test_cases = 300)]
fn my_settings_fit_their_lexicon_and_read_back(tc: TestCase) {
    let (base, meaning) = loose_base(&tc);
    let now = time(&tc);
    let record = publish::settings_record(&base, now);
    lexicon::validate(SETTINGS, &record).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&record.to_string()).unwrap(), record);

    let expected: IndexMap<String, String> =
        meaning.values().flatten().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let back = publish::settings_from_record(&record).unwrap();
    assert_eq!(back.values.clone().into_iter().collect::<IndexMap<_, _>>(), expected);
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
    assert_eq!(back.updated_at, Some(publish::timestamp(now)));
    assert_eq!(record["updatedAt"].as_str().unwrap().len(), "2026-10-05T12:00:00.123Z".len());
    for (field, v) in record.as_object().unwrap() {
        match field.as_str() {
            "$type" | "updatedAt" => {}
            g => assert!(["player", "display", "mixer"].contains(&g) && !v.as_object().unwrap().is_empty(), "{record}"),
        }
    }

    let mut broken = record.clone();
    let want = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 if !expected.is_empty() => {
            let (key, _) = expected.iter().next().unwrap();
            let def = settings::find(key).unwrap();
            let field = publish::camel(def.key);
            broken[group(def)][&field] = json!(1);
            format!("{}.{field} should be text", group(def))
        }
        1 => {
            broken["player"] = json!({ "slipReverse": "on" });
            "player.slipReverse isn't in the Lexicon".to_string()
        }
        2 => {
            broken.as_object_mut().unwrap().remove("updatedAt");
            "the record is missing updatedAt".to_string()
        }
        _ => {
            broken["display"] = json!("bright");
            "display should be an object".to_string()
        }
    };
    let err = lexicon::validate(SETTINGS, &broken).unwrap_err().to_string();
    assert!(err.contains(&want), "{err} (wanted {want})");
}

/// Reading someone's My Settings keeps each setting they published in its own group with a
/// value this version knows, and skips (on one line) everything else: settings in another
/// group, values or settings this version doesn't know, values that aren't text. Fields beside
/// the groups are passed over; a record of another type is refused.
#[hegel::test(test_cases = 300)]
fn reading_my_settings_keeps_what_it_knows_and_skips_the_rest(tc: TestCase) {
    let mut record = Map::new();
    // For each group and field, whether it's a known setting (its key and value) or skipped.
    let mut known: IndexMap<(String, String), Option<(String, String)>> = IndexMap::new();
    for name in ["player", "display", "mixer"] {
        if !tc.draw(gs::booleans()) {
            continue;
        }
        let mut fields = Map::new();
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
            let (def, choice) = setting(&tc);
            let field = publish::camel(def.key);
            let (field, value, meant) = match tc.draw(gs::integers::<u8>().max_value(5)) {
                0 => (field, json!("sideways"), None),
                1 => (field, tc.draw(gs::sampled_from(vec![json!(1), json!(true), Value::Null, json!(["on"])])), None),
                2 => {
                    let odd = format!("new{}", tc.draw(gs::text().max_size(6)));
                    tc.assume(settings::find(&publish::snake(&odd)).is_none());
                    (odd, json!("on"), None)
                }
                _ if group(def) != name => (field, json!(choice), None),
                _ => (field, json!(choice), Some((def.key.to_string(), choice.to_string()))),
            };
            fields.insert(field.clone(), value);
            known.insert((name.to_string(), field), meant);
        }
        record.insert(name.into(), Value::Object(fields));
    }
    let updated = tc.draw(gs::optional(gs::sampled_from(vec![json!("2026-10-05T12:00:00.000Z"), json!(7)])));
    if let Some(u) = &updated {
        record.insert("updatedAt".into(), u.clone());
    }
    if tc.draw(gs::booleans()) {
        record.insert("version".into(), json!(2));
    }
    let ty = tc.draw(gs::sampled_from(vec![json!(SETTINGS), json!(SETTINGS), json!(PROFILE), Value::Null]));
    if !ty.is_null() {
        record.insert("$type".into(), ty.clone());
    }
    let record = Value::Object(record);

    let read = publish::settings_from_record(&record);
    if ty != SETTINGS {
        let err = read.unwrap_err().to_string();
        assert!(err.contains("isn't a My Settings record") && !err.chars().any(char::is_control), "{err}");
        return;
    }
    let read = read.unwrap();
    let values: IndexMap<String, String> = known.values().flatten().cloned().collect();
    assert_eq!(read.values.into_iter().collect::<IndexMap<_, _>>(), values);
    assert_eq!(read.skipped.len(), known.values().filter(|m| m.is_none()).count(), "{:?}", read.skipped);
    for s in &read.skipped {
        assert!(!s.chars().any(char::is_control), "{s:?}");
        assert!(["player.", "display.", "mixer."].iter().any(|g| s.starts_with(g)), "{s:?}");
    }
    assert_eq!(read.updated_at.as_deref(), updated.as_ref().and_then(Value::as_str));
}

/// Setting keys and record fields convert both ways: `camel` then `snake` gives back any
/// lowercase snake-case key, `snake` then `camel` any camel-case field made from one, and every
/// setting's field is in its group of the settings Lexicon.
#[hegel::test(test_cases = 300)]
fn setting_keys_and_fields_convert_both_ways(tc: TestCase) {
    let key: String = tc.draw(hegel::one_of!(
        gs::sampled_from(SCHEMA.iter().map(|d| d.key.to_string()).collect::<Vec<_>>()),
        gs::from_regex("[a-z][a-z0-9]{0,6}(_[a-z][a-z0-9]{0,6}){0,3}").fullmatch(true),
    ));
    let field = publish::camel(&key);
    assert!(!field.contains('_'), "{field}");
    assert_eq!(publish::snake(&field), key);
    assert_eq!(publish::camel(&publish::snake(&field)), field);
    if let Some(def) = settings::find(&key) {
        let doc = &lexicon::document(SETTINGS).unwrap()["defs"][group(def)]["properties"];
        assert!(doc.get(&field).is_some(), "{field}");
    }
}

/// A share as the record should have it: a whole percent from 0 to 100, at most half a percent
/// from the share (clamped to 0 and 1).
fn assert_percent(p: &Value, x: f64) {
    let p = p.as_i64().unwrap_or_else(|| panic!("{p} for {x}"));
    assert!((0..=100).contains(&p), "{x} became {p}");
    if !x.is_nan() {
        assert!((p as f64 - (x * 100.0).clamp(0.0, 100.0)).abs() <= 0.5, "{x} became {p}");
    }
}

/// A tempo as the record should have it: a whole BPM, 0 for anything below.
fn assert_bpm(b: &Value, x: f64) {
    let b = b.as_i64().unwrap();
    assert!(b >= 0 && (b as f64 - x.max(0.0)).abs() <= 0.5, "{x} became {b}");
}

fn share() -> impl hegel::PrintableGenerator<f64> {
    hegel::one_of!(gs::floats::<f64>().min_value(0.0).max_value(1.0), gs::floats::<f64>())
}

#[hegel::test_helper]
fn count(tc: &TestCase) -> Count {
    let n = tc.draw(gs::integers::<u64>().max_value(1 << 40));
    if tc.draw(gs::booleans()) { Count::Exact { count: n } } else { Count::AtLeast { at_least: n } }
}

/// A profile document as `profile::document` makes one: any sections, shares and tempos (even
/// out of range), and lists within the Lexicon's limits, except the signature tracks, which can
/// run past the 50 a record names.
#[hegel::test_helper]
fn document(tc: &TestCase) -> ProfileDocument {
    let some = || tc.draw(gs::booleans());
    let x = || tc.draw(share());
    let tempo = || tc.draw(gs::floats::<f64>().min_value(-20.0).max_value(400.0));
    let shares = || {
        let names: Vec<String> = tc.draw(gs::vecs(gs::text().max_size(12)).max_size(10));
        names.into_iter().map(|name| DocShare { name, share: x() }).collect::<Vec<_>>()
    };
    let key = || tc.draw(keys()).code();
    let opt = |v: f64| some().then_some(v);
    ProfileDocument {
        version: profile::DOCUMENT_VERSION,
        generated_at: time(tc),
        precision: if some() { Precision::Exact } else { Precision::Rounded },
        basis: tc.draw(gs::sampled_from(vec![Basis::Plays, Basis::Sessions, Basis::Collection])),
        headline: tc.draw(gs::optional(gs::text().max_size(120))),
        tags: tc.draw(gs::vecs(gs::text().max_size(60)).max_size(20)),
        sets: some().then(|| DocSets {
            count: count(tc),
            since: tc.draw(gs::optional(gs::integers::<i32>().min_value(1950).max_value(2100))),
            median_tracks: tc.draw(gs::integers::<u64>().max_value(500)),
        }),
        tempo: some().then(|| DocTempo { low: tempo(), high: tempo(), median: tempo() }),
        arc: some().then(|| DocArc {
            shape: tc.draw(gs::sampled_from(vec![
                ArcShape::Builds,
                ArcShape::Steady,
                ArcShape::WindsDown,
                ArcShape::Varies,
            ])),
            builds: x(),
            steady: x(),
            winds_down: x(),
        }),
        mixing: some().then(|| DocMixing { harmonic: opt(x()), smooth_tempo: opt(x()), genre_changes: opt(x()) }),
        keys: some().then(|| DocKeys {
            top: (0..tc.draw(gs::integers::<usize>().max_value(12))).map(|_| key()).collect(),
            minor: opt(x()),
        }),
        energy: some().then(|| DocEnergy { high: x(), mid: x(), low: x() }),
        genres: some().then(&shares),
        labels: some().then(&shares),
        artists: some().then(&shares),
        era: some().then(|| DocEra {
            median_year: tc.draw(gs::optional(gs::integers::<i32>().min_value(1900).max_value(2100))),
            recent: opt(x()),
        }),
        signature: some().then(|| {
            let tracks = tc.draw(
                gs::vecs(gs::tuples!(
                    hegel::one_of!(gs::just(String::new()), gs::text().max_size(8)),
                    gs::text().max_size(8)
                ))
                .max_size(60),
            );
            tracks.into_iter().map(|(artist, title)| DocTrack { artist, title }).collect()
        }),
        prep: some().then(|| DocPrep { hot_cues: x(), memory_cues: x(), loops: x() }),
        collection: some().then(|| DocCollection { tracks: count(tc), playlists: count(tc) }),
        style: some().then(|| {
            tc.draw(gs::subsequences(&profile::STYLE_SETTINGS))
                .into_iter()
                .map(|k| {
                    let def = settings::find(k).unwrap();
                    (publish::camel(k), tc.draw(gs::sampled_from(def.choices)).id.to_string())
                })
                .collect()
        }),
    }
}

fn assert_count(v: &Value, c: Count) {
    match c {
        Count::Exact { count } => assert_eq!(v, &json!({ "count": count })),
        Count::AtLeast { at_least } => assert_eq!(v, &json!({ "atLeast": at_least })),
    }
}

/// Whatever the document holds, its profile record fits the profile Lexicon and survives JSON:
/// shares become whole percents and tempos whole BPM, counts keep whether they're exact, the
/// signature names at most 50 tracks (an artist only when there is one), and a section the
/// document leaves out stays out. A share over 100, a missing basis or more than 20 tags is
/// refused.
#[hegel::test(test_cases = 200)]
fn a_profile_record_fits_its_lexicon_whatever_the_document_holds(tc: TestCase) {
    let doc = document(&tc);
    let r = publish::profile_record(&doc);
    lexicon::validate(PROFILE, &r).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&r.to_string()).unwrap(), r);
    assert_eq!(r["generatedAt"], publish::timestamp(doc.generated_at));
    assert_eq!(r.get("headline").and_then(Value::as_str), doc.headline.as_deref());
    let tags: Vec<Value> = doc.tags.iter().map(|t| json!({ "name": t })).collect();
    assert_eq!(r.get("tags"), (!tags.is_empty()).then(|| json!(tags)).as_ref());
    let present = |field: &str, section: bool| assert_eq!(r.get(field).is_some(), section, "{field}");
    present("sets", doc.sets.is_some());
    present("tempo", doc.tempo.is_some());
    present("arc", doc.arc.is_some());
    present("mixing", doc.mixing.is_some());
    present("keys", doc.keys.is_some());
    present("energy", doc.energy.is_some());
    present("genres", doc.genres.is_some());
    present("era", doc.era.is_some());
    present("signature", doc.signature.is_some());
    present("prep", doc.prep.is_some());
    present("collection", doc.collection.is_some());
    present("style", doc.style.is_some());
    if let Some(s) = &doc.sets {
        assert_count(&r["sets"]["total"], s.count);
        assert_eq!(r["sets"].get("since").and_then(Value::as_i64), s.since.map(i64::from));
    }
    if let Some(t) = &doc.tempo {
        for (field, v) in [("low", t.low), ("high", t.high), ("median", t.median)] {
            assert_bpm(&r["tempo"][field], v);
        }
    }
    if let Some(a) = &doc.arc {
        for (field, v) in [("builds", a.builds), ("steady", a.steady), ("windsDown", a.winds_down)] {
            assert_percent(&r["arc"][field], v);
        }
    }
    if let Some(m) = &doc.mixing {
        for (field, v) in [("harmonic", m.harmonic), ("smoothTempo", m.smooth_tempo), ("genreChanges", m.genre_changes)]
        {
            match v {
                Some(v) => assert_percent(&r["mixing"][field], v),
                None => assert!(r["mixing"].get(field).is_none()),
            }
        }
    }
    if let Some(k) = &doc.keys {
        assert_eq!(r["keys"]["top"], json!(k.top.iter().map(|key| json!({ "key": key })).collect::<Vec<_>>()));
    }
    for (field, list) in [("genres", &doc.genres), ("labels", &doc.labels), ("artists", &doc.artists)] {
        for (s, v) in list.iter().flatten().zip(r[field].as_array().into_iter().flatten()) {
            assert_eq!(v["name"], s.name);
            assert_percent(&v["share"], s.share);
        }
    }
    if let Some(tracks) = &doc.signature {
        let named = r["signature"].as_array().unwrap();
        assert_eq!(named.len(), tracks.len().min(publish::SIGNATURE_LIMIT));
        for (t, v) in tracks.iter().zip(named) {
            let artist = (!t.artist.is_empty()).then(|| json!(t.artist));
            assert_eq!((v.get("artist"), &v["title"]), (artist.as_ref(), &json!(t.title)));
        }
    }
    if let Some(c) = &doc.collection {
        assert_count(&r["collection"]["tracks"], c.tracks);
        assert_count(&r["collection"]["playlists"], c.playlists);
    }

    let mut broken = r.clone();
    let want = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => {
            broken["energy"] = json!({ "high": 101, "mid": 0, "low": 0 });
            "energy.high is 101, outside"
        }
        1 => {
            broken.as_object_mut().unwrap().remove("basis");
            "missing basis"
        }
        _ => {
            broken["tags"] = json!(vec![json!({ "name": "x" }); 21]);
            "tags has 21 entries"
        }
    };
    let err = lexicon::validate(PROFILE, &broken).unwrap_err().to_string();
    assert!(err.contains(want), "{err}");
}

/// ISRCs as libraries spell them, and what a set record should name (`None`: left out).
const ISRCS: [(&str, Option<&str>); 6] = [
    ("", None),
    ("GB-AHT-17-00123", Some("GBAHT1700123")),
    (" gbaht1700123 ", Some("GBAHT1700123")),
    ("us-s1z-99-00001", Some("USS1Z9900001")),
    ("123", None),
    ("GB-AHT-17-0012\u{3a9}", None),
];
/// Keys as libraries spell them, and the Camelot key a set record should name.
const KEYS: [(&str, Option<&str>); 6] =
    [("", None), ("Am", Some("8A")), ("8A", Some("8A")), ("C", Some("8B")), ("12b", Some("12B")), ("H#", None)];

/// What a set says that must never go out: notes, the recording, sessions, files, the label and
/// the genre.
const PRIVATE: &str = "PRIVATE";

/// A set's track `i` (with `may_break`, now and then with a title too long for a record), and
/// (as `isrc` and `key`) what its record should name.
#[hegel::test_helper]
fn entry(tc: &TestCase, i: usize, may_break: bool) -> (SetEntry, Option<&'static str>, Option<&'static str>) {
    let (isrc, isrc_out) = tc.draw(gs::sampled_from(ISRCS.to_vec()));
    let (key, key_out) = tc.draw(gs::sampled_from(KEYS.to_vec()));
    let mut title: String = tc.draw(gs::text().max_size(10));
    if may_break && tc.draw(gs::weighted_booleans(0.05)) {
        title = "t".repeat(301);
    }
    let bpm = tc.draw(hegel::one_of!(
        gs::just(0.0),
        gs::floats::<f64>().min_value(-5.0).max_value(300.0),
        gs::floats::<f64>()
    ));
    let e = SetEntry {
        title,
        artist: tc.draw(hegel::one_of!(
            gs::sampled_from(vec!["", "Bicep", "Four Tet", "Overmono"]).map(String::from),
            gs::text().max_size(10)
        )),
        album: tc.draw(hegel::one_of!(
            gs::sampled_from(vec!["", " Isles ", "Selected Ambient Works 85-92"]).map(String::from),
            gs::text().max_size(10),
            gs::just("a".repeat(257))
        )),
        label: format!("{PRIVATE}-LABEL"),
        genre: format!("{PRIVATE}-GENRE"),
        isrc: isrc.into(),
        key: key.into(),
        bpm,
        year: 2021,
        duration_secs: 300,
        file: Some(format!("/Contents/{PRIVATE}-FILE-{i}.mp3")),
        start_seconds: tc.draw(gs::optional(gs::integers::<u32>())),
        musicbrainz: MusicBrainzIds {
            recording: tc.draw(gs::optional(gs::integers::<u128>())).map(Uuid::from_u128),
            release: tc.draw(gs::optional(gs::integers::<u128>())).map(Uuid::from_u128),
            artists: tc.draw(gs::vecs(gs::integers::<u128>()).max_size(3)).into_iter().map(Uuid::from_u128).collect(),
        },
        unnamed: tc.draw(gs::weighted_booleans(0.2)),
        origin: tc.draw(gs::sampled_from(vec![EntryOrigin::Session, EntryOrigin::Recording, EntryOrigin::Added])),
        session: Some(format!("{PRIVATE}SESSION")),
    };
    (e, isrc_out, key_out)
}

/// What a named track's record should give as its ISRC and key.
type Named = (Option<&'static str>, Option<&'static str>);

/// A set as the DJ might have described it, with what its tracks' records should name, and
/// (with `may_break`) now and then something its record can't hold: why, for each.
#[hegel::test_helper]
fn set(tc: &TestCase, may_break: bool) -> (Set, Vec<Named>, Vec<&'static str>) {
    let mut broken = Vec::new();
    let mut breaks = |why: &'static str| {
        let b = may_break && tc.draw(gs::weighted_booleans(0.06));
        if b {
            broken.push(why);
        }
        b
    };
    let title =
        if breaks("the title") { Some("x".repeat(101)) } else { tc.draw(gs::optional(gs::text().max_size(100))) };
    let kind = if breaks("the kind") { Some("k".repeat(65)) } else { tc.draw(gs::optional(gs::text().max_size(64))) };
    let venue =
        if breaks("the venue") { Some("v".repeat(101)) } else { tc.draw(gs::optional(gs::text().max_size(100))) };
    let country = if breaks("the country") {
        Some(" G ".to_string())
    } else {
        tc.draw(gs::optional(hegel::one_of!(gs::just("  ".to_string()), gs::from_regex("[A-Z]{2,3}").fullmatch(true))))
    };
    let tid = || gs::from_regex("[2-7a-j][2-7a-z]{12}").fullmatch(true);
    let event = if breaks("the event") {
        Some("https://example.com/event".to_string())
    } else {
        tc.draw(gs::optional(tid().map(|k| repo::at_uri("did:plc:club", "community.lexicon.calendar.event", &k))))
    };
    let did = || gs::from_regex("did:plc:[a-z2-7]{24}").fullmatch(true);
    let mut with: Vec<Partner> = tc
        .draw(gs::vecs(gs::tuples!(gs::text().max_size(64), gs::optional(did()))).max_size(3))
        .into_iter()
        .map(|(name, did)| Partner { name, did })
        .collect();
    if breaks("a partner's DID") {
        with.push(Partner { name: "Ayla".into(), did: Some("ayla.example.com".into()) });
    }
    if breaks("the partners") {
        with.extend((0..21).map(|i| Partner { name: format!("P{i}"), did: None }));
    }
    let link = if breaks("the link") {
        Some("mixcloud dot com".to_string())
    } else {
        tc.draw(gs::optional(
            gs::from_regex("[a-z0-9-]{1,12}").fullmatch(true).map(|s| format!("https://www.mixcloud.com/dj/{s}/")),
        ))
    };
    let n = tc.draw(gs::integers::<usize>().max_value(6));
    let (mut tracks, mut out) = (Vec::new(), Vec::new());
    for i in 0..n {
        let (e, isrc, key) = entry(tc, i, may_break);
        tracks.push(e);
        out.push((isrc, key));
    }
    let now = time(tc);
    let set = Set {
        id: tc.draw(tid()),
        title,
        date: tc.draw(
            gs::default::<NaiveDate>()
                .min_value(NaiveDate::from_num_days_from_ce_opt(730_000).unwrap())
                .max_value(NaiveDate::from_num_days_from_ce_opt(740_000).unwrap()),
        ),
        kind,
        venue,
        city: tc.draw(gs::optional(gs::text().max_size(12))),
        country,
        event,
        with,
        link,
        recording: tc.draw(gs::booleans()).then(|| Recording {
            path: format!("/Users/dj/{PRIVATE}-REC.wav").into(),
            report: Some(format!("{PRIVATE}-REPORT")),
        }),
        notes: Some(format!("{PRIVATE}-NOTES")),
        sessions: vec![format!("{PRIVATE}SESSION")],
        tracks,
        created_at: now,
        updated_at: now,
    };
    (set, out, broken)
}

fn trimmed(s: &Option<String>) -> Option<String> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// `r` is `set`'s record with the tracks at the positions in `hidden` unidentified: each field
/// trimmed (blank ones left out), an address only with a country, sources in a fixed order,
/// tracks named as `names` says or saying only where they start, and nothing private.
fn assert_set_record(r: &Value, set: &Set, names: &[Named], hidden: &BTreeSet<usize>) {
    for (field, v) in [("title", &set.title), ("kind", &set.kind), ("venue", &set.venue), ("event", &set.event)] {
        assert_eq!(r.get(field).and_then(Value::as_str), trimmed(v).as_deref(), "{field}");
    }
    assert_eq!(r["date"], set.date.format("%Y-%m-%d").to_string());
    match trimmed(&set.country) {
        None => assert!(r.get("location").is_none()),
        Some(country) => {
            let mut place = json!({ "$type": ADDRESS, "country": country });
            if let Some(v) = trimmed(&set.venue) {
                place["name"] = v.into();
            }
            if let Some(c) = trimmed(&set.city) {
                place["locality"] = c.into();
            }
            assert_eq!(r["location"], place);
        }
    }
    let with: Vec<Value> = set
        .with
        .iter()
        .map(|p| match trimmed(&p.did) {
            Some(did) => json!({ "name": p.name.trim(), "did": did }),
            None => json!({ "name": p.name.trim() }),
        })
        .collect();
    assert_eq!(r.get("playedWith"), (!with.is_empty()).then(|| json!(with)).as_ref());
    assert_eq!(r.get("links"), trimmed(&set.link).map(|l| json!([{ "uri": l }])).as_ref());
    let sources: Vec<Value> =
        [(EntryOrigin::Session, "history"), (EntryOrigin::Recording, "mix"), (EntryOrigin::Added, "added")]
            .into_iter()
            .filter(|(o, _)| set.tracks.iter().any(|e| e.origin == *o))
            .map(|(_, kind)| json!({ "kind": kind }))
            .collect();
    assert_eq!(r.get("sources"), (!sources.is_empty()).then(|| json!(sources)).as_ref());
    let tracks = r["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), set.tracks.len());
    for (i, ((e, (isrc, key)), t)) in set.tracks.iter().zip(names).zip(tracks).enumerate() {
        let start = e.start_seconds.map(Value::from);
        if e.unnamed || hidden.contains(&(i + 1)) {
            let mut want = json!({ "unidentified": true });
            if let Some(s) = start {
                want["startSeconds"] = s;
            }
            assert_eq!(t, &want, "track {}", i + 1);
            continue;
        }
        let text = |s: &str| Some(s.trim()).filter(|s| !s.is_empty()).map(Value::from);
        assert_eq!(t.get("title").cloned(), text(&e.title), "track {}", i + 1);
        assert_eq!(t.get("artist").cloned(), text(&e.artist));
        assert_eq!(t.get("isrc").and_then(Value::as_str), *isrc);
        assert_eq!(t.get("key").and_then(Value::as_str), *key);
        assert_eq!(t.get("startSeconds"), start.as_ref());
        let release = text(&e.album).filter(|a| a.as_str().is_some_and(|a| a.chars().count() <= 256));
        assert_eq!(t.get("releaseName").cloned(), release);
        let mbid = |id: Option<Uuid>| id.map(|id| Value::from(format!("mbid:{id}")));
        assert_eq!(t.get("recordingMbId").cloned(), mbid(e.musicbrainz.recording));
        assert_eq!(t.get("releaseMbId").cloned(), mbid(e.musicbrainz.release));
        // Artists one by one, as the profile counts them, when the credit names more than one or
        // the tags identify its one; their IDs only when the tags have one for each.
        let (names, ids) = (profile::artist_credits(&e.artist), &e.musicbrainz.artists);
        let paired = ids.len() == names.len();
        match t.get("artists") {
            None => assert!(names.len() < 2 && !(paired && names.len() == 1), "{names:?} {ids:?}"),
            Some(artists) => {
                let want: Vec<Value> = names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| match paired {
                        true => json!({ "artistName": n, "artistMbId": format!("mbid:{}", ids[i]) }),
                        false => json!({ "artistName": n }),
                    })
                    .collect();
                assert_eq!(artists, &json!(want));
            }
        }
        assert!(t.get("transitionIn").is_none(), "no recording's report");
        match t.get("bpmCentis") {
            None => assert!(!e.bpm.is_finite() || e.bpm <= 0.0, "{}", e.bpm),
            Some(c) => {
                let c = c.as_u64().unwrap() as f64;
                assert!(
                    e.bpm > 0.0 && (e.bpm * 100.0 > u32::MAX as f64 || (c - e.bpm * 100.0).abs() <= 0.5),
                    "{} {c}",
                    e.bpm
                );
            }
        }
        assert!(t.get("unidentified").is_none());
    }
    let text = r.to_string();
    assert!(!text.contains(PRIVATE), "{text}");
    for field in ["updatedAt", "unnamed", "origin", "notes", "recording", "sessions", "musicbrainz"] {
        assert!(!text.contains(&format!("\"{field}\"")), "{field}: {text}");
    }
}

/// Whatever the set, its record says where and when it was played and its tracks in order, with
/// the ones marked unnamed or hidden saying only where they start, and nothing private; it
/// survives JSON, and fits the set Lexicon exactly when the set holds nothing the Lexicon can't
/// (a title over 100 graphemes, a partner's DID that isn't one, more than 20 partners...).
#[hegel::test(test_cases = 300)]
fn a_set_record_names_only_what_it_should_and_fits_unless_the_set_cant(tc: TestCase) {
    let (s, names, mut broken) = set(&tc, true);
    let hidden: BTreeSet<usize> = tc
        .draw(gs::vecs(gs::integers::<usize>().min_value(1).max_value(s.tracks.len() + 2)).max_size(3))
        .into_iter()
        .collect();
    // A title too long breaks the record only when the track is named in it.
    let named = |i: usize, e: &SetEntry| !e.unnamed && !hidden.contains(&(i + 1));
    if s.tracks.iter().enumerate().any(|(i, e)| named(i, e) && e.title.graphemes(true).count() > 300) {
        broken.push("a track's title");
    }
    let now = time(&tc);
    let r = publish::set_record(&s, &hidden, None, now);
    assert_set_record(&r, &s, &names, &hidden);
    assert_eq!(r["createdAt"], publish::timestamp(now));
    assert_eq!(serde_json::from_str::<Value>(&r.to_string()).unwrap(), r);
    let checked = lexicon::validate(SET, &r);
    assert_eq!(checked.is_ok(), broken.is_empty(), "{broken:?}: {checked:?}");
}

/// The Lexicon check refuses each way of breaking a set record, naming what's wrong, and takes
/// each change the Lexicon allows: a kind or source it doesn't know, a link's name, a location of
/// another type, a track's MusicBrainz ID, an optional field left out.
#[hegel::test(test_cases = 300)]
fn the_lexicon_check_refuses_each_break_and_takes_each_allowed_change(tc: TestCase) {
    let (s, _, _) = set(&tc, false);
    let mut r = publish::set_record(&s, &BTreeSet::new(), None, time(&tc));
    let n = s.tracks.len();
    let track = || tc.draw(gs::integers::<usize>().max_value(n.max(1) - 1));
    let want: Option<String> = match tc.draw(gs::integers::<u8>().max_value(17)) {
        0 => {
            let f = tc.draw(gs::sampled_from(vec!["date", "tracks", "createdAt"]));
            r.as_object_mut().unwrap().remove(f);
            Some(format!("the record is missing {f}"))
        }
        1 => {
            let f = tc.draw(gs::sampled_from(vec!["mood", "with", "link", "source", "updatedAt", "notes"]));
            r[f] = json!("x");
            Some(format!("{f} isn't in the Lexicon"))
        }
        2 => {
            r["$type"] =
                tc.draw(gs::sampled_from(vec![json!(PROFILE), json!(SETTINGS), json!("app.example.set"), json!(1)]));
            Some(format!("its $type isn't {SET}"))
        }
        3 => {
            r = tc.draw(gs::sampled_from(vec![json!([]), json!("set"), Value::Null]));
            Some("it isn't an object".into())
        }
        4 => {
            r["title"] = "x".repeat(tc.draw(gs::integers::<usize>().min_value(101).max_value(300))).into();
            Some("title is longer than 100 graphemes".into())
        }
        5 if n > 0 => {
            let i = track();
            let (field, value, why) = tc.draw(gs::sampled_from(vec![
                ("bpmCentis", json!(12500.5), "should be a whole number"),
                ("bpmCentis", json!(-1), "is -1, outside"),
                ("startSeconds", json!("0:00"), "should be a whole number"),
                ("unidentified", json!("yes"), "should be true or false"),
                ("key", json!("10AB"), "is 4 bytes long"),
                ("isrc", json!("GB"), "is 2 bytes long"),
                ("title", json!(["Glue"]), "should be text"),
            ]));
            r["tracks"][i][field] = value;
            Some(format!("tracks[{i}].{field} {why}"))
        }
        6 => {
            r["date"] = "3 Oct".into();
            Some("date is 5 bytes long".into())
        }
        7 => {
            r["createdAt"] = tc.draw(gs::sampled_from(vec!["yesterday", "2026-10-04", "2026-10-04 09:30"])).into();
            Some("createdAt isn't a date and time".into())
        }
        8 => {
            r["playedWith"] = json!([{ "did": "did:plc:abc" }]);
            Some("playedWith[0] is missing name".into())
        }
        9 => {
            r["location"] = json!({ "$type": ADDRESS, "name": "Fabric" });
            Some("location is missing country".into())
        }
        10 => {
            r["links"] = json!(vec![json!({ "uri": "https://example.com/" }); 11]);
            Some("links has 11 entries".into())
        }
        11 => {
            r["kind"] = tc.draw(gs::text().max_size(64)).into();
            None
        }
        12 => {
            r["sources"] = json!([{ "kind": tc.draw(gs::from_regex("[a-z]{1,20}").fullmatch(true)) }]);
            None
        }
        13 => {
            r["links"] =
                json!([{ "uri": "https://station.example/archive", "name": tc.draw(gs::text().max_size(64)) }]);
            None
        }
        14 => {
            r["location"] =
                json!({ "$type": "community.lexicon.location.geo", "latitude": "51.52", "longitude": "-0.10" });
            None
        }
        15 if n > 0 => {
            r["tracks"][track()]["recordingMbId"] = "mbid:c1a3e5c0-7b5e-4d51-9d0e-3c8b1f1f8a2b".into();
            None
        }
        16 if n > 0 => {
            let i = track();
            r["tracks"][i]["recordingMbId"] = "c1a3e5c0-7b5e-4d51-9d0e-3c8b1f1f8a2b".into();
            Some(format!("tracks[{i}].recordingMbId isn't a URI"))
        }
        _ => {
            let f = tc.draw(gs::sampled_from(vec![
                "title",
                "kind",
                "venue",
                "location",
                "event",
                "playedWith",
                "links",
                "sources",
            ]));
            r.as_object_mut().unwrap().remove(f);
            None
        }
    };
    let checked = lexicon::validate(SET, &r);
    match want {
        None => assert!(checked.is_ok(), "{checked:?}"),
        Some(want) => {
            let err = checked.unwrap_err();
            assert_eq!(err.code(), "invalid");
            let err = err.to_string();
            assert!(
                err.starts_with("the set record doesn't fit its Lexicon: ") && err.contains(&want),
                "{err} (wanted {want})"
            );
        }
    }
    let err = lexicon::validate("app.example.set", &r).unwrap_err().to_string();
    assert!(err.contains("isn't one of locallibrary's record types"), "{err}");
}

/// Sessions granted these scopes, and whether they may write sets.
fn scope_sets() -> Vec<(Vec<String>, bool)> {
    let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    vec![
        (s(&["atproto"]), false),
        (crate::mock_pds::granted(&atmosphere::publish_scopes()), true),
        (s(&["atproto", "transition:generic"]), true),
        (s(&["atproto", "repo:*"]), true),
        (s(&["atproto", &format!("repo:{SETTINGS}?action=create&action=update")]), false),
        (s(&["atproto", &format!("repo:{SET}?action=create")]), false),
        (s(&["atproto", &format!("repo:{SET}?action=create&action=update")]), true),
    ]
}

fn account_with(scopes: Vec<String>) -> AccountInfo {
    AccountInfo {
        did: DID.into(),
        handle: Some("dj.example.com".into()),
        pds: "https://127.0.0.1:9".into(),
        auth_server: "https://127.0.0.1:9".into(),
        scopes,
        signed_in_at: Utc::now(),
        sign_in_again_by: Utc::now() + chrono::Duration::days(14),
        access_expires_at: None,
        client: Client::Loopback,
        storage: Storage::File,
    }
}

/// A plan for a set over what's published in its place: signed out, it can only be looked at;
/// signed in, it keeps when the published record says it was first published (when that's a
/// time), changes something exactly when the record differs in more than its times, and says
/// which scope the session lacks. Applying a plan that changes nothing writes nothing and says
/// what's there; one the session can't write, or with nobody signed in, stops with nothing sent.
#[hegel::test(test_cases = 100)]
fn a_plan_keeps_when_a_set_first_went_out_and_knows_when_nothing_changed(tc: TestCase) {
    let (s, _, _) = set(&tc, false);
    let record = publish::set_record(&s, &BTreeSet::new(), None, time(&tc));
    let (scopes, may_write) = tc.draw(gs::sampled_from(scope_sets()));
    let signed_in = tc.draw(gs::booleans());
    let account = signed_in.then(|| account_with(scopes));
    let uri = repo::at_uri(DID, SET, &s.id);
    let mut value = record.clone();
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => value["createdAt"] = publish::timestamp(time(&tc)).into(),
        1 => value["title"] = "Published from elsewhere".into(),
        2 => value["createdAt"] = json!(1_700_000_000),
        _ => drop(value.as_object_mut().unwrap().remove("createdAt")),
    }
    let current = tc.draw(gs::booleans()).then(|| Fetched {
        uri: uri.clone(),
        cid: tc.draw(gs::optional(gs::just("bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm".to_string()))),
        value: value.clone(),
    });
    let plan = publish::plan_over(account.clone(), SET, &s.id, record.clone(), current.clone()).unwrap();

    let mut want = record.clone();
    if signed_in && let Some(created) = current.as_ref().map(|c| &c.value["createdAt"]).filter(|c| c.is_string()) {
        want["createdAt"] = created.clone();
    }
    let changed = !signed_in || current.as_ref().is_none_or(|c| timeless(c.value.clone()) != timeless(record.clone()));
    assert_eq!(plan.record, want);
    assert_eq!(plan.changed, changed);
    assert_eq!(plan.uri, signed_in.then_some(uri));
    assert_eq!(plan.current, if signed_in { current.clone() } else { None });
    let missing = format!("repo:{SET}?action=create&action=update");
    assert_eq!(plan.missing_scope, (signed_in && !may_write).then_some(missing));
    assert_eq!((plan.collection.as_str(), plan.rkey.as_str()), (SET, s.id.as_str()));

    let dir = tempfile::tempdir().unwrap();
    let applied = publish::apply(dir.path(), &plan);
    match (signed_in, changed) {
        (false, _) => assert!(applied.unwrap_err().to_string().contains("not signed in")),
        (true, false) => {
            let p = applied.unwrap();
            let c = current.clone().unwrap();
            assert_eq!((p.written, p.uri, p.cid, p.record), (false, c.uri, c.cid, want));
        }
        (true, true) if !may_write => assert_eq!(applied.unwrap_err().reason(), Some(Reason::MissingScope)),
        // Planned for an account this computer isn't signed in to: nothing to write with.
        (true, true) => assert!(applied.unwrap_err().to_string().contains("not signed in")),
    }
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing written here");

    let mut broken = record;
    broken["mood"] = "dark".into();
    assert_eq!(publish::plan_over(account, SET, &s.id, broken, current).unwrap_err().code(), "invalid");
}

/// The keys `plays publish` remembered read back for the account and history list they were
/// remembered for, and for nothing else; with nothing remembered there are none, and a damaged
/// file is an error rather than nothing.
#[hegel::test(test_cases = 30)]
fn the_keys_plays_publish_gave_read_back(tc: TestCase) {
    let dids = vec!["did:plc:a", "did:plc:b", "did:plc:c"];
    let lists = vec!["0123", "4567", "89ab"];
    let keys: Vec<(&str, &str, String)> = tc.draw(
        gs::vecs(gs::tuples!(
            gs::sampled_from(dids.clone()),
            gs::sampled_from(lists.clone()),
            gs::from_regex("[2-7a-j][2-7a-z]{12}").fullmatch(true)
        ))
        .max_size(6),
    );
    let mut model: IndexMap<(&str, &str), String> = IndexMap::new();
    let mut file: IndexMap<&str, IndexMap<&str, String>> = IndexMap::new();
    for (did, list, key) in keys {
        model.insert((did, list), key.clone());
        file.entry(did).or_default().insert(list, key);
    }
    let dir = tempfile::tempdir().unwrap();
    for did in &dids {
        for list in &lists {
            assert_eq!(publish::published_set_key(dir.path(), did, list).unwrap(), None);
        }
    }
    std::fs::write(dir.path().join(publish::SET_KEYS_FILE), json!(file).to_string()).unwrap();
    for did in &dids {
        for list in &lists {
            assert_eq!(publish::published_set_key(dir.path(), did, list).unwrap(), model.get(&(*did, *list)).cloned());
        }
    }
    std::fs::write(dir.path().join(publish::SET_KEYS_FILE), "{\"did:plc:a\": [").unwrap();
    assert!(publish::published_set_key(dir.path(), "did:plc:a", "0123").is_err());
}

/// Something that happens to My Settings between publishes.
#[derive(Debug, Clone)]
enum Step {
    /// The DJ changes settings in the profile's base.
    Change(IndexMap<String, String>),
    /// My Settings are planned and published.
    Publish,
    /// Another app (or computer) publishes My Settings.
    Elsewhere(&'static str),
    /// Another app publishes between the plan and applying it.
    Overtaken(&'static str),
    /// The server refuses the next publish so; then the DJ deals with it and publishes again.
    Refused(Writes),
    /// The server changes the DPoP nonce writes must carry.
    NewNonce,
    /// The DJ signs in without allowing publishing, tries, and signs in again allowing it.
    Unpermitted,
}

/// How many kinds of [`Step`] there are.
const KINDS: u8 = 7;

/// A step of the kind numbered `kind`.
#[hegel::test_helper]
fn step(tc: &TestCase, kind: u8) -> Step {
    let elsewhere = || tc.draw(gs::sampled_from(vec!["on", "off"]));
    match kind {
        0 => Step::Change(canonical_settings(tc, 1, 3)),
        1 => Step::Publish,
        2 => Step::Elsewhere(elsewhere()),
        3 => Step::Overtaken(elsewhere()),
        4 => Step::Refused(tc.draw(gs::sampled_from(vec![
            Writes::RefuseScope,
            Writes::RefuseToken,
            Writes::RefuseRecord,
        ]))),
        5 => Step::NewNonce,
        _ => Step::Unpermitted,
    }
}

/// My Settings as another app publishes them.
fn theirs(quantize: &str) -> Value {
    json!({ "$type": SETTINGS, "player": { "quantize": quantize }, "updatedAt": "2026-10-05T00:00:00.000Z" })
}

/// What the test knows: the profile's base, what the server holds, and the nonce it wants.
struct Model {
    base: IndexMap<String, String>,
    server: Option<Value>,
    nonce: String,
}

impl Model {
    /// Plan publishing My Settings, checking the plan against the model: `None` when there's
    /// nothing to publish.
    fn plan(&self, root: &Path) -> Option<api::PlannedPublish> {
        let plan = api::settings_publish_plan(root);
        if self.base.is_empty() {
            assert_eq!(plan.unwrap_err().reason(), Some(Reason::EmptyProfile));
            return None;
        }
        let plan = plan.unwrap();
        let base: IndexMap<String, String> = self.base.clone().into_iter().collect();
        let want = timeless(publish::settings_record(&base, Utc::now()));
        let changed = self.server.as_ref().is_none_or(|s| timeless(s.clone()) != want);
        assert_eq!(timeless(plan.record.clone()), want);
        assert_eq!(plan.current.as_ref().map(|c| &c.value), self.server.as_ref());
        assert_eq!((plan.changed, plan.noop, plan.destructive), (changed, !changed, changed && self.server.is_some()));
        assert_eq!(plan.uri, Some(repo::at_uri(DID, SETTINGS, "self")));
        Some(plan)
    }

    /// Publish My Settings, applying the plan or (`at_once`) planning and publishing in one go,
    /// refused as `refused` says: the server ends up with what the model says, a write replaces
    /// only the record that was read, and a refusal leaves it alone.
    fn publish(&mut self, root: &Path, pds: &MockPds, refused: Option<&Writes>, at_once: bool) {
        let Some(plan) = self.plan(root) else { return };
        let before = pds.record(DID, SETTINGS, "self");
        let writes = pds.calls(PUT).len();
        let result = if at_once { api::settings_publish_apply(root) } else { plan.apply(None, &Ctx::silent()) };
        if !plan.changed {
            assert!(!result.unwrap().written);
            assert_eq!(pds.calls(PUT).len(), writes, "nothing written");
            return;
        }
        let Some(refusal) = refused else {
            let published = result.unwrap();
            let (stored, cid) = pds.record(DID, SETTINGS, "self").unwrap();
            assert!(published.written);
            assert_eq!(published.cid.as_deref(), Some(cid.as_str()));
            assert_eq!(timeless(stored.clone()), timeless(plan.record.clone()));
            if !at_once {
                assert_eq!(stored, plan.record, "exactly the record planned");
            }
            let last = pds.calls(PUT).pop().unwrap();
            assert_eq!(
                last.json()["swapRecord"],
                before.map_or(Value::Null, |(_, cid)| cid.into()),
                "swaps what was read"
            );
            assert_eq!(last.dpop().unwrap().1["nonce"], self.nonce.as_str(), "follows the server's nonce");
            self.server = Some(stored);
            return;
        };
        let err = result.unwrap_err();
        match refusal {
            Writes::RefuseScope => assert_eq!(err.reason(), Some(Reason::MissingScope), "{err}"),
            Writes::RefuseToken => assert_eq!(err.reason(), Some(Reason::SessionEnded), "{err}"),
            _ => assert!(
                err.to_string().contains("refused to publish My Settings (InvalidRequest: Record is too big)"),
                "{err}"
            ),
        }
        assert_eq!(pds.record(DID, SETTINGS, "self"), before, "a refusal changes nothing");
    }
}

/// Random edits, publishes, other apps' writes, refusals, nonce changes and sessions that may
/// not publish leave the server holding what a model of My Settings says: a first publish
/// creates the record and later ones replace exactly the record they read, nothing is written
/// when nothing changed, a different record published in between makes applying a stale plan
/// fail with theirs kept, a session without the scope sends nothing, and after a refusal (once the DJ
/// signed in again, if the session ended) publishing again goes through. Applying a plan and
/// `settings_publish_apply` agree.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn publishing_my_settings_converges_on_the_model(tc: TestCase) {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, &publishing());
    let mut model = Model { base: IndexMap::new(), server: None, nonce: "nonce-1".into() };
    // Some settings first (publishing none is refused, as another test checks), then every kind
    // of step once, in any order, now and then with another edit or publish among them.
    let mut kinds: Vec<u8> = tc.draw(gs::permutations((0..KINDS).collect::<Vec<_>>()));
    kinds.insert(0, 0);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(1)) {
        let at = tc.draw(gs::integers::<usize>().max_value(kinds.len()));
        kinds.insert(at, tc.draw(gs::integers::<u8>().max_value(1)));
    }
    for (n, kind) in kinds.into_iter().enumerate() {
        let at_once = tc.draw(gs::booleans());
        match step(&tc, kind) {
            Step::Change(values) => {
                let store = Store::open(&root).unwrap();
                let values: IndexMap<String, String> = values.clone().into_iter().collect();
                api::settings_set(&store, &SetSettings { values: values.clone(), drive: None }).unwrap();
                model.base.extend(values);
            }
            Step::Publish => model.publish(&root, &pds, None, at_once),
            Step::Elsewhere(q) => {
                pds.put(DID, SETTINGS, "self", theirs(q));
                model.server = Some(theirs(q));
            }
            Step::Overtaken(q) => {
                let Some(plan) = model.plan(&root) else { continue };
                // Records are known by their content: writing what's there already changes nothing.
                let same = model.server == Some(theirs(q));
                pds.put(DID, SETTINGS, "self", theirs(q));
                let applied = plan.apply(None, &Ctx::silent());
                model.server = Some(theirs(q));
                match (plan.changed, same) {
                    (false, _) => assert!(!applied.unwrap().written),
                    (true, true) => {
                        assert!(applied.unwrap().written);
                        model.server = Some(plan.record.clone());
                    }
                    (true, false) => assert_eq!(applied.unwrap_err().code(), "stale_plan"),
                }
                assert_eq!(pds.record(DID, SETTINGS, "self").map(|r| r.0), model.server, "theirs isn't lost");
            }
            Step::Refused(how) => {
                pds.set_writes(how.clone());
                model.publish(&root, &pds, Some(&how), at_once);
                pds.set_writes(Writes::Accept);
                if atmosphere::status(&root).unwrap().is_none() {
                    sign_in(&root, &pds.url, &publishing());
                }
                model.publish(&root, &pds, None, at_once);
            }
            Step::Unpermitted => {
                sign_in(&root, &pds.url, "atproto");
                if let Some(plan) = model.plan(&root) {
                    assert_eq!(plan.missing_scope, Some(format!("repo:{SETTINGS}?action=create&action=update")));
                    assert!(plan.warnings.iter().any(|w| w.contains("can't publish yet")), "{:?}", plan.warnings);
                    let writes = pds.calls(PUT).len();
                    match (plan.changed, plan.apply(None, &Ctx::silent())) {
                        (true, applied) => assert_eq!(applied.unwrap_err().reason(), Some(Reason::MissingScope)),
                        (false, applied) => assert!(!applied.unwrap().written),
                    }
                    assert_eq!(pds.calls(PUT).len(), writes, "nothing sent");
                }
                sign_in(&root, &pds.url, &publishing());
            }
            Step::NewNonce => {
                model.nonce = format!("nonce-{}", n + 2);
                pds.set_nonce(&model.nonce);
            }
        }
        assert_eq!(pds.record(DID, SETTINGS, "self").map(|r| r.0), model.server, "after step {n}");
    }
}

/// Importing someone's published My Settings (the signed-in account's own, or anyone's on a
/// server named) adds them to the profile's base (theirs winning) or, asked to, replaces the base
/// with them; the plan lists exactly the values that change, says it's a no-op when none do and
/// destructive when one already set changes, warns about a few of the settings it skips, and
/// applying it (or importing in one go) leaves the base as the model says. Nobody to import from,
/// a server that isn't https, or nothing published, and there's nothing to import.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn importing_my_settings_adds_to_the_base_or_replaces_it(tc: TestCase) {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    assert_eq!(api::settings_publish_plan(&root).unwrap_err().reason(), Some(Reason::EmptyProfile));
    // Without a store yet, the base is empty and importing makes one.
    let base = if tc.draw(gs::booleans()) { canonical_settings(&tc, 0, 5) } else { IndexMap::new() };
    if !base.is_empty() {
        let store = Store::open_or_init(&root).unwrap();
        api::settings_set(&store, &SetSettings { values: base.clone().into_iter().collect(), drive: None }).unwrap();
    }
    if !base.is_empty() {
        // Signed out, publishing them can be looked at.
        let plan = api::settings_publish_plan(&root).unwrap();
        assert!(plan.account.is_none() && plan.uri.is_none() && plan.current.is_none() && plan.changed);
        assert!(plan.warnings[0].starts_with("not signed in"), "{:?}", plan.warnings);
    }
    let published = canonical_settings(&tc, 0, 6);
    // Settings this version doesn't know: a few say enough, so some at and past the 20 shown.
    let unknown = tc.draw(gs::sampled_from(vec![0usize, 3, 20, 21, 25]));
    let replace = tc.draw(gs::booleans());
    // Nobody signed in or named, or a server that isn't https: nothing to import from.
    let refused = |from: Option<&str>, pds: Option<&str>| {
        let req = ImportPublishedSettings { from: from.map(String::from), pds: pds.map(String::from), replace };
        api::settings_import_published_plan(&root, &req).unwrap_err()
    };
    assert_eq!(refused(None, None).reason(), Some(Reason::NotSignedIn));
    assert!(refused(Some(DID), Some("http://10.0.0.1")).to_string().contains("https://"));
    assert_eq!(refused(Some("not a handle!"), None).code(), "not_found");
    // Whose: the signed-in account's own, or someone named with their server.
    let (from, server) = match tc.draw(gs::booleans()) {
        true => {
            sign_in(&root, &pds.url, "atproto");
            (None, None)
        }
        false => (Some(format!(" @{DID} ")), Some(pds.url.clone())),
    };
    let req = ImportPublishedSettings { from, pds: server, replace };
    if tc.draw(gs::weighted_booleans(0.1)) {
        assert_eq!(api::settings_import_published_plan(&root, &req).unwrap_err().code(), "not_found");
        return;
    }
    let mut record = publish::settings_record(&published.clone().into_iter().collect(), Utc::now());
    for i in 0..unknown {
        record["mixer"][format!("newSetting{i}")] = "on".into();
    }
    pds.put(DID, SETTINGS, "self", record);

    let plan = api::settings_import_published_plan(&root, &req).unwrap();
    let mut after = if replace { IndexMap::new() } else { base.clone() };
    after.extend(published.clone());
    let keys: BTreeSet<&String> = base.keys().chain(after.keys()).collect();
    let changes: Vec<(String, Option<String>, Option<String>)> = keys
        .into_iter()
        .map(|k| (k.clone(), base.get(k).cloned(), after.get(k).cloned()))
        .filter(|(_, from, to)| from != to)
        .collect();
    let planned: Vec<_> = plan.changes.iter().map(|c| (c.key.clone(), c.from.clone(), c.to.clone())).collect();
    assert_eq!(planned, changes);
    assert_eq!(plan.published.values.clone().into_iter().collect::<IndexMap<_, _>>(), published);
    assert_eq!(plan.published.skipped.len(), unknown);
    assert_eq!(plan.warnings.len(), unknown.min(20) + usize::from(unknown > 20), "{:?}", plan.warnings);
    assert_eq!((plan.noop, plan.destructive), (changes.is_empty(), changes.iter().any(|c| c.1.is_some())));
    assert_eq!((plan.from.did.as_str(), plan.uri.clone()), (DID, repo::at_uri(DID, SETTINGS, "self")));
    let profile = match tc.draw(gs::booleans()) {
        true => api::settings_import_published_apply(&root, &req).unwrap(),
        false => plan.apply(None, &Ctx::silent()).unwrap(),
    };
    assert_eq!(profile.base.into_iter().collect::<IndexMap<_, _>>(), after);
}

/// The key `plays publish` gave a history list, and when it first published it.
const OLD_KEY: &str = "3lzzzzzzzzz22";
const FIRST_OUT: &str = "2026-10-04T09:30:00.000Z";

/// `track 3 isn't named: why`, or `tracks 3, 7 aren't named: why`.
fn unnamed_note(positions: &[usize], why: &str) -> Option<String> {
    let list: Vec<String> = positions.iter().map(usize::to_string).collect();
    match list.as_slice() {
        [] => None,
        [one] => Some(format!("track {one} isn't named: {why}")),
        many => Some(format!("tracks {} aren't named: {why}", many.join(", "))),
    }
}

/// A set published from the store is the record `set_record` makes for it, with the tracks the
/// set marks unnamed, the request hides and the profile hides left unidentified, and notes saying
/// which and why; a track that isn't there can't be hidden. Signed in, it goes out under the
/// set's id, or under the key `plays publish` gave its history list (which becomes its id, with
/// when that first went out); publishing it again unchanged writes nothing, and once it's edited
/// replaces exactly what was read, keeping when it first went out. Signed out, it can only be
/// looked at.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_published_set_is_its_record_and_replaces_only_what_was_read(tc: TestCase) {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let signed_in = tc.draw(gs::weighted_booleans(0.8));
    if signed_in {
        sign_in(&root, &pds.url, &publishing());
    }
    let (s, names, _) = set(&tc, false);
    let store = Store::open_or_init(&root).unwrap();
    let mut book = SetBook::load(&store).unwrap();
    book.sets.push(s.clone());
    book.save(&store).unwrap();
    let artists: Vec<String> = s.tracks.iter().map(|e| e.artist.trim().to_string()).filter(|a| !a.is_empty()).collect();
    let hide_artist: Vec<String> = if artists.is_empty() {
        Vec::new()
    } else {
        tc.draw(gs::vecs(gs::sampled_from(artists)).min_size(1).max_size(1))
    };
    // A set from a history list `plays publish` published takes over its key.
    let legacy = signed_in && tc.draw(gs::weighted_booleans(0.7));
    // Its old record may be gone since; the key is still the set's to take.
    let old_record = legacy && tc.draw(gs::weighted_booleans(0.75));
    if old_record {
        let old = json!({ "$type": publish::PLAYS_PUBLISH_COLLECTION, "date": "2026-10-03", "tracks": [], "createdAt": FIRST_OUT });
        pds.put(DID, publish::PLAYS_PUBLISH_COLLECTION, OLD_KEY, old);
    }
    if legacy {
        let session = s.sessions[0].clone();
        std::fs::write(root.join(publish::SET_KEYS_FILE), json!({ DID: { session: OLD_KEY } }).to_string()).unwrap();
    }
    api::profile_prefs_update(&store, &PrefsUpdate { hide_artist, ..Default::default() }).unwrap();
    let prefs = api::profile_prefs(&store).unwrap();
    let n = s.tracks.len();
    let bad = SetPublishRequest { set: s.id.clone(), hide: vec![tc.draw(gs::sampled_from(vec![0, n + 1]))] };
    let err = api::set_publish_plan(&root, &bad).unwrap_err();
    assert!(err.to_string().contains(&format!("there's no track {} to hide", bad.hide[0])), "{err}");
    let hide: Vec<usize> = if n == 0 {
        Vec::new()
    } else {
        tc.draw(gs::vecs(gs::integers::<usize>().min_value(1).max_value(n)).max_size(3))
    };
    let req = SetPublishRequest { set: s.id.clone(), hide: hide.clone() };
    let plan = api::set_publish_plan(&root, &req).unwrap();
    let (mut marked, mut asked, mut profile) = (Vec::new(), Vec::new(), Vec::new());
    for (i, e) in s.tracks.iter().enumerate() {
        match i + 1 {
            n if e.unnamed => marked.push(n),
            n if hide.contains(&n) => asked.push(n),
            n if !prefs.may_name(&e.name(), &e.artist) => profile.push(n),
            _ => {}
        }
    }
    let hidden: BTreeSet<usize> = asked.iter().chain(&profile).copied().collect();
    assert_set_record(&plan.record, &s, &names, &hidden);
    assert_eq!(timeless(plan.record.clone()), timeless(publish::set_record(&s, &hidden, None, Utc::now())));
    let mut notes: Vec<String> = [
        unnamed_note(&marked, "marked unnamed (`sets edit --named`)"),
        unnamed_note(&asked, "hidden as asked"),
        unnamed_note(&profile, "hidden by your profile (`profile unhide`)"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if plan.record.get("location").is_none() && trimmed(&s.city).is_some() {
        notes.push("the city is left out: it goes out with a country (`sets edit --country`)".into());
    }
    let key = if legacy { OLD_KEY.to_string() } else { s.id.clone() };
    if legacy {
        notes.push(format!(
            "takes the key `plays publish` gave session {}; the set's id becomes {OLD_KEY}",
            &s.sessions[0][..8]
        ));
    }
    if old_record {
        notes.push(format!(
            "what `plays publish` published stays in {}: publishing can't remove it",
            publish::PLAYS_PUBLISH_COLLECTION
        ));
        assert_eq!(plan.record["createdAt"], FIRST_OUT, "first published by plays publish");
    }
    assert_eq!(plan.notes, notes);
    assert_eq!(plan.rkey, key);
    assert!(!plan.destructive);
    if !signed_in {
        assert!(plan.account.is_none() && plan.uri.is_none() && plan.current.is_none());
        assert_eq!(api::set_publish_apply(&root, &req).unwrap_err().code(), "account");
        assert!(pds.calls(PUT).is_empty());
        return;
    }

    let published = plan.apply(None, &Ctx::silent()).unwrap();
    assert_eq!(published.uri, repo::at_uri(DID, SET, &key));
    let (first, cid) = pds.record(DID, SET, &key).unwrap();
    assert_eq!(first, plan.record);
    assert_eq!(pds.calls(PUT).last().unwrap().json()["swapRecord"], Value::Null);
    assert_eq!(SetBook::load(&store).unwrap().sets[0].id, key, "the set's id is its key");
    assert_eq!(pds.record(DID, SET, &s.id).is_some(), !legacy, "published once");
    // The set goes by its key from now on.
    let req = SetPublishRequest { set: "last".into(), ..req };
    let again = api::set_publish_plan(&root, &req).unwrap();
    assert!(again.noop && !again.changed && !again.destructive);
    assert!(!again.apply(None, &Ctx::silent()).unwrap().written);

    let mut book = SetBook::load(&store).unwrap();
    book.sets[0].title = Some("An edited title".into());
    tc.assume(trimmed(&s.title).as_deref() != Some("An edited title"));
    book.save(&store).unwrap();
    let edited = api::set_publish_plan(&root, &req).unwrap();
    assert!(edited.changed && edited.destructive);
    assert!(edited.apply(None, &Ctx::silent()).unwrap().written);
    let (second, _) = pds.record(DID, SET, &key).unwrap();
    assert_eq!((&second["title"], &second["createdAt"]), (&edited.record["title"], &first["createdAt"]));
    assert_eq!(pds.calls(PUT).last().unwrap().json()["swapRecord"], cid.as_str());
}

/// The profile published from the store's sets is the record of the profile document built from
/// them, fits its Lexicon, goes out as the account's profile, and publishing it again unchanged
/// writes nothing.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn the_published_profile_is_the_documents_record(tc: TestCase) {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, &publishing());
    let drive = common::bigger_drive(dir.path(), "Main");
    for n in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let tracks: BTreeSet<u32> =
            tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(12)).max_size(8)).into_iter().collect();
        common::add_history(&drive, &format!("HISTORY 00{}", n + 1), &tracks.into_iter().collect::<Vec<_>>());
    }
    let store = Store::open(&root).unwrap();
    let sections = tc.draw(gs::vecs(gs::sampled_from(Section::ALL.to_vec())).max_size(15));
    api::profile_prefs_update(&store, &PrefsUpdate { sections: Some(sections), ..Default::default() }).unwrap();
    let req = ProfileRequest { drives: vec![drive.display().to_string()], ..Default::default() };
    let plan = api::profile_publish_plan(&root, &req, &Ctx::silent()).unwrap();
    let doc = api::profile_document(Some(&store), &req, &Ctx::silent()).unwrap();
    assert_eq!(timeless(plan.record.clone()), timeless(publish::profile_record(&doc)));
    lexicon::validate(PROFILE, &plan.record).unwrap();
    let published = plan.apply(None, &Ctx::silent()).unwrap();
    assert_eq!((published.written, published.uri), (true, repo::at_uri(DID, PROFILE, "self")));
    assert_eq!(pds.record(DID, PROFILE, "self").unwrap().0, plan.record);
    let again = api::profile_publish_apply(&root, &req, &Ctx::silent()).unwrap();
    assert!(!again.written);
}
