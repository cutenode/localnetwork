//! Properties of the editor's track rows, read back through the reader's named accessors.

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use rekordbox_pdb::pdb::TRACK_STRING_NAMES;
use rekordbox_pdb::{Database, ExtDatabase, ExtEditor, NewTrack, PdbEditor, PdbError, file_type_code};

const ONE_SONG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/one-song-export.pdb");

/// A file's format code comes from its last extension in any case, and is 0 for an extension
/// rekordbox doesn't list or a name without one.
#[hegel::test(test_cases = 300)]
fn file_type_codes_follow_the_last_extension(tc: TestCase) {
    let codes = [("mp3", 1), ("m4a", 4), ("mp4", 4), ("aac", 4), ("flac", 5), ("wav", 0x0b), ("aiff", 0x0c), ("aif", 0x0c)];
    let (ext, code) = tc.draw(gs::sampled_from(codes.to_vec()));
    let upper: Vec<bool> = tc.draw(gs::vecs(gs::booleans()).min_size(ext.len()).max_size(ext.len()));
    let spelled: String =
        ext.chars().zip(upper).map(|(c, u)| if u { c.to_ascii_uppercase() } else { c }).collect();
    let stem = tc.draw(gs::text().max_size(12));
    assert_eq!(file_type_code(&format!("{stem}.{spelled}")), code);
    assert_eq!(file_type_code(&format!("{stem}.{spelled}.part")), 0);
    let bare = tc.draw(gs::text().max_size(12).filter(|s: &String| !s.contains('.')));
    assert_eq!(file_type_code(&bare), 0);
}

/// A track added with any text in its string fields reads back through the named accessors
/// (and the same strings by position), and setting one string by name changes only that one.
#[hegel::test(test_cases = 100)]
fn track_strings_read_back_by_name(tc: TestCase) {
    let text = || gs::text().max_size(20).exclude_characters("\0");
    let t = NewTrack {
        title: tc.draw(text()),
        file_path: format!("/Contents/{}.mp3", tc.draw(gs::text().alphabet("abc").max_size(8))),
        comment: tc.draw(text()),
        mix_name: tc.draw(text()),
        release_date: tc.draw(text()),
        analyze_path: tc.draw(text()),
        date_added: tc.draw(text()),
        ..Default::default()
    };
    let mut ed = PdbEditor::from_bytes(std::fs::read(ONE_SONG).unwrap());
    let id = ed.add_track(&t).unwrap();
    let field = tc.draw(gs::sampled_from(vec!["message", "analyze_date", "comment", "mix_name"]));
    let value = tc.draw(text());
    ed.set_track_strings(id, &[(field, &value)]).unwrap();

    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let row = db.tracks.iter().find(|r| r.id == id).unwrap();
    let want = |name: &str, default: &str| if name == field { value.clone() } else { default.to_string() };
    let named = [
        ("title", row.title(), t.title.clone()),
        ("comment", row.comment(), t.comment.clone()),
        ("mix_name", row.mix_name(), t.mix_name.clone()),
        ("release_date", row.release_date(), t.release_date.clone()),
        ("analyze_path", row.analyze_path(), t.analyze_path.clone()),
        ("date_added", row.date_added(), t.date_added.clone()),
        ("file_path", row.file_path(), t.file_path.clone()),
        ("filename", row.filename(), t.file_path.rsplit('/').next().unwrap().to_string()),
        ("message", row.message(), String::new()),
        // Analysed when added, as far as the row says.
        ("analyze_date", row.analyze_date(), t.date_added.clone()),
    ];
    for (name, got, written) in named {
        assert_eq!(got, want(name, &written), "{name}");
        let at = TRACK_STRING_NAMES.iter().position(|n| *n == name).unwrap();
        assert_eq!(row.strings[at], got, "{name} is string {at}");
    }
    assert!(matches!(ed.set_track_strings(u32::MAX, &[(field, "x")]), Err(PdbError::TrackNotFound(_))));
}

/// Every refusal says what was refused, naming the value it was given.
#[hegel::test(test_cases = 100)]
fn errors_name_what_they_refused(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>());
    let name = tc.draw(gs::text().max_size(10));
    let errors = [
        (PdbError::UnknownString { kind: n as u8, offset: n as usize }, format!("{:#04x}", n as u8)),
        (PdbError::UnknownField(name.clone()), format!("{name:?}")),
        (PdbError::TrackNotFound(n), n.to_string()),
        (PdbError::PlaylistNotFound(n), n.to_string()),
        (PdbError::RowNotFound(n as usize), format!("{:#x}", n as usize)),
        (PdbError::RowTooLarge { alloc: n as usize, max: 7 }, n.to_string()),
        (PdbError::ValueOutOfRange { field: name.clone(), value: u64::from(n), size: 2 }, format!("{name}={n}")),
        (PdbError::TagNotFound(n), n.to_string()),
        (PdbError::IdInUse(n), n.to_string()),
        (PdbError::Malformed(name.clone()), name.clone()),
        (PdbError::Io(std::io::Error::other(name.clone())), name.clone()),
    ];
    for (e, named) in errors {
        let shown = e.to_string();
        assert!(shown.contains(&named), "{shown:?} doesn't name {named:?}");
        let _: &dyn std::error::Error = &e;
    }
    for e in [PdbError::TooManySlots, PdbError::StringTooLong] {
        assert!(!e.to_string().is_empty());
    }
    let io: PdbError = std::io::Error::other(name.clone()).into();
    assert!(matches!(io, PdbError::Io(_)));
}

