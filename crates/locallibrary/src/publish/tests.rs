use chrono::NaiveDate;

use super::*;
use crate::profile::{
    ArcShape, Basis, DocArc, DocCollection, DocEnergy, DocEra, DocKeys, DocMixing, DocPrep, DocSets, DocShare,
    DocTempo, DocTrack, Precision,
};
use crate::sets::MusicBrainzIds;

fn now() -> DateTime<Utc> {
    "2026-10-05T12:00:00.123456Z".parse().unwrap()
}

#[test]
fn keys_and_fields_convert_both_ways() {
    for def in settings::SCHEMA {
        assert_eq!(snake(&camel(def.key)), def.key);
    }
    assert_eq!(camel("channel_fader_curve_long"), "channelFaderCurveLong");
    assert_eq!(timestamp(now()), "2026-10-05T12:00:00.123Z");
}

#[test]
fn every_setting_fits_the_settings_lexicon_and_reads_back() {
    let base: IndexMap<String, String> =
        settings::SCHEMA.iter().map(|d| (d.key.to_string(), d.choices.last().unwrap().id.to_string())).collect();
    let record = settings_record(&base, now());
    lexicon::validate(lexicon::SETTINGS, &record).unwrap();
    assert_eq!(record["player"]["jogMode"], "vinyl");
    assert_eq!(record["mixer"]["crossfaderCurve"], "fast-cut");
    let back = settings_from_record(&record).unwrap();
    assert_eq!(back.values, base);
    assert!(back.skipped.is_empty());
    assert_eq!(back.updated_at.as_deref(), Some("2026-10-05T12:00:00.123Z"));

    // A partial profile publishes only what it sets.
    let partial: IndexMap<String, String> = [("quantize".to_string(), "off".to_string())].into();
    let record = settings_record(&partial, now());
    lexicon::validate(lexicon::SETTINGS, &record).unwrap();
    assert!(record.get("display").is_none() && record.get("mixer").is_none());
}

#[test]
fn settings_this_version_doesnt_know_are_skipped() {
    let record = json!({
        "$type": lexicon::SETTINGS,
        "player": { "quantize": "off", "jogMode": "hover", "slipReverse": "on" },
        "mixer": { "quantize": "on" },
        "updatedAt": "2026-10-05T12:00:00.000Z",
    });
    let s = settings_from_record(&record).unwrap();
    assert_eq!(s.values, [("quantize".to_string(), "off".to_string())].into());
    assert_eq!(s.skipped.len(), 3, "{:?}", s.skipped);
    // A change that breaks readers gets a new record type, so there's no version to refuse: a
    // top-level field this version doesn't know (an early draft's `version`) is passed over.
    let drafted = json!({ "$type": lexicon::SETTINGS, "version": 2, "player": { "quantize": "off" } });
    assert_eq!(settings_from_record(&drafted).unwrap().values, [("quantize".to_string(), "off".to_string())].into());
    // Another record type isn't taken for My Settings.
    let other = json!({ "$type": lexicon::PROFILE, "player": { "quantize": "off" } });
    assert!(settings_from_record(&other).unwrap_err().to_string().contains("isn't a My Settings record"));
    // A field name is whatever the record says: none of it reaches a terminal as a control.
    let tricky = json!({ "$type": lexicon::SETTINGS, "player": { "x\u{1b}]0;owned\u{7}": "on" } });
    let s = settings_from_record(&tricky).unwrap();
    assert!(!s.skipped[0].chars().any(char::is_control), "{:?}", s.skipped);
}

