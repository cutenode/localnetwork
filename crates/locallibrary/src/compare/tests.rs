use super::*;

use crate::model::Track;

fn lib(paths: &[&str], added: &str) -> Library {
    Library {
            tracks: paths
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let mut t: Track = serde_json::from_value(serde_json::json!({
                        "id": i as u32 + 1, "title": p, "artist": "", "album": "", "album_artist": "", "genre": "",
                        "label": "", "key": "", "remixer": "", "original_artist": "", "composer": "", "comment": "",
                        "mix_name": "", "isrc": "", "bpm": 120.0, "duration_secs": 100, "year": 0, "track_number": 0,
                        "disc_number": 0, "rating": 0, "color": null, "play_count": 0, "bitrate": 320,
                        "sample_rate": 44100, "sample_depth": 16, "file_size": 1, "file_path": format!("/Contents/{p}"),
                        "analyze_path": "", "date_added": added, "release_date": "", "artwork_id": null, "artwork_path": null
                    }))
                    .unwrap();
                    t.id = i as u32 + 1;
                    t
                })
                .collect(),
            ..Default::default()
        }
}

fn side(label: &str, fp: u8, lineage: &[u8], library: Library) -> Side {
    Side {
        label: label.into(),
        id: None,
        library_id: None,
        fingerprint: Some(Hash::of(&[fp])),
        aliases: Vec::new(),
        lineage: lineage
            .iter()
            .map(|f| LineageEntry {
                fingerprint: Hash::of(&[*f]),
                at: Utc::now(),
                op: LineageOp::Sync,
                snapshot: None,
                source: None,
                tracks: 0,
            })
            .collect(),
        library,
        sequence: None,
        exported_at: None,
        pending: false,
        mid_copy: false,
        packed_from: None,
    }
}

#[test]
fn lineage_decides() {
    let l = lib(&["a", "b"], "2026-01-01");
    // A re-exported (fp 2) after both were synced at fp 1.
    let a = side("A", 2, &[1], l.clone());
    let b = side("B", 1, &[1], l.clone());
    let c = compare(&a, &b);
    assert_eq!((c.relation, c.confidence), (Relation::LeftNewer, Confidence::High));
    assert_eq!(compare(&b, &a).relation, Relation::RightNewer);
    // Both re-exported independently.
    let b2 = side("B", 3, &[1], l.clone());
    assert_eq!(compare(&a, &b2).relation, Relation::Diverged);
    assert_eq!(compare(&a, &side("A2", 2, &[], l)).relation, Relation::Identical);
}

#[test]
fn more_tracks_alone_dont_make_a_drive_newer() {
    // A newer export that removed a track, against an older drive: nothing else tells them
    // apart (OneLibrary-only drives have no save counter).
    let older = side("older", 1, &[], lib(&["a", "b", "c", "d"], "2026-01-01"));
    let newer = side("newer", 2, &[], lib(&["a", "b", "c"], "2026-01-01"));
    let c = compare(&older, &newer);
    assert_eq!((c.relation, c.confidence), (Relation::LeftNewer, Confidence::Low));
    // Whichever side the other signals favour, one with fewer tracks isn't copied on them.
    let mut newer = newer;
    newer.sequence = Some(9);
    newer.exported_at = Some(Utc::now());
    let mut older = older;
    older.sequence = Some(1);
    older.exported_at = Some(Utc::now() - chrono::Duration::days(3));
    newer.library.tracks[0].date_added = "2026-02-01".into();
    let c = compare(&older, &newer);
    assert_eq!((c.relation, c.confidence), (Relation::Undetermined, Confidence::Low), "{:?}", c.evidence);
}

#[test]
fn a_drive_whose_last_operation_was_cut_off_isnt_copied_from() {
    let l = lib(&["a", "b"], "2026-01-01");
    let mut a = side("A", 2, &[1], l.clone());
    let b = side("B", 1, &[1], l);
    a.pending = true;
    let c = compare(&a, &b);
    assert_eq!((c.relation, c.confidence), (Relation::Undetermined, Confidence::Low));
    assert_eq!(compare(&b, &a).relation, Relation::Undetermined);
    // Onto it is fine: B is behind, and copying onto it replaces what was cut off.
    let (mut a, mut b) = (a, b);
    std::mem::swap(&mut a.pending, &mut b.pending);
    assert_eq!(compare(&a, &b).relation, Relation::LeftNewer);
}

