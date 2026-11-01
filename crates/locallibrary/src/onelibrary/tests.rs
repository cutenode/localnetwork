use super::*;

/// rekordbox's columns are loosely typed: numbers can come back as text or reals, and text
/// as numbers.
#[test]
fn values_read_as_the_type_a_column_should_have() {
    assert_eq!(int(ValueRef::Real(2.9)), 2);
    assert_eq!(int(ValueRef::Text(b" 42 ")), 42);
    assert_eq!(int(ValueRef::Text(b"forty")), 0);
    assert_eq!(int(ValueRef::Null), 0);
    assert_eq!(text(ValueRef::Integer(5)), "5");
    assert_eq!(text(ValueRef::Real(1.5)), "1.5");
    assert_eq!(text(ValueRef::Null), "");
}

#[test]
fn a_database_without_a_content_table_isnt_onelibrarys() {
    let one = OneLibrary::create().unwrap();
    one.conn.execute_batch("DROP TABLE content").unwrap();
    let err = OneLibrary::from_encrypted(&one.to_encrypted().unwrap(), None).unwrap_err();
    assert_eq!(
        err.to_string(),
        Error::OneLibrary("no content table; not a rekordbox OneLibrary database".into()).to_string()
    );
    assert!(format!("{one:?}").starts_with("OneLibrary { path: "));
}

#[test]
fn creates_empty_databases_sqlite_and_the_cipher_accept() {
    let mut one = OneLibrary::create().unwrap();
    let check: String = one.conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(check, "ok");
    let menus: i64 = one.conn.query_row("SELECT count(*) FROM menuItem", [], |r| r.get(0)).unwrap();
    assert_eq!(menus, 27);
    let p = one.create_playlist("Set", 0, false).unwrap();
    let defaults = one.content_defaults().unwrap();
    let rec =
        ContentRecord::from_track(&loclib_model::test_util::track(1, "A", "B", "/Contents/A/B.mp3"), true, &defaults);
    let t = one.add_track(&rec, "/Contents/A/B.mp3", "", None).unwrap();
    one.add_to_playlist(p, t).unwrap();
    let bytes = one.to_encrypted().unwrap();
    let (_, plain) = decrypt(&bytes, None).unwrap();
    assert_eq!((plain[18], plain[19], plain[20]), (2, 2, cipher::RESERVE as u8), "WAL mode, room for the cipher");
    let lib = OneLibrary::from_encrypted(&bytes, None).unwrap().library().unwrap();
    assert_eq!(lib.tracks.len(), 1);
    assert_eq!(lib.find_playlist("Set").unwrap().track_ids, [t]);
    // Two new databases never share a salt.
    assert_ne!(bytes[..16], OneLibrary::create().unwrap().to_encrypted().unwrap()[..16]);
}

/// A database whose b-tree for `table` claims more cells than a page holds, re-encrypted.
pub(crate) fn with_broken_table(conn: &Connection, codec: &Codec, mut plain: Vec<u8>, table: &str) -> Vec<u8> {
    let root: u32 =
        conn.query_row("SELECT rootpage FROM sqlite_master WHERE name = ?1", [table], |r| r.get(0)).unwrap();
    let at = (root as usize - 1) * PAGE_SIZE;
    plain[at + 3..at + 5].copy_from_slice(&u16::MAX.to_be_bytes());
    codec.encrypt(&plain).unwrap()
}

#[test]
fn a_damaged_b_tree_is_refused_before_anything_reads_it() {
    let one = OneLibrary::create().unwrap();
    let (codec, plain) = decrypt(&one.to_encrypted().unwrap(), None).unwrap();
    let bytes = with_broken_table(&one.conn, &codec, plain, "menuItem");
    let err = OneLibrary::from_encrypted(&bytes, None).unwrap_err().to_string();
    assert!(err.contains("SQLite finds it damaged"), "{err}");
}