fn full_document(precision: Precision) -> ProfileDocument {
    let at_least =
        |n| if precision == Precision::Exact { Count::Exact { count: n } } else { Count::AtLeast { at_least: n } };
    ProfileDocument {
        version: crate::profile::DOCUMENT_VERSION,
        generated_at: now(),
        precision,
        basis: Basis::Plays,
        headline: Some("Techno and House · 122-128 BPM · 45+ sets".into()),
        tags: vec!["Mixes in key".into(), "Plays with quantize off".into()],
        sets: Some(DocSets { count: at_least(45), since: Some(2019), median_tracks: 24 }),
        tempo: Some(DocTempo { low: 122.0, high: 128.4, median: 125.5 }),
        arc: Some(DocArc { shape: ArcShape::WindsDown, builds: 0.2, steady: 0.25, winds_down: 0.55 }),
        mixing: Some(DocMixing { harmonic: Some(0.7), smooth_tempo: Some(0.853), genre_changes: None }),
        keys: Some(DocKeys { top: vec!["8A".into(), "5A".into(), "9A".into()], minor: Some(0.8) }),
        energy: Some(DocEnergy { high: 0.85, mid: 0.1, low: 0.1 }),
        genres: Some(vec![DocShare { name: "Techno".into(), share: 0.6 }]),
        labels: Some(vec![DocShare { name: "Ninja Tune".into(), share: 0.15 }]),
        artists: Some(vec![DocShare { name: "Bicep".into(), share: 0.054 }]),
        era: Some(DocEra { median_year: Some(2021), recent: Some(0.4) }),
        signature: Some(vec![
            DocTrack { artist: "Bicep".into(), title: "Glue".into() },
            DocTrack { artist: String::new(), title: "ID".into() },
        ]),
        prep: Some(DocPrep { hot_cues: 0.95, memory_cues: 0.5, loops: 0.05 }),
        collection: Some(DocCollection { tracks: at_least(3500), playlists: Count::Exact { count: 7 } }),
        style: Some(
            crate::profile::STYLE_SETTINGS
                .iter()
                .map(|k| (camel(k), settings::find(k).unwrap().default.into()))
                .collect(),
        ),
    }
}

#[test]
fn a_profile_document_with_every_section_fits_the_profile_lexicon() {
    for precision in [Precision::Rounded, Precision::Exact] {
        let record = profile_record(&full_document(precision));
        lexicon::validate(lexicon::PROFILE, &record).unwrap();
        assert_eq!(record["tempo"], json!({ "low": 122, "high": 128, "median": 126 }));
        assert_eq!(record["mixing"], json!({ "harmonic": 70, "smoothTempo": 85 }));
        assert_eq!(record["arc"]["shape"], "windsDown");
        assert_eq!(record["signature"][1], json!({ "title": "ID" }));
        // Lists hold objects, so their entries can gain fields.
        assert_eq!(record["tags"], json!([{ "name": "Mixes in key" }, { "name": "Plays with quantize off" }]));
        assert_eq!(record["keys"]["top"], json!([{ "key": "8A" }, { "key": "5A" }, { "key": "9A" }]));
        assert_eq!(record["labels"], json!([{ "name": "Ninja Tune", "share": 15 }]));
        assert_eq!(record["artists"], json!([{ "name": "Bicep", "share": 5 }]));
        assert!(record.get("version").is_none(), "a breaking change gets a new record type instead");
    }
    let rounded = profile_record(&full_document(Precision::Rounded));
    assert_eq!(rounded["sets"], json!({ "total": { "atLeast": 45 }, "since": 2019, "medianTracks": 24 }));
    assert_eq!(rounded["collection"]["tracks"], json!({ "atLeast": 3500 }));
    assert_eq!(rounded["precision"], "rounded");

    // Sections left out of the document stay out of the record.
    let mut doc = full_document(Precision::Rounded);
    doc.signature = None;
    doc.artists = None;
    let record = profile_record(&doc);
    lexicon::validate(lexicon::PROFILE, &record).unwrap();
    assert!(record.get("signature").is_none() && record.get("artists").is_none());
}

const RECORDING: Uuid = uuid::uuid!("c1a3e5c0-7b5e-4d51-9d0e-3c8b1f1f8a2b");
const RELEASE: Uuid = uuid::uuid!("5f6f2c4e-1d7a-4b4e-8f5c-2a9b3c1d0e7f");