// -- crafted and damaged files ------------------------------------------------------------------

const BIGGER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bigger-export.pdb");
const ONE_SONG_EXT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/one-song-exportExt.pdb");
const PAGE: usize = 4096;

/// `s` the way FORMAT.md frames a DeviceSQL string: short ASCII (kind `(len + 1) * 2 + 1`) up to
/// 126 bytes, then long ASCII (`0x40`), and UTF-16LE (`0x90`) for anything else; a long string's
/// u16 length counts its 4-byte header.
fn devicesql(s: &str) -> Vec<u8> {
    if s.is_ascii() && s.len() <= 126 {
        return [vec![((s.len() + 1) * 2 + 1) as u8], s.as_bytes().to_vec()].concat();
    }
    let (kind, body) = match s.is_ascii() {
        true => (0x40, s.as_bytes().to_vec()),
        false => (0x90, s.encode_utf16().flat_map(u16::to_le_bytes).collect()),
    };
    [vec![kind], ((body.len() + 4) as u16).to_le_bytes().to_vec(), vec![0], body].concat()
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn put_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// Where table `ty`'s entry sits in the file header's directory.
fn directory_entry(b: &[u8], ty: u32) -> usize {
    (0..u32_at(b, 8) as usize).map(|i| 0x1c + i * 16).find(|&o| u32_at(b, o) == ty).unwrap()
}

/// A name for a raw row: short or long ASCII, or UTF-16.
#[hegel::test_helper]
fn raw_name(tc: &TestCase) -> String {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => tc.draw(gs::text().alphabet("abcdefgh XYZ").min_size(1).max_size(20)),
        1 => "n".repeat(tc.draw(gs::integers::<usize>().min_value(127).max_value(400))),
        _ => tc.draw(gs::text().min_size(1).max_size(30).filter(|s: &String| !s.is_ascii())),
    }
}

/// Several genre, label, key or album rows sharing a name (an album's by the same artist), added
/// behind the editor's back: a lookup by that name finds one of them, and keeps finding the same
/// one while others go; when the one it finds is removed it finds another, and only once they're
/// all gone does it add a row.
#[hegel::test(test_cases = 100)]
fn lookups_fall_back_to_another_row_of_the_same_name(tc: TestCase) {
    use rekordbox_pdb::TableType as T;
    let tables = [T::Genres, T::Labels, T::Keys, T::Albums];
    let table = tables[tc.draw(gs::integers::<usize>().max_value(3))];
    let name = raw_name(&tc);
    let artist = tc.draw(gs::integers::<u32>().max_value(3));
    let lookup = |ed: &mut PdbEditor| match table {
        T::Genres => ed.genre_id_for(&name),
        T::Labels => ed.label_id_for(&name),
        T::Keys => ed.key_id_for(&name),
        _ => ed.album_id_for(&name, artist),
    };
    let mut ids: Vec<u32> =
        tc.draw(gs::vecs(gs::integers::<u32>().min_value(100).max_value(10_000)).min_size(2).max_size(5).unique(true));
    let mut ed = PdbEditor::from_bytes(std::fs::read(BIGGER).unwrap());
    for &id in &ids {
        let encoded = devicesql(&name);
        // Rows as FORMAT.md lays them out: a key holds its id twice before the name; an album has
        // a header, its artist and id, and the name's offset at 0x15 (a long name 4-aligned).
        let mut row = match table {
            T::Keys => [id.to_le_bytes(), id.to_le_bytes()].concat(),
            T::Albums => {
                let at = if encoded[0] & 1 == 0 { 24 } else { 22 };
                let mut row = vec![0x80, 0x00, 0x00, 0x00, 0, 0, 0, 0];
                row.extend_from_slice(&artist.to_le_bytes());
                row.extend_from_slice(&id.to_le_bytes());
                row.extend_from_slice(&[0, 0, 0, 0, 0x03, at]);
                row.resize(at as usize, 0);
                row
            }
            _ => id.to_le_bytes().to_vec(),
        };
        row.extend_from_slice(&encoded);
        let alloc = row.len().next_multiple_of(4);
        ed.append_raw_row(table as u32, &row, alloc, (table == T::Albums).then_some(2)).unwrap();
    }
    let all = ids.clone();
    let mut found = lookup(&mut ed).unwrap();
    assert!(ids.contains(&found), "{found} isn't one of {ids:?}");
    while !ids.is_empty() {
        let gone = tc.draw(gs::sampled_from(ids.clone()));
        assert_eq!(ed.remove_rows(table, &[gone]).unwrap(), 1);
        ids.retain(|&i| i != gone);
        let now = lookup(&mut ed).unwrap();
        if ids.is_empty() {
            assert!(!all.contains(&now), "{now} was removed");
        } else if gone == found {
            assert!(ids.contains(&now), "{now} isn't one of {ids:?}");
        } else {
            assert_eq!(now, found, "the lookup moved off a row that's still there");
        }
        found = now;
    }
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let named: Vec<u32> = match table {
        T::Genres => db.genres.iter().filter(|g| g.name == name).map(|g| g.id).collect(),
        T::Labels => db.labels.iter().filter(|l| l.name == name).map(|l| l.id).collect(),
        T::Keys => db.keys.iter().filter(|k| k.name == name).map(|k| k.id).collect(),
        _ => db.albums.iter().filter(|a| a.name == name && a.artist_id == artist).map(|a| a.id).collect(),
    };
    assert_eq!(named, [found]);
}

