//! Property tests for editing a OneLibrary database in place: tracks' details (and the lookup
//! rows they leave unused), playlist and history nodes found by parent and name, tracks copied
//! between databases with their My Tags and cues, the refusal to delete while a newer
//! rekordbox's table points at tracks, and writing the database to a drive.

use indexmap::{IndexMap, IndexSet};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::TrackFields;
use locallibrary::model::TrackColor;
use locallibrary::onelibrary::{self, ContentRecord, CueRow, OneLibrary, Tree};
use locallibrary::{Error, Track, paths};

use crate::props_onelibrary::{any_track, field, new_library};

/// A new database holding `n` drawn tracks, with the tracks as `library()` should read them.
#[hegel::test_helper]
fn with_tracks(tc: &TestCase, n: usize) -> (OneLibrary, IndexMap<u32, Track>) {
    let mut one = OneLibrary::from_encrypted(new_library(), None).unwrap();
    let defaults = one.content_defaults().unwrap();
    let mut tracks = IndexMap::new();
    for i in 0..n {
        let mut t = any_track(tc, i, None);
        let rec = ContentRecord::from_track(&t, !t.analyze_path.is_empty(), &defaults);
        t.id = one.add_track(&rec, &t.file_path, &t.analyze_path, None).unwrap();
        // The album artist is the album's: without an album there is none.
        if t.album.is_empty() {
            t.album_artist.clear();
        }
        tracks.insert(t.id, t);
    }
    (one, tracks)
}

/// Lookup rows (artists, albums, genres, labels, keys) that no track or album refers to.
fn orphans(one: &OneLibrary) -> Vec<(String, i64)> {
    let sql = "SELECT 'artist', artist_id FROM artist a WHERE NOT EXISTS (SELECT 1 FROM content c WHERE a.artist_id IN
                   (c.artist_id_artist, c.artist_id_remixer, c.artist_id_originalArtist, c.artist_id_composer,
                    c.artist_id_lyricist))
                 AND NOT EXISTS (SELECT 1 FROM album al WHERE al.artist_id = a.artist_id)
               UNION ALL SELECT 'album', album_id FROM album x
                 WHERE NOT EXISTS (SELECT 1 FROM content c WHERE c.album_id = x.album_id)
               UNION ALL SELECT 'genre', genre_id FROM genre x
                 WHERE NOT EXISTS (SELECT 1 FROM content c WHERE c.genre_id = x.genre_id)
               UNION ALL SELECT 'label', label_id FROM label x
                 WHERE NOT EXISTS (SELECT 1 FROM content c WHERE c.label_id = x.label_id)
               UNION ALL SELECT 'key', key_id FROM \"key\" x
                 WHERE NOT EXISTS (SELECT 1 FROM content c WHERE c.key_id = x.key_id)";
    one.connection()
        .prepare(sql)
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// A colour as an edit names it, with what it means: `None` for one that isn't a colour.
#[hegel::test_helper]
fn colour(tc: &TestCase) -> (String, Option<Option<TrackColor>>) {
    const NAMES: [&str; 8] = ["pink", "red", "orange", "yellow", "green", "aqua", "blue", "purple"];
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => {
            let n = tc.draw(gs::integers::<u8>().max_value(9));
            let meant = match n {
                0 => Some(None),
                9 => None,
                n => Some(TrackColor::from_id(n)),
            };
            (n.to_string(), meant)
        }
        1 => {
            let i = tc.draw(gs::integers::<usize>().max_value(NAMES.len() - 1));
            (NAMES[i].to_uppercase(), Some(TrackColor::from_id(i as u8 + 1)))
        }
        2 => ("none".into(), Some(None)),
        _ => ("chartreuse".into(), None),
    }
}

