//! Play statistics and playlist exports of drawn drives: every track is played or never played,
//! the most played come first, what was played since a snapshot is exactly the counts that went
//! up and the history lists recorded since, and an exported list holds the selection's tracks
//! in order.

use crate::{common, drawn};

use std::collections::BTreeSet;
use std::path::Path;

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::api::{self, PlayStatsRequest};
use locallibrary::device::Device;
use locallibrary::edit::{Edit, TrackFields};
use locallibrary::export::{self, Format, Locator, PathStyle};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{import, paths, pdb};

fn set_plays(root: &Path, file: &str, n: u16) {
    common::apply(
        root,
        &[Edit::EditTracks {
            tracks: TrackSelector::paths([file.to_string()]),
            fields: TrackFields { play_count: Some(n), ..Default::default() },
        }],
    );
}

#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn plays_are_counted_and_compared_with_a_snapshot(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let (root, lib) = drawn::drive(&tc, dir.path(), "A");
    let files: Vec<String> = lib.tracks.iter().map(|t| t.file_path.clone()).collect();
    for f in drawn::some(&tc, &files).into_iter().take(4) {
        set_plays(&root, &f, tc.draw(gs::integers::<u16>().min_value(1).max_value(9)));
    }
    let mut d = Device::open_export(&root).unwrap();
    d.adopt(Some("A")).unwrap();
    sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    let snapshot = store.libraries().unwrap()[0].head.unwrap();

    // At a set: more plays, and a history list.
    let more: Vec<String> = drawn::some(&tc, &files).into_iter().take(3).collect();
    let before = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    for f in &more {
        let now = before.tracks.iter().find(|t| t.file_path == *f).unwrap().play_count;
        set_plays(&root, f, now + 1);
    }
    let listed = tc.draw(gs::booleans());
    if listed {
        common::add_history(&root, "HISTORY 099", &[before.tracks[0].id]);
    }
    let limit = tc.draw(gs::integers::<usize>().min_value(1).max_value(20));
    let req = PlayStatsRequest {
        source: common::path(&root),
        since: Some(format!("snapshot:{}", snapshot.to_hex())),
        limit: Some(limit),
    };
    let s = api::play_stats(Some(&store), &req).unwrap();
    let after = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    assert_eq!(s.tracks, after.tracks.len());
    assert_eq!(s.played + s.never_played.len(), s.tracks);
    let in_lists: BTreeSet<u32> = after.history.iter().flat_map(|h| h.track_ids.iter().copied()).collect();
    let never: BTreeSet<u32> =
        after.tracks.iter().filter(|t| t.play_count == 0 && !in_lists.contains(&t.id)).map(|t| t.id).collect();
    assert_eq!(s.never_played.iter().map(|t| t.id).collect::<BTreeSet<_>>(), never);
    assert_eq!(s.most_played.len(), s.played.min(limit));
    assert!(
        s.most_played
            .windows(2)
            .all(|w| (w[0].play_count, w[0].history_lists) >= (w[1].play_count, w[1].history_lists))
    );

    let since = s.since.unwrap();
    assert_eq!(since.snapshot, snapshot);
    assert_eq!(since.new_histories, if listed { vec!["HISTORY 099".to_string()] } else { Vec::new() });
    let mut want: BTreeSet<String> = more.iter().cloned().collect();
    if listed {
        want.insert(before.tracks[0].file_path.clone());
    }
    let got: BTreeSet<String> = since.tracks.iter().map(|t| t.track.file_path.clone()).collect();
    assert_eq!(got, want);
    for t in &since.tracks {
        assert!(t.after > t.before || t.history_lists > 0);
    }
}

/// A playlist (or folder, whole or one file per playlist) exports its tracks in order, in each
/// format; the M3U and the CSV read back as those tracks.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn exported_lists_hold_their_selection(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = drawn::drive(&tc, dir.path(), "A");
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let mut names: Vec<Option<String>> = lib.playlists.iter().map(|p| Some(lib.playlist_path(p.id))).collect();
    names.push(None);
    let which = tc.draw(gs::sampled_from(names));
    let split = tc.draw(gs::booleans());
    let Ok(selections) = export::select(&lib, which.as_deref(), split, "Everything") else {
        // A name two playlists share picks neither.
        assert!(which.as_deref().is_some_and(|q| lib.find_playlist(q).is_none()));
        return;
    };
    let format = tc.draw(gs::sampled_from(vec![Format::M3u8, Format::Csv, Format::Txt]).print_as_debug());
    let style =
        tc.draw(gs::sampled_from(vec![PathStyle::Drive, PathStyle::Relative, PathStyle::Absolute]).print_as_debug());
    let d = Device::open_export(&root).unwrap();
    let locator = Locator::new(style, d.root(), &d.root().join("Lists"));
    let mut files = BTreeSet::new();
    for s in &selections {
        assert!(files.insert(s.file.to_string_lossy().to_lowercase()), "one file each");
        let text = export::render(format, &s.name, &s.tracks, &locator);
        match format {
            Format::Txt => assert_eq!(text.lines().count(), s.tracks.len()),
            Format::Csv => assert_eq!(text.lines().count(), s.tracks.len() + 1),
            _ => assert_eq!(text.lines().filter(|l| !l.starts_with('#')).count(), s.tracks.len()),
        }
        if format != Format::Txt {
            let entries = import::parse(text.as_bytes(), Some(&format!("x.{}", format.extension()))).unwrap();
            let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
            let want: Vec<&str> = s.tracks.iter().map(|t| t.title.as_str()).collect();
            assert_eq!(titles, want);
        }
    }
    if which.is_none() {
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].tracks.len(), lib.tracks.len());
    }
}
