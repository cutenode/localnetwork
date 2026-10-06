//! Properties of the differences between two libraries (`diff`, `model::LibraryDiff`).

use std::collections::BTreeSet;

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use locallibrary::diff::{self, PlaylistDiff};
use locallibrary::model::LibraryDiff;
use locallibrary::{Library, Playlist, Track, paths};

/// Files both sides draw from, so libraries overlap.
const FILES: &[&str] = &["a", "b", "c", "d", "e", "F", "g", "h"];
/// Playlists with distinct folder paths (folders end in `/`).
const NODES: &[&str] = &["Shows/", "Shows/2026/", "Set", "Warmup", "Shows/Friday", "Shows/Set", "Shows/2026/Opening"];

fn blank(id: u32, path: &str) -> Track {
    Track {
        id,
        title: String::new(),
        artist: String::new(),
        album: String::new(),
        album_artist: String::new(),
        genre: String::new(),
        label: String::new(),
        key: String::new(),
        remixer: String::new(),
        original_artist: String::new(),
        composer: String::new(),
        comment: String::new(),
        mix_name: String::new(),
        isrc: String::new(),
        bpm: 120.0,
        duration_secs: 300,
        year: 0,
        track_number: 0,
        disc_number: 0,
        rating: 0,
        color: None,
        play_count: 0,
        bitrate: 320,
        sample_rate: 44100,
        sample_depth: 16,
        file_size: 1,
        file_path: path.into(),
        analyze_path: String::new(),
        date_added: String::new(),
        release_date: String::new(),
        artwork_id: None,
        artwork_path: None,
    }
}

/// Some of [`FILES`] (under ids of this export's own), with a few fields that may differ between
/// exports, and some of [`NODES`] (with the folders they need) holding any of those tracks.
#[hegel::composite]
fn library(tc: &TestCase) -> Library {
    let files: Vec<&str> = FILES.iter().copied().filter(|_| tc.draw(gs::booleans())).collect();
    let ids = tc.draw(
        gs::vecs(gs::integers::<u32>().min_value(1).max_value(40))
            .unique(true)
            .min_size(files.len())
            .max_size(files.len()),
    );
    let tracks: Vec<Track> = files
        .iter()
        .zip(&ids)
        .map(|(f, id)| Track {
            title: f.to_uppercase(),
            artist: tc.draw(gs::sampled_from(vec!["A", "B"])).into(),
            rating: tc.draw(gs::integers::<u8>().max_value(1)),
            bpm: tc.draw(gs::sampled_from(vec![120.0, 128.0])),
            ..blank(*id, &format!("/Contents/{f}.mp3"))
        })
        .collect();
    let mut playlists: Vec<Playlist> = Vec::new();
    for node in NODES {
        let folder = node.ends_with('/');
        let path = node.trim_end_matches('/');
        if !tc.draw(gs::booleans()) {
            continue;
        }
        let (parent, name) = match path.rsplit_once('/') {
            Some((dir, name)) => {
                match playlists.iter().find(|p| p.is_folder && p.name == dir.rsplit('/').next().unwrap()) {
                    Some(p) => (Some(p.id), name),
                    None => continue, // its folder isn't in this library
                }
            }
            None => (None, path),
        };
        let entries = if folder || ids.is_empty() {
            Vec::new()
        } else {
            tc.draw(gs::vecs(gs::sampled_from(ids.clone())).max_size(6))
        };
        let id = playlists.len() as u32 + 1;
        playlists.push(Playlist {
            id,
            parent_id: parent,
            name: name.into(),
            is_folder: folder,
            sort_order: id,
            track_ids: entries,
        });
    }
    Library { tracks, playlists, ..Default::default() }
}

fn playlist_count(lib: &Library) -> usize {
    lib.playlists.iter().filter(|p| !p.is_folder).count()
}

/// File keys of a playlist's entries, by its path.
fn entries(lib: &Library, path: &str) -> Vec<String> {
    let p = lib.playlists.iter().find(|p| !p.is_folder && lib.playlist_path(p.id) == path).unwrap();
    p.track_ids.iter().filter_map(|id| lib.track(*id)).map(|t| paths::key(&t.file_path).to_lowercase()).collect()
}

