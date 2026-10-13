use super::*;
use crate::test_util::track;

#[test]
fn diff_detects_adds_removes_changes() {
    let old = Library {
        tracks: vec![track(1, "A", "One", "/Contents/A/1.mp3"), track(2, "B", "Two", "/Contents/B/2.mp3")],
        playlists: vec![Playlist {
            id: 1,
            parent_id: None,
            name: "Set".into(),
            is_folder: false,
            sort_order: 0,
            track_ids: vec![1, 2],
        }],
        history_playlists: 0,
        history: Vec::new(),
    };
    let mut new = old.clone();
    new.tracks.remove(1);
    new.tracks[0].rating = 5;
    // ids differ between exports; matching must use paths
    new.tracks[0].id = 10;
    new.tracks.push(track(11, "C", "Three", "/Contents/C/3.mp3"));
    new.playlists[0].track_ids = vec![10, 11];

    let d = LibraryDiff::between(&old, &new);
    assert_eq!(d.added_tracks.len(), 1);
    assert_eq!(d.removed_tracks.len(), 1);
    assert_eq!(d.changed_tracks.len(), 1);
    assert_eq!(d.changed_tracks[0].fields[0].field, "rating");
    assert_eq!(d.changed_playlists, vec!["Set".to_string()]);
    assert!(LibraryDiff::between(&old, &old).is_empty());
}

/// A playlist whose folder isn't there is shown at the top level, so it's the second of its
/// name there: its key differs from the first's, and a diff sees it go.
#[test]
fn a_playlist_whose_folder_is_missing_is_keyed_at_the_top_level() {
    let node = |id, parent_id| Playlist {
        id,
        parent_id,
        name: "A".into(),
        is_folder: false,
        sort_order: 0,
        track_ids: vec![],
    };
    let old = Library { playlists: vec![node(1, None), node(2, Some(3))], ..Default::default() };
    let keys: Vec<PlaylistKey> = old.keyed_playlists().map(|(k, ..)| k).collect();
    assert_eq!(keys, [vec![("A", false, 0)], vec![("A", false, 1)]]);
    let new = Library { playlists: vec![node(1, None)], ..Default::default() };
    assert_eq!(LibraryDiff::between(&old, &new).removed_playlists, ["A"]);
}

#[test]
fn search_and_playlist_paths() {
    let lib = Library {
        tracks: vec![track(1, "Daft Punk", "Around the World", "/Contents/x.mp3")],
        playlists: vec![
            Playlist {
                id: 1,
                parent_id: None,
                name: "Shows".into(),
                is_folder: true,
                sort_order: 0,
                track_ids: vec![],
            },
            Playlist {
                id: 2,
                parent_id: Some(1),
                name: "Warmup".into(),
                is_folder: false,
                sort_order: 0,
                track_ids: vec![1],
            },
        ],
        history_playlists: 0,
        history: Vec::new(),
    };
    assert_eq!(lib.search("daft  punk").len(), 1);
    assert_eq!(lib.search("#1").len(), 1);
    assert_eq!(lib.playlist_path(2), "Shows/Warmup");
    assert_eq!(lib.find_playlist("shows/warmup").unwrap().id, 2);
    assert_eq!(lib.find_playlist("Warmup").unwrap().id, 2);
}

#[test]
fn playlists_match_exactly_then_ignoring_case_when_only_one_does() {
    let node = |id, parent_id, name: &str| Playlist {
        id,
        parent_id,
        name: name.into(),
        is_folder: parent_id.is_none(),
        sort_order: 0,
        track_ids: vec![],
    };
    let lib = Library {
        playlists: vec![
            node(1, None, "Shows"),
            node(2, Some(1), "Foo"),
            node(3, Some(1), "FoO"),
            node(4, None, "Sets"),
            node(5, Some(4), "foo"),
        ],
        ..Default::default()
    };
    let found = |q| lib.find_playlist(q).map(|p| p.id);
    assert_eq!(found("Shows/FoO"), Some(3));
    assert_eq!(found("Shows/Foo"), Some(2));
    assert_eq!(found("shows/foo"), None, "ambiguous path");
    assert_eq!(found("sets/FOO"), Some(5));
    assert_eq!(found("FoO"), Some(3));
    assert_eq!(found("foo"), Some(5));
    assert_eq!(found("FOO"), None, "ambiguous name");
}

/// Of playlists sharing a path, the one shown first is found, top down, whatever order they're
/// stored in.
#[test]
fn same_named_siblings_are_found_in_display_order() {
    let node = |id, parent_id, name: &str, sort_order| Playlist {
        id,
        parent_id,
        name: name.into(),
        is_folder: name == "Shows",
        sort_order,
        track_ids: vec![],
    };
    let lib = Library {
        playlists: vec![
            node(1, None, "Set", 1),
            node(2, None, "Set", 0),
            node(3, None, "Shows", 2),
            node(4, None, "Shows", 3),
            // Shown after 6 (its folder is), though first among the two by its own order.
            node(5, Some(4), "Set", 0),
            node(6, Some(3), "Set", 1),
        ],
        ..Default::default()
    };
    assert_eq!(lib.find_playlist("Set").map(|p| p.id), Some(2));
    assert_eq!(lib.find_playlist("Shows/Set").map(|p| p.id), Some(6));
    // Ignoring case too: the twins share one path, so that's not ambiguous.
    assert_eq!(lib.find_playlist("shows/set").map(|p| p.id), Some(6));
}

