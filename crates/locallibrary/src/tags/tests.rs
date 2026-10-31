use super::*;

#[test]
fn paths_split_at_the_first_slash() {
    assert_eq!(split_path("Genre/Deep House").unwrap(), ("Genre", Some("Deep House")));
    assert_eq!(split_path(" Genre / Drum/Bass ").unwrap(), ("Genre", Some("Drum/Bass")));
    assert_eq!(split_path("Situation").unwrap(), ("Situation", None));
    assert!(split_path("/Deep").is_err());
    assert!(split_path("Genre/").is_err());
}

#[test]
fn names_match_exactly_then_ignoring_case() {
    let names = ["Deep", "deep", "House"];
    assert_eq!(find_named(&names, |n| n, "deep"), Some(&"deep"));
    assert_eq!(find_named(&names, |n| n, "DEEP"), None, "ambiguous");
    assert_eq!(find_named(&names, |n| n, "house"), Some(&"House"));
}

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

use crate::model::Track;
use hegel::TestCase;
use hegel::generators as gs;

/// Six tracks, ids 1 to 6, as both databases list them.
fn six_tracks() -> Library {
    let tracks = (1..=6)
        .map(|id| Track {
            id,
            title: format!("Track {id}"),
            file_path: format!("/Contents/Artist/Album/Track {id}.mp3"),
            ..Default::default()
        })
        .collect();
    Library { tracks, ..Default::default() }
}

/// A OneLibrary database listing `library`'s tracks with their ids. Making one derives a key,
/// which takes a while: it's made once, and opened afresh (its key remembered) each time.
fn onelibrary_of(library: &Library) -> OneLibrary {
    static BYTES: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let bytes = BYTES.get_or_init(|| {
        let mut one = OneLibrary::create().unwrap();
        let defaults = one.content_defaults().unwrap();
        for t in &library.tracks {
            let rec = crate::onelibrary::ContentRecord::from_track(t, false, &defaults);
            assert_eq!(one.add_track(&rec, &t.file_path, "", None).unwrap(), t.id);
        }
        one.to_encrypted().unwrap()
    });
    OneLibrary::from_encrypted(bytes, None).unwrap()
}

/// What a database must refuse `op` for, as the model has it: an id it has no row for, or, to
/// add, one it uses already; exportExt.pdb also refuses an id outside its 32 bits.
fn refused(tree: &Tree, op: &Op, source: Source) -> bool {
    let category = |id: &i64| tree.categories.iter().any(|c| c.id == *id);
    let tag = |id: &i64| tree.categories.iter().flat_map(|c| &c.tags).any(|t| t.id == *id);
    let storable = |id: &i64| source == Source::OneLibrary || Ids::fits(source, *id);
    let new = |id: &i64| !tree.ids.contains(id) && storable(id);
    match op {
        Op::AddCategory { id, .. } => !new(id),
        Op::AddTag { category: c, id, .. } => !new(id) || !category(c),
        Op::Rename { id, .. } | Op::Remove { id } => !category(id) && !tag(id),
        Op::Link { tag: t, .. } => !tag(t),
        Op::Unlink { tag: t, .. } => source == Source::ExportExt && u32::try_from(*t).is_err(),
    }
}

