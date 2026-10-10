//! Read-path tests against real exports, asserting the same hand-verified
//! values as the Python library's test suite.

use std::collections::HashMap;

use rekordbox_pdb::{Database, ExtDatabase, ExtTableType, PdbEditor, PdbError, TableType};

fn one_song() -> Database {
    Database::from_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/one-song-export.pdb"
    ))
    .unwrap()
}

fn bigger() -> Database {
    Database::from_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/bigger-export.pdb"
    ))
    .unwrap()
}

#[test]
fn file_structure() {
    let db = one_song();
    assert_eq!(db.page_size, 4096);
    assert_eq!(db.tables.len(), 20);
    let mut types: Vec<u32> = db.tables.iter().map(|t| t.table_type).collect();
    types.sort();
    assert_eq!(types, (0..20).collect::<Vec<_>>());
}

#[test]
fn track_fixed_fields() {
    let db = one_song();
    assert_eq!(db.tracks.len(), 1);
    let t = &db.tracks[0];
    assert_eq!(t.id, 1);
    assert_eq!(t.tempo, 14600);
    assert_eq!(t.year, 2023);
    assert_eq!(t.duration, 153);
    assert_eq!(t.rating, 4);
    assert_eq!(t.sample_rate, 48000);
    assert_eq!(t.sample_depth, 16);
    assert_eq!(t.bitrate, 128);
    assert_eq!(t.file_size, 2_561_982);
    assert_eq!(t.track_number, 2);
    assert_eq!(t.disc_number, 1);
    assert_eq!(t.artist_id, 1);
    assert_eq!(t.album_id, 1);
    assert_eq!(t.key_id, 1);
    assert_eq!(t.artwork_id, 1);
}

#[test]
fn track_strings() {
    let db = one_song();
    let t = &db.tracks[0];
    assert_eq!(t.title(), "Super Smash Bros.");
    assert_eq!(
        t.filename(),
        "BABY GRAVY, Yung Gravy, bbno$ - Super Smash .mp3"
    );
    assert_eq!(
        t.file_path(),
        "/Contents/BABY GRAVY_Yung Gravy_bbno$/Baby Gravy 3/BABY GRAVY, Yung Gravy, bbno$ - Super Smash .mp3"
    );
    assert_eq!(
        t.analyze_path(),
        "/.PIONEER/USBANLZ/P011/00005719/ANLZ0000.DAT"
    );
    assert_eq!(t.date_added(), "2023-10-21");
    assert_eq!(t.analyze_date(), "2024-02-29");
}

#[test]
fn lookup_tables() {
    let db = one_song();
    let artists: HashMap<u32, &str> = db.artists.iter().map(|a| (a.id, a.name.as_str())).collect();
    assert_eq!(artists.len(), 2);
    assert_eq!(artists[&1], "BABY GRAVY/Yung Gravy/bbno$");
    assert_eq!(artists[&2], "BABY GRAVY");

    assert_eq!(db.albums.len(), 1);
    assert_eq!(db.albums[0].id, 1);
    assert_eq!(db.albums[0].name, "Baby Gravy 3");
    assert_eq!(db.albums[0].artist_id, 2);

    assert_eq!(db.keys.len(), 1);
    assert_eq!(db.keys[0].name, "Em");

    let colors: HashMap<u16, &str> = db.colors.iter().map(|c| (c.id, c.name.as_str())).collect();
    assert_eq!(colors[&1], "Pink");
    assert_eq!(colors[&2], "Red");
    assert_eq!(colors[&3], "Orange");
    assert_eq!(colors[&4], "Yellow");
    assert_eq!(colors[&5], "Green");
    assert_eq!(colors[&6], "Aqua");
    assert_eq!(colors[&7], "Blue");
    assert_eq!(colors[&8], "Purple");
}

#[test]
fn playlists() {
    let db = one_song();
    assert_eq!(db.playlist_tree.len(), 1);
    let node = &db.playlist_tree[0];
    assert_eq!(node.id, 1);
    assert_eq!(node.name, "aac");
    assert_eq!(node.parent_id, 0);
    assert!(!node.is_folder);

    assert_eq!(db.playlist_entries.len(), 1);
    let e = db.playlist_entries[0];
    assert_eq!(e.playlist_id, 1);
    assert_eq!(e.track_id, 1);
    assert_eq!(e.entry_index, 1);
}

#[test]
fn columns_use_utf16() {
    let db = one_song();
    let names: HashMap<u16, &str> = db.columns.iter().map(|c| (c.id, c.name.as_str())).collect();
    assert_eq!(names[&1], "\u{FFFA}GENRE\u{FFFB}");
    assert_eq!(names[&2], "\u{FFFA}ARTIST\u{FFFB}");
}

#[test]
fn row_locations_parallel() {
    let db = one_song();
    assert_eq!(db.row_locations(TableType::Tracks), &[2 * 4096 + 0x210]);
    for ty in [
        TableType::Artists,
        TableType::Colors,
        TableType::PlaylistEntries,
    ] {
        assert_eq!(db.row_locations(ty).len(), db_row_count(&db, ty));
    }
}