#[test]
fn a_set_fits_the_set_lexicon_and_unidentified_tracks_say_nothing() {
    crate::testing::address_lexicon_stand_in();
    let mut track = loclib_model::test_util::track(1, "Bicep", "Glue", "/Contents/Bicep/Glue.mp3");
    track.isrc = "GB-AHT-17-00123".into();
    track.bpm = 129.98;
    track.key = "Am".into();
    track.album = "Isles".into();
    let mut named = SetEntry { start_seconds: Some(0), ..SetEntry::from_track(&track, EntryOrigin::Recording, None) };
    named.musicbrainz = MusicBrainzIds {
        recording: Some(RECORDING),
        release: Some(RELEASE),
        artists: vec!["e0ddb3c4-0a5c-44d5-9c38-04d9a2e4e8a0".parse().unwrap()],
    };
    let unnamed = SetEntry { unnamed: true, start_seconds: Some(312), ..named.clone() };
    let mut set = Set {
        id: "3m2abcdefgh22".into(),
        title: Some("Friday warm-up".into()),
        date: NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
        venue: Some(" ".into()),
        city: Some("London".into()),
        tracks: vec![named.clone(), unnamed, named, SetEntry::added("ID")],
        created_at: now(),
        updated_at: now(),
        ..Default::default()
    };
    let record = set_record(&set, &[3].into(), None, now());
    lexicon::validate(lexicon::SET, &record).unwrap();
    assert_eq!(
        record["tracks"][0],
        json!({
            "title": "Glue",
            "artist": "Bicep",
            "artists": [{ "artistName": "Bicep", "artistMbId": "e0ddb3c4-0a5c-44d5-9c38-04d9a2e4e8a0" }],
            "releaseName": "Isles",
            "isrc": "GBAHT1700123",
            "recordingMbId": "c1a3e5c0-7b5e-4d51-9d0e-3c8b1f1f8a2b",
            "releaseMbId": "5f6f2c4e-1d7a-4b4e-8f5c-2a9b3c1d0e7f",
            "bpmCentis": 12998,
            "key": "8A",
            "startSeconds": 0,
        })
    );
    assert_eq!(record["tracks"][1], json!({ "unidentified": true, "startSeconds": 312 }), "marked unnamed");
    assert_eq!(record["tracks"][2], json!({ "unidentified": true, "startSeconds": 0 }), "hidden");
    assert_eq!(record["tracks"][3], json!({ "title": "ID" }));
    assert_eq!(
        (&record["date"], &record["sources"]),
        (&json!("2026-10-03"), &json!([{ "kind": "mix" }, { "kind": "added" }]))
    );
    // Blank fields stay out, and without a country there's no address.
    assert!(record.get("venue").is_none() && record.get("location").is_none());
    assert!(record.get("playedWith").is_none() && record.get("links").is_none());

    set.country = Some("GB".into());
    set.link = Some("https://www.mixcloud.com/dj/friday-warm-up/".into());
    let record = set_record(&set, &BTreeSet::new(), None, now());
    lexicon::validate(lexicon::SET, &record).unwrap();
    assert_eq!(record["location"], json!({ "$type": lexicon::ADDRESS, "locality": "London", "country": "GB" }));
    assert_eq!(record["links"], json!([{ "uri": "https://www.mixcloud.com/dj/friday-warm-up/" }]));
}

