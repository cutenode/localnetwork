//! `exportExt.pdb` (My Tags): reading, and editing with `ExtEditor` (locallibrary patch).

use rekordbox_pdb::{ExtDatabase, ExtEditor, ExtTableType, PdbError};

const ONE_SONG_EXT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/one-song-exportExt.pdb"
);

fn original() -> Vec<u8> {
    std::fs::read(ONE_SONG_EXT).unwrap()
}

fn names(db: &ExtDatabase, category: u32) -> Vec<String> {
    db.tags_in(category).iter().map(|t| t.name.clone()).collect()
}

fn table(db: &ExtDatabase, ty: ExtTableType) -> rekordbox_pdb::pdb::Table {
    *db.tables.iter().find(|t| t.table_type == ty as u32).unwrap()
}

#[test]
fn reads_categories_and_tags_in_display_order() {
    let db = ExtDatabase::from_bytes(original()).unwrap();
    let cats: Vec<(u32, &str, u32)> = db
        .categories()
        .iter()
        .map(|c| (c.id, c.name.as_str(), c.position))
        .collect();
    assert_eq!(
        cats,
        [(1, "Genre", 0), (2, "Components", 1), (3, "Situation", 2), (4, "Untitled Column", 3)]
    );
    assert_eq!(
        names(&db, 1),
        ["Acid House", "Deep House", "Techno", "Nu Disco", "Electro House", "Bass Music", "Trap"]
    );
    let positions: Vec<u32> = db.tags_in(2).iter().map(|t| t.position).collect();
    assert_eq!(positions, (0..positions.len() as u32).collect::<Vec<_>>());
    assert!(db.tags.iter().filter(|t| t.is_category).all(|c| c.category_id == 0));
    // Rows are numbered 0x20 apart.
    for (i, t) in db.tags.iter().enumerate() {
        assert_eq!(t.index_shift as usize, 0x20 * i);
    }
    // The tag tracks table is type 4, and empty here (only its index page).
    assert!(db.has_table(ExtTableType::TagTracks));
    assert!(db.tag_tracks.is_empty());
    let tt = table(&db, ExtTableType::TagTracks);
    assert_eq!(tt.first_page, tt.last_page);
}

#[test]
fn an_unedited_file_is_written_back_unchanged() {
    let mut ed = ExtEditor::from_bytes(original()).unwrap();
    assert_eq!(ed.to_bytes().unwrap(), original());
}

#[test]
fn rewritten_rows_match_rekordbox_byte_for_byte() {
    let orig = original();
    let before = ExtDatabase::from_bytes(orig.clone()).unwrap();
    let mut ed = ExtEditor::from_bytes(orig.clone()).unwrap();
    // Renaming to the same name changes nothing.
    ed.rename(1, "Genre").unwrap();
    assert_eq!(ed.to_bytes().unwrap(), orig);
    // A real change rewrites every row, the way rekordbox saves.
    ed.rename(4, "Mood").unwrap();
    let out = ed.to_bytes().unwrap();
    let after = ExtDatabase::from_bytes(out.clone()).unwrap();
    assert_eq!(after.tags.len(), before.tags.len());
    let (old_locs, new_locs) = (
        before.row_locations(ExtTableType::Tags),
        after.row_locations(ExtTableType::Tags),
    );
    for (i, (o, n)) in old_locs.iter().zip(new_locs).enumerate() {
        let len = new_locs.get(i + 1).map_or(0x30, |next| next - n);
        let (old_row, new_row) = (&orig[*o..*o + len], &out[*n..*n + len]);
        if before.tags[i].name == "Untitled Column" {
            assert_eq!(after.tags[i].name, "Mood");
            continue;
        }
        // Everything but the index shift (bytes 2-3, the row's slot) is identical.
        assert_eq!(old_row[..2], new_row[..2]);
        assert_eq!(old_row[4..], new_row[4..], "row {i} ({})", before.tags[i].name);
    }
    // Rows kept their order, with new slots.
    let shifts: Vec<u16> = after.tags.iter().map(|t| t.index_shift).collect();
    assert_eq!(shifts[0] as usize, 0x20 * before.tags.len());
    // The edit is recorded like rekordbox's: the header's sequence number moves on.
    let seq = |b: &[u8]| u32::from_le_bytes(b[0x14..0x18].try_into().unwrap());
    assert_eq!(seq(&out), seq(&orig) + 1);
}