fn names(v: &[locallibrary::model::TrackRef]) -> BTreeSet<String> {
    v.iter().map(|t| t.file_path.clone()).collect()
}

fn summary(d: &PlaylistDiff) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>, BTreeSet<String>, usize) {
    let set = |v: &[String]| v.iter().cloned().collect::<BTreeSet<_>>();
    (
        names(&d.added_tracks),
        names(&d.removed_tracks),
        set(&d.added_playlists),
        d.changed_playlists.iter().map(|c| c.path.clone()).collect(),
        d.unchanged_playlists,
    )
}

/// A library compared with itself, or with an export of it numbered differently, has no
/// differences, and every playlist counts as unchanged.
#[hegel::test(test_cases = 300)]
fn a_library_equals_itself(tc: TestCase) {
    let a = tc.draw(library().print_as_debug());
    let mut renumbered = a.clone();
    for t in &mut renumbered.tracks {
        t.id += 1000;
    }
    for p in &mut renumbered.playlists {
        p.track_ids.iter_mut().for_each(|id| *id += 1000);
    }
    for b in [&a, &renumbered] {
        let d = diff::between(&a, b, ("a", "b"), None);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(d.unchanged_playlists, playlist_count(&a));
        assert!(LibraryDiff::between(&a, b).is_empty());
    }
}

