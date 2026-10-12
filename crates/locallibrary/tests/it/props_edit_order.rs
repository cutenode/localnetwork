//! Edits that reorder a playlist on drawn drives: sorting keeps every entry and orders them by the
//! key (ties as they were), dropping repeats keeps the first of each track or recording, ordering
//! a set keeps each track once (in place, or in a new playlist beside the old one), and a sorted
//! add puts the new tracks in order. Replaying the journal after a new export orders the same
//! files the same way.

use crate::{common, drawn};

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, TrackFields};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Library, Track, paths};
use rekordbox_pdb::NewTrack;

/// How to read a number from a track.
type Read = fn(&Track) -> f64;

/// The numeric sort keys and how to read each from a track.
const KEYS: &[(&str, Read)] = &[
    ("bpm", |t| t.bpm),
    ("rating", |t| f64::from(t.rating)),
    ("year", |t| f64::from(t.year)),
    ("plays", |t| f64::from(t.play_count)),
    ("time", |t| f64::from(t.duration_secs)),
    ("bitrate", |t| f64::from(t.bitrate)),
];

/// Plan and apply `edits` without recording them: the drive as rekordbox exported it.
fn unrecorded(root: &Path, edits: &[Edit]) {
    let mut d = Device::open_export(root).unwrap();
    let opts = EditOptions { lenient: false, no_journal: true };
    let plan = edit::plan(&d, edits, &opts, &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
}

fn files(lib: &Library, playlist: &str) -> Vec<String> {
    let p = lib.find_playlist(playlist).unwrap();
    p.track_ids.iter().map(|id| paths::key(&lib.track(*id).unwrap().file_path)).collect()
}

fn by_file<'l>(lib: &'l Library, file: &str) -> &'l Track {
    lib.tracks.iter().find(|t| paths::key(&t.file_path) == file).unwrap()
}

