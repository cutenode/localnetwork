use super::*;
use loclib_model::test_util::track;

fn tracks(n: u32) -> Vec<SetEntry> {
    (1..=n).map(|i| SetEntry::added(&format!("A - {i}"))).collect()
}

fn titles(tracks: &[SetEntry]) -> Vec<&str> {
    tracks.iter().map(|e| e.title.as_str()).collect()
}

fn typed(text: &str) -> Result<SetEntry> {
    Ok(SetEntry::added(text))
}

#[test]
fn positions_name_the_tracklist_before_the_edit() {
    let c = Changes { drop: vec![1], moves: vec![(5, 2)], ..Default::default() };
    assert_eq!(titles(&c.apply(&tracks(6), typed).unwrap()), ["5", "2", "3", "4", "6"]);

    let up = Changes { moves: vec![(5, 2)], ..Default::default() };
    assert_eq!(titles(&up.apply(&tracks(6), typed).unwrap()), ["1", "5", "2", "3", "4", "6"]);
    let down = Changes { moves: vec![(2, 5)], ..Default::default() };
    assert_eq!(titles(&down.apply(&tracks(6), typed).unwrap()), ["1", "3", "4", "5", "2", "6"]);
    let still = Changes { moves: vec![(3, 3)], ..Default::default() };
    assert_eq!(titles(&still.apply(&tracks(3), typed).unwrap()), ["1", "2", "3"]);

    let c = Changes {
        drop: vec![2, 3],
        insert: vec![(3, "X - a".into()), (5, "X - end".into()), (3, "X - b".into())],
        moves: vec![(4, 3)],
        ..Default::default()
    };
    assert_eq!(titles(&c.apply(&tracks(4), typed).unwrap()), ["1", "a", "b", "4", "end"]);
}

#[test]
fn tracks_are_marked_unnamed_and_named() {
    let mut list = tracks(3);
    list[2].unnamed = true;
    let c = Changes { unnamed: vec![1], named: vec![3], moves: vec![(1, 3)], ..Default::default() };
    let out = c.apply(&list, typed).unwrap();
    assert_eq!(titles(&out), ["2", "3", "1"]);
    assert_eq!(out.iter().map(|e| e.unnamed).collect::<Vec<_>>(), [false, false, true]);
}

#[test]
fn bad_positions_are_refused_before_anything_changes() {
    let err = |c: Changes, len: u32| {
        let mut asked = false;
        let e = c.apply(&tracks(len), |t| {
            asked = true;
            typed(t)
        });
        assert!(!asked, "nothing is looked up for a refused edit");
        e.unwrap_err().to_string()
    };
    assert_eq!(err(Changes { drop: vec![4], ..Default::default() }, 3), "there's no track 4: the set has 3");
    assert_eq!(err(Changes { drop: vec![0], ..Default::default() }, 3), "there's no track 0: the set has 3");
    assert_eq!(err(Changes { named: vec![1], ..Default::default() }, 0), "there's no track 1: the set has no tracks");
    assert_eq!(err(Changes { moves: vec![(1, 4)], ..Default::default() }, 3), "there's no track 4: the set has 3");
    let insert = |at: usize| Changes { insert: vec![(at, "x".into())], ..Default::default() };
    assert_eq!(err(insert(5), 3), "can't insert at 5: positions go from 1 to 4 (one past the last track appends)");
    assert!(err(insert(0), 3).starts_with("can't insert at 0"));
    assert_eq!(err(Changes { moves: vec![(1, 2), (1, 3)], ..Default::default() }, 3), "track 1 is moved twice");
    assert_eq!(
        err(Changes { drop: vec![2], moves: vec![(2, 1)], ..Default::default() }, 3),
        "track 2 can't be dropped and moved"
    );
    assert_eq!(
        err(Changes { unnamed: vec![2], named: vec![2], ..Default::default() }, 3),
        "track 2 can't be named and unnamed"
    );
    assert_eq!(titles(&insert(4).apply(&tracks(3), typed).unwrap()), ["1", "2", "3", "x"]);
}

#[test]
fn inserts_take_the_one_library_track_that_matches() {
    let lib = Library {
        tracks: vec![
            track(1, "Floorplan", "Never Grow Old", "/Contents/f.mp3"),
            track(2, "Floorplan", "Tell You No Lie", "/Contents/g.mp3"),
        ],
        ..Default::default()
    };
    let (e, note) = lookup("floorplan - never grow", Some(&lib)).unwrap();
    assert_eq!(
        (e.title.as_str(), e.file.as_deref(), e.origin),
        ("Never Grow Old", Some("/Contents/f.mp3"), EntryOrigin::Added)
    );
    assert_eq!(note, "Floorplan - Never Grow Old went in from the library");
    let (e, note) = lookup("Floorplan", Some(&lib)).unwrap();
    assert_eq!((e.artist.as_str(), e.file.as_deref()), ("", None));
    assert_eq!(note, "\"Floorplan\" went in as typed: 2 tracks match it");
    let (e, note) = lookup("Someone - Dubplate", Some(&lib)).unwrap();
    assert_eq!((e.artist.as_str(), e.title.as_str()), ("Someone", "Dubplate"));
    assert_eq!(note, "\"Someone - Dubplate\" went in as typed: no track matches it");
    assert!(lookup("x", None).unwrap().1.ends_with("no drive to look it up on"));
    assert!(lookup("  ", Some(&lib)).is_err());
}