/// A table's next free page is used when its last page leads to it and it lies at most 64
/// pages past the end of the file (the file grows to hold it); otherwise the edit that needs it
/// is refused as malformed, naming the table and the page, and the file is left as it was.
#[hegel::test(test_cases = 100)]
fn a_free_page_out_of_place_is_refused(tc: TestCase) {
    let orig = std::fs::read(ONE_SONG).unwrap();
    let pages = orig.len() / PAGE;
    // History playlists (11) and genres (1) are empty in this file: the first row needs a page.
    let ty = tc.draw(gs::sampled_from(vec![1u32, 11]));
    let entry = directory_entry(&orig, ty);
    let last = u32_at(&orig, entry + 12) as usize;
    let candidate = tc.draw(hegel::one_of!(
        gs::integers::<u32>().min_value(pages as u32).max_value(pages as u32 + 70),
        gs::integers::<u32>().min_value(pages as u32)
    ));
    let linked = tc.draw(gs::booleans());
    let mut data = orig.clone();
    put_u32(&mut data, entry + 4, candidate);
    if linked {
        put_u32(&mut data, last * PAGE + 0x0c, candidate);
    }
    let mut ed = PdbEditor::from_bytes(data.clone());
    let name = tc.draw(gs::text().max_size(10));
    let result = if ty == 1 { ed.genre_id_for(&name) } else { ed.create_history_playlist(&name) };
    if linked && candidate as usize <= pages + 64 {
        let id = result.unwrap();
        let out = ed.to_bytes();
        assert_eq!(out.len(), (candidate as usize + 1) * PAGE);
        let db = Database::from_bytes(out).unwrap();
        let rows: Vec<(u32, &str)> = match ty {
            1 => db.genres.iter().map(|g| (g.id, g.name.as_str())).collect(),
            _ => db.history_playlists.iter().map(|h| (h.id, h.name.as_str())).collect(),
        };
        assert_eq!(rows, [(id, name.as_str())]);
        assert_eq!(
            db.row_locations(rekordbox_pdb::TableType::Tracks),
            Database::from_bytes(orig).unwrap().row_locations(rekordbox_pdb::TableType::Tracks)
        );
    } else {
        let Err(PdbError::Malformed(why)) = result else { panic!("{result:?}") };
        assert!(why.contains(&format!("table {ty}")) && why.contains(&format!("({candidate})")), "{why}");
        assert_eq!(ed.to_bytes(), data);
    }
}

/// A row's string rewritten as an empty long string (`0x40` or `0x90`, length 4) reads back as
/// empty, in every table and every one of a track's string slots (its ISRC too); any other even
/// kind byte is refused, naming the kind and where the string starts.
#[hegel::test(test_cases = 150)]
fn strings_of_an_unknown_kind_are_refused(tc: TestCase) {
    use rekordbox_pdb::TableType as T;
    let mut ed = PdbEditor::from_bytes(std::fs::read(ONE_SONG).unwrap());
    // A row in every table with names of 3 or more characters, so each string takes 4 bytes or
    // more and the rewrite stays inside it.
    let word = || tc.draw(gs::text().alphabet("abcdef").min_size(3).max_size(8));
    let mut t = NewTrack::new(word(), format!("/Contents/{}.mp3", word()));
    (t.comment, t.mix_name, t.release_date, t.analyze_path, t.date_added) = (word(), word(), word(), word(), word());
    (t.artist, t.album, t.genre, t.key, t.label) =
        (Some(word()), Some(word()), Some(word()), Some(word()), Some(word()));
    let track = ed.add_track(&t).unwrap();
    ed.set_track_strings(track, &[("isrc", &word())]).unwrap();
    ed.create_playlist(&word(), 0, false).unwrap();
    ed.create_history_playlist(&word()).unwrap();
    ed.add_artwork(&word()).unwrap();
    let bytes = ed.to_bytes();
    let db = Database::from_bytes(bytes.clone()).unwrap();
    let tables = [
        T::Tracks,
        T::Genres,
        T::Artists,
        T::Albums,
        T::Labels,
        T::Keys,
        T::Colors,
        T::PlaylistTree,
        T::HistoryPlaylists,
        T::Artwork,
        T::Columns,
    ];
    for table in tables {
        let locs = db.row_locations(table);
        // The track the editor added (the fixture's own has short strings).
        let i = if table == T::Tracks {
            locs.len() - 1
        } else {
            tc.draw(gs::integers::<usize>().max_value(locs.len() - 1))
        };
        // The ISRC half the time: it's decoded apart from a track's other strings.
        let slot = match tc.draw(gs::booleans()) {
            true => 0,
            false => tc.draw(gs::sampled_from(vec![10, 11, 12, 14, 15, 16, 17, 19, 20])),
        };
        let row = locs[i];
        let at = row
            + match table {
                T::Tracks => u16::from_le_bytes([bytes[row + 0x5e + 2 * slot], bytes[row + 0x5f + 2 * slot]]) as usize,
                T::Artists => bytes[row + 9] as usize,
                T::Albums => bytes[row + 0x15] as usize,
                T::Keys | T::Colors => 8,
                T::PlaylistTree => 20,
                _ => 4,
            };
        let kind = match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => 0x40,
            1 => 0x90,
            _ => tc.draw(gs::integers::<u8>()) & !1,
        };
        let mut data = bytes.clone();
        data[at..at + 4].copy_from_slice(&[kind, 4, 0, 0]);
        let read = Database::from_bytes(data);
        if kind != 0x40 && kind != 0x90 {
            let Err(e @ PdbError::UnknownString { kind: k, offset }) = read else {
                panic!("{table:?}: kind {kind:#04x} read")
            };
            assert_eq!((k, offset), (kind, at));
            assert!(e.to_string().contains(&format!("{at:#x}")), "{e}");
            continue;
        }
        let names = |db: &Database| -> Vec<String> {
            match table {
                T::Tracks => db.tracks.iter().map(|t| t.strings[slot].clone()).collect(),
                T::Genres => db.genres.iter().map(|r| r.name.clone()).collect(),
                T::Artists => db.artists.iter().map(|r| r.name.clone()).collect(),
                T::Albums => db.albums.iter().map(|r| r.name.clone()).collect(),
                T::Labels => db.labels.iter().map(|r| r.name.clone()).collect(),
                T::Keys => db.keys.iter().map(|r| r.name.clone()).collect(),
                T::Colors => db.colors.iter().map(|r| r.name.clone()).collect(),
                T::PlaylistTree => db.playlist_tree.iter().map(|r| r.name.clone()).collect(),
                T::HistoryPlaylists => db.history_playlists.iter().map(|r| r.name.clone()).collect(),
                T::Artwork => db.artwork.iter().map(|r| r.path.clone()).collect(),
                _ => db.columns.iter().map(|r| r.name.clone()).collect(),
            }
        };
        let mut want = names(&db);
        want[i] = String::new();
        assert_eq!(names(&read.unwrap()), want, "{table:?}");
    }
}

