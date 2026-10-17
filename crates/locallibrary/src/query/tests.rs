use super::*;
use crate::model::Playlist;
use loclib_model::test_util::track;

fn lib() -> Library {
    let mut a = track(1, "Daft Punk", "Around the World", "/Contents/a.mp3");
    a.key = "8A".into();
    a.bpm = 121.0;
    a.rating = 5;
    a.genre = "House".into();
    a.color = Some(TrackColor::Aqua);
    a.date_added = "2026-09-30".into();
    a.duration_secs = 429;
    let mut b = track(2, "Floorplan", "Never Grow Old", "/Contents/b.mp3");
    b.key = "Am".into();
    b.bpm = 128.0;
    b.rating = 3;
    b.genre = "Techno".into();
    b.play_count = 4;
    b.date_added = "2025-01-02".into();
    let mut c = track(3, "Nobody", "Untitled", "/Contents/c.mp3");
    c.key = "3B".into();
    c.bpm = 64.0;
    Library {
        tracks: vec![a, b, c],
        playlists: vec![
            Playlist { id: 1, parent_id: None, name: "Sets".into(), is_folder: true, sort_order: 0, track_ids: vec![] },
            Playlist {
                id: 2,
                parent_id: Some(1),
                name: "Friday".into(),
                is_folder: false,
                sort_order: 0,
                track_ids: vec![2, 1, 2],
            },
        ],
        ..Default::default()
    }
}

fn ids(f: &str) -> Vec<u32> {
    let lib = lib();
    let ctx = QueryContext::new(&lib);
    Filter::parse(f).unwrap().select(&ctx).iter().map(|t| t.id).collect()
}