/// Which fields to change, each with a drawn value (names often blank, which clears them).
#[hegel::test_helper]
fn track_fields(tc: &TestCase) -> (TrackFields, Option<Option<Option<TrackColor>>>) {
    let some = |tc: &TestCase| tc.draw(gs::weighted_booleans(0.4));
    let name =
        |tc: &TestCase| some(tc).then(|| if tc.draw(gs::weighted_booleans(0.2)) { " ".into() } else { field(tc) });
    let colour = some(tc).then(|| colour(tc));
    let f = TrackFields {
        title: name(tc),
        artist: name(tc),
        album: name(tc),
        genre: name(tc),
        label: name(tc),
        key: some(tc).then(|| tc.draw(gs::sampled_from(vec!["", "8A", "Abm", "11B"])).to_string()),
        remixer: name(tc),
        original_artist: name(tc),
        composer: name(tc),
        comment: name(tc),
        mix_name: name(tc),
        rating: some(tc).then(|| tc.draw(gs::integers::<u8>().max_value(5))),
        color: colour.as_ref().map(|c| c.0.clone()),
        year: some(tc).then(|| tc.draw(gs::integers::<u16>())),
        play_count: some(tc).then(|| tc.draw(gs::integers::<u16>())),
        track_number: some(tc).then(|| tc.draw(gs::integers::<u32>())),
        disc_number: some(tc).then(|| tc.draw(gs::integers::<u16>())),
        album_artist: name(tc),
        bpm: some(tc).then(|| tc.draw(gs::floats::<f64>().min_value(1.0).max_value(999.99))),
        isrc: some(tc).then(|| tc.draw(gs::sampled_from(vec!["", "GBDUW0000059", "USRC17607839"])).to_string()),
        release_date: some(tc).then(|| tc.draw(gs::sampled_from(vec!["", "2001-03-12"])).to_string()),
    };
    (f, colour.map(|c| c.1))
}

/// What `t` reads back as once `f` (whose colour means `colour`) is applied. A blank name clears
/// its field; a new album keeps the old one's artist, and without an album there is none.
fn applied(t: &mut Track, f: &TrackFields, colour: Option<Option<TrackColor>>) {
    let name = |v: &str| if v.trim().is_empty() { String::new() } else { v.to_string() };
    for (field, value) in [
        (&mut t.title, &f.title),
        (&mut t.comment, &f.comment),
        (&mut t.mix_name, &f.mix_name),
        (&mut t.isrc, &f.isrc),
        (&mut t.release_date, &f.release_date),
    ] {
        if let Some(v) = value {
            *field = v.clone();
        }
    }
    for (field, value) in [
        (&mut t.artist, &f.artist),
        (&mut t.remixer, &f.remixer),
        (&mut t.original_artist, &f.original_artist),
        (&mut t.composer, &f.composer),
        (&mut t.genre, &f.genre),
        (&mut t.label, &f.label),
        (&mut t.key, &f.key),
        (&mut t.album, &f.album),
        (&mut t.album_artist, &f.album_artist),
    ] {
        if let Some(v) = value {
            *field = name(v);
        }
    }
    if t.album.is_empty() {
        t.album_artist.clear();
    }
    t.bpm = f.bpm.map_or(t.bpm, |b| (b * 100.0).round() / 100.0);
    t.rating = f.rating.unwrap_or(t.rating);
    t.color = colour.unwrap_or(t.color);
    t.year = f.year.unwrap_or(t.year);
    t.play_count = f.play_count.unwrap_or(t.play_count);
    t.track_number = f.track_number.unwrap_or(t.track_number);
    t.disc_number = f.disc_number.unwrap_or(t.disc_number);
}

/// Editing tracks' details in place reads back as a model of the edit predicts, and leaves no
/// artist, album, genre, label or key behind that nothing refers to any more (an album's artist
/// goes with the album). An edit naming a track that isn't there, or a colour that isn't one,
/// is refused before anything changes.
#[hegel::test(test_cases = 40, suppress_health_check = [HealthCheck::TooSlow])]
fn edited_track_details_read_back(tc: TestCase) {
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let (mut one, mut model) = with_tracks(&tc, count);
    assert!(orphans(&one).is_empty());
    let all: Vec<u32> = model.keys().copied().collect();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        let mut ids: Vec<u32> = tc.draw(gs::vecs(gs::sampled_from(all.clone())).min_size(1).max_size(3).unique(true));
        let (f, colour) = track_fields(&tc);
        let missing = tc.draw(gs::weighted_booleans(0.15));
        if missing {
            ids.insert(0, u32::MAX);
        }
        match (one.set_track_fields(&ids, &f), colour) {
            (Err(e), Some(None)) => assert_eq!(e.code(), "invalid", "{e}"),
            (Err(e), _) if missing => assert!(matches!(e, Error::NotFound(_)), "{e}"),
            (Ok(()), colour) if !missing && colour != Some(None) => {
                for id in &ids {
                    applied(&mut model[id], &f, colour.flatten());
                }
            }
            (got, colour) => panic!("{f:?} (colour {colour:?}) on {ids:?}: {got:?}"),
        }
        assert_eq!(one.library().unwrap().tracks, model.values().cloned().collect::<Vec<_>>(), "after {f:?}");
        assert_eq!(orphans(&one), [], "after {f:?}");
    }
}

/// `(id, parent, name)` of a tree's nodes, as made.
type Nodes = IndexMap<u32, (u32, String)>;