/// An artist row with a two-byte name offset (subtype `0x64`, for a name past byte 255) reads
/// its name from there, in any of DeviceSQL's string framings, and lookups find it by name.
#[hegel::test(test_cases = 100)]
fn far_artist_rows_read_their_name_from_the_two_byte_offset(tc: TestCase) {
    let name = raw_name(&tc);
    let id = tc.draw(gs::integers::<u32>().min_value(3).max_value(1000));
    let at = tc.draw(gs::integers::<usize>().min_value(0x0c).max_value(0x300));
    let mut row = vec![0x64, 0x00, 0x00, 0x00];
    row.extend_from_slice(&id.to_le_bytes());
    // The one-byte offset (byte 9) is a decoy: a far row doesn't use it.
    row.extend_from_slice(&[0x03, 0x0c]);
    row.extend_from_slice(&(at as u16).to_le_bytes());
    row.resize(at, 0xee);
    row.extend_from_slice(&devicesql(&name));
    let mut ed = PdbEditor::from_bytes(std::fs::read(ONE_SONG).unwrap());
    let alloc = row.len().next_multiple_of(4);
    ed.append_raw_row(2, &row, alloc, Some(2)).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let artist = db.artists.iter().find(|a| a.id == id).unwrap();
    assert_eq!(artist.name, name);
    assert_eq!(ed.artist_id_for(&name).unwrap(), id);
}

/// A data page whose header counts no row slots holds no rows, whatever its row directory says:
/// only the rows on that page are gone from its table.
#[hegel::test(test_cases = 100)]
fn pages_that_count_no_slots_hold_no_rows(tc: TestCase) {
    use rekordbox_pdb::TableType as T;
    let orig = std::fs::read(tc.draw(gs::sampled_from(vec![ONE_SONG, BIGGER]))).unwrap();
    let before = Database::from_bytes(orig.clone()).unwrap();
    let tables =
        [T::Tracks, T::Artists, T::Albums, T::Keys, T::Colors, T::PlaylistTree, T::PlaylistEntries, T::Columns];
    let table = tables[tc.draw(gs::integers::<usize>().max_value(tables.len() - 1))];
    let locs = before.row_locations(table);
    let page = locs[tc.draw(gs::integers::<usize>().max_value(locs.len() - 1))] / PAGE;
    let mut data = orig;
    data[page * PAGE + 0x18] = 0;
    data[page * PAGE + 0x19] &= !1;
    let after = Database::from_bytes(data).unwrap();
    let kept: Vec<usize> = locs.iter().copied().filter(|l| l / PAGE != page).collect();
    assert_eq!(after.row_locations(table), kept);
    for other in tables.into_iter().filter(|&t| t != table) {
        assert_eq!(after.row_locations(other), before.row_locations(other));
    }
}

