use locallibrary::sets::{EntryOrigin, SetEntry};

use super::super::app::testing::{library, set, track};
use super::*;

fn by_id(lib: &Library) -> FxHashMap<u32, usize> {
    lib.tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect()
}

fn summary(gs: &[Group]) -> Vec<(String, Vec<usize>)> {
    gs.iter().map(|g| (g.label.clone(), g.tracks.clone())).collect()
}

#[test]
fn groups_tracks_by_category() {
    let lib = library();
    let ids = by_id(&lib);
    let (copy_of, copies) = duplicates(&lib);
    let problems = Problems { missing: None, copy_of, copies };
    let g = |cat| summary(&groups(&lib, cat, &ids, &problems, None, &[]));
    // Case and spacing don't split an artist; the first spelling names the group.
    assert_eq!(g(Category::Artist), [("Daft Punk".into(), vec![0, 3]), ("floorplan".into(), vec![1, 2])]);
    assert_eq!(g(Category::Album), [("No album".into(), vec![0, 1, 2, 3])]);
    // Am and 8A are the same key; round the wheel from 1A.
    assert_eq!(g(Category::Key), [("3B  Db".into(), vec![2]), ("8A  Am".into(), vec![0, 1, 3])]);
    assert_eq!(
        g(Category::Bpm),
        [("120–124".into(), vec![0, 2, 3]), ("125–129".into(), vec![1])],
        "124.9 is in the 120 band"
    );
    assert_eq!(
        g(Category::Rating),
        [("★★★★★".into(), vec![0, 3]), ("★★★··".into(), vec![1]), ("Unrated".into(), vec![2])]
    );
    assert_eq!(g(Category::Colour), [("Aqua".into(), vec![0, 3]), ("No colour".into(), vec![1, 2])]);
    assert_eq!(
        g(Category::Added),
        [("2026-09".into(), vec![0, 3]), ("2025-01".into(), vec![1]), ("Unknown".into(), vec![2])]
    );
    assert_eq!(g(Category::History), [("HISTORY 001".into(), vec![2, 1])]);
    assert_eq!(g(Category::Problems), [("Duplicates".into(), vec![0, 3])]);
    let problems = Problems { missing: Some(vec![false, true, false, false]), ..problems };
    assert_eq!(
        summary(&groups(&lib, Category::Problems, &ids, &problems, None, &[]))[0],
        ("Missing files".into(), vec![1])
    );
    assert_eq!(problems.others(0).collect::<Vec<_>>(), [3]);
    assert!(problems.is_missing(1) && !problems.is_missing(0));
}

#[test]
fn sets_hold_their_tracks_on_the_drive_newest_first() {
    let lib = library();
    let ids = by_id(&lib);
    let entry = |i: usize| SetEntry::from_track(&lib.tracks[i], EntryOrigin::Session, None);
    // Played from another drive; on this one, in other case.
    let elsewhere = SetEntry { file: Some("/Contents/elsewhere.mp3".into()), ..entry(0) };
    let shouted = SetEntry { file: Some("/CONTENTS/2.MP3".into()), ..entry(0) };
    let friday = set(
        "3m2friday2222",
        "2026-10-03",
        Some("Friday at the Warehouse"),
        Some("Warehouse"),
        vec![entry(2), elsewhere, entry(0)],
    );
    let mut early = set("3m2early22222", "2026-10-03", None, None, vec![shouted]);
    early.created_at = friday.created_at - chrono::Duration::hours(1);
    let typed = SetEntry::added("Floorplan - Never Grow Old");
    let corsica = set("3lzcorsica222", "2026-09-12", None, Some("Corsica Studios"), vec![typed, entry(1)]);
    let sets = [corsica, early, friday];
    let g = groups(&lib, Category::Sets, &ids, &Problems::default(), None, &sets);
    let shown: Vec<_> = g.iter().map(|g| (g.key.as_str(), g.label.as_str(), g.tracks.clone(), g.left_out)).collect();
    assert_eq!(
        shown,
        [
            ("3m2friday2222", "2026-10-03 Friday at the Warehouse", vec![2, 0], 1),
            ("3m2early22222", "2026-10-03", vec![1], 0),
            ("3lzcorsica222", "2026-09-12 Corsica Studios", vec![1], 1),
        ],
        "newest first, in the order played; what isn't on the drive left out and counted"
    );
    // Beside the sessions, under Browse.
    let all = FxHashMap::from_iter([
        (Category::Sets, g),
        (Category::History, groups(&lib, Category::History, &ids, &Problems::default(), None, &sets)),
    ]);
    let t = tree(&lib, &FxHashSet::from_iter([NodeId::Browse, NodeId::Category(Category::Sets)]), &all);
    let names: Vec<String> = t.iter().map(|n| format!("{}{} {}", "-".repeat(n.depth), n.name, n.count)).collect();
    assert_eq!(
        names[2..7],
        [
            "-Sets 3",
            "--2026-10-03 Friday at the Warehouse 2",
            "--2026-10-03 1",
            "--2026-09-12 Corsica Studios 1",
            "-Sessions 1"
        ]
    );
    // No store, or no sets in it: nothing to show.
    assert!(groups(&lib, Category::Sets, &ids, &Problems::default(), None, &[]).is_empty());
}