/// An op on the ids `trees` use, or on ids neither does (a new one, one past exportExt.pdb's 32
/// bits), with the tracks of `keys`.
#[hegel::test_helper]
fn draw_op(tc: &TestCase, trees: [&Tree; 2], keys: &[String]) -> Op {
    let used: Vec<i64> = trees.iter().flat_map(|t| t.ids.iter().copied()).collect();
    let tags: Vec<i64> = trees.iter().flat_map(|t| t.categories.iter().flat_map(|c| &c.tags)).map(|t| t.id).collect();
    let categories: Vec<i64> = trees.iter().flat_map(|t| &t.categories).map(|c| c.id).collect();
    let id = |tc: &TestCase, known: &[i64]| -> i64 {
        match tc.draw(gs::integers::<u8>().max_value(5)) {
            0..=2 if !known.is_empty() => tc.draw(gs::sampled_from(known.to_vec())),
            // Not 0: OneLibrary takes it, but its categories hang off parent 0, so removing a
            // category 0 would remove them all. Planning never makes one (see `Ids::fits`).
            3 => tc.draw(gs::sampled_from(vec![i64::from(u32::MAX) + 1, i64::MAX])),
            _ => tc.draw(gs::integers::<i64>().min_value(1).max_value(400)),
        }
    };
    let name = |tc: &TestCase| tc.draw(gs::sampled_from(vec!["Deep", "deep", "Peak Time", "Ünï", "x"])).to_string();
    let tracks = |tc: &TestCase| -> Vec<(u32, String)> {
        let picked: Vec<usize> =
            tc.draw(gs::vecs(gs::integers::<usize>().max_value(keys.len() - 1)).min_size(1).max_size(3));
        let picked: BTreeSet<usize> = picked.into_iter().collect();
        picked.into_iter().map(|i| (i as u32 + 1, keys[i].clone())).collect()
    };
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => Op::AddCategory { id: id(tc, &used), name: name(tc) },
        1 => Op::AddTag { category: id(tc, &categories), id: id(tc, &used), name: name(tc) },
        2 => Op::Rename { id: id(tc, &used), name: name(tc) },
        3 => Op::Remove { id: id(tc, &used) },
        4 => Op::Link { tag: id(tc, &tags), tracks: tracks(tc) },
        _ => Op::Unlink { tag: id(tc, &tags), tracks: tracks(tc) },
    }
}

/// [`Tree::apply`] is what verifying an edit relies on: whatever ops exportExt.pdb's editor and
/// OneLibrary take, each database reads back as the tree they're applied to, and an op the model
/// says names a row that isn't there (or an id taken or too large) is refused, the tree left as
/// it was. Removing every category then leaves only the rows that belonged nowhere.
#[hegel::test(test_cases = 30)]
fn the_tree_model_is_what_both_databases_read_back(tc: TestCase) {
    let library = six_tracks();
    let keys: Vec<String> = library.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect();
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vendor/rekordbox-pdb/tests/data/one-song-exportExt.pdb");
    let path = Path::new(paths::EXPORT_EXT_PDB);
    let mut ext = std::fs::read(fixture).unwrap();
    let mut ext_model = ext_tree(&parse_ext(ext.clone(), path).unwrap(), &library);
    let mut one = onelibrary_of(&library);
    let mut plus_model = plus_tree(&one, &library).unwrap();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(12)) {
        let op = draw_op(&tc, [&ext_model, &plus_model], &keys);
        tc.note(&format!("{op:?}"));

        let edited = edit_ext(ext.clone(), std::slice::from_ref(&op));
        let mut unchanged = ext_model.clone();
        unchanged.apply(&op);
        match (edited, refused(&ext_model, &op, Source::ExportExt)) {
            (Ok(bytes), false) => {
                ext = bytes;
                ext_model.apply(&op);
            }
            (Err(e), true) => assert!(
                matches!(op, Op::AddCategory { .. } | Op::AddTag { .. }) || unchanged == ext_model,
                "exportExt.pdb refused {op:?} ({e}), but the model changed"
            ),
            (r, _) => panic!("exportExt.pdb: {op:?} gave {:?}", r.map(|_| ())),
        }
        assert_eq!(ext_tree(&parse_ext(ext.clone(), path).unwrap(), &library), ext_model, "exportExt.pdb after {op:?}");

        let mut unchanged = plus_model.clone();
        unchanged.apply(&op);
        match (edit_plus(&mut one, std::slice::from_ref(&op)), refused(&plus_model, &op, Source::OneLibrary)) {
            (Ok(()), false) => plus_model.apply(&op),
            (Err(e), true) => assert!(
                matches!(op, Op::AddCategory { .. } | Op::AddTag { .. }) || unchanged == plus_model,
                "exportLibrary.db refused {op:?} ({e}), but the model changed"
            ),
            (r, _) => panic!("exportLibrary.db: {op:?} gave {r:?}"),
        }
        assert_eq!(plus_tree(&one, &library).unwrap(), plus_model, "exportLibrary.db after {op:?}");
    }
    // Every category removed, with its tags and their links: only rows that belong nowhere stay.
    for id in ext_model.categories.iter().map(|c| c.id).collect::<Vec<_>>() {
        ext = edit_ext(ext, &[Op::Remove { id }]).unwrap();
        ext_model.apply(&Op::Remove { id });
    }
    let read = ext_tree(&parse_ext(ext, path).unwrap(), &library);
    assert_eq!(read, ext_model, "exportExt.pdb with every category removed");
    assert!(read.categories.is_empty() && read.ids.len() == read.strays, "{read:?}");
    for id in plus_model.categories.iter().map(|c| c.id).collect::<Vec<_>>() {
        edit_plus(&mut one, &[Op::Remove { id }]).unwrap();
        plus_model.apply(&Op::Remove { id });
    }
    let read = plus_tree(&one, &library).unwrap();
    assert_eq!(read, plus_model, "exportLibrary.db with every category removed");
    assert!(read.categories.is_empty() && read.ids.len() == read.strays, "{read:?}");
}

