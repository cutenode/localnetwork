//! Property tests for OneLibrary databases someone else made: its key is public, so a drive's
//! `exportLibrary.db` can hold any schema SQLite accepts, damaged b-trees, a working copy changed
//! behind SQLite's back, oddly typed values and a property table of many pages.

use std::sync::OnceLock;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::edit::{AudioFile, TrackFields};
use locallibrary::onelibrary::cipher::{Codec, PAGE_SIZE};
use locallibrary::onelibrary::{self, ContentRecord, CueRow, OneLibrary, Tree};
use rusqlite::types::Value;

use crate::props_onelibrary::new_library;

const LIST: u32 = 2;
const HISTORY: u32 = 1;
const BANK: u32 = 1;

/// A library using every table the readers and writers here touch: three tracks sharing an
/// album (with artwork, as is a playlist's), a folder holding a playlist, a history, cues of two
/// tracks in a hot cue bank list and a My Tag on two tracks. Built and encrypted once.
pub(crate) fn rich_library() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let mut one = OneLibrary::from_encrypted(new_library(), None).unwrap();
        let defaults = one.content_defaults().unwrap();
        let image = one.add_image("/PIONEER/Artwork/00001/b1.jpg").unwrap();
        for n in 1..=3 {
            let path = format!("/Contents/{n}.mp3");
            let mut t = loclib_model::test_util::track(0, &format!("Artist {n}"), &format!("Title {n}"), &path);
            (t.album, t.album_artist, t.genre, t.label, t.key) =
                ("Album".into(), "Band".into(), "House".into(), "Label".into(), "8A".into());
            let rec = ContentRecord::from_track(&t, true, &defaults);
            let anlz = format!("/PIONEER/USBANLZ/P000/0000000{n}/ANLZ0000.DAT");
            assert_eq!(one.add_track(&rec, &path, &anlz, Some(image)).unwrap(), n);
        }
        let folder = one.create_playlist("Folder", 0, true).unwrap();
        assert_eq!(one.create_playlist("List", folder, false).unwrap(), LIST);
        assert_eq!(one.create_history("History", 0, false).unwrap(), HISTORY);
        for t in 1..=3 {
            one.add_to_playlist(LIST, t).unwrap();
            one.add_to_history(HISTORY, t).unwrap();
        }
        one.connection()
            .execute("UPDATE album SET image_id = ?1", [image])
            .and_then(|_| {
                one.connection().execute("UPDATE playlist SET image_id = ?1 WHERE playlist_id = ?2", [image, LIST])
            })
            .unwrap();
        let at = |s: i64| CueRow { in_usec: s * 1_000_000, ..Default::default() };
        let rows = [
            CueRow { hot_cue: Some(0), comment: "Intro".into(), ..at(1) },
            at(2),
            CueRow { out_usec: Some(4_000_000), loop_beats: Some((4, 1)), ..at(3) },
        ];
        one.replace_cues(1, &rows).unwrap();
        one.replace_cues(2, &[CueRow { hot_cue: Some(1), ..at(5) }]).unwrap();
        assert_eq!(one.create_hot_cue_bank_list("Bank", 0, false).unwrap(), BANK);
        for cue in [1, 2, 4, 3] {
            one.add_to_hot_cue_bank_list(BANK, cue).unwrap();
        }
        one.add_my_tag(1, 0, "Mood", true).unwrap();
        one.add_my_tag(2, 1, "Dark", false).unwrap();
        one.link_my_tag(2, &[1, 3]).unwrap();
        one.to_encrypted().unwrap()
    })
}

/// Track 1 of [`rich_library`], exported to be added again.
fn record() -> &'static ContentRecord {
    static REC: OnceLock<ContentRecord> = OnceLock::new();
    REC.get_or_init(|| OneLibrary::from_encrypted(rich_library(), None).unwrap().export_track(1).unwrap().unwrap())
}

/// Readers, which change nothing, run first.
const READS: usize = 9;
/// Then writers, the one removing a track last.
const STEPS: usize = READS + 13;

