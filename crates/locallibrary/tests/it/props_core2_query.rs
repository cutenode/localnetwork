//! Filters against a model on hand-built libraries: `bpm:~X` holds tracks within 6% of X, half of
//! it or double it; `color:` the tracks of any colour named; `playlist:` a folder's tracks even
//! when its tree loops; and a playlist the library doesn't have, or none named at all, is refused.

use std::collections::BTreeSet;

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::model::TrackColor;
use locallibrary::query::{Filter, QueryContext};
use locallibrary::{Library, Playlist};

const COLOURS: &[(&str, Option<u8>)] =
    &[("none", None), ("pink", Some(1)), ("red", Some(2)), ("aqua", Some(6)), ("purple", Some(8))];

fn list(id: u32, parent_id: Option<u32>, name: &str, is_folder: bool, track_ids: Vec<u32>) -> Playlist {
    Playlist { id, parent_id, name: name.into(), is_folder, sort_order: id, track_ids }
}

#[hegel::test(test_cases = 200)]
fn tempo_colour_and_playlist_terms_select_what_they_name(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(8));
    let mut lib = Library::default();
    for id in 1..=n {
        let mut t = loclib_model::test_util::track(id, "A", &format!("T{id}"), &format!("/Contents/{id}.mp3"));
        t.bpm = f64::from(tc.draw(gs::integers::<u32>().min_value(500).max_value(2_000))) / 10.0;
        t.color = tc.draw(gs::sampled_from(COLOURS)).1.and_then(TrackColor::from_id);
        lib.tracks.push(t);
    }
    let ids: Vec<u32> = (1..=n).collect();
    let in_list: Vec<u32> = ids.iter().copied().filter(|_| tc.draw(gs::booleans())).collect();
    let in_nested: Vec<u32> = ids.iter().copied().filter(|_| tc.draw(gs::booleans())).collect();
    // A folder holding a playlist and a folder that holds the first folder again (a second row
    // for it, as damaged data can have).
    lib.playlists = vec![
        list(1, None, "Crates", true, vec![]),
        list(2, Some(1), "Peak", false, in_list.clone()),
        list(3, Some(1), "Deeper", true, vec![]),
        list(4, Some(3), "Late", false, in_nested.clone()),
        list(1, Some(3), "Crates", true, vec![]),
    ];
    let ctx = QueryContext::new(&lib);
    let select = |f: &str| -> BTreeSet<u32> {
        let filter = Filter::parse(f).unwrap();
        filter.check(&ctx).unwrap();
        filter.select(&ctx).iter().map(|t| t.id).collect()
    };

    let target = f64::from(tc.draw(gs::integers::<u32>().min_value(600).max_value(1_800))) / 10.0;
    let near = |bpm: f64, to: f64| (bpm - to).abs() <= 0.06 * to;
    let want: BTreeSet<u32> = lib
        .tracks
        .iter()
        .filter(|t| near(t.bpm, target) || near(t.bpm, target / 2.0) || near(t.bpm, target * 2.0))
        .map(|t| t.id)
        .collect();
    // Clear of the edge, where rounding decides.
    let edge = lib
        .tracks
        .iter()
        .any(|t| [target, target / 2.0, target * 2.0].iter().any(|to| ((t.bpm - to).abs() - 0.06 * to).abs() < 1e-6));
    if !edge {
        assert_eq!(select(&format!("bpm:~{target}")), want, "~{target}");
    }

    let named: Vec<(&str, Option<u8>)> = COLOURS.iter().copied().filter(|_| tc.draw(gs::booleans())).collect();
    if !named.is_empty() {
        let names: Vec<&str> = named.iter().map(|c| c.0).collect();
        let colours: Vec<Option<TrackColor>> = named.iter().map(|c| c.1.and_then(TrackColor::from_id)).collect();
        let want: BTreeSet<u32> = lib.tracks.iter().filter(|t| colours.contains(&t.color)).map(|t| t.id).collect();
        assert_eq!(select(&format!("color:{}", names.join("|"))), want);
    }

    let below: BTreeSet<u32> = in_list.iter().chain(&in_nested).copied().collect();
    assert_eq!(select("playlist:Crates"), below, "the loop ends");
    assert_eq!(select("playlist:Crates/Peak"), in_list.iter().copied().collect());

    let missing = Filter::parse("playlist:Nowhere").unwrap();
    assert_eq!(missing.check(&ctx).unwrap_err().code(), "not_found");
    assert_eq!(
        Filter::parse(tc.draw(gs::sampled_from(vec!["playlist:|", "playlist: | "]))).unwrap_err().code(),
        "invalid"
    );
}