/// A database's tree drawn for picking ids: some of `names` as categories (in drawn case), each
/// with tags of `names` too, every row with a distinct id from `ids` (and maybe a stray row).
#[hegel::test_helper]
fn draw_tree(tc: &TestCase, names: &[&str], ids: &dyn Fn(&TestCase) -> i64) -> Tree {
    let mut tree = Tree::default();
    let fresh = |tc: &TestCase, tree: &Tree| loop {
        let id = ids(tc);
        if !tree.ids.contains(&id) {
            return id;
        }
    };
    let cased = |tc: &TestCase, n: &str| if tc.draw(gs::booleans()) { n.to_string() } else { n.to_uppercase() };
    for c in tc.draw(gs::subsequences(names)) {
        let id = fresh(tc, &tree);
        tree.ids.insert(id);
        let mut node = CategoryNode { id, name: cased(tc, c), tags: Vec::new() };
        for t in tc.draw(gs::subsequences(names)) {
            let id = fresh(tc, &tree);
            tree.ids.insert(id);
            node.tags.push(TagNode { id, name: cased(tc, t), tracks: BTreeSet::new(), strays: 0 });
        }
        tree.categories.push(node);
    }
    if tc.draw(gs::booleans()) {
        let stray = fresh(tc, &tree);
        tree.ids.insert(stray);
    }
    tree
}