/// Every (parent, name) a lookup could ask for: each node's, and some no node has.
fn probes(nodes: &Nodes, names: &[&str]) -> IndexSet<(u32, String)> {
    let parents = nodes.keys().chain(nodes.values().map(|(p, _)| p)).copied().chain([0, 999]);
    parents.flat_map(|p| names.iter().map(move |n| (p, n.to_string()))).collect()
}

/// A playlist or history node is found by its parent and name wherever it's been put and
/// whatever it's been renamed to (one of several alike, if siblings share a name), and not once
/// it's removed. Moving, renaming or removing a node that isn't there is refused, and a removal
/// naming one removes none.
#[hegel::test(test_cases = 60)]
fn nodes_are_found_by_parent_and_name(tc: TestCase) {
    const NAMES: [&str; 4] = ["Set", "Warm up", "", "Ünï"];
    let mut one = OneLibrary::from_encrypted(new_library(), None).unwrap();
    let tree = [Tree::Playlist, Tree::History][tc.draw(gs::integers::<usize>().max_value(1))];
    let name = |tc: &TestCase| tc.draw(gs::sampled_from(NAMES.to_vec())).to_string();
    let mut nodes = Nodes::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(8)) {
        let ids: Vec<u32> = nodes.keys().copied().collect();
        let pick = |tc: &TestCase| if ids.is_empty() { 0 } else { tc.draw(gs::sampled_from(ids.clone())) };
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 | 1 => {
                let (parent, n) = (pick(&tc), name(&tc));
                let id = match tree {
                    Tree::Playlist => one.create_playlist(&n, parent, tc.draw(gs::booleans())),
                    Tree::History => one.create_history(&n, parent, tc.draw(gs::booleans())),
                }
                .unwrap();
                nodes.insert(id, (parent, n));
            }
            2 => {
                let (id, n) = (pick(&tc), name(&tc));
                match one.rename_node(tree, id, &n) {
                    Ok(()) => nodes[&id].1 = n,
                    Err(e) => assert!(!nodes.contains_key(&id) && matches!(e, Error::NotFound(_)), "{e}"),
                }
            }
            3 => {
                let (id, parent) = (pick(&tc), pick(&tc));
                let seq = tc.draw(gs::integers::<u32>().max_value(20));
                match one.place_node(tree, id, parent, seq) {
                    Ok(()) => nodes[&id].0 = parent,
                    Err(e) => assert!(!nodes.contains_key(&id) && matches!(e, Error::NotFound(_)), "{e}"),
                }
            }
            _ => {
                let mut gone: Vec<u32> = if ids.is_empty() {
                    Vec::new()
                } else {
                    tc.draw(gs::vecs(gs::sampled_from(ids.clone())).max_size(2).unique(true))
                };
                let missing = tc.draw(gs::weighted_booleans(0.3));
                if missing {
                    gone.push(u32::MAX);
                }
                match one.remove_nodes(tree, &gone) {
                    Ok(()) => nodes.retain(|id, _| !gone.contains(id)),
                    Err(e) => assert!(missing && matches!(e, Error::NotFound(_)), "{e}"),
                }
            }
        }
        for (parent, n) in probes(&nodes, &NAMES) {
            let alike: Vec<u32> = nodes.iter().filter(|(_, v)| **v == (parent, n.clone())).map(|(id, _)| *id).collect();
            match one.node_id(tree, parent, &n).unwrap() {
                Some(id) => assert!(alike.contains(&id), "{n:?} under {parent} found as {id}, made {alike:?}"),
                None => assert!(alike.is_empty(), "{n:?} under {parent} not found, made {alike:?}"),
            }
        }
    }
}

/// My Tags as (category, tag) names: a few, so that databases share some.
#[hegel::test_helper]
fn tag_pairs(tc: &TestCase) -> Vec<(String, String)> {
    let pair = gs::tuples!(gs::sampled_from(vec!["Genre", "Mood"]), gs::sampled_from(vec!["Dark", "Warm", "Peak"]));
    let pairs: Vec<(&str, &str)> = tc.draw(gs::vecs(pair).max_size(3).unique(true));
    pairs.into_iter().map(|(c, t)| (c.to_string(), t.to_string())).collect()
}