/// A playlist shown after a folder of its name is found by its path, and the folder by the
/// path with a `/` after it (as folders are listed), or when a folder is what's wanted.
#[test]
fn a_playlist_and_a_folder_sharing_a_path_are_told_apart_by_kind() {
    let node = |id, name: &str, is_folder, sort_order| Playlist {
        id,
        parent_id: None,
        name: name.into(),
        is_folder,
        sort_order,
        track_ids: vec![],
    };
    let lib = Library {
        playlists: vec![node(1, "Set", true, 0), node(2, "Set", false, 1), node(3, "Foo", false, 2)],
        ..Default::default()
    };
    let found = |q| lib.find_playlist(q).map(|p| p.id);
    assert_eq!((found("Set"), found("set"), found("/Set/"), found("set/")), (Some(2), Some(2), Some(1), Some(1)));
    assert_eq!(lib.find_playlist_of_kind("Set/", false).map(|p| p.id), Some(2));
    assert_eq!(lib.find_playlist_of_kind("Set", true).map(|p| p.id), Some(1));
    assert_eq!(lib.find_playlist_of_kind("Foo", true), None);
}

/// A blank name in a query is no name, as edits read paths: `Set/ ` is the folder `Set/`,
/// never a node named ` ` in it, and a query of blanks finds nothing.
#[test]
fn blank_names_in_a_query_are_no_names() {
    let node = |id, parent_id, name: &str, is_folder| Playlist {
        id,
        parent_id,
        name: name.into(),
        is_folder,
        sort_order: id,
        track_ids: vec![],
    };
    let lib = Library {
        playlists: vec![
            node(1, None, "Set", true),
            node(2, None, "Set", false),
            node(3, Some(1), "  ", true),
            node(4, Some(1), "  ", false),
            node(5, None, " ", false),
        ],
        ..Default::default()
    };
    let found = |q| lib.find_playlist(q).map(|p| p.id);
    assert_eq!((found("Set/  "), found("Set/  /"), found(" /Set")), (Some(1), Some(1), Some(2)));
    assert_eq!(lib.find_playlist_of_kind("Set/  ", false).map(|p| p.id), Some(2));
    assert_eq!((found(" "), found("  "), found(" / ")), (None, None, None));
}

#[test]
fn untangled_trees_have_each_id_once_no_loops_and_bounded_depth() {
    let node = |id, parent_id, name: &str| Playlist {
        id,
        parent_id,
        name: name.into(),
        is_folder: true,
        sort_order: 0,
        track_ids: vec![],
    };
    let parents = |rows: &[Playlist]| rows.iter().map(|p| (p.id, p.parent_id)).collect::<Vec<_>>();
    // A folder, then two rows repeating its id that name it as their folder (a damaged
    // export.pdb): only the first stays.
    let rows = untangle(vec![node(1, None, "F"), node(1, Some(1), "P"), node(1, Some(1), "Q")]);
    assert_eq!((parents(&rows), rows[0].name.as_str()), (vec![(1, None)], "F"));
    // A row in its own folder, and two folders in each other: the row closing each loop moves
    // to the top. Rows below a folder that isn't there keep their parent.
    let rows = untangle(vec![
        node(2, Some(2), "self"),
        node(3, Some(4), "a"),
        node(4, Some(3), "b"),
        node(5, Some(3), "in a"),
        node(6, Some(99), "lost"),
    ]);
    assert_eq!(parents(&rows), [(2, None), (3, Some(4)), (4, None), (5, Some(3)), (6, Some(99))]);
    // A chain of 200 nested folders is cut every 64.
    let chain: Vec<Playlist> = (1..=200).map(|id| node(id, (id > 1).then(|| id - 1), "x")).collect();
    let lib = Library { playlists: untangle(chain), ..Default::default() };
    let top: Vec<u32> = lib.children(None).iter().map(|p| p.id).collect();
    assert_eq!(top, [1, 65, 129, 193]);
    assert_eq!(lineage(64, |id| lib.playlist(id)).len(), 64);
    // Built by hand, a library can still hold a loop: a path through it names MAX_DEPTH nodes.
    let looped = Library { playlists: vec![node(1, Some(2), "a"), node(2, Some(1), "b")], ..Default::default() };
    assert_eq!(looped.playlist_names(1).len(), MAX_DEPTH);
}

use hegel::TestCase;
use hegel::generators as gs;