/// A new category or tag takes the id the other database gives it when that one can store it and
/// doesn't use it, so both agree; otherwise an id neither uses: past every category's for a
/// category, and for a tag one from its names (ignoring case) that fits exportExt.pdb, the next
/// free one when that's taken.
#[hegel::test(test_cases = 60)]
fn new_categories_and_tags_get_ids_both_databases_can_share(tc: TestCase) {
    const NAMES: &[&str] = &["Genre", "Mood", "Situation"];
    let small = |tc: &TestCase| tc.draw(gs::integers::<i64>().min_value(1).max_value(12));
    let any = |tc: &TestCase| match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => tc.draw(gs::integers::<i64>().min_value(i64::from(u32::MAX) - 2).max_value(i64::from(u32::MAX) + 2)),
        1 => tc.draw(gs::integers::<i64>().min_value(0x1_0000).max_value(0x1_0010)),
        _ => tc.draw(gs::integers::<i64>().min_value(1).max_value(12)),
    };
    let tree = draw_tree(&tc, NAMES, &small);
    let other = draw_tree(&tc, NAMES, &any);
    let ids = Ids::of([&tree, &other]);
    let used: BTreeSet<i64> = tree.ids.union(&other.ids).copied().collect();
    let max_category = tree.categories.iter().chain(&other.categories).map(|c| c.id).max().unwrap_or(0);
    for source in [Source::ExportExt, Source::OneLibrary] {
        let usable = |id: i64| Ids::fits(source, id) && !tree.ids.contains(&id);
        for name in NAMES.iter().flat_map(|n| [n.to_string(), n.to_lowercase()]) {
            let got = ids.category(source, &tree, Some(&other), &name);
            match other.category(&name).map(|c| c.id).filter(|id| usable(*id)) {
                Some(theirs) => assert_eq!(got, theirs, "{name}: the other database's id"),
                None => assert!(got > max_category && !used.contains(&got), "{name}: {got}, used {used:?}"),
            }
            for tag in NAMES {
                let got = ids.tag(source, &tree, Some(&other), &name, tag);
                match other.tag(&name, tag).map(|(_, t)| t.id).filter(|id| usable(*id)) {
                    Some(theirs) => assert_eq!(got, theirs, "{name}/{tag}: the other database's id"),
                    None => {
                        assert!((0x1_0000..=i64::from(u32::MAX)).contains(&got) && !used.contains(&got), "{got}");
                        let alone = Ids::of([&tree]);
                        let shouted = alone.tag(source, &tree, None, &name.to_uppercase(), &tag.to_uppercase());
                        assert_eq!(alone.tag(source, &tree, None, &name, tag), shouted, "ignoring case");
                        // Taken by a row of this database: the next one along.
                        let mut taken = tree.clone();
                        taken.ids.insert(shouted);
                        let next = Ids::of([&taken]).tag(source, &taken, None, &name, tag);
                        assert_eq!(next, if shouted == i64::from(u32::MAX) { 0x1_0000 } else { shouted + 1 });
                    }
                }
            }
        }
    }
    // A category or tag just planned is taken too.
    let mut ids = Ids::of([&tree]);
    let id = ids.category(Source::OneLibrary, &tree, None, "Venue");
    ids.take(id, true);
    let next = ids.category(Source::OneLibrary, &tree, None, "Other");
    assert!(next > id && !tree.ids.contains(&next), "{next} after {id}");
}

/// A tree with `categories` (the first holding `first_tags` tags, the rest two each), ids from 1,
/// and drawn tracks of `keys` on its tags.
#[hegel::test_helper]
fn tree_of(tc: &TestCase, categories: &[&str], first_tags: usize, keys: &[String]) -> Tree {
    let mut tree = Tree::default();
    let mut next = 0;
    for (i, c) in categories.iter().enumerate() {
        next += 1;
        let mut node = CategoryNode { id: next, name: c.to_string(), tags: Vec::new() };
        for n in 0..if i == 0 { first_tags } else { 2 } {
            next += 1;
            let tracks = keys.iter().filter(|_| tc.draw(gs::weighted_booleans(0.2))).cloned().collect();
            node.tags.push(TagNode { id: next, name: format!("Tag {n}"), tracks, strays: 0 });
        }
        tree.categories.push(node);
    }
    tree.ids = (1..=next).collect();
    tree
}