#[test]
fn adds_categories_and_tags_and_links_tracks() {
    let mut ed = ExtEditor::from_bytes(original()).unwrap();
    ed.add_tag(1, 0x1234_5678, "Minimal").unwrap();
    ed.add_category(5, "Energy").unwrap();
    ed.add_tag(5, 0x0bad_cafe, "Peak").unwrap();
    assert!(ed.link(0x1234_5678, 7).unwrap());
    assert!(!ed.link(0x1234_5678, 7).unwrap(), "already linked");
    assert!(ed.link(0x1234_5678, 8).unwrap());
    assert!(ed.link(0x0bad_cafe, 7).unwrap());
    let db = ExtDatabase::from_bytes(ed.to_bytes().unwrap()).unwrap();

    assert_eq!(names(&db, 1).last().unwrap(), "Minimal");
    let minimal = db.tags.iter().find(|t| t.name == "Minimal").unwrap();
    assert_eq!((minimal.id, minimal.category_id, minimal.position), (0x1234_5678, 1, 7));
    let energy = db.categories().into_iter().find(|c| c.name == "Energy").unwrap();
    assert_eq!((energy.id, energy.position, energy.is_category), (5, 4, true));
    assert_eq!(names(&db, 5), ["Peak"]);
    let mut links: Vec<(u32, u32)> = db.tag_tracks.iter().map(|l| (l.tag_id, l.track_id)).collect();
    links.sort_unstable();
    assert_eq!(links, [(0x0bad_cafe, 7), (0x1234_5678, 7), (0x1234_5678, 8)]);

    // The first link gave the empty tag tracks table a data page after its index page.
    let tt = table(&db, ExtTableType::TagTracks);
    assert_ne!(tt.first_page, tt.last_page);
    let before = ExtDatabase::from_bytes(original()).unwrap();
    assert_eq!(tt.last_page, table(&before, ExtTableType::TagTracks).empty_candidate);
}

#[test]
fn removing_a_tag_takes_its_links_and_closes_the_gap() {
    let mut ed = ExtEditor::from_bytes(original()).unwrap();
    let techno = ed.tags(1).iter().find(|(_, n)| *n == "Techno").unwrap().0;
    let trap = ed.tags(1).iter().find(|(_, n)| *n == "Trap").unwrap().0;
    ed.link(techno, 1).unwrap();
    ed.link(trap, 1).unwrap();
    let mut ed = ExtEditor::from_bytes(ed.to_bytes().unwrap()).unwrap();
    assert!(ed.is_linked(techno, 1));
    ed.remove(techno).unwrap();
    let db = ExtDatabase::from_bytes(ed.to_bytes().unwrap()).unwrap();
    assert_eq!(
        names(&db, 1),
        ["Acid House", "Deep House", "Nu Disco", "Electro House", "Bass Music", "Trap"]
    );
    let positions: Vec<u32> = db.tags_in(1).iter().map(|t| t.position).collect();
    assert_eq!(positions, [0, 1, 2, 3, 4, 5]);
    assert_eq!(db.tag_tracks.len(), 1);
    assert_eq!(db.tag_tracks[0].tag_id, trap);

    // A category goes with its tags and their links.
    let mut ed = ExtEditor::from_bytes(ed.to_bytes().unwrap()).unwrap();
    ed.remove(1).unwrap();
    let db = ExtDatabase::from_bytes(ed.to_bytes().unwrap()).unwrap();
    assert!(db.tags.iter().all(|t| t.id != 1 && t.category_id != 1));
    assert!(db.tag_tracks.is_empty());
    let cats: Vec<(&str, u32)> = db.categories().iter().map(|c| (c.name.as_str(), c.position)).collect();
    assert_eq!(cats, [("Components", 0), ("Situation", 1), ("Untitled Column", 2)]);
}