/// A track with every field drawn: short text, any numbers, a tempo in hundredths of a beat
/// per minute (as rekordbox stores it).
#[hegel::test_helper]
fn any_track(tc: &TestCase, id: u32) -> Track {
    let text = || tc.draw(gs::text().max_size(6));
    Track {
        id,
        title: text(),
        artist: text(),
        album: text(),
        album_artist: text(),
        genre: text(),
        label: text(),
        key: text(),
        remixer: text(),
        original_artist: text(),
        composer: text(),
        comment: text(),
        mix_name: text(),
        isrc: text(),
        bpm: f64::from(tc.draw(gs::integers::<u32>().max_value(30_000))) / 100.0,
        duration_secs: tc.draw(gs::integers()),
        year: tc.draw(gs::integers()),
        track_number: tc.draw(gs::integers()),
        disc_number: tc.draw(gs::integers()),
        rating: tc.draw(gs::integers::<u8>().max_value(5)),
        color: TrackColor::from_id(tc.draw(gs::integers::<u8>().max_value(9))),
        play_count: tc.draw(gs::integers()),
        bitrate: tc.draw(gs::integers()),
        sample_rate: tc.draw(gs::integers()),
        sample_depth: tc.draw(gs::integers()),
        // Up to 4 GiB, so a library's total stays far from u64's limit.
        file_size: u64::from(tc.draw(gs::integers::<u32>())),
        file_path: text(),
        analyze_path: text(),
        date_added: text(),
        release_date: text(),
        artwork_id: tc.draw(gs::optional(gs::integers())),
        artwork_path: tc.draw(gs::optional(gs::text().max_size(6))),
    }
}

/// Up to `n` playlist rows with ids and folders from a small range, so ids repeat, folders
/// are missing or hold themselves, and names collide; or a long chain of folders, maybe
/// closed into a loop.
#[hegel::test_helper]
fn rows(tc: &TestCase, n: usize) -> Vec<Playlist> {
    let node = |id, parent_id| Playlist {
        id,
        parent_id,
        name: tc.draw(gs::text().alphabet("aAb ").min_size(1).max_size(2)),
        is_folder: tc.draw(gs::booleans()),
        sort_order: tc.draw(gs::integers::<u32>().max_value(3)),
        track_ids: tc.draw(gs::vecs(gs::integers::<u32>().max_value(4)).max_size(3)),
    };
    if tc.draw(gs::weighted_booleans(0.15)) {
        let len = tc.draw(gs::integers::<u32>().min_value(1).max_value(150));
        let closed = tc.draw(gs::booleans());
        return (1..=len).map(|id| node(id, if id == 1 { closed.then_some(len) } else { Some(id - 1) })).collect();
    }
    let row_count = tc.draw(gs::integers::<usize>().max_value(n));
    (0..row_count)
        .map(|_| {
            let id = tc.draw(gs::integers::<u32>().min_value(1).max_value(8));
            node(id, tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(9))))
        })
        .collect()
}

/// How many rows a walk up from `p` meets (itself included) before the top, a folder that
/// isn't there, or a row it met already; and whether it ended on such a repeat.
fn ancestors(rows: &[Playlist], p: &Playlist) -> (usize, bool) {
    let mut seen = vec![p.id];
    let mut cur = p.parent_id;
    while let Some(parent) = cur.and_then(|id| rows.iter().find(|r| r.id == id)) {
        if seen.contains(&parent.id) {
            return (seen.len(), true);
        }
        seen.push(parent.id);
        cur = parent.parent_id;
    }
    (seen.len(), false)
}

/// Libraries, playlist trees, diffs and tempo ranges read back from JSON as they were.
#[hegel::test(test_cases = 100)]
fn libraries_round_trip_through_json(tc: TestCase) {
    let count = tc.draw(gs::integers::<u32>().max_value(3));
    let tracks: Vec<Track> = (0..count).map(|id| any_track(&tc, id)).collect();
    let lib = Library {
        tracks,
        playlists: rows(&tc, 4),
        history_playlists: tc.draw(gs::integers::<usize>().max_value(10)),
        history: rows(&tc, 2),
    };
    let json = serde_json::to_string(&lib).unwrap();
    assert_eq!(serde_json::from_str::<Library>(&json).unwrap(), lib);
    // JSON from before history was kept reads as having none.
    let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
    old.as_object_mut().unwrap().remove("history");
    assert!(serde_json::from_value::<Library>(old).unwrap().history.is_empty());

    let diff = LibraryDiff::between(&lib, &Library::default());
    let value = serde_json::to_value(&diff).unwrap();
    let back: LibraryDiff = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), value);
    assert_eq!(back.is_empty(), lib.tracks.is_empty() && lib.playlists.is_empty());

    let low = lib.tracks.first().map_or(0.0, |t| t.bpm);
    let high = f64::from(tc.draw(gs::integers::<u16>()));
    let range = BpmRange { low, high };
    assert_eq!(serde_json::to_value(range).unwrap(), serde_json::json!([low, high]));
    assert_eq!(serde_json::from_value::<BpmRange>(serde_json::json!([low, high])).unwrap(), range);
}