#[test]
fn tree_shows_categories_under_browse_and_the_playlists() {
    let lib = library();
    let ids = by_id(&lib);
    let all: FxHashMap<Category, Vec<Group>> =
        Category::ALL.iter().map(|&c| (c, groups(&lib, c, &ids, &Problems::default(), None, &[]))).collect();
    let names = |t: &[Node]| t.iter().map(|n| format!("{}{}", "-".repeat(n.depth), n.name)).collect::<Vec<_>>();
    let closed = tree(&lib, &FxHashSet::default(), &all);
    assert_eq!(names(&closed), ["All tracks", "Browse", "Sets"]);
    let open = FxHashSet::from_iter([NodeId::Browse, NodeId::Category(Category::Key), NodeId::Playlist(1)]);
    let t = tree(&lib, &open, &all);
    assert_eq!(
        names(&t),
        [
            "All tracks",
            "Browse",
            "-Artist",
            "-Album",
            "-Genre",
            "-Key",
            "--3B  Db",
            "--8A  Am",
            "-BPM",
            "-Rating",
            "-Colour",
            "-Date added",
            "-Sessions",
            "Sets",
            "-Friday",
        ],
        "Problems has nothing in it without a check"
    );
    assert_eq!(t[7].id, NodeId::Group(Category::Key, "8A".into()));
    assert_eq!((t[7].count, t[14].count), (3, 3));
}

#[test]
fn sorting_by_rank_keeps_ties_in_list_order_both_ways() {
    let lib = library();
    let r = ranks(&lib, SortField::Key);
    let mut list = vec![3, 1, 2, 0];
    sort_list(&mut list, &r, false);
    assert_eq!(list, [2, 3, 1, 0], "3B before 8A; the three 8A tracks as they were");
    sort_list(&mut list, &r, true);
    assert_eq!(list, [3, 1, 0, 2]);
    let r = ranks(&lib, SortField::Artist);
    assert_eq!(r[1], r[2], "artists compare without case");
    let mut list = vec![1, 0, 2];
    sort_list(&mut list, &ranks(&lib, SortField::Bpm), true);
    assert_eq!(list, [1, 2, 0]);
}

#[test]
fn ranks_agree_with_the_library_sort() {
    let lib = library();
    for field in SORT_FIELDS {
        let mut refs: Vec<&Track> = lib.tracks.iter().collect();
        locallibrary::query::sort_tracks(&mut refs, &[SortKey { field, descending: false }]);
        let expected: Vec<u32> = refs.iter().map(|t| t.id).collect();
        let mut list: Vec<usize> = (0..lib.tracks.len()).collect();
        sort_list(&mut list, &ranks(&lib, field), false);
        let got: Vec<u32> = list.iter().map(|&i| lib.tracks[i].id).collect();
        assert_eq!(got, expected, "{field:?}");
    }
}

