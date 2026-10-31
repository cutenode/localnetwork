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
    s.set(rule("Smart/Peak", "bpm:>=128")).unwrap();
    s.set(rule("Smart / Peak ", "bpm:>=130")).unwrap();
    assert_eq!(s.playlists.len(), 1);
    assert_eq!(s.playlists[0].filter, "bpm:>=130");
    assert!(s.set(rule("Smart/Bad", "key:H")).is_err());
    assert!(s.set(rule("", "bpm:1")).is_err());
    assert!(s.set(rule("Smart/All", "")).is_err() && s.set(rule("Smart/All", "  ")).is_err());
    let only = Uuid::new_v4();
    s.set(SmartPlaylist { drives: vec![only], ..rule("Tokyo", "genre:techno") }).unwrap();
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
    s.set(rule("Smart/Foo")).unwrap();
    s.set(rule("Smart/FoO")).unwrap();
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
    s.set(rule("Mine", "bpm:>=128", vec![other])).unwrap();
    s.set(rule("Old", "bpm:1", vec![drive])).unwrap();
    s.set(rule("Gone", "bpm:2", vec![drive])).unwrap();
    s.set(rule("Gone too", "bpm:3", vec![other])).unwrap();
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