/// Copies of some of the drive's tracks (same artist, title and length, another file), so the
/// playlist holds other copies of a recording; each copy joins `playlist`'s end.
#[hegel::test_helper]
fn add_copies(tc: &TestCase, root: &Path, lib: &Library, playlist: &str) {
    let originals = drawn::some(tc, &lib.tracks);
    for (n, t) in originals.iter().take(2).enumerate() {
        let file = format!("/Contents/Copies/{n} {}.mp3", t.title);
        let track = NewTrack {
            artist: Some(t.artist.clone()),
            duration: u16::try_from(t.duration_secs).unwrap(),
            ..NewTrack::new(t.title.clone(), file.clone())
        };
        common::add_track(root, track, &common::placeholder_audio(&file));
        unrecorded(
            root,
            &[Edit::AddTracks {
                playlist: playlist.into(),
                tracks: TrackSelector::paths([file]),
                position: None,
                sort: None,
                allow_repeats: true,
            }],
        );
    }
}

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn reordering_a_playlist_keeps_its_tracks_in_the_asked_order(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    // The twelve-track export drawn on, or the demo export, whose analysis has phrases (the
    // energy a set is ordered by).
    let (root, lib) = match tc.draw(gs::booleans()) {
        true => drawn::drive(&tc, dir.path(), "Main"),
        false => {
            let root = common::demo_drive(dir.path(), "Main");
            let files: Vec<String> = common::library(&root).tracks.iter().map(|t| t.file_path.clone()).collect();
            let paths = [files.clone(), files].concat();
            unrecorded(&root, &[Edit::SetEntries { playlist: "Set".into(), paths, create: true }]);
            let lib = common::library(&root);
            (root, lib)
        }
    };
    let lists: Vec<String> = lib.playlists.iter().filter(|p| !p.is_folder).map(|p| lib.playlist_path(p.id)).collect();
    let playlist = tc.draw(gs::sampled_from(lists));
    // Some ratings, so sorting by them has ties and differences.
    let rated = drawn::some(&tc, &lib.tracks);
    for t in rated.iter().take(4) {
        let fields = TrackFields { rating: Some(tc.draw(gs::integers::<u8>().max_value(5))), ..Default::default() };
        unrecorded(&root, &[Edit::EditTracks { tracks: TrackSelector::paths([t.file_path.clone()]), fields }]);
    }
    add_copies(&tc, &root, &lib, &playlist);
    // At times OneLibrary alone, as rekordbox 7 can export.
    if tc.draw(gs::weighted_booleans(0.3)) {
        common::onelibrary_only(&root);
    }
    let lib = common::library(&root);
    let exported: Vec<(&str, Vec<u8>)> = [paths::EXPORT_PDB, paths::LIBRARY_PLUS_DB]
        .into_iter()
        .filter_map(|k| fs::read(root.join(k)).ok().map(|b| (k, b)))
        .collect();

    let (field, read) = KEYS[tc.draw(gs::integers::<usize>().max_value(KEYS.len() - 1))];
    let descending = tc.draw(gs::booleans());
    let by = format!("{}{field}", if descending { "-" } else { "" });
    let in_order = |list: &[String]| {
        list.windows(2).all(|w| {
            let (a, b) = (read(by_file(&lib, &w[0])), read(by_file(&lib, &w[1])));
            if descending { a >= b } else { a <= b }
        })
    };
    let sorted_multiset = |l: &[String]| {
        let mut l = l.to_vec();
        l.sort();
        l
    };
    let distinct_by = |l: &[String], key: &dyn Fn(&String) -> String| -> Vec<String> {
        let mut seen = BTreeSet::new();
        l.iter().filter(|f| seen.insert(key(f))).cloned().collect()
    };
    // Each kind of reordering once, in an order that leaves each something to do: a sorted add
    // (with repeats), a sort, dropping repeats, ordering the set, then dropping other copies.
    let start = |before: &[String]| match before.is_empty() {
        true => None,
        false => tc.draw(gs::optional(gs::sampled_from(before.to_vec()))).map(|f| TrackSelector::paths([f])),
    };
    let mut after_lib = lib.clone();
    for step in 0..5 {
        let before = files(&after_lib, &playlist);
        let edit = match step {
            0 => Edit::AddTracks {
                playlist: playlist.clone(),
                tracks: TrackSelector::paths(drawn::some(&tc, &lib.tracks).into_iter().map(|t| t.file_path)),
                position: tc.draw(gs::optional(gs::integers::<usize>().min_value(1).max_value(20))),
                sort: Some(by.clone()),
                allow_repeats: true,
            },
            1 => Edit::SortPlaylist { playlist: playlist.clone(), by: by.clone() },
            2 => Edit::Dedupe { playlist: playlist.clone(), recordings: false },
            3 => Edit::PlanSet {
                playlist: playlist.clone(),
                start: start(&before),
                into: tc.draw(gs::booleans()).then(|| "Planned/Set".to_string()),
            },
            _ => Edit::Dedupe { playlist: playlist.clone(), recordings: true },
        };
        if let Edit::AddTracks { tracks, .. } = &edit
            && tracks.paths.is_empty()
        {
            continue;
        }
        common::apply(&root, std::slice::from_ref(&edit));
        after_lib = common::library(&root);
        let after = files(&after_lib, &playlist);
        match &edit {
            Edit::AddTracks { tracks, position, .. } => {
                let added: Vec<String> = tracks.paths.iter().map(|p| paths::key(p)).collect();
                assert_eq!(after.len(), before.len() + added.len());
                let at = position.map_or(before.len(), |n| (n - 1).min(before.len()));
                let block = &after[at..at + added.len()];
                assert_eq!(sorted_multiset(block), sorted_multiset(&added));
                assert!(in_order(block), "the added tracks are sorted by {by}: {block:?}");
                assert_eq!([&after[..at], &after[at + added.len()..]].concat(), before, "the rest stay as they were");
            }
            Edit::SortPlaylist { .. } => {
                assert_eq!(sorted_multiset(&after), sorted_multiset(&before), "every entry stays");
                assert!(in_order(&after), "sorted by {by}: {after:?}");
                // Ties keep their order: a stable sort. Each key's entries are as they were.
                let with_key = |l: &[String], v: f64| -> Vec<String> {
                    l.iter().filter(|f| read(by_file(&lib, f)) == v).cloned().collect()
                };
                for f in &after {
                    let v = read(by_file(&lib, f));
                    assert_eq!(
                        with_key(&after, v),
                        with_key(&before, v),
                        "the tracks with {field} {v} keep their order"
                    );
                }
            }
            Edit::Dedupe { recordings: false, .. } => assert_eq!(after, distinct_by(&before, &|f| f.clone())),
            Edit::Dedupe { recordings: true, .. } => {
                assert_eq!(after, distinct_by(&before, &|f| by_file(&lib, f).fingerprint_key()));
            }
            Edit::PlanSet { start, into, .. } => {
                let planned = match into {
                    Some(target) => {
                        assert_eq!(after, before, "the playlist itself stays as it was");
                        files(&after_lib, target)
                    }
                    None => after.clone(),
                };
                let distinct = distinct_by(&before, &|f| f.clone());
                assert_eq!(sorted_multiset(&planned), sorted_multiset(&distinct), "each track once");
                if let Some(first) = start.as_ref().map(|s| paths::key(&s.paths[0])) {
                    assert_eq!(planned.first(), Some(&first), "the set starts where asked");
                }
            }
            _ => unreachable!(),
        }
    }

    // rekordbox exports over the edit: replaying the journal makes it again.
    let want =
        |lib: &Library| (files(lib, &playlist), lib.find_playlist("Planned/Set").map(|_| files(lib, "Planned/Set")));
    let edited = want(&after_lib);
    for (key, bytes) in &exported {
        fs::write(root.join(key), bytes).unwrap();
    }
    let (_, replayed) = common::replay(&root);
    assert_eq!(want(&replayed), edited);
}
