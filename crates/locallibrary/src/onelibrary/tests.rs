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
