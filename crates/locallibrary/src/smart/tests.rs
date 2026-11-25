use super::*;

#[test]
fn rules_are_validated_and_replaced_by_path() {
    let mut s = SmartPlaylists::default();
    let rule = |path: &str, filter: &str| SmartPlaylist {
        path: path.into(),
        filter: filter.into(),
        sort: Some("-rating".into()),
        limit: None,
        drives: vec![],
    };
    s.set(rule("Smart/Peak", "bpm:>=128"), &Default::default()).unwrap();
    s.set(rule("Smart / Peak ", "bpm:>=130"), &Default::default()).unwrap();
    assert_eq!(s.playlists.len(), 1);
    assert_eq!(s.playlists[0].filter, "bpm:>=130");
    assert!(s.set(rule("Smart/Bad", "key:H"), &Default::default()).is_err());
    assert!(s.set(rule("", "bpm:1"), &Default::default()).is_err());
    assert!(
        s.set(rule("Smart/All", ""), &Default::default()).is_err()
            && s.set(rule("Smart/All", "  "), &Default::default()).is_err()
    );
    let only = Uuid::new_v4();
    s.set(SmartPlaylist { drives: vec![only], ..rule("Tokyo", "genre:techno") }, &Default::default()).unwrap();
    assert_eq!(s.edits_for(&[]).len(), 1);
    assert_eq!(s.edits_for(&[only]).len(), 2);
    s.remove("SMART/PEAK").unwrap();
    assert!(s.remove("nope").is_err());
}

/// Regression: rules were keyed by path ignoring case, so a rule for `FoO` replaced the one
/// for `Foo` (and filled its playlist).
#[test]
fn rules_whose_paths_differ_only_in_case_are_two_rules() {
    let mut s = SmartPlaylists::default();
    let rule = |path: &str| SmartPlaylist {
        path: path.into(),
        filter: "rating:5".into(),
        sort: None,
        limit: None,
        drives: vec![],
    };
    s.set(rule("Smart/Foo"), &Default::default()).unwrap();
    s.set(rule("Smart/FoO"), &Default::default()).unwrap();
    assert_eq!(s.playlists.len(), 2);
    assert!(s.remove("smart/foo").is_err(), "ambiguous");
    assert_eq!(s.remove("Smart/FoO").unwrap().path, "Smart/FoO");
    assert_eq!(s.remove("smart/foo").unwrap().path, "Smart/Foo");
}

/// A collection export's rules for a drive: added, put in place of the rule an earlier export
/// made for the drive, but never of another rule for the same path; and taken away once their
/// playlist is gone, with nothing else.
#[test]
fn an_export_keeps_its_own_rules_for_its_drive() {
    let (drive, other) = (Uuid::new_v4(), Uuid::new_v4());
    let rule = |path: &str, filter: &str, drives: Vec<Uuid>| SmartPlaylist {
        path: path.into(),
        filter: filter.into(),
        sort: None,
        limit: None,
        drives,
    };
    let mut s = SmartPlaylists::default();
    s.set(rule("Mine", "bpm:>=128", vec![other]), &Default::default()).unwrap();
    s.set(rule("Old", "bpm:1", vec![drive]), &Default::default()).unwrap();
    s.set(rule("Gone", "bpm:2", vec![drive]), &Default::default()).unwrap();
    s.set(rule("Gone too", "bpm:3", vec![other]), &Default::default()).unwrap();
    let exported = [rule("Mine", "genre:house", vec![]), rule("Old", "bpm:4", vec![]), rule("New", "bpm:5", vec![])];
    let left = s.keep_for_drive(drive, &exported, &["Gone".into(), "Gone too".into()]).unwrap();
    assert_eq!(left, ["Mine"]);
    let held: Vec<(&str, &str, &[Uuid])> =
        s.playlists.iter().map(|p| (p.path.as_str(), p.filter.as_str(), p.drives.as_slice())).collect();
    assert_eq!(
        held,
        [
            ("Mine", "bpm:>=128", &[other][..]),
            ("Old", "bpm:4", &[drive][..]),
            ("Gone too", "bpm:3", &[other][..]),
            ("New", "bpm:5", &[drive][..]),
        ]
    );
}

/// Where a filter can say an intelligent playlist, it matches exactly the tracks the playlist's
/// rules do, and it's a rule `smart add` takes.
#[hegel::test(test_cases = 400)]
fn prop_a_filter_matches_what_the_rules_match(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use loclib_rekordbox_db::masterdb::smartlist::SmartList;
    use loclib_rekordbox_db::test_util::{KINDS, draw_condition, draw_track};

    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(6));
    let tracks: Vec<crate::model::Track> = (1..=n).map(|i| draw_track(&tc, i)).collect();
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
    let parsed = crate::query::Filter::parse(&filter).unwrap_or_else(|e| panic!("{filter}: {e}"));
    let rule =
        SmartPlaylist { path: "Smart".into(), filter: filter.clone(), sort: None, limit: None, drives: Vec::new() };
    rule.validate(&Default::default()).unwrap();
    let library = crate::model::Library { tracks, ..Default::default() };
    let ctx = crate::query::QueryContext::new(&library).with_tags(&tags);
    for t in &library.tracks {
        let own = tags.of(t);
        assert_eq!(
            parsed.matches(t, &ctx),
            list.matches(t, own, chrono::NaiveDate::from_ymd_opt(2024, 6, 1).unwrap()),
            "{filter} on {t:?} with {own:?}"
        );
    }
}