/// A file too short for its header, or whose table directory doesn't fit in its first page or
/// the file, is refused as malformed by the readers and the editors (a `PdbEditor` at its first
/// edit); a directory of fewer tables than the file has is read. A missing file is an I/O error.
#[hegel::test(test_cases = 200)]
fn damaged_headers_are_refused(tc: TestCase) {
    let ext = tc.draw(gs::booleans());
    let orig = std::fs::read(if ext { ONE_SONG_EXT } else { ONE_SONG }).unwrap();
    let real_tables = u32_at(&orig, 8);
    let mut data = orig.clone();
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => data.truncate(tc.draw(gs::integers::<usize>().max_value(0x1b))),
        // Just the header: room for no table at all.
        3 => {
            data.truncate(0x1c);
            put_u32(&mut data, 8, tc.draw(gs::integers::<u32>().max_value(1)));
        }
        1 => put_u32(
            &mut data,
            8,
            tc.draw(hegel::one_of!(gs::integers::<u32>().max_value(real_tables), gs::integers::<u32>().min_value(255))),
        ),
        _ => put_u32(&mut data, 4, tc.draw(gs::integers::<u32>().max_value(0x1c + 16 * real_tables - 1))),
    }
    let damaged =
        data.len() < 0x1c || 0x1c + 16 * u32_at(&data, 8) as usize > (u32_at(&data, 4) as usize).min(data.len());
    let malformed = |e: PdbError| assert!(matches!(e, PdbError::Malformed(_)), "{e}");
    match (damaged, ext) {
        (true, false) => {
            malformed(Database::from_bytes(data.clone()).err().unwrap());
            // Every edit parses the file first.
            type Edit = fn(&mut PdbEditor) -> Result<(), PdbError>;
            let edits: [Edit; 22] = [
                |ed| ed.add_artwork("a").map(drop),
                |ed| ed.add_track(&NewTrack::new("t", "/t.mp3")).map(drop),
                |ed| ed.set_track_field(1, "rating", 1),
                |ed| ed.set_track_strings(1, &[("title", "t")]),
                |ed| ed.artist_id_for("a").map(drop),
                |ed| ed.album_id_for("a", 0).map(drop),
                |ed| ed.genre_id_for("a").map(drop),
                |ed| ed.remove_tracks(&[1]),
                |ed| ed.remove_rows(rekordbox_pdb::TableType::Genres, &[1]).map(drop),
                |ed| ed.create_history_playlist("h").map(drop),
                |ed| ed.add_to_history(1, 1),
                |ed| ed.remove_history_playlists(&[1]),
                |ed| ed.create_playlist("p", 0, false).map(drop),
                |ed| ed.create_playlist("p", 1, false).map(drop),
                |ed| ed.rename_playlist(1, "p"),
                |ed| ed.move_playlist(1, 0, 0),
                |ed| ed.set_sort_order(1, 0),
                |ed| ed.remove_playlists(&[1]),
                |ed| ed.add_to_playlist(1, 1, None),
                |ed| ed.set_playlist_entries(1, &[1]),
                |ed| ed.repoint_track(1, 1).map(drop),
                |ed| ed.database().map(drop),
            ];
            for edit in edits {
                let mut ed = PdbEditor::from_bytes(data.clone());
                malformed(edit(&mut ed).unwrap_err());
                assert_eq!(ed.to_bytes(), data);
            }
        }
        (true, true) => {
            malformed(ExtDatabase::from_bytes(data.clone()).err().unwrap());
            malformed(ExtEditor::from_bytes(data).err().unwrap());
        }
        (false, false) => assert_eq!(Database::from_bytes(data.clone()).unwrap().tables.len() as u32, u32_at(&data, 8)),
        (false, true) => {
            assert_eq!(ExtDatabase::from_bytes(data.clone()).unwrap().tables.len() as u32, u32_at(&data, 8))
        }
    }
    let missing = std::env::temp_dir().join(format!("rekordbox-pdb-missing-{}", tc.draw(gs::integers::<u64>())));
    let inside = missing.join("export.pdb");
    assert!(matches!(PdbEditor::from_bytes(orig.clone()).save(&inside), Err(PdbError::Io(_))));
    assert!(matches!(
        ExtEditor::from_bytes(std::fs::read(ONE_SONG_EXT).unwrap()).unwrap().save(&inside),
        Err(PdbError::Io(_))
    ));
    assert!(matches!(Database::from_file(&missing), Err(PdbError::Io(_))));
    assert!(matches!(PdbEditor::from_file(&missing), Err(PdbError::Io(_))));
    assert!(matches!(ExtDatabase::from_file(&missing), Err(PdbError::Io(_))));
    assert!(matches!(ExtEditor::from_file(&missing), Err(PdbError::Io(_))));
}

// -- My Tags (`exportExt.pdb`) --------------------------------------------------------------------

/// A category's id and name, and its tags' (id, name).
type Category = (u32, String, Vec<(u32, String)>);

/// The categories and their tags, as the reader sees them.
fn my_tags(db: &ExtDatabase) -> Vec<Category> {
    let tags = |c: u32| db.tags_in(c).iter().map(|t| (t.id, t.name.clone())).collect();
    db.categories().iter().map(|c| (c.id, c.name.clone(), tags(c.id))).collect()
}

/// A My Tag row as the module docs lay it out: subtype `0x0680`, index shift, 8 unknown bytes,
/// category, position, id, `raw_is_category`, the offsets (`0x03`, name, empty string) and the
/// strings.
fn tag_row(name: &str, category: u32, position: u32, id: u32) -> Vec<u8> {
    let mut row = vec![0x80, 0x06, 0, 0];
    row.extend_from_slice(&[0; 8]);
    for v in [category, position, id, if category == 0 { 0x0100_0000 } else { 0 }] {
        row.extend_from_slice(&v.to_le_bytes());
    }
    let encoded = devicesql(name);
    row.extend_from_slice(&[0x03, 0x1f, (0x1f + encoded.len()) as u8]);
    row.extend_from_slice(&encoded);
    row.push(0x03);
    row
}

