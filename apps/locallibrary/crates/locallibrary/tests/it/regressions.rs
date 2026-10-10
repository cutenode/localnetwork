//! Regression tests for bugs found in review.
use crate::common;

use indexmap::IndexMap;

use binrw::BinRead;

use locallibrary::api::{self, DriveRef, LibraryCache};
use locallibrary::compare::{self, Relation, Side};
use locallibrary::device::Device;
use locallibrary::interop::{self, InteropOptions, Software};
use locallibrary::merge::{self, MergeOptions};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, PushOptions, SyncOptions};
use locallibrary::xml::{self, XmlOptions};
use locallibrary::{Error, anlz, backup, export, paths, pdb};
use rekordbox_pdb::{Database, NewTrack, PdbEditor, TableType};

fn add_raw_track(root: &std::path::Path, title: &str, file_path: &str) {
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let n = title.len() as u16;
    ed.add_track(&NewTrack { file_size: 900 + n as u32, duration: 100 + n, ..NewTrack::new(title, file_path) })
        .unwrap();
    ed.save(&pdb_path).unwrap();
}

#[test]
fn database_paths_cannot_reach_outside_managed_folders() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let b_root = common::demo_drive(dir.path(), "B");
    // B's database references a file outside the drive and one outside Contents/.
    std::fs::write(dir.path().join("victim.txt"), "keep").unwrap();
    std::fs::create_dir_all(b_root.join("Music")).unwrap();
    std::fs::write(b_root.join("Music/mine.mp3"), "keep").unwrap();
    add_raw_track(&b_root, "evil", "/../victim.txt");
    add_raw_track(&b_root, "elsewhere", "/Music/mine.mp3");
    let mut b = Device::open_export(&b_root).unwrap();

    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join("victim.txt")).unwrap(), "keep");
    assert_eq!(std::fs::read_to_string(b_root.join("Music/mine.mp3")).unwrap(), "keep");
    assert!(b.checked_path("../victim.txt").is_err());

    // Merging such a library never writes outside the drive either.
    let src_root = common::demo_drive(dir.path(), "Src");
    add_raw_track(&src_root, "evil", "/../../escaped.mp3");
    add_raw_track(&src_root, "db", "/PIONEER/rekordbox/export.pdb");
    let src = Device::open_export(&src_root).unwrap();
    let mut dst = Device::open_export(common::bigger_drive(dir.path(), "Dst")).unwrap();
    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert!(plan.new_tracks.iter().all(|t| paths::is_safe_key(&t.audio) && t.audio.starts_with("Contents/")));
    merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert!(!dir.path().join("escaped.mp3").exists() && !dir.path().parent().unwrap().join("escaped.mp3").exists());
}

#[test]
fn push_refuses_to_drop_an_unsnapshotted_merge() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    sync::pull(&store, &mut a, &PullOptions::default(), &Ctx::silent()).unwrap();

    let c = Device::open_export(common::bigger_drive(dir.path(), "C")).unwrap();
    let plan = merge::plan(&c, &a, &MergeOptions::default(), &Ctx::silent()).unwrap();
    merge::apply(&plan, &c, &mut a, &MergeOptions::default(), &Ctx::silent()).unwrap();

    // Immediately (same second as the pull, so mtimes can't tell): the store's head is older.
    let lib = store.libraries().unwrap().remove(0);
    let head = store.load_snapshot(&lib.head.unwrap()).unwrap();
    let cmp = compare::compare(
        &Side::from_snapshot(&store, &head, "store").unwrap(),
        &Side::from_device(&a, &Ctx::silent()).unwrap(),
    );
    assert_eq!(cmp.relation, Relation::RightNewer);

    let err = sync::push(&store, &head, &mut a, &PushOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(_)), "{err}");
    assert_eq!(a.library().unwrap().tracks.len(), 14);
}

#[test]
fn case_only_differences_are_in_sync_after_sync() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::bigger_drive(dir.path(), "A")).unwrap();
    let b_root = dir.path().join("B");
    common::copy_dir(a.root(), &b_root);
    // Same files, different letter case in a folder name.
    let from = b_root.join("Contents/[a]pendics.shuffle/Framed And Defamed");
    let to = b_root.join("Contents/[a]pendics.shuffle/FRAMED AND DEFAMED");
    std::fs::rename(&from, b_root.join("Contents/[a]pendics.shuffle/tmp")).unwrap();
    std::fs::rename(b_root.join("Contents/[a]pendics.shuffle/tmp"), &to).unwrap();
    let mut b = Device::open_export(&b_root).unwrap();

    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let c = compare::compare_devices(&a, &b, &Ctx::silent()).unwrap();
    assert_eq!(c.relation, Relation::Identical);
}

#[test]
fn concurrent_writers_are_locked_out() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let held = d.lock().unwrap();
    assert!(matches!(d.lock(), Err(Error::Locked(_))));
    let mut other = Device::open_export(d.root()).unwrap();
    let src = Device::open_export(common::bigger_drive(dir.path(), "S")).unwrap();
    let plan = merge::plan(&src, &other, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert!(matches!(
        merge::apply(&plan, &src, &mut other, &MergeOptions::default(), &Ctx::silent()),
        Err(Error::Locked(_))
    ));
    drop(held);
    assert!(d.lock().is_ok());
}