#[test]
fn filters() {
    assert_eq!(ids("daft"), [1]);
    assert_eq!(ids("artist:\"daft punk\""), [1]);
    assert_eq!(ids("-genre:house"), [2, 3]);
    assert_eq!(ids("genre:house|techno"), [1, 2]);
    assert_eq!(ids("key:8A"), [1, 2], "Am is 8A");
    assert_eq!(ids("key:9A~"), [1, 2]);
    assert_eq!(ids("bpm:120-125"), [1]);
    assert_eq!(ids("bpm:128"), [2]);
    assert_eq!(ids("bpm:>=124"), [2]);
    assert_eq!(ids("bpm:~130"), [2, 3], "64 is half time");
    assert_eq!(ids("rating:>=4"), [1]);
    assert_eq!(ids("plays:0"), [1, 3]);
    assert_eq!(ids("color:aqua"), [1]);
    assert_eq!(ids("color:none"), [2, 3]);
    assert_eq!(ids("added:2026"), [1]);
    assert_eq!(ids("added:2025-01..2025-12"), [2]);
    assert_eq!(ids("added:>=2026-01-01"), [1]);
    assert_eq!(ids("time:>6:00"), [1]);
    assert_eq!(ids("playlist:sets"), [1, 2]);
    assert_eq!(ids("playlist:sets/friday"), [1, 2]);
    assert_eq!(ids("is:unfiled"), [3]);
    assert_eq!(ids("is:played"), [2]);
    assert_eq!(ids("has:key -key:8A"), [3]);
    assert_eq!(ids(""), [1, 2, 3], "a filter with no terms lists everything (a selector refuses it)");
    for bad in
        ["key:H", "bpm:fast", "color:mauve", "is:loud", "rating:", "added:yesterday", "-genre:|", "-\" \"", "is:|"]
    {
        assert!(Filter::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn numbers_and_dates_compare_every_way() {
    assert_eq!(ids("bpm:<100"), [3]);
    assert_eq!(ids("bpm:>121"), [2]);
    assert_eq!(ids("bpm:<=121"), [1, 3]);
    // A bare month or year on the open side of a bound takes the whole of it.
    assert_eq!(ids("added:<=2025-12"), [2]);
    assert_eq!(ids("added:>2025"), [1]);
    assert_eq!(ids("added:<2026"), [2]);
}

#[test]
fn colours_parse_by_number_too() {
    assert_eq!(parse_color("0").unwrap(), None);
    assert_eq!(parse_color(" 5 ").unwrap(), TrackColor::from_id(5));
    assert_eq!(parse_color("9").unwrap_err().to_string(), Error::invalid("no colour number 9").to_string());
}

/// Regression: a word before a colon that named no field was text to look for, so
/// `-gnere:house` matched every track; `playlist:` compared full paths only, so a nested
/// playlist's name matched nothing, and negated, everything.
#[test]
fn filters_fail_closed() {
    let e = Filter::parse("bpm:>120 -gnere:house").unwrap_err().to_string();
    assert!(e.contains("no field \"gnere\" (did you mean genre?)"), "{e}");
    let e = Filter::parse("dur:>6:00").unwrap_err().to_string();
    assert!(e.contains("(did you mean duration?)"), "the start of a field's name: {e}");
    let e = Filter::parse("x:1").unwrap_err().to_string();
    assert!(!e.contains("did you mean"), "not a one-letter alias: {e}");
    assert!(Filter::parse("bogus:1").unwrap_err().to_string().contains("no field \"bogus\";"));
    // A colon in quotes, after something that isn't a word, or in a field's value is text.
    assert_eq!(ids("\"daft:\""), Vec::<u32>::new());
    assert_eq!(ids("-\"never:grow\""), [1, 2, 3]);
    assert_eq!(ids("3:30 -c:30"), Vec::<u32>::new());
    assert_eq!(ids("-https://example.com"), [1, 2, 3]);
    assert_eq!(ids("artist:\"daft punk\""), [1]);

    assert_eq!(ids("playlist:Friday"), [1, 2], "by name, as --playlist takes it");
    assert_eq!(ids("-playlist:friday"), [3]);
    assert_eq!(ids("playlist:Sets/"), [1, 2], "below a folder");
    let lib = lib();
    let ctx = QueryContext::new(&lib);
    let select = |f: &str| TrackSelector { filter: Some(f.into()), ..Default::default() }.select_with(&ctx);
    assert_eq!(select("-playlist:Friday").unwrap(), [3]);
    let e = select("-playlist:Saturday|Friday").unwrap_err().to_string();
    assert!(e.contains("playlist \"Saturday\""), "{e}");
    // My Tags that weren't read can't be matched.
    assert!(select("-tag:deep").is_err());
}

/// Regression: each entry of a playlist was looked up with a scan of the library, so
/// selecting a long playlist on a big library took minutes.
#[test]
fn long_playlists_are_selected_in_one_pass() {
    let n = 50_000u32;
    let lib = Library {
        tracks: (1..=n).map(|i| track(i, "A", "T", &format!("/Contents/{i}.mp3"))).collect(),
        playlists: vec![Playlist {
            id: 1,
            parent_id: None,
            name: "All".into(),
            is_folder: false,
            sort_order: 0,
            track_ids: (1..=n).rev().collect(),
        }],
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let all = TrackSelector { playlist: Some("All".into()), ..Default::default() };
    assert_eq!(all.select_ordered(&QueryContext::new(&lib), false).unwrap().len(), n as usize);
    let tracks = lib.tracks_by_id(&lib.playlists[0].track_ids);
    assert_eq!((tracks.len(), tracks[0].id), (n as usize, n));
    assert!(start.elapsed() < std::time::Duration::from_secs(2), "{:?}", start.elapsed());
}

/// Regression: a history list a player recorded only in OneLibrary, on a drive that has
/// `export.pdb` too, couldn't be selected.
#[test]
fn history_lists_of_both_databases_are_selected_by_file() {
    let lib = lib();
    // OneLibrary's rows: other ids, the same files.
    let mut played = vec![lib.tracks[2].clone(), lib.tracks[0].clone()];
    played[0].id = 70;
    played[1].id = 71;
    let lists = [HistoryList {
        name: "HISTORY 003".into(),
        path: "HISTORY 003".into(),
        database: "onelibrary",
        tracks: played,
        missing: 0,
        missing_at: Vec::new(),
    }];
    let sel = TrackSelector { history: Some("history 003".into()), ..Default::default() };
    assert_eq!(sel.select_ordered(&QueryContext::new(&lib).with_history(&lists), false).unwrap(), [3, 1]);
    assert!(sel.select(&lib).is_err(), "the library's own lists don't have it");
}

#[test]
fn tags_filter_uses_the_tag_map() {
    let lib = lib();
    let tags = FxHashMap::from_iter([("contents/b.mp3".to_string(), vec!["Genre/Deep".to_string()])]);
    let ctx = QueryContext::new(&lib).with_tags(&tags);
    let f = Filter::parse("tag:deep").unwrap();
    assert!(f.uses_tags());
    assert_eq!(f.select(&ctx).iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
    let f = Filter::parse("tag:genre/deep").unwrap();
    assert_eq!(f.select(&ctx).len(), 1);
}

#[test]
fn sorting() {
    let lib = lib();
    let mut v: Vec<&Track> = lib.tracks.iter().collect();
    sort_tracks(&mut v, &parse_sort("key,bpm").unwrap());
    assert_eq!(v.iter().map(|t| t.id).collect::<Vec<_>>(), [3, 1, 2]);
    sort_tracks(&mut v, &parse_sort("-rating").unwrap());
    assert_eq!(v.iter().map(|t| t.id).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(parse_sort("loudness").is_err());
    assert!(parse_sort("").is_err());
}

#[test]
fn selectors() {
    let lib = lib();
    let ctx = QueryContext::new(&lib);
    let s = TrackSelector { filter: Some("genre:techno".into()), ..Default::default() };
    assert_eq!(s.select(&lib).unwrap(), [2]);
    assert!(TrackSelector::default().select(&lib).is_err());
    // Fields given together narrow the selection, in the order of the first ordered one.
    let s = TrackSelector { paths: vec!["/contents/C.mp3".into()], ids: vec![3, 1], ..Default::default() };
    assert_eq!(s.select(&lib).unwrap(), [3]);
    let friday = |s: TrackSelector| TrackSelector { playlist: Some("Friday".into()), ..s };
    let s = friday(TrackSelector { filter: Some("bpm:>=100".into()), ..Default::default() });
    assert_eq!(s.select_ordered(&ctx, false).unwrap(), [2, 1]);
    let s = friday(TrackSelector { ids: vec![1, 3, 2], ..Default::default() });
    assert_eq!(s.select_ordered(&ctx, false).unwrap(), [1, 2]);
    let s = friday(TrackSelector { ids: vec![3], ..Default::default() });
    let e = s.select_ordered(&ctx, false).unwrap_err().to_string();
    assert!(e.contains("no track fits the ids and the playlist together"), "{e}");
    assert!(s.select_ordered(&ctx, true).unwrap().is_empty());
    let s = TrackSelector { filter: Some("genre:polka".into()), ..Default::default() };
    assert!(s.select_ordered(&ctx, false).is_err());
    assert!(s.select_ordered(&ctx, true).unwrap().is_empty());
    assert!(TrackSelector { ids: vec![99], ..Default::default() }.select(&lib).is_err());
    assert_eq!(parse_color("6").unwrap(), Some(TrackColor::Aqua));
    assert_eq!(parse_color("none").unwrap(), None);
    assert!(parse_color("9").is_err());
}

/// Regression: an empty filter or query selected every track, so `track rm --filter ""`
/// planned to delete the whole library.
#[test]
fn blank_selectors_select_nothing() {
    let lib = lib();
    let ctx = QueryContext::new(&lib);
    let text = |b: &str| Some(b.to_string());
    for b in ["", "  ", "\t"] {
        for s in [
            TrackSelector { filter: text(b), ..Default::default() },
            TrackSelector { query: text(b), ..Default::default() },
            TrackSelector { album: text(b), ..Default::default() },
            TrackSelector { playlist: text(b), ..Default::default() },
            TrackSelector { history: text(b), ..Default::default() },
            TrackSelector { filter: text(b), ids: vec![1], ..Default::default() },
            TrackSelector { query: text(b), playlist: text("Friday"), ..Default::default() },
        ] {
            assert!(s.select_ordered(&ctx, true).is_err(), "{s:?}");
        }
    }
    let quotes = TrackSelector { filter: text("\"\""), ..Default::default() };
    assert!(quotes.select_ordered(&ctx, true).is_err());
}

/// Regression: a history list's name matched ignoring case before an exact match was looked
/// for.
#[test]
fn history_lists_are_selected_by_exact_name_first() {
    let mut lib = lib();
    lib.history = ["HISTORY 001", "history 001"]
        .into_iter()
        .zip(1..)
        .map(|(name, id)| Playlist {
            id,
            parent_id: None,
            name: name.into(),
            is_folder: false,
            sort_order: id,
            track_ids: vec![id],
        })
        .collect();
    let history = |name: &str| TrackSelector { history: Some(name.into()), ..Default::default() }.select(&lib);
    assert_eq!(history("history 001").unwrap(), [2]);
    assert!(history("History 001").is_err(), "two match ignoring case");
}

/// What analyzers worked out for the test library: `sound.bpm` (a number) for the first two
/// tracks, `sound.key` (text) for all three, `sound.vocal` (a switch) and `sound.moods` (a list)
/// for the first; `sound.beats` (whole numbers) is declared but none has it.
fn values() -> TrackValues {
    let declared = [
        ("sound.bpm", FieldType::Number),
        ("sound.beats", FieldType::Integer),
        ("sound.key", FieldType::String),
        ("sound.vocal", FieldType::Boolean),
        ("sound.moods", FieldType::Strings),
    ];
    let declared = declared.into_iter().map(|(n, t)| (n.to_string(), t)).collect();
    let fields = |pairs: Vec<(&str, FieldValue)>| pairs.into_iter().map(|(n, v)| (n.to_string(), v)).collect();
    let tracks = [
        (
            1,
            fields(vec![
                ("sound.bpm", FieldValue::Number(121.04)),
                ("sound.key", FieldValue::String("8A".into())),
                ("sound.vocal", FieldValue::Boolean(true)),
                ("sound.moods", FieldValue::Strings(vec!["deep house".into(), "warm".into()])),
            ]),
        ),
        (2, fields(vec![("sound.bpm", FieldValue::Number(127.6)), ("sound.key", FieldValue::String("11A".into()))])),
        (3, fields(vec![("sound.key", FieldValue::String("1A".into()))])),
    ];
    TrackValues { declared, tracks: tracks.into_iter().collect() }
}

/// The tracks a filter naming plugins' fields selects, with [`values`].
fn ids_with_values(f: &str) -> Result<Vec<u32>> {
    let (lib, values) = (lib(), values());
    let ctx = QueryContext { fields: Some(&values), ..QueryContext::new(&lib) };
    let filter = Filter::parse(f)?;
    filter.check(&ctx)?;
    Ok(filter.select(&ctx).iter().map(|t| t.id).collect())
}

/// A plugin's field matches its values: numbers by condition (an exact one to the precision
/// typed), text by whole words, a switch by true or false, a list by any of it; a track without
/// a value matches nothing, and negated, everything.
#[test]
fn plugins_fields_filter_by_their_values() {
    let ids = |f: &str| ids_with_values(f).unwrap();
    assert_eq!(ids("sound.bpm:>120"), [1, 2]);
    assert_eq!(ids("sound.bpm:>=125"), [2]);
    assert_eq!(ids("sound.bpm:120-122"), [1]);
    assert_eq!(ids("sound.bpm:121"), [1], "121.04 is 121");
    assert_eq!(ids("sound.bpm:128"), [2], "127.6 is 128");
    assert_eq!(ids("sound.bpm:121.0"), [1]);
    assert_eq!(ids("sound.bpm:127.6"), [2], "to the tenth typed");
    assert!(ids("sound.bpm:121.1").is_empty(), "121.04 isn't 121.1");
    assert_eq!(ids("-sound.bpm:>120"), [3], "no value: not matched, so negated it is");
    assert_eq!(ids("sound.key:8A"), [1]);
    assert_eq!(ids("sound.key:1a"), [3], "a whole word: not 11A");
    assert_eq!(ids("sound.key:1A|11A"), [2, 3]);
    assert_eq!(ids("Sound.Key:8a"), [1], "case doesn't matter");
    assert_eq!(ids("sound.vocal:true"), [1]);
    assert_eq!(ids("sound.vocal:no"), Vec::<u32>::new());
    assert_eq!(ids("sound.moods:deep"), [1]);
    assert_eq!(ids("sound.moods:warm"), [1]);
    assert!(ids("sound.moods:dee").is_empty(), "words, not parts of them");
    assert!(ids("sound.beats:>0").is_empty(), "declared, but none has it");
    assert_eq!(ids("sound.bpm:>120 sound.key:8A"), [1]);
    // Quoted, a dotted word before a colon is text.
    assert!(ids("\"sound.bpm:>120\"").is_empty());
}

/// A plugin's field no analyzer declares, a value its type can't hold, nothing after the colon,
/// and a filter naming one with nothing analyzers worked out are refused, never matched.
#[test]
fn plugins_fields_are_refused_when_they_cant_be_answered() {
    let refused = |f: &str| ids_with_values(f).unwrap_err().to_string();
    assert!(refused("sound.bpmm:>120").contains("did you mean sound.bpm?"), "{}", refused("sound.bpmm:>120"));
    assert!(refused("other.bpm:1").contains("there's no field \"other.bpm\""), "{}", refused("other.bpm:1"));
    assert!(refused("sound.bpm:fast").contains("it's a number"), "{}", refused("sound.bpm:fast"));
    assert!(refused("sound.beats:x").contains("it's a number"));
    assert!(refused("sound.vocal:maybe").contains("true or false"));
    assert!(refused("sound.bpm:").contains("nothing after the colon"));
    assert!(refused("sound.key:|").contains("nothing to look for"));
    let lib = lib();
    let f = Filter::parse("sound.bpm:>120").unwrap();
    assert!(f.uses_fields() && !Filter::parse("bpm:>120").unwrap().uses_fields());
    let said = f.check(&QueryContext::new(&lib)).unwrap_err().to_string();
    assert!(said.contains("a sound.bpm: term needs what analyzers worked out"), "{said}");
}

/// A sort names a plugin's field where its values are given, and its declared fields: by value,
/// either way, tracks without one last; elsewhere it's refused.
#[test]
fn plugins_fields_sort_with_their_values() {
    let (lib, values) = (lib(), values());
    let sorted = |s: &str| {
        let keys = parse_sort_with(s, Some(&values.declared)).unwrap();
        let mut v: Vec<&Track> = lib.tracks.iter().rev().collect();
        sort_tracks_with(&mut v, &keys, Some(&values));
        v.iter().map(|t| t.id).collect::<Vec<_>>()
    };
    assert_eq!(sorted("sound.bpm"), [1, 2, 3]);
    assert_eq!(sorted("-sound.bpm"), [2, 1, 3], "the track without one stays last");
    assert_eq!(sorted("sound.key"), [2, 3, 1], "text: 11a, 1a, 8a");
    assert_eq!(sorted("sound.beats,title"), [1, 2, 3], "none has it: the next key orders them");
    let keys = parse_sort_with("-sound.bpm", Some(&values.declared)).unwrap();
    assert_eq!(keys, [SortKey { field: SortField::Field(FieldName::new("sound.bpm")), descending: true }]);
    let mut none: Vec<&Track> = lib.tracks.iter().rev().collect();
    sort_tracks(&mut none, &keys);
    assert_eq!(none.iter().map(|t| t.id).collect::<Vec<_>>(), [3, 2, 1], "without values: as it was");
    let said = parse_sort_with("sound.bpmm", Some(&values.declared)).unwrap_err().to_string();
    assert!(said.contains("did you mean sound.bpm?"), "{said}");
    let said = parse_sort("sound.bpm").unwrap_err().to_string();
    assert!(said.contains("a plugin's field sorts track listings only"), "{said}");
    // Kept in a saved sort by its name, and read back as the same.
    let json = serde_json::to_string(&keys[0]).unwrap();
    assert_eq!(json, r#"{"field":{"field":"sound.bpm"},"descending":true}"#);
    assert_eq!(serde_json::from_str::<SortKey>(&json).unwrap(), keys[0]);
}

/// Values compare by number, then by text; a missing value isn't compared here.
#[test]
fn field_values_compare_numbers_then_text() {
    use std::cmp::Ordering::*;
    let (n, i, s) = (FieldValue::Number, FieldValue::Integer, |x: &str| FieldValue::String(x.into()));
    assert_eq!(compare_values(&n(9.5), &i(10)), Less);
    assert_eq!(compare_values(&i(10), &n(9.5)), Greater);
    assert_eq!(compare_values(&s("b"), &s("A")), Greater);
    assert_eq!(compare_values(&FieldValue::Boolean(false), &FieldValue::Boolean(true)), Less);
    assert_eq!(compare_values(&FieldValue::Strings(vec!["a".into()]), &FieldValue::Strings(vec![])), Greater);
}

/// Any values, some tracks without one: `sound.bpm:>T` selects exactly the tracks whose value is
/// above `T`, negated the rest; sorted by it either way, the tracks with one come in order and
/// those without last.
#[hegel::test(test_cases = 200)]
fn a_plugins_number_filters_and_sorts_as_its_values_say(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let lib = lib();
    let mut values = values();
    values.tracks.clear();
    for t in &lib.tracks {
        if tc.draw(gs::booleans()) {
            let v = tc.draw(gs::sampled_from(&[60.0, 119.99, 120.0, 120.5, 128.0, 174.0][..]));
            values.tracks.insert(t.id, [("sound.bpm".to_string(), FieldValue::Number(v))].into_iter().collect());
        }
    }
    let over = tc.draw(gs::sampled_from(&[0.0, 120.0, 120.25, 200.0][..]));
    let value = |id: u32| match values.get(id, "sound.bpm") {
        Some(FieldValue::Number(v)) => Some(*v),
        _ => None,
    };
    let ctx = QueryContext { fields: Some(&values), ..QueryContext::new(&lib) };
    let selected = |f: &str| Filter::parse(f).unwrap().select(&ctx).iter().map(|t| t.id).collect::<Vec<_>>();
    let above: Vec<u32> = lib.tracks.iter().map(|t| t.id).filter(|&id| value(id).is_some_and(|v| v > over)).collect();
    let rest: Vec<u32> = lib.tracks.iter().map(|t| t.id).filter(|id| !above.contains(id)).collect();
    assert_eq!(selected(&format!("sound.bpm:>{over}")), above);
    assert_eq!(selected(&format!("-sound.bpm:>{over}")), rest);
    let descending = tc.draw(gs::booleans());
    let keys = [SortKey { field: SortField::Field(FieldName::new("sound.bpm")), descending }];
    let mut sorted: Vec<&Track> = lib.tracks.iter().collect();
    sort_tracks_with(&mut sorted, &keys, Some(&values));
    let got: Vec<Option<f64>> = sorted.iter().map(|t| value(t.id)).collect();
    let with = got.iter().take_while(|v| v.is_some()).count();
    assert!(got[with..].iter().all(Option::is_none), "{got:?}");
    let ordered = got[..with].windows(2).all(|w| if descending { w[0] >= w[1] } else { w[0] <= w[1] });
    assert!(ordered, "{got:?}");
}