/// My Tags saved to a file open again (in a reader and an editor) as the editor had them, with
/// the same links, and the file holds what `to_bytes` gives.
#[hegel::test(test_cases = 60)]
fn my_tags_saved_to_a_file_read_back(tc: TestCase) {
    let mut ed = ExtEditor::from_file(ONE_SONG_EXT).unwrap();
    let mut links = Vec::new();
    for i in 0..tc.draw(gs::integers::<u32>().max_value(4)) {
        let name = raw_name(&tc);
        let category = tc.draw(gs::integers::<u32>().min_value(1).max_value(5));
        match category {
            5 if ed.categories().iter().all(|c| c.0 != 5) => ed.add_category(5, &name).unwrap(),
            c => {
                let id = 1000 + i;
                ed.add_tag(c.min(4), id, &name).unwrap();
                let track = tc.draw(gs::integers::<u32>().min_value(1).max_value(50));
                assert!(ed.link(id, track).unwrap());
                links.push((id, track));
            }
        }
    }
    let path = std::env::temp_dir().join(format!(
        "rekordbox-pdb-ext-{}-{}.pdb",
        std::process::id(),
        tc.draw(gs::integers::<u64>())
    ));
    ed.save(&path).unwrap();
    let (db, reopened) = (ExtDatabase::from_file(&path), ExtEditor::from_file(&path));
    let saved = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let (db, reopened) = (db.unwrap(), reopened.unwrap());
    assert_eq!(saved, ed.to_bytes().unwrap());
    let categories: Vec<(u32, String)> = my_tags(&db).into_iter().map(|c| (c.0, c.1)).collect();
    let in_editor: Vec<(u32, String)> = ed.categories().into_iter().map(|(id, n)| (id, n.to_string())).collect();
    assert_eq!(categories, in_editor);
    assert_eq!(reopened.categories(), ed.categories());
    for (id, _) in &in_editor {
        assert_eq!(reopened.tags(*id), ed.tags(*id));
    }
    let mut read: Vec<(u32, u32)> = db.tag_tracks.iter().map(|l| (l.tag_id, l.track_id)).collect();
    read.sort_unstable();
    links.sort_unstable();
    assert_eq!(read, links);
    assert!(links.iter().all(|&(tag, track)| reopened.is_linked(tag, track)));
}

/// A file whose directory has no tag tracks table reads with no links, and linking a tag is
/// refused as `NoTable(4)` while every other edit still saves.
#[hegel::test(test_cases = 60)]
fn linking_without_a_tag_tracks_table_is_refused(tc: TestCase) {
    let mut data = std::fs::read(ONE_SONG_EXT).unwrap();
    let entry = directory_entry(&data, 4);
    put_u32(&mut data, entry, tc.draw(gs::integers::<u32>().min_value(20)));
    let db = ExtDatabase::from_bytes(data.clone()).unwrap();
    assert!(!db.has_table(rekordbox_pdb::ExtTableType::TagTracks) && db.tag_tracks.is_empty());
    let mut ed = ExtEditor::from_bytes(data).unwrap();
    let (tag, _) = ed.tags(1)[tc.draw(gs::integers::<usize>().max_value(ed.tags(1).len() - 1))];
    let Err(e) = ed.link(tag, tc.draw(gs::integers::<u32>())) else { panic!("linked without a table") };
    assert!(matches!(e, PdbError::NoTable(4)) && e.to_string().contains('4'), "{e}");
    let name = raw_name(&tc);
    ed.rename(tag, &name).unwrap();
    let db = ExtDatabase::from_bytes(ed.to_bytes().unwrap()).unwrap();
    assert_eq!(db.tags.iter().find(|t| t.id == tag).unwrap().name, name);
}

/// A second category row with an id already in use stays as it is through an edit: the editor
/// lists the id once, as the row placed first, and the file keeps both rows' names.
#[hegel::test(test_cases = 60)]
fn a_second_category_row_with_its_id_stays_as_it_is(tc: TestCase) {
    let mut raw = PdbEditor::from_bytes(std::fs::read(ONE_SONG_EXT).unwrap());
    let id = tc.draw(gs::integers::<u32>().min_value(1).max_value(4));
    let position = tc.draw(gs::integers::<u32>().max_value(6));
    let name = tc.draw(gs::text().alphabet("xyz").min_size(1).max_size(30));
    let row = tag_row(&name, 0, position, id);
    raw.append_raw_row(3, &row, row.len().next_multiple_of(4) + 8, Some(2)).unwrap();
    let data = raw.to_bytes();
    let before = ExtDatabase::from_bytes(data.clone()).unwrap();
    let original = before.categories().into_iter().find(|c| c.id == id && c.name != name).unwrap().clone();
    let mut ed = ExtEditor::from_bytes(data).unwrap();
    let first = if position < original.position { &name } else { &original.name };
    let listed: Vec<&str> = ed.categories().iter().filter(|c| c.0 == id).map(|c| c.1).collect();
    assert_eq!(listed, [first.as_str()]);
    // An edit elsewhere rewrites the categories it holds.
    let other = (1..=4).find(|&c| c != id).unwrap();
    ed.rename(other, "Renamed").unwrap();
    let after = ExtDatabase::from_bytes(ed.to_bytes().unwrap()).unwrap();
    let mut names: Vec<&str> = after.tags.iter().filter(|t| t.id == id).map(|t| t.name.as_str()).collect();
    names.sort_unstable();
    let mut want = vec![name.as_str(), original.name.as_str()];
    want.sort_unstable();
    assert_eq!(names, want);
}