#[test]
fn sort_cycles_and_clicks() {
    let k = |field, descending| Some(SortKey { field, descending });
    assert_eq!(cycle(None), k(SortField::Title, false));
    assert_eq!(cycle(k(SortField::Key, true)), k(SortField::Rating, true), "ratings start highest first");
    assert_eq!(cycle(k(SortField::Plays, true)), None);
    assert_eq!(click(None, SortField::Bpm), k(SortField::Bpm, false));
    assert_eq!(click(k(SortField::Bpm, false), SortField::Bpm), k(SortField::Bpm, true));
    assert_eq!(click(k(SortField::Bpm, true), SortField::Bpm), None);
    assert_eq!(click(k(SortField::Title, false), SortField::Added), k(SortField::Added, true));
    assert_eq!(sort_spec(SortKey { field: SortField::Bpm, descending: true }), "-bpm");
    assert!(
        locallibrary::query::parse_sort(&sort_spec(SortKey { field: SortField::Added, descending: false })).is_ok()
    );
}

#[test]
fn searches_split_into_words_and_the_rest() {
    let words = |q: &str| Search::parse(q).words;
    assert_eq!(words("  Daft  PUNK "), [vec!["daft"], vec!["punk"]]);
    assert_eq!(words("\"daft  punk\" house|techno"), [vec!["daft punk"], vec!["house", "techno"]]);
    assert_eq!(words("a| \"\""), [vec!["a"]]);
    let s = Search::parse("re - edit -techno -5 artist:\"daft punk\" bpm:120-128 -genre:house");
    assert_eq!(s.words, [vec!["re"], vec!["-"], vec!["edit"], vec!["-5"]]);
    assert_eq!(s.not, [vec!["techno"]]);
    assert_eq!(s.rest, "artist:\"daft punk\" bpm:120-128 -genre:house", "as typed");
    assert!(s.matches("re-edit, -5 - edit") && !s.matches("re - edit -5 techno"));
    let mut t = tests::track(1, "Daft Punk", "Around the World");
    t.comment = "Peak".into();
    let hay = haystack(&t);
    assert!(hay.contains("peak") && hay.contains("/contents/1.mp3"));
    assert!(!hay.contains("punk around"), "fields stay apart");
}

/// Every column the library sorts by, not only the ones the sort key cycles through.
const EVERY_FIELD: [SortField; 15] = [
    SortField::Title,
    SortField::Artist,
    SortField::Album,
    SortField::Genre,
    SortField::Label,
    SortField::Key,
    SortField::Bpm,
    SortField::Rating,
    SortField::Color,
    SortField::Year,
    SortField::Plays,
    SortField::Added,
    SortField::Time,
    SortField::Bitrate,
    SortField::Path,
];

/// Sorted by any column (a sort remembered from before, say), the list comes out as the
/// library sorts it, and a playlist sorted on the drive by that column reads the same.
#[test]
fn every_column_sorts_as_the_library_does_and_on_the_drive() {
    let mut lib = library();
    for (i, t) in lib.tracks.iter_mut().enumerate() {
        t.label = ["Warp", "ostgut", "R&S", "Kompakt"][i].into();
        t.year = [1997, 2014, 2014, 1990][i];
        t.bitrate = [320, 256, 1411, 320][i];
        t.color = [None, Some(TrackColor::Red), Some(TrackColor::Pink), None][i];
    }
    let mut labels = FxHashSet::default();
    for field in EVERY_FIELD {
        let mut refs: Vec<&Track> = lib.tracks.iter().collect();
        locallibrary::query::sort_tracks(&mut refs, &[SortKey { field, descending: false }]);
        let expected: Vec<u32> = refs.iter().map(|t| t.id).collect();
        let mut list: Vec<usize> = (0..lib.tracks.len()).collect();
        sort_list(&mut list, &ranks(&lib, field), false);
        let got: Vec<u32> = list.iter().map(|&i| lib.tracks[i].id).collect();
        assert_eq!(got, expected, "{field:?}");
        for descending in [false, true] {
            let k = SortKey { field, descending };
            assert_eq!(locallibrary::query::parse_sort(&sort_spec(k)).unwrap(), [k]);
        }
        assert!(labels.insert(sort_label(field)), "{field:?} is named like another column");
    }
}