#[test]
fn unlinking() {
    let mut ed = ExtEditor::from_bytes(original()).unwrap();
    let acid = ed.tags(1)[0].0;
    let deep = ed.tags(1)[1].0;
    for track in 1..=3 {
        ed.link(acid, track).unwrap();
        ed.link(deep, track).unwrap();
    }
    assert!(ed.unlink(acid, 2).unwrap());
    assert!(!ed.unlink(acid, 2).unwrap());
    assert_eq!(ed.unlink_tracks(&[3]).unwrap(), 2);
    let db = ExtDatabase::from_bytes(ed.to_bytes().unwrap()).unwrap();
    let mut links: Vec<(u32, u32)> = db.tag_tracks.iter().map(|l| (l.track_id, l.tag_id)).collect();
    links.sort_unstable();
    let mut want = vec![(1, acid), (1, deep), (2, deep)];
    want.sort_unstable();
    assert_eq!(links, want);
}

#[test]
fn long_and_non_ascii_names() {
    let mut ed = ExtEditor::from_bytes(original()).unwrap();
    let long = "x".repeat(300);
    ed.add_tag(2, 100, &long).unwrap(); // past one-byte offsets: subtype 0x0684
    ed.add_tag(2, 101, "Café Del Mar").unwrap(); // UTF-16
    ed.add_tag(2, 102, "夜").unwrap();
    ed.rename(3, "Situation ✓").unwrap();
    let out = ed.to_bytes().unwrap();
    let db = ExtDatabase::from_bytes(out.clone()).unwrap();
    let got = names(&db, 2);
    assert_eq!(got[got.len() - 3..], [long.clone(), "Café Del Mar".into(), "夜".into()]);
    assert!(db.categories().iter().any(|c| c.name == "Situation ✓"));
    let locs = db.row_locations(ExtTableType::Tags);
    let far = db.tags.iter().position(|t| t.id == 100).unwrap();
    assert_eq!(u16::from_le_bytes([out[locs[far]], out[locs[far] + 1]]), 0x0684);
    let near = db.tags.iter().position(|t| t.id == 101).unwrap();
    assert_eq!(u16::from_le_bytes([out[locs[near]], out[locs[near] + 1]]), 0x0680);
}

#[test]
fn many_links_span_pages() {
    let mut ed = ExtEditor::from_bytes(original()).unwrap();
    let tag = ed.tags(1)[0].0;
    for track in 1..=1000 {
        ed.link(tag, track).unwrap();
    }
    let db = ExtDatabase::from_bytes(ed.to_bytes().unwrap()).unwrap();
    assert_eq!(db.tag_tracks.len(), 1000);
    let mut tracks: Vec<u32> = db.tag_tracks.iter().map(|l| l.track_id).collect();
    tracks.sort_unstable();
    assert_eq!(tracks, (1..=1000).collect::<Vec<_>>());
}

#[test]
fn refuses_bad_edits() {
    let mut ed = ExtEditor::from_bytes(original()).unwrap();
    assert!(matches!(ed.add_category(1, "Again"), Err(PdbError::IdInUse(1))));
    assert!(matches!(ed.add_tag(1, 0, "Zero"), Err(PdbError::IdInUse(0))));
    let acid = ed.tags(1)[0].0;
    assert!(matches!(ed.add_tag(2, acid, "Dup"), Err(PdbError::IdInUse(_))));
    assert!(matches!(ed.add_tag(99, 500, "Orphan"), Err(PdbError::TagNotFound(99))));
    assert!(matches!(ed.link(1, 1), Err(PdbError::TagNotFound(1))), "categories aren't tags");
    assert!(matches!(ed.rename(500, "x"), Err(PdbError::TagNotFound(500))));
    assert!(matches!(ed.remove(500), Err(PdbError::TagNotFound(500))));
    // Nothing was changed by the failures.
    assert_eq!(ed.to_bytes().unwrap(), original());
}

#[test]
fn a_file_without_a_tag_tracks_table_cant_link() {
    let mut bytes = original();
    // Rename table type 4 in the directory to an unused type.
    let num = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let entry = (0..num).map(|i| 0x1c + i * 16).find(|&o| bytes[o] == 4).unwrap();
    bytes[entry] = 0x63;
    let db = ExtDatabase::from_bytes(bytes.clone()).unwrap();
    assert!(!db.has_table(ExtTableType::TagTracks));
    let mut ed = ExtEditor::from_bytes(bytes).unwrap();
    let acid = ed.tags(1)[0].0;
    assert!(matches!(ed.link(acid, 1), Err(PdbError::NoTable(4))));
}