/// When rewriting My Tags needs a page the file's directory has out of place, saving fails as
/// malformed; with the directory intact the same edits save.
#[hegel::test(test_cases = 30)]
fn my_tags_needing_a_page_out_of_place_fail_to_save(tc: TestCase) {
    let orig = std::fs::read(ONE_SONG_EXT).unwrap();
    let corrupt = tc.draw(gs::booleans());
    let mut data = orig.clone();
    if corrupt {
        let entry = directory_entry(&data, 3);
        put_u32(&mut data, entry + 4, tc.draw(gs::integers::<u32>().min_value(1000)));
    }
    let mut ed = ExtEditor::from_bytes(data).unwrap();
    // More than a page of new categories.
    let count = tc.draw(gs::integers::<u32>().min_value(3).max_value(6));
    for id in 0..count {
        ed.add_category(10 + id, &"c".repeat(tc.draw(gs::integers::<usize>().min_value(1500).max_value(3000))))
            .unwrap();
    }
    let path = std::env::temp_dir().join(format!(
        "rekordbox-pdb-ext-{}-{}.pdb",
        std::process::id(),
        tc.draw(gs::integers::<u64>())
    ));
    let saved = ed.save(&path).map(|()| std::fs::read(&path).unwrap());
    let _ = std::fs::remove_file(&path);
    match saved {
        Ok(saved) => {
            assert!(!corrupt);
            assert_eq!(ExtDatabase::from_bytes(saved).unwrap().categories().len() as u32, 4 + count);
        }
        Err(e) => {
            assert!(corrupt && matches!(&e, PdbError::Malformed(why) if why.contains("table 3")), "{e}");
            assert!(!path.exists(), "a file that failed to save was written");
        }
    }
}

/// A category or tag name is refused up front exactly when DeviceSQL can't frame it
/// (`StringTooLong`) or its row can't fit on a page (`RowTooLarge`), whether added or renamed
/// to, as is an id in use (or 0) and a category or tag that isn't there; refused edits leave the
/// file as it was.
#[hegel::test(test_cases = 100)]
fn my_tags_refuse_names_and_ids_they_cant_hold(tc: TestCase) {
    let orig = std::fs::read(ONE_SONG_EXT).unwrap();
    let mut ed = ExtEditor::from_bytes(orig.clone()).unwrap();
    let ascii = tc.draw(gs::booleans());
    // Around a page's worth of name, or around the longest DeviceSQL can frame.
    let len = match (tc.draw(gs::booleans()), ascii) {
        (true, true) => tc.draw(gs::integers::<usize>().min_value(3900).max_value(4100)),
        (true, false) => tc.draw(gs::integers::<usize>().min_value(1950).max_value(2050)),
        (false, true) => 0x7fff - 4 + 3 - tc.draw(gs::integers::<usize>().max_value(6)),
        (false, false) => (0x7fff - 4) / 2 + 3 - tc.draw(gs::integers::<usize>().max_value(6)),
    };
    let name = if ascii { "x".repeat(len) } else { "ü".repeat(len) };
    let bytes = if ascii { len + 4 } else { 2 * len + 4 };
    let id = tc.draw(gs::integers::<u32>().min_value(5));
    tc.assume(!ed.id_in_use(id));
    let edit = tc.draw(gs::integers::<u8>().max_value(2));
    let result = match edit {
        0 => ed.add_category(id, &name),
        1 => ed.add_tag(1, id, &name),
        _ => ed.rename(1, &name),
    };
    match result {
        // The row: 0x1f to 0x24 bytes before the name (by the width of its offsets and the
        // name's alignment), the empty string after it, padding to 4 bytes and 8 spare ones.
        Ok(()) => assert!(bytes + 0x1f + 9 <= 4050, "a {bytes}-byte name was taken"),
        Err(PdbError::StringTooLong) => assert!(bytes > 0x7fff),
        Err(PdbError::RowTooLarge { alloc, max }) => {
            assert!(bytes <= 0x7fff && alloc > max && bytes + 0x24 + 12 > 4050)
        }
        Err(e) => panic!("{e}"),
    }
    assert_eq!(ed.check_name(&name).is_ok(), result.is_ok());
    let mut fresh = ExtEditor::from_bytes(orig.clone()).unwrap();
    let taken = tc.draw(gs::sampled_from(vec![0, 1, 2, 3, 4]));
    assert!(matches!(fresh.add_category(taken, "x"), Err(PdbError::IdInUse(i)) if i == taken));
    assert!(matches!(fresh.add_tag(1, taken, "x"), Err(PdbError::IdInUse(i)) if i == taken));
    assert!(matches!(fresh.add_tag(id, id, "x"), Err(PdbError::TagNotFound(c)) if c == id));
    assert!(matches!(fresh.rename(id, "x"), Err(PdbError::TagNotFound(t)) if t == id));
    assert_eq!(fresh.to_bytes().unwrap(), orig);
}

/// A My Tag row whose name has an unknown (even) string kind is refused, by the reader and the
/// editor, naming the kind and where the string starts.
#[hegel::test(test_cases = 100)]
fn tags_named_in_an_unknown_kind_are_refused(tc: TestCase) {
    let mut data = std::fs::read(ONE_SONG_EXT).unwrap();
    let db = ExtDatabase::from_bytes(data.clone()).unwrap();
    let locs = db.row_locations(rekordbox_pdb::ExtTableType::Tags);
    let row = locs[tc.draw(gs::integers::<usize>().max_value(locs.len() - 1))];
    // These rows are near ones: the name's offset is the byte at 0x1d.
    let at = row + data[row + 0x1d] as usize;
    let kind = tc.draw(gs::integers::<u8>().filter(|k| k & 1 == 0 && *k != 0x40 && *k != 0x90));
    data[at..at + 4].copy_from_slice(&[kind, 4, 0, 0]);
    for e in [ExtDatabase::from_bytes(data.clone()).err(), ExtEditor::from_bytes(data).err()] {
        assert!(matches!(e, Some(PdbError::UnknownString { kind: k, offset }) if k == kind && offset == at), "{e:?}");
    }
}