/// A corrupt database whose folders hold each other: the tree and the playlists' paths
/// still come out, rather than going round forever.
#[test]
fn a_tree_that_loops_still_lists() {
    let folder = |id, parent| Playlist {
        id,
        parent_id: parent,
        name: format!("F{id}"),
        is_folder: true,
        sort_order: 0,
        track_ids: Vec::new(),
    };
    let lib = Library { playlists: vec![folder(1, None), folder(2, Some(1)), folder(1, Some(2))], ..library() };
    let expanded: FxHashSet<NodeId> = [NodeId::Playlist(1), NodeId::Playlist(2)].into_iter().collect();
    let t = tree(&lib, &expanded, &FxHashMap::default());
    assert!(t.len() > 2, "{} rows", t.len());
    let looped = Library { playlists: vec![folder(1, Some(2)), folder(2, Some(1))], ..library() };
    let paths = playlist_paths(&looped);
    assert!(paths[&1].starts_with("F") && paths[&1].ends_with("F1"), "{}", paths[&1]);
}

/// Playlists and folders, each under any folder (or none: some under folders that aren't
/// there, or in a loop, as a damaged database has them), in any order on the drive.
#[hegel::test_helper]
fn any_playlists(tc: &hegel::TestCase) -> Vec<Playlist> {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<u32>().max_value(12));
    (1..=n)
        .map(|id| Playlist {
            id,
            parent_id: tc.draw(gs::booleans()).then(|| tc.draw(gs::integers::<u32>().max_value(n + 1))),
            name: format!("P{id}"),
            is_folder: tc.draw(gs::booleans()),
            sort_order: tc.draw(gs::integers::<u32>().max_value(3)),
            track_ids: vec![1; tc.draw(gs::integers::<usize>().max_value(2))],
        })
        .collect()
}

/// The tree lists the playlists as the library orders them: a folder's children (in their
/// order) straight after it, one deeper, when it's open; and each path is the library's.
#[hegel::test(test_cases = 300)]
fn the_tree_and_its_paths_are_the_librarys(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let lib = Library { playlists: any_playlists(&tc), ..library() };
    let mut open = FxHashSet::default();
    for p in &lib.playlists {
        if tc.draw(gs::booleans()) {
            open.insert(NodeId::Playlist(p.id));
        }
    }
    fn walk(lib: &Library, parent: Option<u32>, depth: usize, open: &FxHashSet<NodeId>, out: &mut Vec<(u32, usize)>) {
        for p in lib.children(parent) {
            out.push((p.id, depth));
            if p.is_folder && open.contains(&NodeId::Playlist(p.id)) && depth < 32 {
                walk(lib, Some(p.id), depth + 1, open, out);
            }
        }
    }
    let mut want = Vec::new();
    walk(&lib, None, 0, &open, &mut want);
    let t = tree(&lib, &open, &FxHashMap::default());
    assert_eq!((&t[0].id, &t[1].id), (&NodeId::All, &NodeId::Browse));
    let got: Vec<(u32, usize)> = (t[2..].iter())
        .map(|n| match n.id {
            NodeId::Playlist(id) => (id, n.depth),
            _ => panic!("{:?} among the playlists", n.id),
        })
        .collect();
    assert_eq!(got, want);
    let paths = playlist_paths(&lib);
    for p in &lib.playlists {
        if lib.playlist_names(p.id).len() <= 32 {
            assert_eq!(paths[&p.id], lib.playlist_path(p.id), "playlist {}", p.id);
        }
    }
}

