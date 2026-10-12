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
