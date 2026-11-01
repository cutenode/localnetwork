use super::*;
use crate::model::Library;
use crate::query::{Filter, QueryContext};
use hegel::TestCase;
use hegel::generators as gs;
use rustc_hash::FxHashMap;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 6, 1).unwrap()
}

fn tag_path(id: &str) -> Option<String> {
    (id == "4294967295").then(|| "Genre/Deep".to_string())
}

#[test]
fn reads_rekordbox_xml() {
    let xml = r#"<NODE Id="-1" LogicalOperator="2" AutomaticUpdate="1"><CONDITION PropertyName="genre" Operator="8" ValueUnit="" ValueLeft="House" ValueRight=""/><CONDITION PropertyName="myTag" Operator="8" ValueUnit="" ValueLeft="-1" ValueRight=""/><CONDITION PropertyName="bpm" Operator="5" ValueUnit="" ValueLeft="12000" ValueRight="128"/></NODE>"#;
    let list = SmartList::parse(xml, tag_path).unwrap();
    assert!(list.any);
    let rules: Vec<&Rule> = list.conditions.iter().map(|c| c.rule.as_ref().unwrap()).collect();
    assert_eq!(rules[0], &Rule::Text { field: TextField::Genre, op: TextOp::Contains, value: "House".into() });
    assert_eq!(rules[1], &Rule::Tag { has: true, path: "Genre/Deep".into() }, "a signed id, as rekordbox writes it");
    assert_eq!(rules[2], &Rule::Number { field: NumberField::Bpm, op: Compare::Range(120.0, 128.0) }, "hundredths");
    assert_eq!(list.conditions[2].text, r#"bpm is in the range "12000" to "128""#);
    assert!(SmartList::parse("not xml <", tag_path).is_none());
    assert!(SmartList::parse("<OTHER/>", tag_path).is_none(), "no NODE");
}

#[test]
fn says_what_a_filter_can_and_cant() {
    let list = |any: bool, conditions: &[(&str, u8, &str, &str, &str)]| SmartList {
        any,
        conditions: conditions.iter().map(|(p, o, u, l, r)| Condition::new(p, *o, u, l, r)).collect(),
    };
    let ok = |l: SmartList| l.filter().unwrap();
    assert_eq!(
        ok(list(false, &[("genre", 8, "", "deep house", ""), ("bpm", 3, "", "124", "")])),
        r#"genre:"deep house" bpm:>124"#
    );
    assert_eq!(ok(list(false, &[("rating", 1, "", "4", "")])), "rating:4..4");
    assert_eq!(ok(list(false, &[("stockDate", 2, "", "2024-03-01", "")])), "-added:2024-03-01..2024-03-01");
    assert_eq!(
        ok(list(false, &[("stockDate", 5, "", "2024-05-31", "2024-03-01")])),
        "added:2024-03-01..2024-05-31",
        "a range's ends in either order"
    );
    assert_eq!(ok(list(true, &[("grouping", 1, "", "2", ""), ("grouping", 1, "", "0", "")])), "color:red|none");
    assert_eq!(ok(list(true, &[("myTag", 8, "", "A/x", ""), ("myTag", 8, "", "B/y", "")])), r#"tag:"A/x|B/y""#);
    let refused = |l: SmartList| l.filter().unwrap_err();
    assert_eq!(refused(list(false, &[("artist", 8, "", "Daft", "")])), r#"artist contains "Daft""#);
    assert_eq!(refused(list(false, &[("name", 10, "", "One", "")])), r#"name starts with "One""#);
    assert_eq!(refused(list(false, &[("stockDate", 6, "day", "30", "")])), "stockDate is in the last 30 day(s)");
    assert_eq!(
        refused(list(true, &[("genre", 8, "", "a", ""), ("bpm", 3, "", "1", "")])),
        r#"bpm is greater than "1""#
    );
    assert_eq!(refused(list(false, &[("dateCreated", 1, "", "2024-01-01", "")])), r#"dateCreated is "2024-01-01""#);
    assert_eq!(refused(list(false, &[])), "it has no conditions");
}

/// A track drawn from small pools, so conditions often hit, at `/Music/{i}.mp3`.
#[hegel::test_helper]
fn draw_track(tc: &TestCase, i: u32) -> Track {
    let text = || gs::sampled_from(vec!["", "Deep House", "deep  house", "Techno", "Ünï", "dub techno", "8A", "Am"]);
    let date = || gs::sampled_from(vec!["", "2024-03-01", "2023-12-24", "2024-05-31", "2024-13-01", "2024-03"]);
    Track {
        id: i,
        file_path: format!("/Music/{}{i}.mp3", tc.draw(text())),
        title: tc.draw(text()).into(),
        artist: tc.draw(text()).into(),
        album: tc.draw(text()).into(),
        album_artist: tc.draw(text()).into(),
        genre: tc.draw(text()).into(),
        label: tc.draw(text()).into(),
        key: tc.draw(text()).into(),
        comment: tc.draw(text()).into(),
        remixer: tc.draw(text()).into(),
        mix_name: tc.draw(text()).into(),
        composer: tc.draw(text()).into(),
        original_artist: tc.draw(text()).into(),
        bpm: tc.draw(gs::sampled_from(vec![0.0, 122.8, 128.0, 128.5, 130.0])),
        rating: tc.draw(gs::integers::<u8>().max_value(5)),
        play_count: tc.draw(gs::integers::<u16>().max_value(3)),
        year: tc.draw(gs::sampled_from(vec![0u16, 1999, 2000, 2024])),
        duration_secs: tc.draw(gs::sampled_from(vec![0u32, 300, 320])),
        date_added: tc.draw(date()).into(),
        release_date: tc.draw(date()).into(),
        color: TrackColor::from_id(tc.draw(gs::integers::<u8>().max_value(8))),
        ..Default::default()
    }
}

/// The properties of each kind of condition, its operators and values to draw from (some that
/// can't be read or said among them), and last any property with any operator and value.
#[allow(clippy::type_complexity)]
const KINDS: &[(&[&str], &[u8], &[&str])] = &[
    (
        &[
            "artist",
            "album",
            "albumArtist",
            "originalArtist",
            "producer",
            "genre",
            "key",
            "label",
            "mixName",
            "remixedBy",
            "name",
            "comments",
            "fileName",
        ],
        &[1, 2, 8, 9, 10, 11],
        &["", "deep", "Deep House", "techno", "ü", "8A", "a|b", "\"q\"", "x"],
    ),
    (
        &["bpm", "rating", "counter", "year", "duration"],
        &[1, 2, 3, 4, 5],
        &["128", "12850", "122.8", "3", "-1", "0", "2000", "300", "x"],
    ),
    (
        &["stockDate", "dateReleased", "dateCreated"],
        &[1, 2, 3, 4, 5, 6, 7],
        &["2024-03-01", "2024-05-31", "2024", "30", "x"],
    ),
    (&["grouping"], &[1, 2, 3], &["0", "2", "9", "x"]),
    (&["myTag"], &[8, 9, 1], &["Genre/Deep", "Situation/Peak", "genre/deep", "Deep", ""]),
    (
        &["artist", "genre", "bpm", "stockDate", "grouping", "myTag", "dateCreated", "unknown"],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        &["", "deep", "128", "2", "2024-03-01", "Genre/Deep", "a|b", "x"],
    ),
];

/// A condition of the `kind`th of [`KINDS`], on `property` when it's given.
#[hegel::test_helper]
fn draw_condition(tc: &TestCase, kind: usize, property: Option<&str>) -> Condition {
    let (properties, operators, values) = KINDS[kind];
    let property = property.unwrap_or_else(|| tc.draw(gs::sampled_from(properties.to_vec())));
    let operator = tc.draw(gs::sampled_from(operators.to_vec()));
    let unit = tc.draw(gs::sampled_from(vec!["day", "week", "month", "year", ""]));
    Condition::new(
        property,
        operator,
        unit,
        tc.draw(gs::sampled_from(values.to_vec())),
        tc.draw(gs::sampled_from(values.to_vec())),
    )
}

/// Where a filter can say an intelligent playlist, it matches exactly the tracks the playlist's
/// rules do, and it's a rule `smart add` takes.
#[hegel::test(test_cases = 400)]
fn a_filter_matches_what_the_rules_match(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(6));
    let tracks: Vec<Track> = (1..=n).map(|i| draw_track(&tc, i)).collect();
    let mut tags: FxHashMap<String, Vec<String>> = FxHashMap::default();
    for t in &tracks {
        let carried: Vec<&str> = tc.draw(gs::subsequences(&["Genre/Deep", "Situation/Peak", "genre/DEEP"]));
        tags.insert(crate::paths::file_key(&t.file_path), carried.into_iter().map(String::from).collect());
    }
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    // Of one property now and then, which a filter can say as alternatives when any one matches.
    let one: Option<(usize, &str)> = tc.draw(gs::booleans()).then(|| {
        let kind = tc.draw(gs::integers::<usize>().max_value(KINDS.len() - 1));
        (kind, tc.draw(gs::sampled_from(KINDS[kind].0.to_vec())))
    });
    let conditions = (0..count)
        .map(|_| match one {
            Some((kind, property)) => draw_condition(&tc, kind, Some(property)),
            None => draw_condition(&tc, tc.draw(gs::integers::<usize>().max_value(KINDS.len() - 1)), None),
        })
        .collect();
    let list = SmartList { any: tc.draw(gs::booleans()), conditions };
    let Ok(filter) = list.filter() else { return };
    let parsed = Filter::parse(&filter).unwrap_or_else(|e| panic!("{filter}: {e}"));
    let rule = crate::smart::SmartPlaylist {
        path: "Smart".into(),
        filter: filter.clone(),
        sort: None,
        limit: None,
        drives: Vec::new(),
    };
    rule.validate().unwrap();
    let library = Library { tracks, ..Default::default() };
    let ctx = QueryContext::new(&library).with_tags(&tags);
    for t in &library.tracks {
        let own = &tags[&crate::paths::file_key(&t.file_path)];
        assert_eq!(parsed.matches(t, &ctx), list.matches(t, own, today()), "{filter} on {t:?} with {own:?}");
    }
}

/// Relative dates count back from today: within the last n units, or before them.
#[test]
fn in_the_last_counts_back_from_today() {
    let t = |d: &str| Track { date_added: d.into(), ..Default::default() };
    let last = |n: &str, unit: &str, within: bool| SmartList {
        any: false,
        conditions: vec![Condition::new("stockDate", if within { 6 } else { 7 }, unit, n, "")],
    };
    assert!(last("1", "month", true).matches(&t("2024-05-02"), &[], today()));
    assert!(!last("1", "month", true).matches(&t("2024-05-01"), &[], today()));
    assert!(last("1", "week", false).matches(&t("2024-05-24"), &[], today()));
    assert!(!last("1", "year", true).matches(&t(""), &[], today()), "no date is never recent");
    assert!(last("2", "day", true).unevaluable().is_none());
    assert!(last("2", "fortnight", true).unevaluable().is_some());
}