/// Sorting a list by rank orders it by rank, either way, ties kept in the list's order; the
/// sort key cycles through every column and back to the list's order; and clicking a column
/// sorts by it, then the other way, then not.
#[hegel::test(test_cases = 300)]
fn sorting_orders_by_rank_and_keeps_ties(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let ranks: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().max_value(4)).max_size(12));
    let mut list: Vec<usize> = (0..ranks.len()).collect();
    list.reverse();
    list.rotate_left(tc.draw(gs::integers::<usize>().max_value(ranks.len())).min(ranks.len()));
    let descending = tc.draw(gs::booleans());
    let mut sorted = list.clone();
    sort_list(&mut sorted, &ranks, descending);
    let place = |v: &[usize], i: usize| v.iter().position(|&x| x == i).unwrap();
    for w in sorted.windows(2) {
        let (a, b) = (ranks[w[0]], ranks[w[1]]);
        assert!(if descending { a >= b } else { a <= b }, "{sorted:?} by {ranks:?}");
        if a == b {
            assert!(place(&list, w[0]) < place(&list, w[1]), "a tie out of the list's order");
        }
    }
    let mut seen = Vec::new();
    let mut k = cycle(None);
    while let Some(key) = k {
        assert_eq!(key.descending, first_descending(key.field));
        seen.push(key.field);
        k = cycle(k);
    }
    assert_eq!(seen, SORT_FIELDS);
    let field = SORT_FIELDS[tc.draw(gs::integers::<usize>().max_value(SORT_FIELDS.len() - 1))];
    let first = click(cycle(None).filter(|k| k.field != field), field);
    assert_eq!(first.map(|k| (k.field, k.descending)), Some((field, first_descending(field))));
    let second = click(first, field);
    assert_eq!(second.map(|k| (k.field, k.descending)), Some((field, !first_descending(field))));
    assert_eq!(click(second, field), None);
}

/// What a category groups a track by, as a person reads the column: `None` for a track it
/// can't place (it goes in the category's last group, of none).
fn value_of(c: Category, t: &Track) -> Option<String> {
    let text = |s: &str| Some(normalize(s)).filter(|s| !s.is_empty());
    match c {
        Category::Artist => text(&t.artist),
        Category::Album => text(&t.album),
        Category::Genre => text(&t.genre),
        Category::Key => Camelot::parse(&t.key).map(Camelot::code),
        Category::Bpm => (t.bpm > 0.0).then(|| format!("{}", (t.bpm / 5.0).floor())),
        Category::Rating => (t.rating > 0).then(|| t.rating.min(5).to_string()),
        Category::Colour => t.color.map(|c| colour_name(c).to_string()),
        Category::Added => {
            let m = t.date_added.get(..7)?;
            let ok = m.chars().enumerate().all(|(i, c)| if i == 4 { c == '-' } else { c.is_ascii_digit() });
            ok.then(|| m.to_string())
        }
        _ => unreachable!(),
    }
}

/// Every category of values (artist, album, genre, key, BPM band, rating, colour, month added)
/// puts each track in exactly one group: with the tracks of its value, in library order, those
/// of none last; groups come in the category's order, each named for its value.
#[hegel::test(test_cases = 300)]
fn every_track_is_grouped_under_its_value(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let lib = Library { tracks: super::super::app::testing::any_tracks(&tc, 10), ..Default::default() };
    let n = lib.tracks.len();
    let c = tc.draw(gs::sampled_from(&Category::ALL[..8]));
    let gs = groups(&lib, c, &by_id(&lib), &Problems::default(), None, &[]);
    let mut seen: Vec<usize> = gs.iter().flat_map(|g| g.tracks.clone()).collect();
    seen.sort_unstable();
    assert_eq!(seen, (0..n).collect::<Vec<_>>(), "{c:?}: each track once");
    let none = gs.last().filter(|g| g.tracks.iter().all(|&i| value_of(c, &lib.tracks[i]).is_none()));
    let valued = &gs[..gs.len() - none.map_or(0, |_| 1)];
    for g in &gs {
        assert!(g.tracks.is_sorted() && !g.tracks.is_empty() && g.left_out == 0, "{c:?} {g:?}");
    }
    let mut values = Vec::new();
    for g in valued {
        let v = value_of(c, &lib.tracks[g.tracks[0]]);
        assert!(v.is_some(), "{c:?}: {:?} holds a track of no value", g.label);
        assert!(g.tracks.iter().all(|&i| value_of(c, &lib.tracks[i]) == v), "{c:?}: {g:?} mixes values");
        values.push(v);
        let t = &lib.tracks[g.tracks[0]];
        match c {
            Category::Rating => assert_eq!(g.label, stars(t.rating)),
            Category::Colour => assert!(g.label.eq_ignore_ascii_case(colour_name(t.color.unwrap()))),
            Category::Bpm => {
                let lo: f64 = g.key.parse().unwrap();
                assert!(g.tracks.iter().all(|&i| (lo..lo + 5.0).contains(&lib.tracks[i].bpm)), "{g:?}");
            }
            Category::Key => assert!(g.label.starts_with(&g.key)),
            Category::Added => assert_eq!(g.label, g.key),
            _ => assert_eq!(normalize(&g.label), g.key, "named as first spelled"),
        }
    }
    let unique: FxHashSet<_> = values.iter().collect();
    assert_eq!(unique.len(), values.len(), "{c:?}: a value split over groups");
    let keys: Vec<&str> = valued.iter().map(|g| g.key.as_str()).collect();
    let ordered = keys.windows(2).all(|w| match c {
        Category::Rating | Category::Added => w[0] > w[1],
        Category::Bpm => w[0].parse::<u32>().unwrap() < w[1].parse::<u32>().unwrap(),
        Category::Colour => w[0].parse::<u8>().unwrap() < w[1].parse::<u8>().unwrap(),
        Category::Key => harmony::key_order(w[0]) < harmony::key_order(w[1]),
        _ => w[0] < w[1],
    });
    assert!(ordered, "{c:?} out of order: {keys:?}");
    // With nothing to group, a value-less track still shows, under its category's none.
    if let Some(g) = none {
        assert!(!g.label.is_empty() && g.key.is_empty(), "{g:?}");
    }
}