#[test]
fn an_undetermined_comparison_says_why() {
    let why = |a: &Side, b: &Side| compare(a, b).undetermined().map(str::to_string);
    let l = lib(&["a", "b", "c"], "");
    assert!(why(&side("A", 1, &[], l.clone()), &side("B", 2, &[], l.clone())).unwrap().contains("doesn't tell"));

    // B's save counter and newest track outweigh A's extra track.
    let mut more = side("A", 1, &[], lib(&["a", "b", "c"], "2026-01-01"));
    more.sequence = Some(1);
    let mut fewer = side("B", 2, &[], lib(&["a", "b"], "2026-01-01"));
    fewer.sequence = Some(9);
    fewer.library.tracks[0].date_added = "2026-02-01".into();
    assert!(why(&more, &fewer).unwrap().contains("fewer tracks"));

    let mut cut_off = side("B", 2, &[1], l.clone());
    cut_off.pending = true;
    assert!(why(&side("A", 1, &[1], l.clone()), &cut_off).unwrap().contains("didn't finish"));
    assert_eq!(why(&side("A", 1, &[1], l.clone()), &side("B", 2, &[1], l)), None, "B is newer");
}

#[test]
fn heuristics_without_lineage() {
    let old = side("old", 1, &[], lib(&["a", "b", "c"], "2026-01-01"));
    let new = side("new", 2, &[], lib(&["a", "b", "c", "d"], "2026-02-01"));
    let c = compare(&old, &new);
    assert_eq!(c.relation, Relation::RightNewer);
    assert_eq!(c.confidence, Confidence::Medium);
    assert_eq!(c.diff.added_tracks.len(), 1);

    let x = side("x", 1, &[], lib(&["a", "b", "c", "x"], ""));
    let y = side("y", 2, &[], lib(&["a", "b", "c", "y"], ""));
    assert_eq!(compare(&x, &y).relation, Relation::Diverged);

    let other = side("other", 3, &[], lib(&["p", "q", "r"], ""));
    let c = compare(&old, &other);
    assert_eq!(c.relation, Relation::Unrelated);
    assert_eq!(c.suggestion, Suggestion::Merge);
}

/// Comparing the other way round says the same with the sides swapped, whatever the two
/// sides hold: an identical state is identical, and a side whose lineage holds the other's
/// state (nothing cut off) is newer, for certain.
#[hegel::test(test_cases = 400)]
fn comparing_the_other_way_round_swaps_the_answer(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let pool = ["a", "b", "c", "d", "e", "f"];
    let draw_side = |tc: &hegel::TestCase, label: &str| {
        let tracks: Vec<&str> = pool.iter().copied().filter(|_| tc.draw(gs::weighted_booleans(0.7))).collect();
        let added = tc.draw(gs::sampled_from(vec!["", "2026-01-01", "2026-02-01"]));
        let lineage: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(4)).max_size(3));
        let mut s = side(label, tc.draw(gs::integers::<u8>().max_value(4)), &lineage, lib(&tracks, added));
        s.sequence = tc.draw(gs::optional(gs::integers::<u32>().max_value(9)));
        s.exported_at = tc
            .draw(gs::optional(gs::integers::<i64>().min_value(0).max_value(5)))
            .map(|d| Utc::now() - chrono::Duration::days(d));
        s.pending = tc.draw(gs::weighted_booleans(0.15));
        s.mid_copy = s.pending && tc.draw(gs::booleans());
        s.library_id = tc.draw(gs::optional(gs::integers::<u128>().max_value(1))).map(Uuid::from_u128);
        s
    };
    let a = draw_side(&tc, "A");
    let b = draw_side(&tc, "B");
    let ab = compare(&a, &b);
    let ba = compare(&b, &a);
    let mirror = |r: Relation| match r {
        Relation::LeftNewer => Relation::RightNewer,
        Relation::RightNewer => Relation::LeftNewer,
        r => r,
    };
    assert_eq!(
        (mirror(ab.relation), ab.confidence),
        (ba.relation, ba.confidence),
        "{:?} / {:?}",
        ab.evidence,
        ba.evidence
    );
    assert_eq!(ab.left_is_newer(), ba.relation == Relation::RightNewer);
    assert!(!ab.evidence.is_empty());
    assert_eq!(ab.undetermined().is_some(), ab.relation == Relation::Undetermined);
    assert_eq!((ab.left.label.as_str(), ab.right.label.as_str()), ("A", "B"));
    if a.fingerprint == b.fingerprint {
        assert_eq!((ab.relation, ab.confidence), (Relation::Identical, Confidence::High));
        assert_eq!(ab.suggestion, Suggestion::Nothing);
    }
    let plain = !a.pending && !b.pending && !a.mid_copy && !b.mid_copy;
    if plain && a.fingerprint != b.fingerprint && b.knows(&a) && !a.knows(&b) {
        assert_eq!((ab.relation, ab.confidence), (Relation::RightNewer, Confidence::High));
        assert_eq!(ab.suggestion, Suggestion::UpdateLeft);
    }
    // The diff runs from the older side to the newer.
    let (older, newer) = if ab.relation == Relation::LeftNewer { (&b, &a) } else { (&a, &b) };
    let added: usize = newer
        .library
        .tracks
        .iter()
        .filter(|t| !older.library.tracks.iter().any(|o| o.file_path == t.file_path))
        .count();
    assert_eq!(ab.diff.added_tracks.len(), added);
}