/// SQLite reads a page from the WAL when a committed frame holds it, so the main file's copy
/// may be anything (zeros an interrupted checkpoint left, say). Anywhere else, zeros are damage.
#[test]
fn only_pages_sqlite_reads_must_authenticate() {
    let one = OneLibrary::create().unwrap();
    let bytes = one.to_encrypted().unwrap();
    let (codec, plain) = decrypt(&bytes, None).unwrap();
    let pages = (plain.len() / PAGE_SIZE) as u32;
    let frame = codec.encrypt_page(2, &plain[PAGE_SIZE..2 * PAGE_SIZE]).unwrap();
    let wal = wal::tests::build(PAGE_SIZE, (1, 2), &[(2, pages, frame)]);
    let mut holed = bytes.clone();
    holed[PAGE_SIZE..2 * PAGE_SIZE].fill(0);
    // Equal but for the reserve, which keeps each page's own IV and HMAC.
    let usable =
        |db: &[u8]| db.chunks(PAGE_SIZE).flat_map(|p| p[..PAGE_SIZE - cipher::RESERVE].to_vec()).collect::<Vec<_>>();
    assert_eq!(usable(&decrypt(&holed, Some(&wal)).unwrap().1), usable(&plain));
    let err = decrypt(&holed, None).map(|_| ()).unwrap_err().to_string();
    assert!(err.contains("page 2 fails authentication"), "{err}");

    let committed = wal::committed(&wal, PAGE_SIZE).unwrap().unwrap();
    assert!(!reads_main_page(2, Some(&committed)));
    assert!(!reads_main_page(pages + 1, Some(&committed)), "past the last commit");
    assert!(reads_main_page(3, Some(&committed)));
    assert!(!reads_main_page(cipher::LOCK_PAGE, None));
    assert!(reads_main_page(1, None));
}

#[test]
fn file_types() {
    assert_eq!(file_type("/Contents/a/b.MP3"), 1);
    assert_eq!(file_type("/Contents/a/b.m4a"), 4);
    assert_eq!(file_type("/Contents/a/b.aiff"), 12);
    assert_eq!(file_type("/Contents/a/b"), 0);
}

/// `numberOfContents` follows the tracks added and removed, also after an add that fails part
/// way (here once its track row is written).
#[test]
fn the_track_count_follows_adds_and_removes() {
    let mut one = OneLibrary::create().unwrap();
    let defaults = one.content_defaults().unwrap();
    let count = |one: &OneLibrary| -> (i64, i64) {
        let q = |sql| one.conn.query_row(sql, [], |r| r.get(0)).unwrap();
        (q("SELECT numberOfContents FROM property"), q("SELECT count(*) FROM content"))
    };
    let add = |one: &mut OneLibrary, n: u32, cues| {
        let path = format!("/Contents/A/{n}.mp3");
        let rec = ContentRecord {
            cues,
            ..ContentRecord::from_track(&loclib_model::test_util::track(n, "A", "B", &path), true, &defaults)
        };
        one.add_track(&rec, &path, "", None)
    };
    let ids: Vec<u32> = (1..=3).map(|n| add(&mut one, n, Vec::new()).unwrap()).collect();
    assert_eq!(count(&one), (3, 3));
    one.remove_tracks(&ids[..1]).unwrap();
    assert_eq!(count(&one), (2, 2));
    one.conn.execute_batch("DROP TABLE cue").unwrap();
    assert!(add(&mut one, 4, vec![(1, Vec::new())]).is_err());
    add(&mut one, 5, Vec::new()).unwrap();
    assert_eq!(count(&one), (4, 4));
}

/// A drawn library's tracks (artist, album, genre, key, cue count, My Tag), playlist entries
/// and tracks then removed, all as indexes into the tracks.
#[hegel::test_helper]
fn drawn_edits(tc: &hegel::TestCase) -> (Vec<[u8; 6]>, Vec<usize>, Vec<usize>) {
    use hegel::Generator;
    use hegel::generators as gs;
    let small = || gs::integers::<u8>().max_value(2);
    let tracks: Vec<[u8; 6]> = tc.draw(
        gs::vecs(
            gs::tuples!(small(), small(), small(), small(), small(), small())
                .map(|(a, b, c, d, e, f)| [a, b, c, d, e, f]),
        )
        .min_size(1)
        .max_size(10),
    );
    let index = || gs::integers::<usize>().max_value(tracks.len() - 1);
    let entries = tc.draw(gs::vecs(index()).max_size(12));
    let removed = tc.draw(gs::vecs(index()).max_size(5));
    (tracks, entries, removed)
}

/// Make the drawn edits on `one`: the tracks, a playlist and a history list of the entries, then
/// the removals.
fn make_edits(one: &mut OneLibrary, (tracks, entries, removed): &(Vec<[u8; 6]>, Vec<usize>, Vec<usize>)) {
    let defaults = one.content_defaults().unwrap();
    let names = ["", "Alpha", "Beta"];
    let mut ids = Vec::new();
    for (i, [artist, album, genre, key, cues, tag]) in tracks.iter().enumerate() {
        let path = format!("/Contents/{i}.mp3");
        let mut t = loclib_model::test_util::track(i as u32 + 1, names[*artist as usize], &format!("T{i}"), &path);
        t.album = names[*album as usize].into();
        t.genre = names[*genre as usize].into();
        t.key = ["", "8A", "11B"][*key as usize].into();
        let mut rec = ContentRecord::from_track(&t, true, &defaults);
        rec.cues = (0..i64::from(*cues))
            .map(|c| (c, vec![("kind".into(), Value::Integer(c)), ("inUsec".into(), Value::Integer(c * 1000))]))
            .collect();
        if *tag > 0 {
            rec.my_tags = vec![("Mood".into(), names[*tag as usize].into())];
        }
        ids.push(one.add_track(&rec, &path, "", None).unwrap());
    }
    let playlist = one.create_playlist("Set", 0, false).unwrap();
    let history = one.create_history("Last night", 0, false).unwrap();
    for &e in entries {
        one.add_to_playlist(playlist, ids[e]).unwrap();
        one.add_to_history(history, ids[e]).unwrap();
    }
    one.remove_tracks(&removed.iter().map(|&r| ids[r]).collect::<Vec<_>>()).unwrap();
}