/// My Tags, the store's sets, the sessions players recorded and the problems found each list
/// what they hold: a tag's tracks are those tagged with it, a set's those of its entries on the
/// drive (the rest counted as left out), newest set first; a session its tracks on the drive in
/// play order; missing files and copies of a recording under Problems.
#[hegel::test(test_cases = 200)]
fn the_other_categories_list_what_they_hold(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::sets::Set;
    let tracks = super::super::app::testing::any_tracks(&tc, 8);
    let n = tracks.len() as u32;
    let id = || gs::integers::<u32>().max_value(n + 2);
    let history: Vec<Playlist> = (0..tc.draw(gs::integers::<u32>().max_value(3)))
        .map(|h| Playlist {
            id: 90 + h,
            parent_id: None,
            name: format!("HISTORY {h:03}"),
            is_folder: tc.draw(gs::booleans()),
            sort_order: 0,
            track_ids: tc.draw(gs::vecs(id()).max_size(5)),
        })
        .collect();
    let lib = Library { tracks, history, ..Default::default() };
    let ids = by_id(&lib);

    const TAGS: [&str; 3] = ["Genre/House", "Mood/Dark", "Genre/Acid"];
    let mut tags: FxHashMap<String, Vec<String>> = FxHashMap::default();
    for t in &lib.tracks {
        let mine: Vec<String> = TAGS.iter().filter(|_| tc.draw(gs::booleans())).map(|s| s.to_string()).collect();
        tags.insert(paths::file_key(&t.file_path), mine);
    }
    let read = tc.draw(gs::booleans());
    let g = groups(&lib, Category::MyTags, &ids, &Problems::default(), read.then_some(&tags), &[]);
    let want: Vec<(String, Vec<usize>)> = (TAGS.iter().copied().collect::<std::collections::BTreeSet<_>>().into_iter())
        .filter(|_| read)
        .map(|tag| {
            let has = |t: &Track| tags[&paths::file_key(&t.file_path)].iter().any(|x| x == tag);
            (tag.to_string(), (0..lib.tracks.len()).filter(|&i| has(&lib.tracks[i])).collect::<Vec<_>>())
        })
        .filter(|(_, v)| !v.is_empty())
        .collect();
    assert_eq!(g.iter().map(|g| (g.key.clone(), g.tracks.clone())).collect::<Vec<_>>(), want);
    assert!(g.iter().all(|g| g.label == g.key.replace('/', " › ")));

    let sets: Vec<Set> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|s| {
            let entries = (0..tc.draw(gs::integers::<usize>().max_value(4)))
                .map(|_| match tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() + 1)) {
                    i if i < lib.tracks.len() => SetEntry::from_track(&lib.tracks[i], EntryOrigin::Session, None),
                    i if i == lib.tracks.len() => SetEntry::added("Someone - Something"),
                    _ => SetEntry { file: Some("/Contents/elsewhere.mp3".into()), ..SetEntry::added("Elsewhere") },
                })
                .collect();
            let date = tc.draw(gs::sampled_from(&["2026-10-03", "2026-09-12"][..]));
            let title = tc.draw(gs::booleans()).then_some("Friday");
            let mut s = set(&format!("set{s}"), date, title, None, entries);
            s.created_at +=
                chrono::Duration::seconds(s.id.len() as i64 * tc.draw(gs::integers::<i64>().min_value(0).max_value(9)));
            s
        })
        .collect();
    let g = groups(&lib, Category::Sets, &ids, &Problems::default(), None, &sets);
    assert_eq!(g.len(), sets.len());
    for w in g.windows(2) {
        let find = |key: &str| sets.iter().find(|s| s.id == key).unwrap();
        assert_ne!(Set::newest_first(find(&w[0].key), find(&w[1].key)), std::cmp::Ordering::Greater, "newest first");
    }
    for grp in &g {
        let s = sets.iter().find(|s| s.id == grp.key).unwrap();
        let on_drive: Vec<usize> = (s.tracks.iter())
            .filter_map(|e| lib.tracks.iter().position(|t| Some(&t.file_path) == e.file.as_ref()))
            .collect();
        assert_eq!((&grp.tracks, grp.left_out), (&on_drive, s.tracks.len() - on_drive.len()), "{grp:?}");
        assert!(grp.label.starts_with(&s.date.to_string()) && grp.label.ends_with(s.name().unwrap_or("")));
    }

    let g = groups(&lib, Category::History, &ids, &Problems::default(), None, &[]);
    let lists: Vec<&Playlist> = lib.history.iter().filter(|h| !h.is_folder).collect();
    assert_eq!(g.len(), lists.len());
    for (grp, h) in g.iter().zip(lists) {
        let played: Vec<usize> = h.track_ids.iter().filter_map(|id| ids.get(id).copied()).collect();
        assert_eq!((&grp.label, &grp.tracks), (&h.name, &played));
    }

    let (copy_of, copies) = duplicates(&lib);
    let missing = tc.draw(gs::booleans()).then(|| lib.tracks.iter().map(|_| tc.draw(gs::booleans())).collect());
    let problems = Problems { missing, copy_of, copies };
    let g = groups(&lib, Category::Problems, &ids, &problems, None, &[]);
    let of = |key: &str| g.iter().find(|g| g.key == key).map(|g| g.tracks.clone()).unwrap_or_default();
    let gone: Vec<usize> = (0..lib.tracks.len()).filter(|&i| problems.is_missing(i)).collect();
    assert_eq!(of("missing"), gone);
    let same = |i: usize, j: usize| {
        let (a, b) = (&lib.tracks[i], &lib.tracks[j]);
        i != j
            && a.fingerprint_key() == b.fingerprint_key()
            && !(a.artist.trim().is_empty() && a.title.trim().is_empty())
    };
    let copied: Vec<usize> = (0..lib.tracks.len()).filter(|&i| (0..lib.tracks.len()).any(|j| same(i, j))).collect();
    assert_eq!(of("duplicates"), copied);
    for &i in &copied {
        assert!(problems.others(i).all(|j| same(i, j)) && problems.others(i).next().is_some());
    }
}