/// A track lists its artists one by one when its credit names several, or the tags give the one
/// artist's MusicBrainz ID; IDs go with names only when there are as many of each. A credit or a
/// release too long for the Lexicon is left out rather than cut short.
#[test]
fn artists_and_releases_go_out_as_far_as_they_are_known() {
    let id = |n: u128| Uuid::from_u128(n);
    let track = |artist: &str, ids: &[u128]| {
        let mut e = SetEntry::added(&format!("{artist} - Title"));
        e.musicbrainz.artists = ids.iter().map(|&n| id(n)).collect();
        set_track(&e, false, None)
    };
    let artists = |artist: &str, ids: &[u128]| track(artist, ids).get("artists").cloned();
    assert_eq!(artists("Bicep", &[]), None, "the credit line says it all");
    assert_eq!(artists("Bicep", &[1]), Some(json!([{ "artistName": "Bicep", "artistMbId": id(1).to_string() }])));
    let names = json!([{ "artistName": "Chris Lake" }, { "artistName": "Amber Mark" }]);
    assert_eq!(artists("Chris Lake, Amber Mark", &[]), Some(names.clone()));
    assert_eq!(artists("Chris Lake, Amber Mark", &[1]), Some(names), "which artist has the one ID?");
    assert_eq!(
        artists("Chris Lake feat. Amber Mark", &[1, 2]),
        Some(json!([
            { "artistName": "Chris Lake", "artistMbId": id(1).to_string() },
            { "artistName": "Amber Mark", "artistMbId": id(2).to_string() },
        ]))
    );
    assert_eq!(artists(&format!("{}, B", "a".repeat(257)), &[]), None, "too long a name");
    let many = (0..33).map(|i| format!("A{i}")).collect::<Vec<_>>().join(", ");
    assert_eq!(artists(&many, &[]), None, "too many artists");

    let mut e = SetEntry::added("Bicep - Glue");
    e.album = "a".repeat(256);
    assert_eq!(set_track(&e, false, None)["releaseName"], json!("a".repeat(256)));
    e.album.push('a');
    assert!(set_track(&e, false, None).get("releaseName").is_none());
}