/// The working copy's file once committed, without the change counters SQLite bumps on every
/// commit (header bytes 24-27 and 92-95, which `content_hash` leaves out too).
fn committed_pages(one: &OneLibrary) -> Vec<u8> {
    one.to_encrypted().unwrap();
    let mut plain = std::fs::read(&one.path).unwrap();
    plain[24..28].fill(0);
    plain[92..96].fill(0);
    plain
}

/// A working copy's edits made in one transaction leave the same database, page for page, as a
/// commit after every statement did.
#[hegel::test(test_cases = 50)]
fn edits_in_one_transaction_make_the_database_a_commit_after_each_did(tc: hegel::TestCase) {
    let edits = drawn_edits(&tc);
    let mut one = OneLibrary::create().unwrap();
    // As working copies were: each statement committed on its own.
    let each = OneLibrary::from_plain(one.codec.clone(), empty_database()).unwrap();
    each.conn.execute_batch("COMMIT").unwrap();
    let mut each = each.into_new().unwrap();
    make_edits(&mut one, &edits);
    make_edits(&mut each, &edits);
    assert_eq!(committed_pages(&one), committed_pages(&each));
    let hash = |one: &OneLibrary| content_hash(&one.to_encrypted().unwrap(), None).unwrap();
    assert_eq!(hash(&one), hash(&each));
}

/// [`OneLibrary::remove_tracks`] as it was: a delete for each track in each table.
fn remove_tracks_one_by_one(one: &mut OneLibrary, ids: &[u32]) -> Result<()> {
    one.check_no_unknown_references()?;
    let removed: FxHashSet<u32> = ids.iter().copied().collect();
    let lookups = one.unused_lookups(&removed)?;
    let tx = one.conn.savepoint()?;
    let mut touched_lists: Vec<(&str, &str, i64)> = Vec::new();
    for (table, list) in [("playlist_content", "playlist_id"), ("history_content", "history_id")] {
        let mut stmt = tx.prepare(&format!("SELECT DISTINCT {list} FROM {table} WHERE content_id = ?1"))?;
        for id in ids {
            for l in stmt.query_map([id], |r| r.get::<_, i64>(0))? {
                touched_lists.push((table, list, l?));
            }
        }
    }
    for id in ids {
        tx.execute(
            "DELETE FROM hotCueBankList_cue WHERE cue_id IN (SELECT cue_id FROM cue WHERE content_id = ?1)",
            [id],
        )?;
        for sql in [
            "DELETE FROM cue WHERE content_id = ?1",
            "DELETE FROM playlist_content WHERE content_id = ?1",
            "DELETE FROM history_content WHERE content_id = ?1",
            "DELETE FROM myTag_content WHERE content_id = ?1",
            "DELETE FROM recommendedLike WHERE content_id_1 = ?1 OR content_id_2 = ?1",
            "DELETE FROM content WHERE content_id = ?1",
        ] {
            tx.execute(sql, [id])?;
        }
    }
    touched_lists.sort_unstable();
    touched_lists.dedup();
    for (table, list, id) in touched_lists {
        let rowids: Vec<i64> = tx
            .prepare(&format!("SELECT rowid FROM {table} WHERE {list} = ?1 ORDER BY sequenceNo, rowid"))?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut update = tx.prepare(&format!("UPDATE {table} SET sequenceNo = ?1 WHERE rowid = ?2"))?;
        for (n, rowid) in rowids.iter().enumerate() {
            update.execute(params![n as i64 + 1, rowid])?;
        }
    }
    for (table, rows) in &lookups {
        let mut stmt = tx.prepare(&format!("DELETE FROM {} WHERE {} = ?1", table.table(), table.id()))?;
        for id in rows {
            stmt.execute([id])?;
        }
    }
    tx.commit()?;
    one.update_count(None)
}