/// With Browse open, the tree lists each category that has values, one deeper, named and
/// counted, and the values of each category open under it, counted by their tracks; closed,
/// none of them. The playlists follow either way.
#[hegel::test(test_cases = 200)]
fn browse_lists_its_categories_and_their_values(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let lib = Library { tracks: super::super::app::testing::any_tracks(&tc, 6), ..library() };
    let ids = by_id(&lib);
    let all: FxHashMap<Category, Vec<Group>> =
        Category::ALL.iter().map(|&c| (c, groups(&lib, c, &ids, &Problems::default(), None, &[]))).collect();
    let mut open = FxHashSet::default();
    if tc.draw(gs::booleans()) {
        open.insert(NodeId::Browse);
    }
    for c in Category::ALL {
        if tc.draw(gs::booleans()) {
            open.insert(NodeId::Category(c));
        }
    }
    let mut want = vec![("All tracks".to_string(), 0, lib.tracks.len()), ("Browse".to_string(), 0, 12)];
    for c in Category::ALL.into_iter().filter(|_| open.contains(&NodeId::Browse)) {
        let Some(gs) = all.get(&c).filter(|g| !g.is_empty()) else { continue };
        want.push((c.name().to_string(), 1, gs.len()));
        if open.contains(&NodeId::Category(c)) {
            want.extend(gs.iter().map(|g| (g.label.clone(), 2, g.tracks.len())));
        }
    }
    let t = tree(&lib, &open, &all);
    let got: Vec<(String, usize, usize)> = t.iter().map(|n| (n.name.clone(), n.depth, n.count)).collect();
    assert_eq!(got[..want.len()], want[..]);
    assert!(t[want.len()..].iter().all(|n| matches!(n.id, NodeId::Playlist(_))), "then the playlists");
    let names: FxHashSet<&str> = Category::ALL.iter().map(|c| c.name()).collect();
    assert_eq!(names.len(), Category::ALL.len(), "each category has a name of its own");
}