#[test]
fn fields_are_set_cleared_and_checked() {
    let mut set = crate::sets::query::tests::set("3m2abcdefgh22", "2026-10-03", None);
    set.city = Some("Leeds".into());
    let fields = SetFields {
        title: Some(" Friday ".into()),
        date: Some("2026-10-02".into()),
        country: Some("gb".into()),
        city: Some(String::new()),
        with: Some(vec!["Someone".into(), "did:plc:abc".into(), " ".into()]),
        ..Default::default()
    };
    fields.apply(&mut set).unwrap();
    assert_eq!(set.title.as_deref(), Some("Friday"));
    assert_eq!(set.date.to_string(), "2026-10-02");
    assert_eq!((set.country.as_deref(), set.city.as_deref()), (Some("GB"), None));
    assert_eq!(
        set.with,
        [
            Partner { name: "Someone".into(), did: None },
            Partner { name: "did:plc:abc".into(), did: Some("did:plc:abc".into()) }
        ]
    );
    SetFields { with: Some(Vec::new()), ..Default::default() }.apply(&mut set).unwrap();
    assert!(set.with.is_empty());

    let before = set.clone();
    for bad in [
        SetFields { date: Some(String::new()), ..Default::default() },
        SetFields { date: Some("3 Oct".into()), ..Default::default() },
        SetFields { title: Some("Changed".into()), country: Some("GBR".into()), ..Default::default() },
        SetFields { event: Some("https://example.com/e".into()), ..Default::default() },
        SetFields { link: Some("mixcloud.com/x".into()), ..Default::default() },
    ] {
        assert!(bad.apply(&mut set).is_err(), "{bad:?}");
    }
    assert_eq!(set, before, "a refused edit changes nothing");
}

use hegel::TestCase;
use hegel::generators as gs;

/// A track typed in by its file path is that library track; any other text is the library's
/// track when it's the only one that matches, else it goes in as typed, saying how many
/// matched; with no library to look in, as typed.
#[hegel::test(test_cases = 300)]
fn inserts_are_the_library_track_or_as_typed(tc: TestCase) {
    const NAMES: &[(&str, &str)] =
        &[("Floorplan", "Never Grow Old"), ("Floorplan", "Tell You No Lie"), ("Someone", "Dubplate"), ("", "ID")];
    let lib = Library {
        tracks: (1..=tc.draw(gs::integers::<u32>().min_value(1).max_value(5)))
            .map(|n| {
                let (artist, title) = tc.draw(gs::sampled_from(NAMES.to_vec()));
                track(n, artist, title, &format!("/Contents/{n}.mp3"))
            })
            .collect(),
        ..Default::default()
    };
    let n = tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1));
    let by_path = &lib.tracks[n];
    let (e, note) = lookup(&format!(" {} ", by_path.file_path), Some(&lib)).unwrap();
    assert_eq!(e, SetEntry::from_track(by_path, EntryOrigin::Added, None));
    assert_eq!(note, format!("{} went in from the library", by_path.display_name()));

    let text =
        tc.draw(gs::sampled_from(vec!["floorplan", "Never Grow Old", "someone - dubplate", "Nobody - Nothing", "ID"]));
    let (e, note) = lookup(text, Some(&lib)).unwrap();
    match lib.tracks.iter().find(|t| e == SetEntry::from_track(t, EntryOrigin::Added, None)) {
        Some(t) => assert_eq!(note, format!("{} went in from the library", t.display_name())),
        None => {
            assert_eq!(e, SetEntry::added(text));
            let matched = note.strip_prefix(&format!("{text:?} went in as typed: ")).unwrap();
            let many = matched.strip_suffix(" tracks match it").map(|k| k.parse::<usize>().unwrap());
            assert!(
                matched == "no track matches it" || many.is_some_and(|k| (2..=lib.tracks.len()).contains(&k)),
                "{note}"
            );
        }
    }
    assert!(lookup(&" ".repeat(n), Some(&lib)).is_err(), "a blank is refused");
    let (e, note) = lookup(text, None).unwrap();
    assert_eq!((e, note), (SetEntry::added(text), format!("{text:?} went in as typed: no drive to look it up on")));
}