fn read(one: &OneLibrary, op: usize) -> locallibrary::Result<String> {
    Ok(match op {
        0 => format!("{:?}", one.library()?),
        1 => format!("{:?}", one.images_used_outside_tracks()?),
        2 => format!("{:?}", one.hot_cue_bank_lists()?),
        3 => format!("{:?} {:?}", one.my_tag_rows()?, one.my_tag_links()?),
        4 => one.dangling_bank_list_entries()?.to_string(),
        5 => format!("{:?}", one.export_track(1)?),
        6 => one.unused_lookup_rows(&[1, 2])?.to_string(),
        7 => format!("{:?}", one.content_defaults()?),
        _ => format!("{:?} {:?}", one.cue_rows(1)?, one.node_id(Tree::Playlist, 1, "List")?),
    })
}

fn write(one: &mut OneLibrary, op: usize) -> locallibrary::Result<String> {
    Ok(match op {
        0 => {
            one.clear_cue_seeks(1)?;
            format!("{:?}", one.cue_rows(1)?)
        }
        1 => format!("{:?}", one.add_track_with_cues(record(), "/Contents/copy.mp3", "", Some(1))?),
        2 => {
            let p = one.create_playlist("New", 0, false)?;
            one.add_to_playlist(p, 2)?;
            p.to_string()
        }
        3 => {
            let h = one.create_history("Played", HISTORY, false)?;
            one.add_to_history(h, 3)?;
            h.to_string()
        }
        4 => {
            one.place_node(Tree::Playlist, LIST, 0, 7)?;
            one.rename_node(Tree::History, HISTORY, "Friday")?;
            format!("{:?}", one.node_id(Tree::Playlist, 0, "List")?)
        }
        5 => {
            let file = AudioFile { size: 1 << 20, sample_rate: 96_000, bit_depth: 24, bitrate: 2_304 };
            format!("{:?}", one.set_track_path(1, "/Contents/1.flac", Some(&file), 500)?)
        }
        6 => {
            let name = Some("New".to_string());
            let f = TrackFields {
                title: name.clone(),
                artist: name.clone(),
                album: name.clone(),
                genre: name.clone(),
                label: name.clone(),
                key: name.clone(),
                remixer: name.clone(),
                original_artist: Some(String::new()),
                composer: name.clone(),
                comment: name.clone(),
                mix_name: name,
                rating: Some(3),
                color: Some("aqua".into()),
                year: Some(1999),
                play_count: Some(7),
                track_number: Some(2),
                disc_number: Some(1),
                album_artist: Some("New".into()),
                bpm: Some(128.5),
                isrc: Some("GBDUW0000059".into()),
                release_date: Some("1999-03-12".into()),
            };
            format!("{:?}", one.set_track_fields(&[1, 2], &f)?)
        }
        7 => {
            one.add_my_tag(90, 0, "Era", true)?;
            one.add_my_tag(91, 90, "Nineties", false)?;
            // Not a category: refused whatever the schema.
            let under_a_tag = one.add_my_tag(92, 91, "Late", false).map_err(|e| e.code());
            one.rename_my_tag(91, "90s")?;
            let linked = one.link_my_tag(91, &[1, 2, 3])?;
            let unlinked = (one.unlink_my_tag(91, &[1])?, one.unlink_my_tags_of(&[2])?);
            one.remove_my_tag(90)?;
            format!("{under_a_tag:?} {linked} {unlinked:?}")
        }
        8 => {
            let mut rows = one.cue_rows(1)?;
            rows.remove(0);
            rows[0].comment = "Renamed".into();
            rows.push(CueRow { hot_cue: Some(4), in_usec: 9_000_000, ..Default::default() });
            format!("{:?}", one.replace_cues(1, &rows)?)
        }
        9 => {
            let image = one.add_image("/PIONEER/Artwork/00001/b9.jpg")?;
            one.set_track_image(&[1, 2], image)?;
            format!("{image} {}", one.next_image_id()?)
        }
        10 => {
            let bank = one.create_hot_cue_bank_list("Second", 0, false)?;
            one.add_to_hot_cue_bank_list(bank, 2)?;
            bank.to_string()
        }
        11 => {
            let moved = one.repoint_track(1, 2)?;
            one.set_playlist_entries(LIST, &[3, 2])?;
            one.remove_nodes(Tree::History, &[HISTORY])?;
            moved.to_string()
        }
        _ => format!("{:?}", one.remove_tracks(&[3])?),
    })
}