/// Sorted by any column, either way, any tracks come out as the library sorts them (so a
/// playlist sorted on the drive by it reads the same), and the sort as the drive's tools spell it
/// reads back as itself.
#[hegel::test(test_cases = 300)]
fn any_tracks_sort_as_the_library_does(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let lib = Library { tracks: super::super::app::testing::any_tracks(&tc, 10), ..Default::default() };
    let field = tc.draw(gs::sampled_from(&EVERY_FIELD[..]));
    let k = SortKey { field, descending: tc.draw(gs::booleans()) };
    let mut refs: Vec<&Track> = lib.tracks.iter().collect();
    locallibrary::query::sort_tracks(&mut refs, &[k]);
    let mut list: Vec<usize> = (0..lib.tracks.len()).collect();
    sort_list(&mut list, &ranks(&lib, field), k.descending);
    let got: Vec<u32> = list.iter().map(|&i| lib.tracks[i].id).collect();
    assert_eq!(got, refs.iter().map(|t| t.id).collect::<Vec<_>>(), "{k:?}");
    assert_eq!(locallibrary::query::parse_sort(&sort_spec(k)).unwrap(), [k]);
    assert!(!sort_label(field).is_empty());
}

/// A word in quotes is the word; words in quotes together are one, spaces and all.
#[hegel::test(test_cases = 200)]
fn quotes_hold_words_together(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let words: Vec<&str> =
        tc.draw(gs::vecs(gs::sampled_from(&["daft", "PUNK", "re-edit", "8a", "x|y"][..])).min_size(1).max_size(3));
    for w in &words {
        assert_eq!(Search::parse(&format!("\"{w}\"")), Search::parse(w), "{w}");
    }
    let phrase = words.join(" ");
    let s = Search::parse(&format!("\"{phrase}\""));
    let alternatives: Vec<String> = normalize(&phrase).split('|').map(String::from).collect();
    assert_eq!((s.words, s.not, s.rest), (vec![alternatives], Vec::<Vec<String>>::new(), String::new()));
}

/// Folders nested deeper than a database could mean (a corrupt one, nesting without end) are
/// listed down to 32 deep and no further.
#[hegel::test(test_cases = 50)]
fn the_tree_stops_32_folders_deep(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(45));
    let playlists: Vec<Playlist> = (1..=n)
        .map(|id| Playlist {
            id,
            parent_id: (id > 1).then(|| id - 1),
            name: format!("F{id}"),
            is_folder: true,
            sort_order: 0,
            track_ids: Vec::new(),
        })
        .collect();
    let lib = Library { playlists, ..library() };
    let open: FxHashSet<NodeId> = (1..=n).map(NodeId::Playlist).collect();
    let t = tree(&lib, &open, &FxHashMap::default());
    let depths: Vec<usize> = t[2..].iter().map(|n| n.depth).collect();
    assert_eq!(depths, (0..(n as usize).min(33)).collect::<Vec<_>>());
}