/// Comparing the other way round swaps what was added and removed and finds the same changes.
#[hegel::test(test_cases = 300)]
fn swapping_sides_swaps_the_diff(tc: TestCase) {
    let (a, b) = (tc.draw(library().print_as_debug()), tc.draw(library().print_as_debug()));
    let (ab, ba) = (diff::between(&a, &b, ("a", "b"), None), diff::between(&b, &a, ("b", "a"), None));
    let (ab_added, ab_removed, ab_new, ab_changed, ab_same) = summary(&ab);
    let (ba_added, ba_removed, ba_new, ba_changed, ba_same) = summary(&ba);
    assert_eq!((ab_added, ab_removed), (ba_removed, ba_added));
    assert_eq!(ab_new, ba.removed_playlists.iter().cloned().collect());
    assert_eq!(ba_new, ab.removed_playlists.iter().cloned().collect());
    assert_eq!((ab_changed, ab_same), (ba_changed, ba_same));
    assert_eq!(
        ab.changed_tracks.iter().map(|c| &c.track.file_path).collect::<BTreeSet<_>>(),
        ba.changed_tracks.iter().map(|c| &c.track.file_path).collect::<BTreeSet<_>>()
    );
    for (x, y) in ab.changed_playlists.iter().zip(&ba.changed_playlists) {
        assert_eq!(x.path, y.path);
        let pos = |v: &[diff::Entry]| v.iter().map(|e| (e.track.file_path.clone(), e.position)).collect::<Vec<_>>();
        assert_eq!(pos(&x.added), pos(&y.removed));
        assert_eq!(pos(&x.removed), pos(&y.added));
        assert_eq!(x.moved.len(), y.moved.len(), "as few moves either way");
    }

    let (m_ab, m_ba) = (LibraryDiff::between(&a, &b), LibraryDiff::between(&b, &a));
    assert_eq!(names(&m_ab.added_tracks), names(&m_ba.removed_tracks));
    assert_eq!(names(&m_ab.removed_tracks), names(&m_ba.added_tracks));
    assert_eq!(m_ab.added_playlists, m_ba.removed_playlists);
    assert_eq!(m_ab.changed_playlists, m_ba.changed_playlists);
    let by_path = |v: &[locallibrary::model::TrackChange]| {
        let mut v = v.to_vec();
        v.sort_by(|x, y| x.track.file_path.cmp(&y.track.file_path));
        v
    };
    for (x, y) in by_path(&m_ab.changed_tracks).iter().zip(&by_path(&m_ba.changed_tracks)) {
        let fields = |c: &locallibrary::model::TrackChange, swap: bool| {
            c.fields
                .iter()
                .map(|f| {
                    if swap {
                        (f.field.clone(), f.new.clone(), f.old.clone())
                    } else {
                        (f.field.clone(), f.old.clone(), f.new.clone())
                    }
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(fields(x, false), fields(y, true));
    }
}

/// Length of the longest strictly increasing subsequence, the slow way.
fn lis(v: &[usize]) -> usize {
    let mut best = vec![1; v.len()];
    for i in 0..v.len() {
        for j in 0..i {
            if v[j] < v[i] {
                best[i] = best[i].max(best[j] + 1);
            }
        }
    }
    best.into_iter().max().unwrap_or(0)
}

/// Within a changed playlist, every left entry is removed, moved or stays, every right entry is
/// added, moved or stays, the entries that stay keep their order, and as few entries as possible
/// count as moved.
#[hegel::test(test_cases = 300)]
fn entry_changes_account_for_every_entry(tc: TestCase) {
    let (a, b) = (tc.draw(library().print_as_debug()), tc.draw(library().print_as_debug()));
    let d = diff::between(&a, &b, ("a", "b"), None);
    for c in &d.changed_playlists {
        let (left, right) = (entries(&a, &c.path), entries(&b, &c.path));
        assert_ne!(left, right);
        let key = |e: &locallibrary::model::TrackRef| paths::key(&e.file_path).to_lowercase();
        for e in &c.removed {
            assert_eq!(left[e.position - 1], key(&e.track));
        }
        for e in &c.added {
            assert_eq!(right[e.position - 1], key(&e.track));
        }
        for m in &c.moved {
            assert_eq!((&left[m.from - 1], &right[m.to - 1]), (&key(&m.track), &key(&m.track)));
        }
        let paired = left.len() - c.removed.len();
        assert_eq!(paired, right.len() - c.added.len(), "pairs balance");
        // What stays, read in order on either side, is the same sequence.
        let out_left: BTreeSet<usize> =
            c.removed.iter().map(|e| e.position).chain(c.moved.iter().map(|m| m.from)).collect();
        let out_right: BTreeSet<usize> =
            c.added.iter().map(|e| e.position).chain(c.moved.iter().map(|m| m.to)).collect();
        let stay = |v: &[String], out: &BTreeSet<usize>| {
            v.iter().enumerate().filter(|(i, _)| !out.contains(&(i + 1))).map(|(_, k)| k.clone()).collect::<Vec<_>>()
        };
        assert_eq!(stay(&left, &out_left), stay(&right, &out_right));
        // Pair the k-th copy of a file on the left with its k-th copy on the right: the paired
        // right positions, in left order, have a longest increasing run of `paired - moved`.
        let mut seen = std::collections::HashMap::<&str, usize>::new();
        let mut rights = Vec::new();
        for k in &left {
            let n = seen.entry(k).or_default();
            if let Some(j) = right.iter().enumerate().filter(|(_, r)| *r == k).nth(*n).map(|(j, _)| j) {
                rights.push(j);
            }
            *n += 1;
        }
        assert_eq!(rights.len(), paired);
        assert_eq!(c.moved.len(), paired - lis(&rights), "minimal moves");
    }
}

/// Diffing one playlist (or folder) reports a subset of the full diff, only below that path, and
/// no collection-level changes.
#[hegel::test(test_cases = 300)]
fn playlist_filter_narrows_the_diff(tc: TestCase) {
    let (a, b) = (tc.draw(library().print_as_debug()), tc.draw(library().print_as_debug()));
    let filter = tc.draw(gs::sampled_from(vec!["Shows", "shows/", "Shows/2026", "Set", "nope"]));
    let all = diff::between(&a, &b, ("a", "b"), None);
    let some = diff::between(&a, &b, ("a", "b"), Some(filter));
    assert!(some.added_tracks.is_empty() && some.removed_tracks.is_empty() && some.changed_tracks.is_empty());
    let below = |p: &str| {
        let (p, f) = (p.to_lowercase(), filter.trim_matches('/').to_lowercase());
        p.trim_end_matches('/') == f || p.starts_with(&format!("{f}/"))
    };
    for p in
        some.added_playlists.iter().chain(&some.removed_playlists).chain(some.changed_playlists.iter().map(|c| &c.path))
    {
        assert!(below(p), "{p} isn't below {filter}");
    }
    assert!(some.added_playlists.iter().all(|p| all.added_playlists.contains(p)));
    assert!(some.removed_playlists.iter().all(|p| all.removed_playlists.contains(p)));
    assert!(some.changed_playlists.iter().all(|c| all.changed_playlists.iter().any(|x| x.path == c.path)));
    assert!(some.unchanged_playlists <= all.unchanged_playlists);
}

/// rekordbox allows two playlists of the same name in one folder, and a `/` in a name. Either way
/// every playlist is still one playlist: comparing a library with itself counts each as
/// unchanged, and emptying one of them shows as a change.
#[hegel::test(test_cases = 100)]
fn every_playlist_is_accounted_for(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(5));
    let names = tc.draw(gs::vecs(gs::sampled_from(vec!["Set", "Shows", "Shows/Set"])).min_size(n).max_size(n));
    let mut lib = Library { tracks: vec![blank(1, "/Contents/a.mp3")], ..Default::default() };
    lib.playlists.push(Playlist {
        id: 1,
        parent_id: None,
        name: "Shows".into(),
        is_folder: true,
        sort_order: 0,
        track_ids: vec![],
    });
    for (i, name) in names.iter().enumerate() {
        let in_folder = tc.draw(gs::booleans());
        let id = i as u32 + 2;
        lib.playlists.push(Playlist {
            id,
            parent_id: in_folder.then_some(1),
            name: name.to_string(),
            is_folder: false,
            sort_order: id,
            track_ids: vec![1],
        });
    }
    let d = diff::between(&lib, &lib, ("a", "b"), None);
    assert_eq!(d.unchanged_playlists, n, "{d:?}");

    let victim = tc.draw(gs::integers::<usize>().min_value(2).max_value(n + 1));
    let mut emptied = lib.clone();
    emptied.playlists[victim - 1].track_ids.clear();
    assert!(!diff::between(&lib, &emptied, ("a", "b"), None).is_empty());
    assert!(!LibraryDiff::between(&lib, &emptied).is_empty());
}

/// The smallest case of the bug above: two top-level playlists both named "Set".
#[test]
fn same_named_siblings_are_two_playlists() {
    let list = |id, track_ids| Playlist {
        id,
        parent_id: None,
        name: "Set".into(),
        is_folder: false,
        sort_order: id,
        track_ids,
    };
    let lib = Library {
        tracks: vec![blank(1, "/Contents/a.mp3")],
        playlists: vec![list(1, vec![1]), list(2, vec![1])],
        ..Default::default()
    };
    let mut emptied = lib.clone();
    emptied.playlists[0].track_ids.clear();
    assert_eq!(diff::between(&lib, &lib, ("a", "b"), None).unchanged_playlists, 2);
    assert!(!diff::between(&lib, &emptied, ("a", "b"), None).is_empty());
}

/// Two folders both named "Shows", each holding a "Set", in an export whose playlist ids differ:
/// each "Set" still pairs with its own counterpart.
#[test]
fn same_named_folders_keep_their_playlists_apart() {
    let node = |id, parent_id, is_folder, track_ids| Playlist {
        id,
        parent_id,
        name: if is_folder { "Shows" } else { "Set" }.into(),
        is_folder,
        sort_order: id,
        track_ids,
    };
    let lib = Library {
        tracks: vec![blank(1, "/Contents/a.mp3")],
        playlists: vec![
            node(1, None, true, vec![]),
            node(2, None, true, vec![]),
            node(3, Some(1), false, vec![1]),
            node(4, Some(2), false, vec![1]),
        ],
        ..Default::default()
    };
    let mut other = lib.clone();
    for p in &mut other.playlists {
        p.id += 100;
        p.parent_id = p.parent_id.map(|id| id + 100);
    }
    let d = diff::between(&lib, &other, ("a", "b"), None);
    assert!(d.is_empty() && d.unchanged_playlists == 2, "{d:?}");
    // The first "Set": a map by path would keep only the last.
    other.playlists[2].track_ids.clear();
    assert_eq!(diff::between(&lib, &other, ("a", "b"), None).changed_playlists.len(), 1);
    assert_eq!(LibraryDiff::between(&lib, &other).changed_playlists, ["Shows/Set"]);
}