/// Track colours are ids 1 to 8 and back, drawn in their tokens' colours in that order, and
/// named in lower case in JSON.
#[hegel::test(test_cases = 100)]
fn track_colours_round_trip_through_their_ids(tc: TestCase) {
    let id = tc.draw(hegel::one_of!(gs::integers::<u8>().max_value(9), gs::integers::<u8>()));
    let tokens = [
        track_colors::PINK,
        track_colors::RED,
        track_colors::ORANGE,
        track_colors::YELLOW,
        track_colors::GREEN,
        track_colors::AQUA,
        track_colors::BLUE,
        track_colors::PURPLE,
    ];
    let every: Vec<Rgb> = (0..=u8::MAX).filter_map(TrackColor::from_id).map(TrackColor::rgb).collect();
    assert_eq!(every, tokens);
    match TrackColor::from_id(id) {
        None => assert!(!(1..=8).contains(&id)),
        Some(c) => {
            assert_eq!(c.id(), id);
            assert_eq!(c.rgb(), tokens[usize::from(id) - 1]);
            let name = serde_json::to_value(c).unwrap();
            assert_eq!(name, format!("{c:?}").to_lowercase());
            assert_eq!(serde_json::from_value::<TrackColor>(name).unwrap(), c);
        }
    }
}

/// The fields a diff compares, in the order it lists them.
const DIFFED_FIELDS: [&str; 12] =
    ["title", "artist", "album", "genre", "key", "label", "comment", "bpm", "rating", "color", "file_size", "artwork"];

/// `t` with `field` changed to something else.
fn change(t: &mut Track, field: &str) {
    match field {
        "title" => t.title.push('x'),
        "artist" => t.artist.push('x'),
        "album" => t.album.push('x'),
        "genre" => t.genre.push('x'),
        "key" => t.key.push('x'),
        "label" => t.label.push('x'),
        "comment" => t.comment.push('x'),
        "bpm" => t.bpm += 1.0,
        "rating" => t.rating = (t.rating + 1) % 6,
        "color" => t.color = TrackColor::from_id(t.color.map_or(1, |c| c.id() % 8 + 1)),
        "file_size" => t.file_size += 1,
        "artwork" => t.artwork_id = if t.artwork_id.is_some() { None } else { Some(1) },
        _ => unreachable!("{field}"),
    }
}