/// Give `one` the categories and tags of `pairs` it lacks (ids from `next`), and return each
/// pair's tag id.
fn add_tags(one: &mut OneLibrary, pairs: &[(String, String)], next: &mut i64) -> Vec<i64> {
    let rows = one.my_tag_rows().unwrap();
    let mut made: IndexMap<(i64, String), i64> = rows.iter().map(|r| ((r.parent, r.name.clone()), r.id)).collect();
    let mut id_of = |one: &mut OneLibrary, parent: i64, name: &str| {
        *made.entry((parent, name.to_string())).or_insert_with(|| {
            *next += 1;
            one.add_my_tag(*next, parent, name, parent == 0).unwrap();
            *next
        })
    };
    pairs
        .iter()
        .map(|(c, t)| {
            let category = id_of(one, 0, c);
            id_of(one, category, t)
        })
        .collect()
}

/// (category, tag) names of every tag in `one`, and how many categories and tags it has.
fn tag_names(one: &OneLibrary) -> (IndexSet<(String, String)>, usize, usize) {
    let rows = one.my_tag_rows().unwrap();
    let categories: IndexMap<i64, &str> =
        rows.iter().filter(|r| r.is_category).map(|r| (r.id, r.name.as_str())).collect();
    let tags: IndexSet<(String, String)> =
        rows.iter().filter(|r| !r.is_category).map(|r| (categories[&r.parent].to_string(), r.name.clone())).collect();
    (tags, categories.len(), rows.len() - categories.len())
}

fn sorted<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort();
    v
}

/// A track exported from one database and added to another reads back there with the same
/// details, cues (new ids, mapped from the old ones) and My Tags. Tags are found by name: those
/// the target has are reused, the rest made once, so the target ends up with exactly the tag
/// names of both and no name twice.
#[hegel::test(test_cases = 40, suppress_health_check = [HealthCheck::TooSlow])]
fn tracks_copied_between_databases_keep_details_cues_and_tags(tc: TestCase) {
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let (mut src, tracks) = with_tracks(&tc, count);
    let mut next = 0;
    for (i, id) in tracks.keys().enumerate() {
        let pairs = tag_pairs(&tc);
        for tag in add_tags(&mut src, &pairs, &mut next) {
            src.link_my_tag(tag, &[*id]).unwrap();
        }
        let cues: Vec<CueRow> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
            .map(|c| CueRow {
                hot_cue: tc.draw(gs::booleans()).then_some(c as u8),
                in_usec: tc.draw(gs::integers::<i64>().min_value(0).max_value(600_000_000)),
                comment: format!("{i}.{c}"),
                ..Default::default()
            })
            .collect();
        src.replace_cues(*id, &cues).unwrap();
    }
    let (mut dst, _) = with_tracks(&tc, 0);
    let mut dst_next = 50;
    add_tags(&mut dst, &tag_pairs(&tc), &mut dst_next);
    let (mut want, ..) = tag_names(&dst);

    for (id, t) in &tracks {
        let rec = src.export_track(*id).unwrap().unwrap();
        assert_eq!(rec.album.is_some(), !t.album.is_empty());
        want.extend(rec.my_tags.iter().cloned());
        let (copy, cue_ids) = dst.add_track_with_cues(&rec, &t.file_path, &t.analyze_path, None).unwrap();
        let lib = dst.library().unwrap();
        assert_eq!(lib.track(copy), Some(&Track { id: copy, ..t.clone() }));
        let copied = dst.export_track(copy).unwrap().unwrap();
        assert_eq!(sorted(copied.my_tags), sorted(rec.my_tags.clone()));
        let (old, new) = (src.cue_rows(*id).unwrap(), dst.cue_rows(copy).unwrap());
        assert_eq!(old.len(), new.len());
        for (o, c) in old.iter().zip(&new) {
            assert!(o.same_as(c), "{o:?} copied as {c:?}");
            assert_eq!(cue_ids[&o.cue_id.unwrap()], c.cue_id.unwrap());
        }
    }
    let (names, categories, tags) = tag_names(&dst);
    assert_eq!(sorted(names.iter().cloned().collect()), sorted(want.iter().cloned().collect()));
    let wanted_categories: IndexSet<&str> = want.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!((categories, tags), (wanted_categories.len(), want.len()), "a tag name was made twice");
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// While a table rekordbox 7 doesn't have (a newer rekordbox's) has a column whose name holds
/// `content_id` or `cue_id`, removing tracks or a track's cues is refused, naming the table,
/// and nothing changes; a table whose columns name neither doesn't stand in the way. Keeping
/// every cue removes nothing, so it is never refused.
#[hegel::test(test_cases = 40, suppress_health_check = [HealthCheck::TooSlow])]
fn removals_wait_for_unknown_tables_that_name_tracks(tc: TestCase) {
    let (mut one, tracks) = with_tracks(&tc, 2);
    let ids: Vec<u32> = tracks.keys().copied().collect();
    let rows = [CueRow { in_usec: 1_000, ..Default::default() }, CueRow { in_usec: 2_000, ..Default::default() }];
    one.replace_cues(ids[0], &rows).unwrap();
    let table = format!("future{}", tc.draw(gs::text().max_size(6).exclude_characters("\0")));
    let candidates = vec!["content_id", "cue_id", "old_content_id", "cue_ids", "contentId", "cue", "name", "value"];
    let columns: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(candidates)).min_size(1).max_size(3).unique(true));
    let sql = format!(
        "CREATE TABLE {} ({})",
        quoted(&table),
        columns.iter().map(|c| format!("{} integer", quoted(c))).collect::<Vec<_>>().join(", ")
    );
    one.connection().execute_batch(&sql).unwrap();
    let points = columns.iter().any(|c| c.contains("content_id") || c.contains("cue_id"));

    let (lib, cues) = (one.library().unwrap(), one.cue_rows(ids[0]).unwrap());
    let refused = |r: locallibrary::Result<()>| match r {
        Ok(()) => false,
        Err(e) => {
            assert!(e.code() == "onelibrary" && e.to_string().contains(&table), "{e}");
            true
        }
    };
    assert_eq!(refused(one.replace_cues(ids[0], &cues[..1]).map(|_| ())), points);
    assert_eq!(refused(one.remove_tracks(&[ids[1]])), points);
    if points {
        assert_eq!(one.library().unwrap(), lib);
        assert_eq!(one.cue_rows(ids[0]).unwrap(), cues);
    } else {
        assert_eq!(one.library().unwrap().tracks.len(), 1);
    }
    let kept = one.cue_rows(ids[0]).unwrap();
    assert!(one.replace_cues(ids[0], &kept).is_ok());
}