/// A set built from a recording says how each track came in where it still has two plays the
/// recording heard one after the other: a blend once both are heard for 8 seconds, otherwise a
/// technique the recording can't tell. Where either track isn't named, only when.
#[test]
fn tracks_heard_one_after_another_say_how_they_came_in() {
    let mut report =
        crate::sets::build::tests::report(&[(1, 0.0, 0.9), (2, 190.4, 0.9), (3, 390.0, 0.9), (4, 600.0, 0.9)]);
    let heard = |from: usize, overlap: f64, harmony: &str, tempo_change: Option<f64>| Transition {
        from,
        to: from + 1,
        at: report.plays[from + 1].start,
        overlap,
        harmony: Some(harmony.into()).filter(|h: &String| !h.is_empty()),
        tempo_change,
    };
    report.transitions =
        vec![heard(0, 9.6, "energy boost", Some(2.5)), heard(1, -3.0, "clash", None), heard(2, 4.0, "±1", Some(-0.34))];
    let mut set = Set { tracks: crate::sets::build::from_recording(&report).tracks, ..Default::default() };
    let came_in = |set: &Set, hidden: &BTreeSet<usize>| -> Vec<Value> {
        let record = set_record(set, hidden, Some(&report), now());
        lexicon::validate(lexicon::SET, &record).unwrap();
        record["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.get("transitionIn").cloned().unwrap_or_default())
            .collect()
    };
    assert_eq!(
        came_in(&set, &BTreeSet::new()),
        [
            Value::Null,
            json!({ "technique": "blend", "durationMs": 9600, "harmony": "energyBoost", "tempoChangePermille": 25 }),
            json!({ "technique": "unknown", "harmony": "clash" }),
            json!({ "technique": "unknown", "durationMs": 4000, "harmony": "adjacent", "tempoChangePermille": -3 }),
        ]
    );
    assert_eq!(
        came_in(&set, &[3].into())[2..],
        [json!({ "technique": "unknown" }), json!({ "technique": "unknown", "durationMs": 4000 })]
    );
    assert!(set_record(&set, &BTreeSet::new(), None, now())["tracks"][1].get("transitionIn").is_none());

    // Moved, dropped or added since, tracks no longer follow the ones the recording heard.
    set.tracks.swap(2, 3);
    set.tracks.insert(1, SetEntry { start_seconds: Some(190), ..SetEntry::added("Typed - In") });
    assert_eq!(came_in(&set, &BTreeSet::new()), vec![Value::Null; 5]);
}

/// Whatever the number (negative, over 1, infinite, not a number), a share becomes a whole
/// percent from 0 to 100 and a tempo a whole BPM of 0 or more, never out of order; a share
/// from 0 to 1, or a tempo, is at most half a unit off.
#[hegel::test(test_cases = 500)]
fn shares_and_tempos_become_whole_numbers_in_order(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (x, y) = (tc.draw(gs::floats::<f64>()), tc.draw(gs::floats::<f64>()));
    for v in [x, y] {
        assert!((0..=100).contains(&percent(v)), "{v}");
        assert!(bpm(v) >= 0, "{v}");
        if (0.0..=1.0).contains(&v) {
            assert!((percent(v) as f64 - v * 100.0).abs() <= 0.5, "{v}");
        }
        if (0.0..1e15).contains(&v) {
            assert!((bpm(v) as f64 - v).abs() <= 0.5, "{v}");
        }
    }
    if x <= y {
        assert!(percent(x) <= percent(y) && bpm(x) <= bpm(y), "{x} {y}");
    }
}

/// Two records are the same exactly when they differ in nothing but when they were made:
/// changing or dropping `updatedAt`, `generatedAt` or `createdAt` changes nothing, and any
/// other change does.
#[hegel::test(test_cases = 300)]
fn only_times_dont_count_as_changes(tc: hegel::TestCase) {
    use hegel::Generator;
    use hegel::generators as gs;
    let field = || gs::sampled_from(vec!["title", "date", "tracks", "updatedAt", "generatedAt", "createdAt"]);
    let value = || {
        hegel::one_of!(
            gs::integers::<i64>().max_value(3).map(Value::from),
            gs::text().max_size(3).map(Value::from),
            gs::just(Value::Null),
        )
    };
    let fields: Vec<(&str, Value)> = tc.draw(gs::vecs(gs::tuples!(field(), value())).max_size(6));
    let a = Value::Object(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    let mut b = a.clone();
    for time in TIME_FIELDS {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {}
            1 => {
                b.as_object_mut().unwrap().remove(time);
            }
            _ => b[time] = tc.draw(gs::text().max_size(24)).into(),
        }
    }
    assert!(same_content(&a, &b) && same_content(&b, &a), "{a} {b}");
    // Wrapping a value in a list changes it, whatever it was.
    let other = tc.draw(gs::sampled_from(vec!["title", "date", "tracks", "venue"]));
    b[other] = json!([a.get(other)]);
    assert!(!same_content(&a, &b) && !same_content(&b, &a), "{a} {b}");
    assert!(same_content(&json!([1]), &json!([1])) && !same_content(&json!(1), &json!("1")));
}

/// The `transitionIn` a model of the record expects for the transition `t`: a blend from 8
/// seconds heard together, a duration only when there was an overlap of at most ten minutes,
/// and with both tracks named, the Lexicon's name for the harmony and the tempo change in
/// permille.
fn expected_transition(t: &Transition, unidentified: bool) -> Value {
    let mut want = json!({ "technique": if t.overlap >= 8.0 { "blend" } else { "unknown" } });
    let ms = (t.overlap * 1000.0).round();
    if ms > 0.0 && ms <= 600_000.0 {
        want["durationMs"] = json!(ms as i64);
    }
    if unidentified {
        return want;
    }
    let named = match t.harmony.as_deref() {
        Some("same key") => Some("sameKey"),
        Some("±1") => Some("adjacent"),
        Some("relative") => Some("relative"),
        Some("energy boost") => Some("energyBoost"),
        Some("clash") => Some("clash"),
        _ => None,
    };
    if let Some(h) = named {
        want["harmony"] = json!(h);
    }
    if let Some(c) = t.tempo_change {
        want["tempoChangePermille"] = json!((c * 10.0).round() as i64);
    }
    want
}

/// Whatever a recording heard, a set built from it says how each track came in exactly where
/// the track and the one before it are plays the recording heard in that order (the first
/// transition it heard between them), and nowhere else: not after a track typed in between, a
/// play dropped, or one the recording heard into another. Unidentified tracks on either side
/// say only when. The record fits the set Lexicon.
#[hegel::test(test_cases = 200)]
fn a_set_says_how_its_tracks_came_in_only_where_the_recording_heard_it(tc: hegel::TestCase) {
    use hegel::Generator;
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(6));
    let plays: Vec<(u32, f64, f64)> = (1..=n)
        .map(|i| (i, f64::from(i - 1) * 300.0 + tc.draw(gs::floats::<f64>().min_value(0.0).max_value(250.0)), 0.9))
        .collect();
    let mut report = crate::sets::build::tests::report(&plays);
    let harmonies = vec!["same key", "±1", "relative", "energy boost", "clash", "", "a fifth"];
    let k = report.plays.len();
    // Most transitions are between plays heard one after the other; the edges of a blend and of
    // the longest duration the Lexicon takes come up often.
    let overlap = || {
        hegel::one_of!(
            gs::sampled_from(vec![-1.0, 0.0, 0.0004, 7.999, 8.0, 8.5, 600.0, 600.0006]),
            gs::floats::<f64>().min_value(-30.0).max_value(700.0)
        )
    };
    let pair = || {
        hegel::one_of!(
            gs::integers::<usize>().max_value(k.saturating_sub(2)).map(|i| (i, i + 1)),
            gs::tuples!(gs::integers::<usize>().max_value(k), gs::integers::<usize>().max_value(k))
        )
    };
    let transitions = tc.draw(
        gs::vecs(gs::tuples!(
            pair(),
            overlap(),
            gs::optional(gs::sampled_from(harmonies)),
            gs::optional(gs::floats::<f64>().min_value(-80.0).max_value(80.0))
        ))
        .max_size(8),
    );
    report.transitions = transitions
        .into_iter()
        .map(|((from, to), overlap, harmony, tempo_change)| Transition {
            from,
            to,
            at: 0.0,
            overlap,
            harmony: harmony.map(String::from),
            tempo_change,
        })
        .collect();
    // The set: the recording's tracks (by play), some dropped, some typed in between, some
    // marked unnamed.
    let heard = crate::sets::build::from_recording(&report).tracks;
    let mut tracks: Vec<(Option<usize>, SetEntry)> = Vec::new();
    for (i, mut e) in heard.into_iter().enumerate() {
        if tc.draw(gs::weighted_booleans(0.15)) {
            tracks.push((None, SetEntry { start_seconds: e.start_seconds, ..SetEntry::added("Typed - In") }));
        }
        if tc.draw(gs::weighted_booleans(0.15)) {
            continue;
        }
        e.unnamed = tc.draw(gs::weighted_booleans(0.2));
        tracks.push((Some(i), e));
    }
    let hidden: BTreeSet<usize> = tc
        .draw(gs::vecs(gs::integers::<usize>().min_value(1).max_value(tracks.len().max(1))).max_size(2))
        .into_iter()
        .collect();
    let set = Set {
        id: "3m2abcdefgh22".into(),
        date: NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
        tracks: tracks.iter().map(|(_, e)| e.clone()).collect(),
        ..Default::default()
    };
    let record = set_record(&set, &hidden, Some(&report), now());
    lexicon::validate(lexicon::SET, &record).unwrap();
    let unidentified = |i: usize| tracks[i].1.unnamed || hidden.contains(&(i + 1));
    for (i, t) in record["tracks"].as_array().unwrap().iter().enumerate() {
        let want = match (i.checked_sub(1).and_then(|b| tracks[b].0), tracks[i].0) {
            (Some(from), Some(to)) => report
                .transitions
                .iter()
                .find(|t| t.from == from && t.to == to)
                .map(|t| expected_transition(t, unidentified(i) || unidentified(i - 1))),
            _ => None,
        };
        assert_eq!(t.get("transitionIn"), want.as_ref(), "track {}: {t}", i + 1);
    }
}