/// Giving tracks their tags in a database (as merge and convert do): every tag ends up on its
/// track, matched by name ignoring case and created where missing (spelled as the other database
/// spells it), unless rekordbox's limits (4 categories, 50 tags in one) leave it out, which the
/// warning says. The steps lead to the tree returned, and the tracks counted are those that
/// gained a tag.
#[hegel::test(test_cases = 30)]
fn carried_tags_reach_their_tracks_within_rekordboxs_limits(tc: TestCase) {
    let library = six_tracks();
    let keys: Vec<String> = library.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect();
    let pair = |c: &str, t: &str| (c.to_string(), t.to_string());
    let choices = [
        pair("Genre", "Tag 1"),
        pair("genre", "tag 1"),
        pair("Genre", "New"),
        pair("Venue", "Club"),
        pair("venue", "club"),
        pair("Mood", "Dark"),
        pair("Situation", "Tag 0"),
    ];
    // Room to spare, and every category taken with the first one full.
    let all: [&str; MAX_CATEGORIES] = ["Genre", "Components", "Situation", "Mood"];
    for full in [false, true] {
        let categories: &[&str] = match full {
            true => &all,
            false => &all[..tc.draw(gs::integers::<usize>().max_value(all.len() - 1))],
        };
        let tree = tree_of(&tc, categories, if full { MAX_TAGS } else { 2 }, &keys);
        let other = tc.draw(gs::booleans()).then(|| {
            let mut other = tree_of(&tc, &["VENUE", "genre"], 1, &[]);
            other.categories[0].tags[0].name = "CLUB".into();
            other
        });
        // A tag named twice, in two spellings, on one track; beyond the limits when full.
        let mut wanted: Wanted = vec![(1, keys[0].clone(), vec![pair("Genre", "Tag 1"), pair("GENRE", "TAG 1")])];
        if full {
            wanted.push((2, keys[1].clone(), vec![pair("Venue", "Club"), pair("Genre", "New")]));
        }
        for (i, key) in keys.iter().enumerate().skip(2) {
            let tags: TagNames = choices.iter().filter(|_| tc.draw(gs::weighted_booleans(0.3))).cloned().collect();
            wanted.push((i as u32 + 1, key.clone(), tags));
        }
        let (ops, after, carried) = carry_ops(Source::OneLibrary, &tree, other.as_ref(), &wanted);
        let mut replayed = tree.clone();
        for op in &ops {
            replayed.apply(op);
        }
        assert_eq!(replayed, after, "the steps lead to the tree returned");
        for (_, key, tags) in &wanted {
            for (c, t) in tags {
                let path = format!("{c}/{t}");
                let category = after.category(c);
                match after.tag(c, t) {
                    Some((_, tag)) => {
                        assert!(tag.tracks.contains(key), "{path} on {key}");
                        assert!(!carried.skipped.contains(&path), "{path} is there, but skipped");
                    }
                    None => {
                        assert!(carried.skipped.contains(&path), "{path} is missing, not skipped");
                        let limit = match category {
                            Some(c) => c.tags.len() >= MAX_TAGS,
                            None => after.categories.len() >= MAX_CATEGORIES,
                        };
                        assert!(limit, "{path} was left out within rekordbox's limits");
                    }
                }
            }
        }
        let had = |tree: &Tree, id: i64, key: &String| {
            tree.categories.iter().flat_map(|c| &c.tags).any(|t| t.id == id && t.tracks.contains(key))
        };
        let gained: BTreeSet<String> = after
            .categories
            .iter()
            .flat_map(|c| &c.tags)
            .flat_map(|t| t.tracks.iter().filter(|k| !had(&tree, t.id, k)))
            .cloned()
            .collect();
        assert_eq!(carried.tracks, gained);
        for path in &carried.created {
            let (c, t) = split_path(path).unwrap();
            let theirs = match t {
                None => other.as_ref().and_then(|o| o.category(c)).map(|c| c.name.clone()),
                Some(t) => other.as_ref().and_then(|o| o.tag(c, t)).map(|(_, t)| t.name.clone()),
            };
            assert!(theirs.is_none_or(|n| path.ends_with(&n)), "{path}: spelled as the other database does");
            let there = |tree: &Tree| match t {
                None => tree.categories.iter().any(|x| x.name == c),
                Some(t) => tree.categories.iter().any(|x| x.name == c && x.tags.iter().any(|y| y.name == t)),
            };
            assert!(there(&after) && !there(&tree), "{path} was created");
        }
        let warning = carried.warning("exportLibrary.db");
        assert_eq!(warning.is_some(), !carried.skipped.is_empty(), "{warning:?}");
        if let Some(w) = warning {
            assert!(
                w.starts_with(&format!("{} My Tag(s) weren't added to exportLibrary.db", carried.skipped.len())),
                "{w}"
            );
        }
        if full {
            assert!(carried.skipped.contains("Venue/Club") && carried.skipped.contains("Genre/New"));
        }
        let preview = carry_preview(&DbTags::new(tree.clone(), &library), None, &wanted);
        assert_eq!(preview.skipped, carry_ops(Source::ExportExt, &tree, None, &wanted).2.skipped);
    }
}