fn step(one: &mut OneLibrary, op: usize) -> locallibrary::Result<String> {
    if op < READS { read(one, op) } else { write(one, op - READS) }
}

/// Every step's result on the intact library.
fn intact() -> &'static [String] {
    static RESULTS: OnceLock<Vec<String>> = OnceLock::new();
    RESULTS.get_or_init(|| {
        let mut one = OneLibrary::from_encrypted(rich_library(), None).unwrap();
        (0..STEPS).map(|op| step(&mut one, op).unwrap_or_else(|e| panic!("step {op}: {e}"))).collect()
    })
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Tables, and columns of them, that the readers and writers name.
const NAMED: [(&str, Option<&str>); 13] = [
    ("hotCueBankList_cue", None),
    ("image", None),
    ("myTag_content", None),
    ("property", None),
    ("album", Some("isComplation")),
    ("playlist", Some("sequenceNo")),
    ("playlist", Some("attribute")),
    ("playlist_content", Some("sequenceNo")),
    ("myTag", Some("attribute")),
    ("myTag", Some("name")),
    ("content", Some("bitrate")),
    ("content", Some("artist_id_lyricist")),
    ("cue", Some("content_id")),
];

/// With tables, or columns, the drive's schema lacks, every reader and writer either works or
/// fails with SQLite's error naming one of them, never anything else and never a panic. Without
/// only whole tables, each step up to the first that fails gives the intact library's result:
/// it didn't need them. (Without a column, readers taking every column, and so their results,
/// may differ.) Without the track table, the database isn't opened again. The explicit cases
/// take away, together, parts that no step needs before another of them.
#[hegel::test(test_cases = 40, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(named = vec![0usize, 4, 5, 10], extra = 0usize)]
#[hegel::explicit_test_case(named = vec![1usize, 2, 6, 11], extra = 0usize)]
#[hegel::explicit_test_case(named = vec![7usize, 8, 3, 12], extra = 0usize)]
#[hegel::explicit_test_case(named = vec![9usize], extra = 0usize)]
fn a_schema_lacking_what_is_needed_is_reported(tc: TestCase) {
    let mut one = OneLibrary::from_encrypted(rich_library(), None).unwrap();
    let named: Vec<usize> =
        tc.draw(gs::vecs(gs::integers::<usize>().max_value(NAMED.len() - 1)).unique(true).max_size(4));
    let extra = tc.draw(gs::integers::<usize>().max_value(2));
    let mut parts: Vec<(String, Option<String>)> =
        named.iter().map(|&i| (NAMED[i].0.to_string(), NAMED[i].1.map(String::from))).collect();
    // And any table, or any column of one but its key.
    let names = |sql: &str, table: &str| -> Vec<String> {
        let mut stmt = one.connection().prepare(sql).unwrap();
        stmt.query_map([table], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    };
    let tables =
        names("SELECT name FROM sqlite_master WHERE type = ?1 AND name NOT LIKE 'sqlite_%' ORDER BY 1", "table");
    for _ in 0..extra {
        let table = tc.draw(gs::sampled_from(tables.clone()));
        let columns = names("SELECT name FROM pragma_table_info(?1) WHERE pk = 0", &table);
        let column = tc.draw(gs::booleans());
        let column = if column && !columns.is_empty() { Some(tc.draw(gs::sampled_from(columns))) } else { None };
        parts.push((table, column));
    }
    let mut missing: Vec<(String, Option<String>)> = Vec::new();
    for (table, column) in parts {
        let sql = match &column {
            None => format!("DROP TABLE {}", quoted(&table)),
            Some(c) => format!("ALTER TABLE {} DROP COLUMN {}", quoted(&table), quoted(c)),
        };
        // SQLite keeps a column an index or a key uses (and what's gone already is gone).
        if one.connection().execute_batch(&sql).is_ok() {
            missing.push((table, column));
        }
    }
    tc.assume(!missing.is_empty());
    tc.note(&format!("without {missing:?}"));
    if missing.contains(&("content".to_string(), None)) {
        let err = OneLibrary::from_encrypted(&one.to_encrypted().unwrap(), None).unwrap_err().to_string();
        assert!(err.contains("no content table"), "{err}");
    }
    let tables_only = missing.iter().all(|(_, c)| c.is_none());
    let names: Vec<&str> = missing.iter().map(|(t, c)| c.as_deref().unwrap_or(t)).collect();
    let mut failed = false;
    for op in 0..STEPS {
        match step(&mut one, op) {
            Ok(got) if tables_only && !failed => assert_eq!(got, intact()[op], "step {op}"),
            Ok(_) => {}
            Err(e) => {
                let named = names.iter().any(|n| e.to_string().contains(n));
                assert!(e.code() == "onelibrary" && named, "step {op}: {e}");
                failed = true;
            }
        }
    }
}

/// `bytes` with the root page of `table`'s b-tree (or the schema table's) damaged in one of
/// three ways, re-encrypted with the file's own salt.
fn damaged(bytes: &[u8], table: &str, kind: u8) -> Vec<u8> {
    let (codec, mut plain) = onelibrary::decrypt(bytes, None).unwrap();
    let one = OneLibrary::from_encrypted(bytes, None).unwrap();
    let root: u32 = one
        .connection()
        .query_row("SELECT rootpage FROM sqlite_master WHERE name = ?1", [table], |r| r.get(0))
        .unwrap();
    let at = (root as usize - 1) * PAGE_SIZE;
    match kind {
        // More cells than a page holds.
        0 => plain[at + 3..at + 5].copy_from_slice(&u16::MAX.to_be_bytes()),
        // A page type b-trees don't have.
        1 => plain[at] = 0x01,
        // The schema table's own page claiming too many cells.
        _ => plain[100 + 3..100 + 5].copy_from_slice(&u16::MAX.to_be_bytes()),
    }
    codec.encrypt(&plain).unwrap()
}

/// Tables with b-trees of their own (not page 1's schema table).
fn tables(one: &OneLibrary) -> Vec<String> {
    let mut stmt = one.connection().prepare("SELECT name FROM sqlite_master WHERE rootpage > 1 ORDER BY 1").unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

/// A database with a damaged b-tree still has a content hash (of its pages as they are), which
/// differs from the intact database's and survives re-encryption, while opening it is refused
/// as damaged (or malformed, when the damage is in the schema table).
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn damaged_databases_hash_but_never_open(tc: TestCase) {
    let intact = rich_library();
    let one = OneLibrary::from_encrypted(intact, None).unwrap();
    let table = tc.draw(gs::sampled_from(tables(&one)));
    let bad = damaged(intact, &table, tc.draw(gs::integers::<u8>().max_value(2)));
    let hashes = onelibrary::content_hashes(&bad, None).unwrap();
    assert_ne!(hashes, onelibrary::content_hashes(intact, None).unwrap());
    let (codec, plain) = onelibrary::decrypt(&bad, None).unwrap();
    assert_eq!(onelibrary::content_hashes(&codec.encrypt(&plain).unwrap(), None).unwrap(), hashes);
    // Damage SQLite meets reading the schema is its own error rather than a check's finding.
    let err = OneLibrary::from_encrypted(&bad, None).unwrap_err().to_string();
    assert!(err.contains("damaged") || err.contains("malformed"), "{err}");
}

/// A working copy damaged behind SQLite's back (a b-tree claiming too many cells, and the
/// change counter bumped so SQLite rereads the file) is never encrypted: the check before saving
/// finds it. An undamaged copy saves.
#[hegel::test(test_cases = 15, suppress_health_check = [HealthCheck::TooSlow])]
fn a_damaged_working_copy_is_never_saved(tc: TestCase) {
    let one = OneLibrary::from_encrypted(rich_library(), None).unwrap();
    let path = one.connection().path().unwrap().to_string();
    let shown = format!("{one:?}");
    assert!(shown.starts_with("OneLibrary { path: ") && shown.ends_with("exportLibrary.db\" }"), "{shown}");
    let table = tc.draw(gs::sampled_from(tables(&one)));
    let root: u32 = one
        .connection()
        .query_row("SELECT rootpage FROM sqlite_master WHERE name = ?1", [&table], |r| r.get(0))
        .unwrap();
    let damage = tc.draw(gs::booleans());
    let mut file = std::fs::read(&path).unwrap();
    let counter = u32::from_be_bytes(file[24..28].try_into().unwrap()).wrapping_add(1);
    file[24..28].copy_from_slice(&counter.to_be_bytes());
    if damage {
        let at = (root as usize - 1) * PAGE_SIZE;
        file[at + 3..at + 5].copy_from_slice(&u16::MAX.to_be_bytes());
    }
    std::fs::write(&path, &file).unwrap();
    match one.to_encrypted() {
        Ok(bytes) => {
            assert!(!damage, "a damaged {table} was saved");
            assert_eq!(OneLibrary::from_encrypted(&bytes, None).unwrap().library().unwrap().tracks.len(), 3);
        }
        Err(e) => assert!(damage && e.code() == "onelibrary", "{e}"),
    }
}

/// The content hash leaves out the drive's name only when the property table is one page,
/// whose rows it reads: spread over several pages (many rows), the table is hashed as its pages
/// are, so renaming the drive changes the hash. Re-encrypting never does, with the table or
/// without it.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn a_property_table_of_many_pages_is_hashed_as_it_is(tc: TestCase) {
    let one = OneLibrary::from_encrypted(new_library(), None).unwrap();
    // Without the table every page is hashed as it is, and still not its IVs.
    let none = tc.draw(gs::weighted_booleans(0.2));
    if none {
        one.connection().execute_batch("DROP TABLE property").unwrap();
        let bytes = one.to_encrypted().unwrap();
        let (codec, plain) = onelibrary::decrypt(&bytes, None).unwrap();
        let again = codec.encrypt(&plain).unwrap();
        assert_eq!(
            onelibrary::content_hashes(&again, None).unwrap(),
            onelibrary::content_hashes(&bytes, None).unwrap()
        );
        return;
    }
    let many = tc.draw(gs::booleans());
    // 40 rows of 200 bytes fill two pages; three short ones fit in one.
    let (rows, width) = if many { (40, 200) } else { (tc.draw(gs::integers::<usize>().max_value(2)), 8) };
    for i in 0..rows {
        one.connection()
            .execute(
                "INSERT INTO property (deviceName, dbVersion) SELECT ?1, dbVersion FROM property LIMIT 1",
                [format!("{i:0width$}")],
            )
            .unwrap();
    }
    let before = one.to_encrypted().unwrap();
    let name = tc.draw(gs::text().min_size(1).max_size(12));
    one.connection().execute("UPDATE property SET deviceName = ?1", [&name]).unwrap();
    let after = one.to_encrypted().unwrap();
    let hash = |b: &[u8]| onelibrary::content_hash(b, None).unwrap();
    assert_eq!(hash(&before) == hash(&after), !many, "{rows} rows");
    let (codec, plain) = onelibrary::decrypt(&after, None).unwrap();
    assert_eq!(hash(&codec.encrypt(&plain).unwrap()), hash(&after));
    assert!(Codec::for_file(&after).is_ok());
}

/// Ids are read as players number them, up to 2^32 - 1: a row whose id, or whose reference to
/// another row, lies past that (or below 0) is refused with an error naming it, not wrapped
/// round. A reference that isn't positive (artwork, a parent) means there is none.
#[hegel::test(test_cases = 30, suppress_health_check = [HealthCheck::TooSlow])]
fn ids_players_cant_number_are_refused(tc: TestCase) {
    let id = tc.draw(hegel::one_of!(
        gs::integers::<i64>().max_value(-1),
        gs::integers::<i64>().min_value(i64::from(u32::MAX) + 1)
    ));
    // (statements setting `{id}`, whether a negative one means none, and the reader to try)
    type Reader = fn(&OneLibrary) -> locallibrary::Result<String>;
    let library: Reader = |one| one.library().map(|l| format!("{l:?}"));
    let images: Reader = |one| one.images_used_outside_tracks().map(|i| format!("{i:?}"));
    let banks: Reader = |one| one.hot_cue_bank_lists().map(|b| format!("{b:?}"));
    let cases: [(&str, bool, Reader); 8] = [
        ("UPDATE content SET content_id = {id} WHERE content_id = 3", false, library),
        ("UPDATE image SET image_id = {id}; UPDATE content SET image_id = {id} WHERE content_id = 1", true, library),
        ("UPDATE playlist SET playlist_id_parent = {id} WHERE playlist_id = 2", true, library),
        ("UPDATE playlist_content SET content_id = {id} WHERE content_id = 2", false, library),
        ("UPDATE history SET history_id = {id}", false, library),
        ("UPDATE image SET image_id = {id}; UPDATE album SET image_id = {id}", false, images),
        ("UPDATE hotCueBankList_cue SET cue_id = {id} WHERE cue_id = 2", false, banks),
        (
            "UPDATE hotCueBankList SET hotCueBankList_id = {id}; UPDATE hotCueBankList_cue SET hotCueBankList_id = {id}",
            false,
            banks,
        ),
    ];
    let (sql, negative_is_none, read) = cases[tc.draw(gs::integers::<usize>().max_value(cases.len() - 1))];
    let one = OneLibrary::from_encrypted(rich_library(), None).unwrap();
    one.connection().execute_batch(&sql.replace("{id}", &id.to_string())).unwrap();
    let refused = id > 0 || !negative_is_none;
    match read(&one) {
        Ok(got) => assert!(!refused, "{sql} with {id}: {got}"),
        Err(e) => assert!(refused && e.to_string().contains(&format!("id {id} out of range")), "{sql} with {id}: {e}"),
    }
}

/// A stored value of any type, and what a numeric column holding it reads as.
#[hegel::test_helper]
fn loose_number(tc: &TestCase, max: i64) -> (Value, i64) {
    let n = tc.draw(gs::integers::<i64>().min_value(0).max_value(max));
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => (Value::Integer(n), n),
        1 => (Value::Real(n as f64 + 0.5), n),
        2 => (Value::Text(format!(" {n} ")), n),
        3 => (Value::Text(format!("n{n}")), 0),
        4 => (Value::Blob(n.to_be_bytes().to_vec()), 0),
        _ => (Value::Null, 0),
    }
}