#[test]
fn restore_refuses_a_backup_whose_files_are_gone() {
    let dir = tempfile::tempdir().unwrap();
    let src = Device::open_export(common::demo_drive(dir.path(), "Guest")).unwrap();
    let mut b = Device::open_export(common::bigger_drive(dir.path(), "B")).unwrap();
    let plan = merge::plan(&src, &b, &MergeOptions::default(), &Ctx::silent()).unwrap();
    merge::apply(&plan, &src, &mut b, &MergeOptions::default(), &Ctx::silent()).unwrap();

    // The library on B is then replaced entirely by another one.
    let mut other = Device::open_export(common::demo_drive(dir.path(), "Other")).unwrap();
    sync::sync_drive_to_drive(&mut other, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();

    let err = backup::restore(&mut b, None, false).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(_)), "{err}");
    assert_eq!(b.library().unwrap().tracks.len(), 2);
    backup::restore(&mut b, None, true).unwrap();
    assert_eq!(b.library().unwrap().tracks.len(), 12);
}

#[test]
fn malformed_inputs_fail_cleanly() {
    // A cyclic page chain must be an error, not a hang.
    let mut bytes =
        std::fs::read(common::fixtures().join("rekordcrate/demo_tracks/PIONEER/rekordbox/export.pdb")).unwrap();
    let page_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let first = u32::from_le_bytes(bytes[0x1c + 8..0x1c + 12].try_into().unwrap()) as usize;
    bytes[0x1c + 12..0x1c + 16].copy_from_slice(&u32::MAX.to_le_bytes()); // last page unreachable
    bytes[first * page_size + 12..first * page_size + 16].copy_from_slice(&(first as u32).to_le_bytes());
    assert!(pdb::parse_database(bytes, std::path::Path::new("x")).is_err());

    // A beat grid claiming four billion beats doesn't allocate for them.
    let mut f = common::synthetic_anlz("/Contents/a.mp3");
    let mut pqtz = b"PQTZ".to_vec();
    pqtz.extend_from_slice(&24u32.to_be_bytes());
    pqtz.extend_from_slice(&32u32.to_be_bytes());
    pqtz.extend_from_slice(&[0, 0, 0, 0, 0, 8, 0, 0]);
    pqtz.extend_from_slice(&u32::MAX.to_be_bytes());
    pqtz.extend_from_slice(&[0, 1, 0x32, 0, 0, 0, 0, 100]);
    f.extend_from_slice(&pqtz);
    let len = f.len() as u32;
    f[8..12].copy_from_slice(&len.to_be_bytes());
    assert_eq!(anlz::beat_grid(&f).unwrap().len(), 1);
}