/// OneLibrary rows that belong to nothing (a tag whose parent isn't a category, a link to a tag
/// or a track that isn't there) are counted as strays and read as nothing else: the tree is the
/// one without them.
#[hegel::test(test_cases = 20)]
fn rows_that_point_nowhere_are_counted_and_left_out(tc: TestCase) {
    let library = six_tracks();
    let mut one = onelibrary_of(&library);
    one.add_my_tag(1, 0, "Genre", true).unwrap();
    one.add_my_tag(2, 1, "Deep", false).unwrap();
    let linked: Vec<u32> = tc.draw(gs::subsequences(&[1, 2, 3, 4, 5, 6]));
    one.link_my_tag(2, &linked).unwrap();
    let clean = plus_tree(&one, &library).unwrap();
    assert_eq!(clean.strays, 0);
    let (orphans, gone_tracks, gone_tags) = (
        tc.draw(gs::integers::<i64>().min_value(0).max_value(3)),
        tc.draw(gs::integers::<u32>().max_value(3)),
        tc.draw(gs::integers::<i64>().min_value(0).max_value(3)),
    );
    for n in 0..orphans {
        // Under the tag, or under a parent that isn't there.
        let parent = if n % 2 == 0 { 2 } else { 900 + n };
        one.connection()
            .execute(
                "INSERT INTO myTag (myTag_id, sequenceNo, name, attribute, myTag_id_parent) VALUES (?1, 0, 'Orphan', 0, ?2)",
                [100 + n, parent],
            )
            .unwrap();
    }
    let missing: Vec<u32> = (0..gone_tracks).map(|n| 700 + n).collect();
    one.link_my_tag(2, &missing).unwrap();
    for n in 0..gone_tags {
        one.connection().execute("INSERT INTO myTag_content (myTag_id, content_id) VALUES (?1, 1)", [500 + n]).unwrap();
    }
    let tree = plus_tree(&one, &library).unwrap();
    let strays = tree.strays + tree.categories.iter().flat_map(|c| &c.tags).map(|t| t.strays).sum::<usize>();
    assert_eq!(strays, (orphans + i64::from(gone_tracks) + gone_tags) as usize);
    let mut without = tree.clone();
    without.strays = 0;
    for t in without.categories.iter_mut().flat_map(|c| &mut c.tags) {
        t.strays = 0;
    }
    without.ids.retain(|id| clean.ids.contains(id));
    assert_eq!(without, clean, "the rows that belong somewhere read as they did");
}

/// [`new_ext`]: a new exportExt.pdb holds the drawn categories and tags in order (up to
/// rekordbox's limits, the rest said as skipped) and links exactly the tracks that carry them,
/// and nothing of its template.
#[hegel::test(test_cases = 30)]
fn a_new_export_ext_holds_the_tags_given_and_nothing_else(tc: TestCase) {
    let library = six_tracks();
    let names = || gs::sampled_from(vec!["Genre", "Deep", "Peak Time", "Ünï", "x", "Situation"]);
    let mut categories: Vec<crate::masterdb::TagCategory> = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(MAX_CATEGORIES + 1)) {
        let name = tc.draw(names()).to_string();
        if categories.iter().any(|c| c.name == name) {
            continue;
        }
        let mut tags: Vec<crate::masterdb::TagEntry> = Vec::new();
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
            let name = tc.draw(names()).to_string();
            if tags.iter().any(|t| t.name == name) {
                continue;
            }
            let ids: BTreeSet<u32> =
                tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(6))).into_iter().collect();
            tags.push(crate::masterdb::TagEntry { name, track_ids: ids.into_iter().collect() });
        }
        categories.push(crate::masterdb::TagCategory { name, tags });
    }
    let given = DbTags::collection(&categories, &library);
    let (bytes, carried) = new_ext(&given, &library).unwrap();
    let got = DbTags::ext(&bytes, &library).unwrap();
    let kept: Vec<CategoryNode> = given.tree.categories.iter().take(MAX_CATEGORIES).cloned().collect();
    assert_eq!(got.tree.categories, kept);
    assert_eq!(got.tree.strays, 0, "nothing of the template is left");
    let past: BTreeSet<String> = given
        .tree
        .categories
        .iter()
        .skip(MAX_CATEGORIES)
        .flat_map(|c| c.tags.iter().filter(|t| !t.tracks.is_empty()).map(move |t| format!("{}/{}", c.name, t.name)))
        .collect();
    assert_eq!(carried.skipped, past);
}