/// Between two exports of one library, tracks are matched by their path however it's written
/// (ids differ), and the diff lists exactly the tracks added, removed and changed (with each
/// field that changed, and only those), and the playlists added, removed and reordered or
/// refilled. Diffing the other way round swaps what's added and removed, and an export
/// against itself differs in nothing.
#[hegel::test(test_cases = 200)]
fn diffs_list_exactly_what_changed(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().max_value(6));
    let old_tracks: Vec<Track> =
        (1..=n).map(|i| track(i, "A", &format!("T{i}"), &format!("/Contents/{i}.mp3"))).collect();
    let playlist = |id, name: String, track_ids| Playlist {
        id,
        parent_id: None,
        name,
        is_folder: false,
        sort_order: id,
        track_ids,
    };
    let entries = || tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(n.max(1))).max_size(4));
    let lists = tc.draw(gs::integers::<u32>().max_value(3));
    let old_lists: Vec<Playlist> = (1..=lists).map(|j| playlist(j, format!("P{j}"), entries())).collect();
    let old = Library { tracks: old_tracks.clone(), playlists: old_lists.clone(), ..Default::default() };

    let mut new_tracks = Vec::new();
    let (mut removed, mut changed) = (Vec::new(), Vec::new());
    for t in &old_tracks {
        if tc.draw(gs::weighted_booleans(0.25)) {
            removed.push(t.file_path.clone());
            continue;
        }
        let mut t = t.clone();
        t.id += 100;
        // Fields the diff doesn't compare.
        t.year += 1;
        t.play_count += 1;
        t.file_path = tc.draw(gs::sampled_from(vec![
            t.file_path.clone(),
            t.file_path.replace('/', "\\"),
            t.file_path.trim_start_matches('/').to_string(),
        ]));
        let fields: Vec<&str> = DIFFED_FIELDS.into_iter().filter(|_| tc.draw(gs::weighted_booleans(0.2))).collect();
        fields.iter().for_each(|f| change(&mut t, f));
        if !fields.is_empty() {
            changed.push((t.id, fields));
        }
        new_tracks.push(t);
    }
    let new_count = tc.draw(gs::integers::<u32>().max_value(2));
    let added: Vec<String> = (0..new_count).map(|k| format!("/Contents/new{k}.mp3")).collect();
    new_tracks.extend(added.iter().enumerate().map(|(k, p)| track(200 + k as u32, "B", "New", p)));

    // Old ids to new: entries of removed tracks drop out.
    let renumbered = |ids: &[u32]| -> Vec<u32> {
        ids.iter().filter(|id| !removed.contains(&format!("/Contents/{id}.mp3"))).map(|id| id + 100).collect()
    };
    let (mut new_lists, mut removed_lists, mut changed_lists) = (Vec::new(), Vec::new(), Vec::new());
    for p in &old_lists {
        if tc.draw(gs::weighted_booleans(0.2)) {
            removed_lists.push(p.name.clone());
            continue;
        }
        let mut ids = renumbered(&p.track_ids);
        if tc.draw(gs::booleans()) {
            ids.reverse();
        }
        if ids.iter().map(|id| id - 100).collect::<Vec<_>>() != p.track_ids {
            changed_lists.push(p.name.clone());
        }
        new_lists.push(Playlist { id: p.id + 10, track_ids: ids, ..p.clone() });
    }
    let new_list_count = tc.draw(gs::integers::<u32>().max_value(2));
    let added_lists: Vec<String> = (0..new_list_count).map(|k| format!("N{k}")).collect();
    new_lists.extend(added_lists.iter().enumerate().map(|(k, name)| playlist(20 + k as u32, name.clone(), vec![])));
    let new = Library { tracks: new_tracks, playlists: new_lists, ..Default::default() };

    let d = LibraryDiff::between(&old, &new);
    let paths = |refs: &[TrackRef]| refs.iter().map(|r| crate::paths::key(&r.file_path)).collect::<Vec<_>>();
    let keys = |v: &[String]| v.iter().map(|p| crate::paths::key(p)).collect::<Vec<_>>();
    assert_eq!(paths(&d.added_tracks), keys(&added));
    assert_eq!(paths(&d.removed_tracks), keys(&removed));
    let got: Vec<(u32, Vec<&str>)> =
        d.changed_tracks.iter().map(|c| (c.track.id, c.fields.iter().map(|f| f.field.as_str()).collect())).collect();
    assert_eq!(got, changed);
    assert!(d.changed_tracks.iter().flat_map(|c| &c.fields).all(|f| f.old != f.new));
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    assert_eq!(sorted(d.added_playlists.clone()), sorted(added_lists));
    assert_eq!(sorted(d.removed_playlists.clone()), sorted(removed_lists));
    assert_eq!(sorted(d.changed_playlists.clone()), sorted(changed_lists));
    let nothing = d.added_tracks.is_empty() && d.removed_tracks.is_empty() && got.is_empty();
    let no_lists = d.added_playlists.is_empty() && d.removed_playlists.is_empty() && d.changed_playlists.is_empty();
    assert_eq!(d.is_empty(), nothing && no_lists);

    let back = LibraryDiff::between(&new, &old);
    assert_eq!(paths(&back.added_tracks), paths(&d.removed_tracks));
    assert_eq!(paths(&back.removed_tracks), paths(&d.added_tracks));
    assert_eq!(back.changed_tracks.len(), d.changed_tracks.len());
    assert_eq!(sorted(back.added_playlists), sorted(d.removed_playlists));
    assert!(LibraryDiff::between(&old, &old).is_empty() && LibraryDiff::between(&new, &new).is_empty());
}

/// Untangling keeps the first row of each id, as it was but for a folder it may drop; walking
/// up from any row then ends within MAX_DEPTH rows without meeting one twice, walking down
/// from the top meets each row once at most, and a tree that was already sound (or already
/// untangled) is left alone.
#[hegel::test(test_cases = 300)]
fn untangled_trees_are_walkable(tc: TestCase) {
    let rows = rows(&tc, 12);
    let mut first: Vec<Playlist> = Vec::new();
    for r in &rows {
        if !first.iter().any(|f| f.id == r.id) {
            first.push(r.clone());
        }
    }
    let out = untangle(rows);
    assert_eq!(out.len(), first.len());
    for (o, f) in out.iter().zip(&first) {
        assert_eq!(*o, Playlist { parent_id: o.parent_id, ..f.clone() });
        assert!(o.parent_id == f.parent_id || o.parent_id.is_none(), "{o:?} from {f:?}");
    }
    for p in &out {
        let (len, looped) = ancestors(&out, p);
        assert!(!looped && len <= MAX_DEPTH, "{} sits {len} deep (loop: {looped})", p.id);
    }
    let lib = Library { playlists: out.clone(), ..Default::default() };
    let mut met = Vec::new();
    let mut level = vec![None];
    for _ in 0..=MAX_DEPTH {
        level = level.into_iter().flat_map(|parent| lib.children(parent)).map(|p| Some(p.id)).collect();
        met.extend(level.iter().flatten().copied());
    }
    assert!(level.is_empty(), "rows below MAX_DEPTH");
    let mut unique = met.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), met.len(), "a row met twice");

    let sound = first.iter().all(|p| matches!(ancestors(&first, p), (len, false) if len <= MAX_DEPTH));
    if sound {
        assert_eq!(out, first);
    }
    assert_eq!(untangle(out.clone()), out);
}