/// The shape of a damaged (or crafted) export.pdb: folder F, then two rows repeating F's id that
/// name F as their folder. Read as it is, every level of F holds two copies of F.
#[test]
fn a_playlist_tree_repeating_a_folder_id_is_walked_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let folder = ed.create_playlist("F", 0, true).unwrap();
    for name in ["P", "Q"] {
        let p = ed.create_playlist(name, folder, false).unwrap();
        ed.add_to_playlist(p, 1, None).unwrap();
    }
    let mut bytes = ed.to_bytes();
    let db = Database::from_bytes(bytes.clone()).unwrap();
    for (n, &at) in db.playlist_tree.iter().zip(db.row_locations(TableType::PlaylistTree)) {
        if n.name != "F" {
            for (offset, value) in [(0, folder), (12, folder), (16, 1)] {
                bytes[at + offset..at + offset + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    std::fs::write(&pdb_path, bytes).unwrap();

    let d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    assert_eq!(lib.playlists.iter().map(|p| (p.id, p.name.as_str())).collect::<Vec<_>>(), [(folder, "F")]);
    // Every walk over the tree ends: the XML, the other software's exports, a folder's playlist
    // files and the sidecar's tree.
    let (xml, _) = xml::export(&d, &XmlOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(xml.matches("Name=\"F\"").count(), 1);
    for software in [Software::Traktor, Software::Serato, Software::Engine] {
        let plan = interop::plan(&d, software, &dir.path().join("out"), &InteropOptions::default(), &Ctx::silent());
        assert_eq!(plan.unwrap().folders, 1);
    }
    assert!(export::select(&lib, Some("F"), true, "all").unwrap().is_empty());
    let tree = api::playlists_tree(&LibraryCache::new(1), &DriveRef { drive: root.display().to_string() }).unwrap();
    assert_eq!((tree.playlists.len(), tree.playlists[0].children.len()), (1, 0));
}

/// rekordbox writes an ISRC as `0x90`, then 3, the code in ASCII and a NUL: all 763 of the large
/// export's read as CJK characters before. rekordcrate, which knows the form, reads each alike.
#[test]
fn isrcs_read_as_rekordbox_writes_them() {
    use rekordcrate::pdb::{Header, PageType, Row};
    let bytes = std::fs::read(common::fixtures().join("rekordcrate/large/export.pdb")).unwrap();
    let lib = pdb::library_from_database(&pdb::parse_database(bytes.clone(), std::path::Path::new("x")).unwrap());
    let ours: IndexMap<u32, String> = lib.tracks.iter().map(|t| (t.id, t.isrc.clone())).collect();
    assert_eq!(ours.values().filter(|i| !i.is_empty()).count(), 763);
    assert!(ours.values().all(|i| i.is_empty() || (i.len() == 12 && i.chars().all(|c| c.is_ascii_alphanumeric()))));
    assert_eq!(ours[&1], "DEOQ91710032");

    // rekordcrate's track fields are private: read them from its Debug output.
    let mut reader = std::io::Cursor::new(&bytes);
    let header = Header::read(&mut reader).unwrap();
    let table = header.tables.iter().find(|t| t.page_type == PageType::Tracks).unwrap();
    let pages = header.read_pages(&mut reader, binrw::Endian::NATIVE, (&table.first_page, &table.last_page)).unwrap();
    let field = |row: &str, name: &str, close: char| {
        let start = row.find(name).unwrap() + name.len();
        row[start..start + row[start..].find(close).unwrap()].to_string()
    };
    let theirs: IndexMap<u32, String> = pages
        .iter()
        .flat_map(|p| p.row_groups.iter())
        .flat_map(|g| g.present_rows())
        .filter_map(|r| match r {
            Row::Track(t) => Some(format!("{t:?}")),
            _ => None,
        })
        .map(|row| (field(&row, "id: TrackId(", ')').parse().unwrap(), field(&row, "isrc: DeviceSQLString(\"", '"')))
        .collect();
    assert_eq!(theirs, ours);
}

/// An artist or album row for a name in a long string puts it where rekordbox does, 4-byte
/// aligned: byte for byte, but for the ids, as in rekordbox's own rows of the large export.
#[test]
fn long_artist_and_album_names_are_stored_as_rekordbox_stores_them() {
    let large = std::fs::read(common::fixtures().join("rekordcrate/large/export.pdb")).unwrap();
    let theirs = Database::from_bytes(large.clone()).unwrap();
    let artist = theirs.artists.iter().position(|a| !a.name.is_ascii()).unwrap();
    let album = theirs.albums.iter().position(|a| !a.name.is_ascii()).unwrap();
    let empty = std::fs::read(common::fixtures().join("rekordcrate/empty/PIONEER/rekordbox/export.pdb")).unwrap();
    let mut ed = PdbEditor::from_bytes(empty);
    let artist_id = ed.artist_id_for(&theirs.artists[artist].name).unwrap();
    let album_id = ed.album_id_for(&theirs.albums[album].name, artist_id).unwrap();
    let mine_bytes = ed.to_bytes();
    let mine = Database::from_bytes(mine_bytes.clone()).unwrap();
    let row = |db: &Database, bytes: &[u8], table: TableType, i: usize, fields: usize, len: usize| {
        let at = db.row_locations(table)[i];
        bytes[at + fields..at + len].to_vec()
    };
    // Artist rows: the name offset from 8 on (the id comes before). Album rows: from 0x14 on.
    let name_len = |name: &str| 4 + 2 * name.encode_utf16().count();
    let a_len = 12 + name_len(&theirs.artists[artist].name);
    assert_eq!(
        row(&mine, &mine_bytes, TableType::Artists, 0, 8, a_len),
        row(&theirs, &large, TableType::Artists, artist, 8, a_len)
    );
    let mine_album = mine.albums.iter().position(|a| a.id == album_id).unwrap();
    let al_len = 0x18 + name_len(&theirs.albums[album].name);
    assert_eq!(
        row(&mine, &mine_bytes, TableType::Albums, mine_album, 0x14, al_len),
        row(&theirs, &large, TableType::Albums, album, 0x14, al_len)
    );
}

#[test]
fn merge_never_reuses_a_path_the_target_database_references() {
    let dir = tempfile::tempdir().unwrap();
    let dst_root = common::bigger_drive(dir.path(), "Dst");
    // The target references a file it doesn't have.
    add_raw_track(&dst_root, "ghost", "/Contents/Shared/track.mp3");
    let src_root = common::demo_drive(dir.path(), "Src");
    add_raw_track(&src_root, "different song", "/Contents/Shared/track.mp3");
    std::fs::create_dir_all(src_root.join("Contents/Shared")).unwrap();
    std::fs::write(src_root.join("Contents/Shared/track.mp3"), "src audio").unwrap();

    let src = Device::open_export(&src_root).unwrap();
    let dst = Device::open_export(&dst_root).unwrap();
    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let t = plan.new_tracks.iter().find(|t| t.name == "different song").unwrap();
    assert_eq!(t.audio, "Contents/Shared/track (2).mp3");
}