/// Writing a database to a drive puts exactly those bytes in place and takes the old WAL and
/// shared-memory index away, so the drive opens as the new database. A WAL that can't be taken
/// away (here a folder in its place) stops the write before the database is replaced, and
/// reading the drive reports it.
#[hegel::test(test_cases = 40, suppress_health_check = [HealthCheck::TooSlow])]
fn written_databases_replace_the_drives_and_its_wal(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    // As the drive sees itself (temporary folders can sit behind a symlink).
    let root = dir.path().canonicalize().unwrap().join("drive");
    let db = paths::to_fs(&root, paths::LIBRARY_PLUS_DB);
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let device = Device::open(&root).unwrap();
    let (wal, shm) = (device.path(paths::LIBRARY_PLUS_WAL), device.path(paths::LIBRARY_PLUS_SHM));

    let old = tc.draw(gs::booleans()).then(|| new_library().to_vec());
    if let Some(old) = &old {
        std::fs::write(&db, old).unwrap();
    }
    let wal_kind = tc.draw(gs::integers::<u8>().max_value(2));
    let wal_bytes = tc.draw(gs::binary().max_size(64));
    match wal_kind {
        0 => {}
        1 => std::fs::write(&wal, &wal_bytes).unwrap(),
        _ => std::fs::create_dir(&wal).unwrap(),
    }
    let with_shm = tc.draw(gs::booleans());
    if with_shm {
        std::fs::write(&shm, b"index").unwrap();
    }
    match (&old, wal_kind, onelibrary::read_files(&root)) {
        (None, _, Err(Error::Io { path, .. })) => assert_eq!(path, db),
        (Some(old), 0, Ok(files)) => assert_eq!(files, (old.clone(), None)),
        (Some(old), 1, Ok(files)) => assert_eq!(files, (old.clone(), Some(wal_bytes.clone()))),
        (Some(_), _, Err(Error::Io { path, .. })) => assert_eq!(path, wal),
        (_, _, got) => panic!("{wal_kind}: {got:?}"),
    }
    if onelibrary::read_files(&root).is_err() {
        assert!(OneLibrary::open_root(&root).is_err());
    }

    let new = crate::props_onelibrary_crafted::rich_library();
    match onelibrary::write(&device, new) {
        Ok(()) => {
            assert_ne!(wal_kind, 2);
            assert_eq!(onelibrary::read_files(&root).unwrap(), (new.to_vec(), None));
            assert!(!shm.exists());
            assert_eq!(OneLibrary::open(&device).unwrap().library().unwrap().tracks.len(), 3);
        }
        Err(e) => {
            assert!(wal_kind == 2 && matches!(&e, Error::Io { path, .. } if *path == wal), "{e}");
            assert_eq!(std::fs::read(&db).ok(), old);
        }
    }
}