/// In an untangled tree, display order lists each playlist once, after its folder, with the
/// path `playlist_path` gives and the names `playlist_names` gives; keys tell every node
/// apart. Each node is found by its path among its kind, the first shown with that path
/// winning; without a kind, playlists win, unless the path ends in `/`. A query's blank names
/// are left out of it, so a path with one finds what the path without it does. Whatever any
/// query finds matches it by path or name, at most ignoring ASCII case.
#[hegel::test(test_cases = 300)]
fn playlists_are_found_by_the_paths_they_are_shown_with(tc: TestCase) {
    let lib = Library { playlists: untangle(rows(&tc, 10)), ..Default::default() };
    let shown = lib.display_order();
    assert_eq!(shown.len(), lib.playlists.len());
    let at = |id: u32| shown.iter().position(|(_, p)| p.id == id).unwrap();
    for (i, (path, p)) in shown.iter().enumerate() {
        assert_eq!(*path, lib.playlist_path(p.id));
        assert_eq!(lib.playlist_names(p.id).join("/"), *path);
        if let Some(parent) = p.parent_id.filter(|&id| lib.playlist(id).is_some()) {
            assert!(at(parent) < i, "{} shown before its folder", p.id);
        }
    }
    // Walking the folders down from the top, as a tree view does, meets every node in display
    // order: one whose folder isn't there among the top-level nodes.
    let mut walked = Vec::new();
    let mut stack: Vec<&Playlist> = lib.children(None).into_iter().rev().collect();
    while let Some(p) = stack.pop() {
        walked.push(p.id);
        stack.extend(lib.children(Some(p.id)).into_iter().rev());
    }
    assert_eq!(walked, shown.iter().map(|(_, p)| p.id).collect::<Vec<_>>());
    let keyed: Vec<_> = lib.keyed_playlists().collect();
    let mut keys: Vec<_> = keyed.iter().map(|(k, ..)| k.clone()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), lib.playlists.len(), "two nodes share a key");
    for (_, path, p) in &keyed {
        let slash = if p.is_folder { "/" } else { "" };
        assert_eq!(*path, format!("{}{slash}", lib.playlist_path(p.id)));
    }

    let first =
        |path: &str, folder: bool| shown.iter().find(|(s, p)| s == path && p.is_folder == folder).map(|(_, p)| p.id);
    let without_blanks = |q: &str| q.split('/').filter(|n| !n.trim().is_empty()).collect::<Vec<_>>().join("/");
    let found = |q: &str| lib.find_playlist(q).map(|f| f.id);
    // A few nodes, not all: each search walks the whole tree, which can be a long chain.
    let picks = tc.draw(gs::vecs(gs::integers::<usize>().max_value(shown.len().max(1) - 1)).max_size(4));
    for (path, p) in picks.iter().filter_map(|&i| shown.get(i)) {
        // Read as it's written without them: `A/ ` as `A/`, the folder.
        let slash = if path.trim_end().ends_with('/') { "/" } else { "" };
        let read = without_blanks(path) + slash;
        if read != *path {
            assert_eq!(found(path), found(&read), "{path:?}");
            assert_eq!(found(&format!("{path}/")), found(&format!("{read}/")), "{path:?}");
            continue;
        }
        assert_eq!(lib.find_playlist_of_kind(path, p.is_folder).map(|f| f.id), first(path, p.is_folder));
        assert_eq!(lib.find_playlist(path).map(|f| f.id), first(path, false).or(first(path, true)));
        let as_folder = lib.find_playlist(&format!("{path}/")).map(|f| f.id);
        assert_eq!(as_folder, first(path, true).or(first(path, false)));
        // A name no path matches finds the one node that has it.
        let path_like = shown.iter().any(|(s, _)| s.eq_ignore_ascii_case(&p.name));
        let named = lib.playlists.iter().filter(|o| o.name == p.name).count();
        if !path_like && named == 1 {
            assert_eq!(lib.find_playlist(&p.name).map(|f| f.id), Some(p.id));
        }
    }
    let query = tc.draw(gs::text().alphabet("aAb /").max_size(6));
    let q = without_blanks(&query);
    if let Some(p) = lib.find_playlist(&query) {
        let path = lib.playlist_path(p.id);
        assert!(!q.is_empty(), "{query:?} has no names but found {p:?}");
        assert!(path.eq_ignore_ascii_case(&q) || p.name.eq_ignore_ascii_case(&q), "{query:?}: {p:?}");
    }
}