/// A stored value of any type, and what a text column holding it reads as.
#[hegel::test_helper]
fn loose_text(tc: &TestCase) -> (Value, String) {
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => {
            let s = tc.draw(gs::text().max_size(8));
            (Value::Text(s.clone()), s)
        }
        1 => {
            let n = tc.draw(gs::integers::<i64>());
            (Value::Integer(n), n.to_string())
        }
        2 => {
            let f = tc.draw(gs::floats::<f64>().allow_nan(false).allow_infinity(false));
            (Value::Real(f), f.to_string())
        }
        3 => (Value::Blob(vec![0x61]), String::new()),
        _ => (Value::Null, String::new()),
    }
}

/// A drive's columns can hold any type (a column declared without one stores values as given):
/// numbers stored as reals, padded text or anything else read as the number they hold (0 for
/// none), and text stored as numbers reads as their digits.
#[hegel::test(test_cases = 40, suppress_health_check = [HealthCheck::TooSlow])]
fn loosely_typed_values_read_as_their_columns_type(tc: TestCase) {
    let one = OneLibrary::from_encrypted(rich_library(), None).unwrap();
    let numbers = [("bpmx100", 30_000), ("length", 4_000), ("trackNo", 99), ("rating", 5), ("djPlayCount", 900)];
    let texts = ["title", "djComment", "isrc"];
    let mut stored: Vec<(&str, Value)> = Vec::new();
    let mut want_numbers = Vec::new();
    for (column, max) in numbers {
        let (v, n) = loose_number(&tc, max);
        stored.push((column, v));
        want_numbers.push(n);
    }
    let mut want_texts = Vec::new();
    for column in texts {
        let (v, s) = loose_text(&tc);
        stored.push((column, v));
        want_texts.push(s);
    }
    for (column, value) in &stored {
        let c = quoted(column);
        one.connection()
            .execute_batch(&format!("ALTER TABLE content DROP COLUMN {c}; ALTER TABLE content ADD COLUMN {c}"))
            .unwrap();
        one.connection().execute(&format!("UPDATE content SET {c} = ?1 WHERE content_id = 1"), [value]).unwrap();
    }
    let lib = one.library().unwrap();
    let t = lib.track(1).unwrap();
    let got_numbers = [
        (t.bpm * 100.0).round() as i64,
        t.duration_secs.into(),
        t.track_number.into(),
        t.rating.into(),
        t.play_count.into(),
    ];
    assert_eq!(got_numbers.to_vec(), want_numbers, "{stored:?}");
    assert_eq!([t.title.clone(), t.comment.clone(), t.isrc.clone()].to_vec(), want_texts, "{stored:?}");
}