fn db_row_count(db: &Database, ty: TableType) -> usize {
    match ty {
        TableType::Artists => db.artists.len(),
        TableType::Colors => db.colors.len(),
        TableType::PlaylistEntries => db.playlist_entries.len(),
        _ => unreachable!(),
    }
}

#[test]
fn bigger_export_smoke() {
    let db = bigger();
    assert!(db.tracks.len() > 10);
    for t in &db.tracks {
        assert!(t.file_path().starts_with('/'));
    }
    let artist_ids: std::collections::HashSet<u32> = db.artists.iter().map(|a| a.id).collect();
    for t in &db.tracks {
        if t.artist_id != 0 {
            assert!(artist_ids.contains(&t.artist_id));
        }
    }
    let track_ids: std::collections::HashSet<u32> = db.tracks.iter().map(|t| t.id).collect();
    for e in &db.playlist_entries {
        assert!(track_ids.contains(&e.track_id));
    }
}

#[test]
fn ext_my_tags() {
    let ext = ExtDatabase::from_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/one-song-exportExt.pdb"
    ))
    .unwrap();
    assert_eq!(ext.tables.len(), 9);

    let cats: HashMap<u32, &str> = ext
        .tags
        .iter()
        .filter(|t| t.is_category)
        .map(|t| (t.id, t.name.as_str()))
        .collect();
    assert_eq!(cats[&1], "Genre");
    assert_eq!(cats[&2], "Components");
    assert_eq!(cats[&3], "Situation");
    assert_eq!(cats[&4], "Untitled Column");

    let acid = ext.tags.iter().find(|t| t.name == "Acid House").unwrap();
    assert_eq!(acid.id, 0x91A5_E419);
    assert_eq!(acid.category_id, 1);

    assert_eq!(ext.tags.len(), 28);
    // every non-category tag belongs to a category
    let cat_ids: std::collections::HashSet<u32> = ext
        .tags
        .iter()
        .filter(|t| t.is_category)
        .map(|t| t.id)
        .collect();
    for tag in &ext.tags {
        if !tag.is_category {
            assert!(cat_ids.contains(&tag.category_id));
        }
    }
    let _ = ExtTableType::Tags;
}

// -- locallibrary patches ----------------------------------------------------

fn one_song_bytes() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/one-song-export.pdb")).unwrap()
}

/// Patch 17: a header that can't be right is an error, not a panic or a huge allocation.
#[test]
fn damaged_headers_are_errors() {
    for data in [vec![], vec![0x90; 0x1b]] {
        assert!(matches!(Database::from_bytes(data.clone()), Err(PdbError::Malformed(_))));
        assert!(matches!(ExtDatabase::from_bytes(data.clone()), Err(PdbError::Malformed(_))));
        assert!(PdbEditor::from_bytes(data).database().is_err());
    }
    let mut data = one_song_bytes();
    data[8..12].copy_from_slice(&u32::MAX.to_le_bytes()); // the table count
    assert!(matches!(Database::from_bytes(data.clone()), Err(PdbError::Malformed(_))));
    assert!(matches!(ExtDatabase::from_bytes(data), Err(PdbError::Malformed(_))));
}

/// Patch 3: a page chain that loops back on itself is walked a bounded number of times, by the
/// reader and by the editor parsing its buffer, instead of for ever.
#[test]
fn a_cyclic_page_chain_ends() {
    let mut data = one_song_bytes();
    let tracks = 0x1c; // the directory's first entry, the tracks table
    let first = u32::from_le_bytes(data[tracks + 8..tracks + 12].try_into().unwrap()) as usize;
    let at = first * 4096 + 12;
    data[at..at + 4].copy_from_slice(&(first as u32).to_le_bytes());
    data[tracks + 12..tracks + 16].copy_from_slice(&u32::MAX.to_le_bytes()); // last page: never reached
    let pages = data.len() / 4096;
    let db = Database::from_bytes(data.clone()).unwrap();
    assert!(db.tracks.len() <= pages + 1);
    assert!(PdbEditor::from_bytes(data).database().is_ok());
}

/// Patch 18: rekordbox writes an ISRC as `0x90` with a body of 3, the ASCII code and a NUL.
#[test]
fn isrcs_read_as_rekordbox_writes_them() {
    let mut ed = PdbEditor::from_bytes(one_song_bytes());
    ed.set_track_strings(1, &[("isrc", "GBAYE6700149"), ("title", "\u{2603} snow")]).unwrap();
    let data = ed.to_bytes();
    // rekordcrate's test vector for this ISRC.
    let stored = b"\x90\x12\x00\x00\x03GBAYE6700149\x00";
    assert!(data.windows(stored.len()).any(|w| w == stored));
    let t = &Database::from_bytes(data).unwrap().tracks[0];
    // Only the ISRC is read that way: a title starting with a byte of 3 stays UTF-16.
    assert_eq!((t.strings[0].as_str(), t.title()), ("GBAYE6700149", "\u{2603} snow"));
}