/// `pick` takes the first item whose path is the query exactly; failing that, the first that
/// matches ignoring ASCII case, but only when every such match has the same path.
#[hegel::test(test_cases = 300)]
fn pick_prefers_exact_paths_then_one_case_insensitive_path(tc: TestCase) {
    let paths = tc.draw(gs::vecs(gs::text().alphabet("aAb/").max_size(3)).max_size(6));
    let query = tc.draw(gs::text().alphabet("aAb/").max_size(3));
    let picked = pick(paths.iter().enumerate().map(|(i, p)| (p.as_str(), i)), &query);
    let exact = paths.iter().position(|p| *p == query);
    let loose: Vec<usize> = (0..paths.len()).filter(|&i| paths[i].eq_ignore_ascii_case(&query)).collect();
    let one_path = loose.iter().all(|&i| paths[i] == paths[loose[0]]);
    let expected = exact.or_else(|| loose.first().copied().filter(|_| one_path));
    assert_eq!(picked, expected, "{query:?} in {paths:?}");
}

/// Searching by `#id` or `id` finds the first track with that id; otherwise every track found
/// has the query (normalised) in its "artist - title" or path, and any piece of a track's
/// normalised name finds it.
#[hegel::test(test_cases = 300)]
fn search_finds_by_id_or_by_any_piece_of_the_name(tc: TestCase) {
    let count = tc.draw(gs::integers::<usize>().max_value(5));
    let tracks: Vec<Track> = (0..count)
        .map(|i| {
            let id = tc.draw(gs::integers::<u32>().max_value(6));
            let word = || tc.draw(gs::text().alphabet("aB 1#\t").max_size(5));
            track(id, &word(), &word(), &format!("/Contents/{i}.mp3"))
        })
        .collect();
    let lib = Library { tracks, ..Default::default() };
    let by_id = |q: &str| q.trim().trim_start_matches('#').parse::<u32>().ok().and_then(|id| lib.track(id));
    let query = tc.draw(gs::text().alphabet("aBb 1#\t").max_size(4));
    let found = lib.search(&query);
    match by_id(&query) {
        Some(t) => assert_eq!(found, [t]),
        None => {
            let needle = normalize(query.trim());
            for t in &found {
                let in_name = normalize(&t.display_name()).contains(&needle);
                assert!(in_name || normalize(&t.file_path).contains(&needle), "{query:?} found {t:?}");
            }
        }
    }
    if let Some(t) = lib.tracks.first() {
        let name: Vec<char> = normalize(&t.display_name()).chars().collect();
        let from = tc.draw(gs::integers::<usize>().max_value(name.len()));
        let to = tc.draw(gs::integers::<usize>().min_value(from).max_value(name.len()));
        let piece: String = name[from..to].iter().collect();
        let found = lib.search(&piece);
        match by_id(&piece) {
            Some(other) => assert_eq!(found, [other]),
            None => assert!(found.iter().any(|f| std::ptr::eq(*f, t)), "{piece:?} misses {t:?}"),
        }
    }
}

/// Normalising is idempotent, ignores ASCII case and how words are spaced, and leaves no
/// blanks at the ends or doubled; so tracks whose artist and title differ only so are taken
/// for one recording, while a different length tells them apart.
#[hegel::test(test_cases = 300)]
fn normalised_names_ignore_case_and_spacing(tc: TestCase) {
    let s = tc
        .draw(hegel::one_of!(gs::text().max_size(10), gs::text().alphabet("aBc \t\u{a0}\u{3000}\u{1c4}").max_size(10)));
    let n = normalize(&s);
    assert_eq!(normalize(&n), n, "{s:?}");
    assert_eq!(normalize(&s.to_ascii_uppercase()), n, "{s:?}");
    assert!(!n.starts_with(' ') && !n.ends_with(' ') && !n.contains("  "), "{n:?}");
    let spaced: String = s.split_whitespace().collect::<Vec<_>>().join(" \t ");
    assert_eq!(normalize(&format!("  {spaced} ")), n, "{s:?}");

    let a = track(1, &s, &s, "/Contents/a.mp3");
    let mut b = track(2, &format!(" {}", s.to_ascii_uppercase()), &spaced, "/Contents/b.mp3");
    assert_eq!(a.fingerprint_key(), b.fingerprint_key());
    b.duration_secs += 1;
    assert_ne!(a.fingerprint_key(), b.fingerprint_key());
}

