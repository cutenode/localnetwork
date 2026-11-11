use super::*;
use crate::model::Library;
use crate::query::{Filter, QueryContext};
use hegel::TestCase;
use hegel::generators as gs;

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
    let mut tags = crate::tags::TagsByTrack::default();
    for t in &tracks {
        let carried: Vec<&str> = tc.draw(gs::subsequences(&["Genre/Deep", "Situation/Peak", "genre/DEEP"]));
        tags.by_id.insert(t.id, carried.into_iter().map(String::from).collect());
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
    rule.validate(&Default::default()).unwrap();
    let library = Library { tracks, ..Default::default() };
    let ctx = QueryContext::new(&library).with_tags(&tags);
    for t in &library.tracks {
        let own = tags.of(t);
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

/// `s` as an XML attribute value.
fn attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

/// The XML rekordbox writes for a list matching `any` with `conditions`, each its property,
/// operator, unit, left and right value as written, with stray elements around them.
fn xml(any: &str, conditions: &[(&str, String, &str, &str, &str)]) -> String {
    let rows: String = conditions
        .iter()
        .map(|(p, o, u, l, r)| {
            format!(
                r#"<CONDITION PropertyName="{}" Operator="{}" ValueUnit="{}" ValueLeft="{}" ValueRight="{}"/>"#,
                attr(p),
                attr(o),
                attr(u),
                attr(l),
                attr(r)
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0"?><NODE Id="-1" LogicalOperator="{any}"><OTHER/>{rows}<NODE LogicalOperator="2"/></NODE>"#
    )
}

/// A list read from rekordbox's XML holds its conditions in order, each as [`Condition::new`]
/// reads it (a My Tag's id, signed or not, as its path, or empty when there's none), and matches
/// any one of them only when the first `NODE`'s `LogicalOperator` is 2.
#[hegel::test(test_cases = 200)]
fn prop_a_list_reads_its_conditions_from_its_xml(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(4));
    let mut written = Vec::new();
    let mut want = Vec::new();
    for _ in 0..n {
        let (properties, operators, values) = KINDS[tc.draw(gs::integers::<usize>().max_value(KINDS.len() - 1))];
        let property = tc.draw(gs::sampled_from(properties.to_vec()));
        let operator: u8 = tc.draw(gs::sampled_from(operators.to_vec()));
        let unit = tc.draw(gs::sampled_from(vec!["day", "week", "month", "year", ""]));
        let right = tc.draw(gs::sampled_from(values.to_vec()));
        let (left, read) = if property == "myTag" {
            let id = tc.draw(gs::sampled_from(vec!["4294967295", "-1", " -1 ", "7", "x", ""]));
            let path =
                if id.trim().parse::<i64>().is_ok_and(|i| i == -1 || i == 4294967295) { "Genre/Deep" } else { "" };
            (id, path)
        } else {
            let left = tc.draw(gs::sampled_from(values.to_vec()));
            (left, left)
        };
        let shown = if tc.draw(gs::booleans()) { format!(" {operator} ") } else { operator.to_string() };
        written.push((property, shown, unit, left, right));
        want.push(Condition::new(property, operator, unit, read, right));
    }
    let any = tc.draw(gs::sampled_from(vec!["1", "2", " 2 ", "0", ""]));
    let list = SmartList::parse(&xml(any, &written), tag_path).unwrap();
    assert_eq!(list, SmartList { any: any.trim() == "2", conditions: want });
    let cut = tc.draw(gs::integers::<usize>().min_value(1).max_value(20));
    let unclosed = format!(r#"<NODE LogicalOperator="{any}"><CONDITION PropertyName="{}"#, &"genre"[..cut.min(5)]);
    assert!(SmartList::parse(&unclosed, tag_path).is_none(), "{unclosed}");
}

/// A list of one text field's "contains" (or "doesn't contain") is that field's filter term,
/// quoted and negated as needed, for the fields a filter has on their own (album, genre, label,
/// mix, remixer, title, comment); the others (artists, key, file name) can't be said.
#[hegel::test(test_cases = 100)]
fn prop_contains_rules_say_their_fields_filter_term(tc: TestCase) {
    let property = tc.draw(gs::sampled_from(KINDS[0].0.to_vec()));
    let contains = tc.draw(gs::booleans());
    let value = tc.draw(gs::sampled_from(vec!["deep", "Deep House", "ü", "8A"]));
    let list = SmartList {
        any: tc.draw(gs::booleans()),
        conditions: vec![Condition::new(property, if contains { 8 } else { 9 }, "", value, "")],
    };
    let field = match property {
        "album" => Some("album"),
        "genre" => Some("genre"),
        "label" => Some("label"),
        "mixName" => Some("mix"),
        "remixedBy" => Some("remixer"),
        "name" => Some("title"),
        "comments" => Some("comment"),
        _ => None,
    };
    let minus = if contains { "" } else { "-" };
    assert_eq!(list.filter().ok(), field.map(|f| format!(r#"{minus}{f}:"{value}""#)), "{property}");
    // Any of two values of it is one term with both, where it isn't negated.
    let other = tc.draw(gs::sampled_from(vec!["techno", "dub"]));
    let op = if contains { 8 } else { 9 };
    let either = SmartList {
        any: true,
        conditions: vec![Condition::new(property, op, "", value, ""), Condition::new(property, op, "", other, "")],
    };
    let want = field.filter(|_| contains).map(|f| format!(r#"{f}:"{value}|{other}""#));
    assert_eq!(either.filter().ok(), want, "{property}");
}

/// A number rule is the filter term for its field (`time` for the duration): `x..x` for is (or,
/// negated, isn't), `>x`, `<x`, and a range low end first.
#[hegel::test(test_cases = 100)]
fn prop_number_rules_say_their_filter_term(tc: TestCase) {
    let (property, name) = tc.draw(gs::sampled_from(vec![
        ("bpm", "bpm"),
        ("rating", "rating"),
        ("counter", "plays"),
        ("year", "year"),
        ("duration", "time"),
    ]));
    let operator = tc.draw(gs::integers::<u8>().min_value(1).max_value(5));
    let left = tc.draw(gs::integers::<u16>().max_value(999));
    let right = tc.draw(gs::integers::<u16>().max_value(999));
    let list = SmartList {
        any: false,
        conditions: vec![Condition::new(property, operator, "", &left.to_string(), &right.to_string())],
    };
    let want = match operator {
        1 => format!("{name}:{left}..{left}"),
        2 => format!("-{name}:{left}..{left}"),
        3 => format!("{name}:>{left}"),
        4 => format!("{name}:<{left}"),
        _ => format!("{name}:{}..{}", left.min(right), left.max(right)),
    };
    assert_eq!(list.filter(), Ok(want));
}

/// The operators that say the opposite of each other (is and isn't, contains and doesn't,
/// colour and My Tag ones too) split every track between them, as do a number's less than, is
/// and greater than; starting or ending with a value means containing it; a range is both ends
/// included in either order; and a list matching all (or any) of its conditions matches what
/// each one does, one it can't evaluate matching nothing.
#[hegel::test(test_cases = 300)]
fn prop_opposite_rules_split_the_tracks(tc: TestCase) {
    let t = draw_track(&tc, 1);
    let tags: Vec<String> = tc
        .draw(gs::subsequences(&["Genre/Deep", "Situation/Peak", "genre/DEEP"]))
        .into_iter()
        .map(String::from)
        .collect();
    let today = tc.draw(gs::sampled_from(vec![today(), NaiveDate::from_ymd_opt(2024, 3, 31).unwrap()]));
    let one = |c: Condition| SmartList { any: false, conditions: vec![c] };
    let holds = |property: &str, operator: u8, unit: &str, left: &str, right: &str| {
        let list = one(Condition::new(property, operator, unit, left, right));
        list.unevaluable().is_none().then(|| list.matches(&t, &tags, today))
    };
    // A count back further than any date: nothing is within it, and every track is before it.
    for unit in ["day", "year"] {
        assert_eq!(holds("stockDate", 6, unit, "4294967295", ""), Some(false));
        assert_eq!(holds("stockDate", 7, unit, "4294967295", ""), Some(true));
    }
    let kind = tc.draw(gs::integers::<usize>().max_value(4));
    let (properties, _, values) = KINDS[kind];
    let property = tc.draw(gs::sampled_from(properties.to_vec()));
    let left = tc.draw(gs::sampled_from(values.to_vec()));
    let right = tc.draw(gs::sampled_from(values.to_vec()));
    let pairs: &[(u8, u8)] = match kind {
        0 => &[(1, 2), (8, 9)],
        3 => &[(1, 2)],
        4 => &[(8, 9)],
        _ => &[(1, 2)],
    };
    for (yes, no) in pairs {
        if let (Some(a), Some(b)) = (holds(property, *yes, "", left, right), holds(property, *no, "", left, right)) {
            assert_ne!(a, b, "{property} {yes}/{no} {left:?} on {t:?}");
        }
    }
    if kind == 0 {
        let contains = holds(property, 8, "", left, "");
        for op in [10, 11] {
            if holds(property, op, "", left, "") == Some(true) {
                assert_eq!(contains, Some(true), "{property} {op} {left:?} on {t:?}");
            }
        }
    }
    let dated = kind == 2
        && date_value(
            DateField::of(if property == "stockDate" { DateField::Added } else { DateField::Released }, &t),
            false,
        )
        .is_some();
    if kind == 1 || dated {
        let [less, is, greater] = [4, 1, 3].map(|op| holds(property, op, "", left, ""));
        if let (Some(l), Some(i), Some(g)) = (less, is, greater) {
            assert_eq!(u8::from(l) + u8::from(i) + u8::from(g), 1, "{property} {left:?} on {t:?}");
        }
        let range = holds(property, 5, "", left, right);
        if let (Some(r), Some(back)) = (range, holds(property, 5, "", right, left)) {
            assert_eq!(r, back, "a range either way round");
            if left == right {
                assert_eq!(Some(r), holds(property, 1, "", left, ""), "a range of one value is that value");
            }
        }
    }
    if kind == 2 {
        let unit = tc.draw(gs::sampled_from(vec!["day", "week", "month", "year"]));
        let n = tc.draw(gs::sampled_from(vec!["0", "1", "3", "30", "4294967295"]));
        if let (Some(within), Some(before)) = (holds(property, 6, unit, n, ""), holds(property, 7, unit, n, "")) {
            assert!(!(within && before), "{property} in the last {n} {unit}s on {t:?}");
        }
    }

    let conditions: Vec<Condition> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
        .map(|_| draw_condition(&tc, tc.draw(gs::integers::<usize>().max_value(KINDS.len() - 1)), None))
        .collect();
    let each: Vec<bool> =
        conditions.iter().map(|c| c.rule.is_some() && one(c.clone()).matches(&t, &tags, today)).collect();
    let all = SmartList { any: false, conditions: conditions.clone() };
    let any = SmartList { any: true, conditions };
    assert_eq!(all.matches(&t, &tags, today), each.iter().all(|m| *m));
    assert_eq!(any.matches(&t, &tags, today), each.iter().any(|m| *m));
}