// -- damaged rows and strings ---------------------------------------------------------------------

/// Regression: a long string (`0x40` or `0x90`) whose u16 length was under its own 4-byte header,
/// or ran past the end of the file, panicked the reader slicing it. Now such a header is refused as
/// `Malformed`, by the readers and the editors, and a length that fits reads that many bytes.
#[hegel::test(test_cases = 200)]
fn long_string_lengths_are_checked(tc: TestCase) {
    use rekordbox_pdb::TableType as T;
    let ext = tc.draw(gs::booleans());
    let mut data = std::fs::read(if ext { ONE_SONG_EXT } else { ONE_SONG }).unwrap();
    // Where a name starts: a My Tag's, or a track's title, a playlist's or a key's.
    let at = if ext {
        let db = ExtDatabase::from_bytes(data.clone()).unwrap();
        let locs = db.row_locations(rekordbox_pdb::ExtTableType::Tags);
        let row = locs[tc.draw(gs::integers::<usize>().max_value(locs.len() - 1))];
        row + data[row + 0x1d] as usize
    } else {
        let db = Database::from_bytes(data.clone()).unwrap();
        let table = tc.draw(gs::sampled_from(vec![T::Tracks, T::PlaylistTree, T::Keys]).print_as_debug());
        let locs = db.row_locations(table);
        let row = locs[tc.draw(gs::integers::<usize>().max_value(locs.len() - 1))];
        row + match table {
            T::Tracks => u16::from_le_bytes([data[row + 0x5e + 2 * 17], data[row + 0x5f + 2 * 17]]) as usize,
            T::PlaylistTree => 20,
            _ => 8,
        }
    };
    let kind = tc.draw(gs::sampled_from(vec![0x40u8, 0x90]));
    let room = data.len() - at;
    let total = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => tc.draw(gs::integers::<usize>().max_value(3)),
        2 if room < 0xffff => tc.draw(gs::integers::<usize>().min_value(room + 1).max_value(0xffff)),
        _ => tc.draw(gs::integers::<usize>().min_value(4).max_value(room.min(0xffff))),
    };
    data[at] = kind;
    data[at + 1..at + 3].copy_from_slice(&(total as u16).to_le_bytes());
    let fits = (4..=room).contains(&total);
    let results = if ext {
        vec![ExtDatabase::from_bytes(data.clone()).err(), ExtEditor::from_bytes(data).err()]
    } else {
        vec![Database::from_bytes(data.clone()).err(), PdbEditor::from_bytes(data).database().err()]
    };
    for e in results {
        match e {
            None => assert!(fits, "a {total}-byte string read with {room} bytes left"),
            Some(PdbError::Malformed(what)) => assert!(!fits, "{what}"),
            Some(e) => panic!("{e}"),
        }
    }
}

/// Regression: the readers trusted every offset and length in a file, so a damaged one panicked
/// them. Any bytes, a real file cut short anywhere, or one with bytes of its rows or pages
/// overwritten either read or fail as `Malformed` or `UnknownString`, never panic: the readers,
/// `PdbEditor`'s first parse and `ExtEditor` alike.
#[hegel::test(test_cases = 400)]
fn damaged_files_never_panic_the_readers(tc: TestCase) {
    let ext = tc.draw(gs::booleans());
    let orig = std::fs::read(match ext {
        true => ONE_SONG_EXT,
        false => tc.draw(gs::sampled_from(vec![ONE_SONG, BIGGER])),
    })
    .unwrap();
    let data = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => tc.draw(gs::binary().max_size(3 * PAGE)),
        1 => orig[..tc.draw(gs::integers::<usize>().max_value(orig.len()))].to_vec(),
        _ => {
            // Overwrites near the start of a page (its header, then rows) or near its end (the row
            // directory), where the offsets and lengths are.
            let mut data = orig.clone();
            for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(8)) {
                let page = tc.draw(gs::integers::<usize>().max_value(data.len() / PAGE - 1));
                let within = tc.draw(hegel::one_of!(
                    gs::integers::<usize>().max_value(0x200),
                    gs::integers::<usize>().min_value(PAGE - 0x50).max_value(PAGE - 1),
                ));
                let bytes = tc.draw(gs::binary().min_size(1).max_size(4));
                for (i, b) in bytes.into_iter().enumerate() {
                    if let Some(at) = data.get_mut(page * PAGE + within + i) {
                        *at = b;
                    }
                }
            }
            data
        }
    };
    let refused = |e: PdbError| assert!(matches!(e, PdbError::Malformed(_) | PdbError::UnknownString { .. }), "{e}");
    match ext {
        true => {
            _ = ExtDatabase::from_bytes(data.clone()).map_err(refused);
            _ = ExtEditor::from_bytes(data).map_err(refused);
        }
        false => {
            _ = Database::from_bytes(data.clone()).map_err(refused);
            _ = PdbEditor::from_bytes(data).database().map_err(refused);
        }
    }
}