/// Looking tracks up by a list of ids is `track` for each, repeats kept and unknown ids left
/// out; the summary counts the tracks, playlists, folders, artists, bytes, seconds and missing
/// artwork the library holds; names read "artist - title".
#[hegel::test(test_cases = 200)]
fn lookups_and_summaries_agree_with_the_library(tc: TestCase) {
    let count = tc.draw(gs::integers::<usize>().max_value(6));
    let tracks: Vec<Track> = (0..count)
        .map(|_| {
            let id = tc.draw(gs::integers::<u32>().max_value(5));
            let mut t = any_track(&tc, id);
            t.artist = tc.draw(gs::sampled_from(vec!["A", "B", "a", ""])).into();
            t
        })
        .collect();
    let lib = Library { tracks, playlists: rows(&tc, 5), ..Default::default() };
    let ids = tc.draw(gs::vecs(gs::integers::<u32>().max_value(7)).max_size(8));
    let looked_up: Vec<*const Track> = lib.tracks_by_id(&ids).into_iter().map(|t| t as *const Track).collect();
    let one_by_one: Vec<*const Track> = ids.iter().filter_map(|&id| lib.track(id)).map(|t| t as *const Track).collect();
    assert_eq!(looked_up, one_by_one);

    let s = lib.summary();
    assert_eq!(s.tracks, lib.tracks.len());
    assert_eq!(s.playlists + s.folders, lib.playlists.len());
    assert_eq!(s.folders, lib.playlists.iter().filter(|p| p.is_folder).count());
    let mut artists: Vec<&str> = lib.tracks.iter().map(|t| t.artist.as_str()).collect();
    artists.sort_unstable();
    artists.dedup();
    assert_eq!(s.artists, artists.len());
    assert_eq!(s.audio_bytes, lib.tracks.iter().map(|t| t.file_size).sum::<u64>());
    assert_eq!(s.total_duration_secs, lib.tracks.iter().map(|t| u64::from(t.duration_secs)).sum::<u64>());
    assert_eq!(s.tracks_without_artwork, lib.tracks.iter().filter(|t| t.artwork_id.is_none()).count());
    for t in &lib.tracks {
        let name = t.display_name();
        let expected = if t.artist.is_empty() { t.title.clone() } else { format!("{} - {}", t.artist, t.title) };
        assert_eq!(name, expected);
        let r = TrackRef::from(t);
        assert_eq!((r.id, r.name, r.file_path), (t.id, name, t.file_path.clone()));
    }
}

crate::labelled_tuple! {
    /// A pair whose first element is documented.
    #[derive(Debug, Clone, PartialEq)]
    struct Described {
        /// The first value,
        ///   over two lines.
        first: u32,
        second: String,
    }
}

/// A labelled tuple's JSON is the array of its fields in order, and reads back as it was; an
/// array of another length, or an object of its fields, doesn't read. Its
/// schema (with the `schemars` feature) is an array of exactly that many elements, each titled
/// by its field and described by the field's doc comment, its lines trimmed and joined, if it
/// has one.
#[hegel::test(test_cases = 100)]
fn labelled_tuples_are_arrays_named_in_their_schema(tc: TestCase) {
    let value = Described { first: tc.draw(gs::integers()), second: tc.draw(gs::text()) };
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(json, serde_json::json!([value.first, value.second]));
    assert_eq!(serde_json::from_value::<Described>(json.clone()).unwrap(), value);
    let mut longer = json.as_array().unwrap().clone();
    longer.push(json[0].clone());
    for wrong in [
        serde_json::json!([value.first]),
        longer.into(),
        serde_json::json!({"first": value.first, "second": value.second}),
    ] {
        assert!(serde_json::from_value::<Described>(wrong.clone()).is_err(), "{wrong} read");
    }
    #[cfg(feature = "schemars")]
    {
        let schema = serde_json::to_value(schemars::schema_for!(Described)).unwrap();
        assert_eq!(
            (schema["type"].as_str(), &schema["minItems"], &schema["maxItems"]),
            (Some("array"), &2.into(), &2.into())
        );
        let items = schema["prefixItems"].as_array().unwrap();
        let titled: Vec<(&str, Option<&str>)> = items
            .iter()
            .map(|i| (i["title"].as_str().unwrap(), i.get("description").and_then(|d| d.as_str())))
            .collect();
        assert_eq!(titled, [("first", Some("The first value, over two lines.")), ("second", None)]);
    }
}

/// The benchmarks' made-up library hangs together: every entry is one of its tracks, each
/// folder holds the playlists after it, and it diffs empty against itself.
#[test]
fn made_up_libraries_hang_together() {
    let lib = crate::test_util::library(500, 30);
    let track = lib.track_lookup();
    assert!(lib.playlists.iter().flat_map(|p| &p.track_ids).all(|&id| track(id).is_some()));
    assert_eq!(track(501), None);
    assert_eq!((lib.playlist_path(2), lib.playlist_path(11)), ("Shows 0/light 1".into(), "Shows 10".into()));
    assert!(LibraryDiff::between(&lib, &lib).is_empty());
}

/// Text that's NFKC already skips normalising and lands where normalising would: every kind of
/// whitespace (vertical tab too) collapses, and nothing else changes but case.
#[hegel::test(test_cases = 200)]
fn normalised_text_skips_normalising(tc: hegel::TestCase) {
    let alphabet = "aZ9 \t\n\u{b}\u{c}\r.&'\u{e9}\u{301}\u{3a3}\u{fb01}\u{3000}";
    let s = tc.draw(hegel::generators::text().alphabet(alphabet).max_size(12));
    let unicode = s.nfkc().collect::<String>().to_lowercase();
    assert_eq!(normalize(&s), unicode.split_whitespace().collect::<Vec<_>>().join(" "), "{s:?}");
}