/// A value a drive's database may hold where a track or cue id belongs: mostly one, sometimes
/// text, a real or nothing.
#[hegel::test_helper]
fn drawn_reference(tc: &hegel::TestCase, max: i64) -> Value {
    use hegel::generators as gs;
    match tc.draw(gs::integers::<u8>().max_value(9)) {
        0 => Value::Null,
        1 => Value::Text(tc.draw(gs::sampled_from(vec!["x", "2", " 3 "])).to_string()),
        2 => Value::Real(tc.draw(gs::sampled_from(vec![1.0, 2.5]))),
        _ => Value::Integer(tc.draw(gs::integers::<i64>().max_value(max))),
    }
}

/// Removing tracks deletes each table's rows from one pass over it and leaves the database
/// exactly as a delete for each track in each table did, whatever the rows hold.
#[hegel::test(test_cases = 100)]
fn removing_tracks_leaves_the_database_deletes_track_by_track_did(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let one = OneLibrary::create().unwrap();
    let tracks = tc.draw(gs::integers::<i64>().min_value(1).max_value(12));
    let insert = |sql: &str, values: Vec<Value>| one.conn.execute(sql, rusqlite::params_from_iter(values)).unwrap();
    for name in ["Alpha", "Beta"] {
        for table in ["artist", "album", "genre", "key"] {
            insert(&format!("INSERT INTO {table} (name) VALUES (?1)"), vec![Value::Text(name.into())]);
        }
        insert("INSERT INTO image (path) VALUES (?1)", vec![Value::Text(format!("/{name}.jpg"))]);
        insert("INSERT INTO playlist (name, playlist_id_parent) VALUES (?1, 0)", vec![Value::Text(name.into())]);
        insert("INSERT INTO history (name, history_id_parent) VALUES (?1, 0)", vec![Value::Text(name.into())]);
        insert(
            "INSERT INTO hotCueBankList (name, hotCueBankList_id_parent) VALUES (?1, 0)",
            vec![Value::Text(name.into())],
        );
        insert("INSERT INTO myTag (name, myTag_id_parent) VALUES (?1, 0)", vec![Value::Text(name.into())]);
    }
    let lookup = || Value::Integer(tc.draw(gs::integers::<i64>().max_value(2)));
    for id in 1..=tracks {
        insert(
            "INSERT INTO content (content_id, artist_id_artist, album_id, genre_id, key_id, image_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            vec![Value::Integer(id), lookup(), lookup(), lookup(), lookup(), lookup()],
        );
    }
    let rows = |n: usize| tc.draw(gs::integers::<usize>().max_value(n));
    for _ in 0..rows(20) {
        insert("INSERT INTO cue (content_id, kind) VALUES (?1, 0)", vec![drawn_reference(&tc, tracks + 1)]);
    }
    let cues = one.conn.query_row("SELECT count(*) FROM cue", [], |r| r.get::<_, i64>(0)).unwrap();
    for (table, list, item, max) in [
        ("playlist_content", "playlist_id", "content_id", tracks + 1),
        ("history_content", "history_id", "content_id", tracks + 1),
        ("hotCueBankList_cue", "hotCueBankList_id", "cue_id", cues + 1),
    ] {
        for seq in 0..rows(20) {
            insert(
                &format!("INSERT INTO {table} ({list}, {item}, sequenceNo) VALUES (?1, ?2, ?3)"),
                vec![lookup(), drawn_reference(&tc, max), Value::Integer(seq as i64 + 1)],
            );
        }
    }
    for _ in 0..rows(10) {
        insert(
            "INSERT INTO myTag_content (myTag_id, content_id) VALUES (?1, ?2)",
            vec![lookup(), drawn_reference(&tc, tracks + 1)],
        );
    }
    for _ in 0..rows(10) {
        insert(
            "INSERT INTO recommendedLike (content_id_1, content_id_2) VALUES (?1, ?2)",
            vec![drawn_reference(&tc, tracks + 1), drawn_reference(&tc, tracks + 1)],
        );
    }
    let gone: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(tracks as u32 + 1)).max_size(6));
    let (codec, plain) = decrypt(&one.to_encrypted().unwrap(), None).unwrap();
    let mut fast = OneLibrary::from_plain(codec.clone(), plain.clone()).unwrap();
    let mut slow = OneLibrary::from_plain(codec, plain).unwrap();
    let outcome = |r: Result<()>| r.map_err(|e| e.to_string());
    assert_eq!(outcome(fast.remove_tracks(&gone)), outcome(remove_tracks_one_by_one(&mut slow, &gone)));
    fast.to_encrypted().unwrap();
    slow.to_encrypted().unwrap();
    assert!(std::fs::read(&fast.path).unwrap() == std::fs::read(&slow.path).unwrap(), "the databases differ");
}
