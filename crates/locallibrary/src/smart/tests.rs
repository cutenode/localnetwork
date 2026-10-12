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
