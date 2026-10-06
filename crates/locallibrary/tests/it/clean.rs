use crate::common;

use std::fs;
use std::io::Cursor;
use std::path::Path;

use binrw::BinRead;
use locallibrary::clean;
use locallibrary::device::{Device, LineageOp};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{backup, paths};
use rekordbox_pdb::{Database, NewTrack, PdbEditor};

const SAMPLES: &str = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES";

/// Present rows of one table as seen by rekordcrate, an independent PDB parser.
fn rekordcrate_rows(pdb: &[u8], page_type: rekordcrate::pdb::PageType) -> usize {
    use rekordcrate::pdb::Header;
    let mut reader = Cursor::new(pdb);
    let header = Header::read(&mut reader).unwrap();
    let table = header.tables.iter().find(|t| t.page_type == page_type).unwrap();
    let pages = header.read_pages(&mut reader, binrw::Endian::NATIVE, (&table.first_page, &table.last_page)).unwrap();
    pages.into_iter().flat_map(|p| p.row_groups.into_iter()).map(|g| g.present_rows().count()).sum()
}

/// Add a track the way rekordbox exports one, with audio and analysis files. Returns its id.
fn export(root: &Path, artist: &str, title: &str, album: Option<&str>, file: &str, anlz: u32) -> u32 {
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let analyze = format!("/PIONEER/USBANLZ/P0AA/{anlz:08X}/ANLZ0000.DAT");
    let id = ed
        .add_track(&NewTrack {
            artist: Some(artist.into()),
            album: album.map(Into::into),
            genre: album.map(|_| "Loop Samples".into()),
            analyze_path: analyze.clone(),
            file_size: 600,
            ..NewTrack::new(title, file)
        })
        .unwrap();
    ed.save(&pdb_path).unwrap();
    for (key, bytes) in [
        (paths::key(file), common::placeholder_audio(file)),
        (paths::key(&analyze), common::synthetic_anlz(file)),
        (paths::key(&analyze.replace(".DAT", ".EXT")), common::synthetic_anlz(file)),
    ] {
        let p = paths::to_fs(root, &key);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    id
}

#[test]
fn clean_removes_demo_samples_and_can_be_undone() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let album = Some("GROOVE CIRCUIT FACTORY SAMPLES");
    let house = export(&root, "rekordbox", "House 1", album, &format!("{SAMPLES}/House1.wav"), 1);
    export(&root, "rekordbox", "House 1", album, &format!("{SAMPLES}/House1-1.wav"), 2);
    let techno = export(&root, "rekordbox", "Techno 1", album, &format!("{SAMPLES}/Techno1.wav"), 3);
    // Same artist, but not on the list: stays, and keeps the "rekordbox" artist row alive.
    export(&root, "rekordbox", "Keeper", None, "/Contents/rekordbox/Keeper.mp3", 4);
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let playlist = ed.database().unwrap().playlist_tree.iter().find(|n| n.name == "aab").unwrap().id;
    ed.add_to_playlist(playlist, techno, Some(5)).unwrap();
    ed.add_to_playlist(playlist, house, None).unwrap();
    ed.save(&pdb_path).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), b"sqlcipher").unwrap();
    let kept_before: Vec<_> = common::library_files(&root)
        .into_iter()
        .filter(|(k, _)| !k.contains("GROOVE") && !k.contains("P0AA"))
        .collect();

    let mut d = Device::open_export(&root).unwrap();
    let plan = clean::plan(&d, &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks.len(), 3);
    assert_eq!(plan.files.len(), 9, "{:?}", plan.files); // audio + .DAT + .EXT each
    assert_eq!(plan.playlists.len(), 1);
    assert_eq!(plan.playlists[0].entries_removed, 2);
    assert_eq!(plan.lookup_rows, 2, "album and genre go; the artist stays");
    assert!(plan.warnings.iter().any(|w| w.contains("exportLibrary.db")));
    assert!(plan.fits);

    let report = clean::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(report.tracks_removed, 3);
    assert_eq!(report.files_deleted, 9);

    let lib = d.library().unwrap();
    assert_eq!(lib.tracks.len(), 13);
    assert!(lib.tracks.iter().any(|t| t.title == "Keeper"));
    let aab = lib.find_playlist("aab").unwrap();
    assert_eq!(aab.track_ids.len(), 12);
    let db = Database::from_bytes(d.pdb_bytes().unwrap()).unwrap();
    let mut indexes: Vec<u32> =
        db.playlist_entries.iter().filter(|e| e.playlist_id == playlist).map(|e| e.entry_index).collect();
    indexes.sort_unstable();
    assert_eq!(indexes, (1..=12).collect::<Vec<_>>(), "entries renumbered without gaps");
    assert!(db.artists.iter().any(|a| a.name == "rekordbox"));
    assert!(db.albums.iter().all(|a| a.name != "GROOVE CIRCUIT FACTORY SAMPLES"));
    assert!(db.genres.iter().all(|g| g.name != "Loop Samples"));

    // Files are gone, empty folders pruned, everything else untouched.
    assert!(!root.join("Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES").exists());
    assert!(root.join("Contents/rekordbox/Keeper.mp3").is_file());
    assert!(!root.join("PIONEER/USBANLZ/P0AA/00000001").exists());
    assert!(root.join("PIONEER/USBANLZ/P0AA/00000004/ANLZ0000.DAT").is_file());
    let kept_after: Vec<_> = common::library_files(&root)
        .into_iter()
        .filter(|(k, _)| k != paths::EXPORT_PDB && !k.contains("P0AA"))
        .collect();
    let kept_before: Vec<_> = kept_before.into_iter().filter(|(k, _)| k != paths::EXPORT_PDB).collect();
    assert_eq!(kept_after, kept_before);

    // An independent parser agrees.
    let pdb = d.pdb_bytes().unwrap();
    assert_eq!(rekordcrate_rows(&pdb, rekordcrate::pdb::PageType::Tracks), 13);
    assert_eq!(rekordcrate_rows(&pdb, rekordcrate::pdb::PageType::PlaylistEntries), db.playlist_entries.len());

    let d = Device::open_export(&root).unwrap();
    assert_eq!(d.state().unwrap().head().unwrap().op, LineageOp::Clean);
    assert!(clean::plan(&d, &Ctx::silent()).unwrap().is_noop());

    // Restoring the backup brings back the tracks and their files.
    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, report.backup.as_deref(), false).unwrap();
    let lib = d.library().unwrap();
    assert_eq!(lib.tracks.len(), 16);
    for t in &lib.tracks {
        assert!(d.path(&paths::key(&t.file_path)).is_file(), "{} restored", t.file_path);
    }
    assert!(root.join("PIONEER/USBANLZ/P0AA/00000003/ANLZ0000.EXT").is_file());
}

#[test]
fn clean_leaves_shared_files_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let file = format!("{SAMPLES}/House1.wav");
    export(&root, "rekordbox", "House 1", None, &file, 1);
    // A track that isn't on the list but points at the same file (and analysis).
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    ed.add_track(&NewTrack {
        analyze_path: "/PIONEER/USBANLZ/P0AA/00000001/ANLZ0000.DAT".into(),
        ..NewTrack::new("My edit", file.clone())
    })
    .unwrap();
    ed.save(&pdb_path).unwrap();

    let mut d = Device::open_export(&root).unwrap();
    let plan = clean::plan(&d, &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks.len(), 1);
    assert!(plan.files.is_empty(), "{:?}", plan.files);
    clean::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert!(d.path(&paths::key(&file)).is_file());
    assert_eq!(d.library().unwrap().tracks.len(), 13);
}

#[test]
fn nothing_to_clean() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open_export(common::demo_drive(dir.path(), "Demo")).unwrap();
    let plan = clean::plan(&d, &Ctx::silent()).unwrap();
    assert!(plan.is_noop());
    let before = d.pdb_bytes().unwrap();
    let report = clean::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(report.tracks_removed, 0);
    assert_eq!(d.pdb_bytes().unwrap(), before);
    assert!(!d.is_adopted(), "a no-op clean writes nothing to the drive");
}

#[test]
fn clean_updates_both_databases() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let album = Some("GROOVE CIRCUIT FACTORY SAMPLES");
    let house = export(&root, "rekordbox", "House 1", album, &format!("{SAMPLES}/House1.wav"), 1);
    export(&root, "rekordbox", "Techno 1", album, &format!("{SAMPLES}/Techno1.wav"), 3);
    export(&root, "rekordbox", "Keeper", None, "/Contents/rekordbox/Keeper.mp3", 4);
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let playlist = ed.database().unwrap().playlist_tree.iter().find(|n| n.name == "aab").unwrap().id;
    ed.add_to_playlist(playlist, house, Some(3)).unwrap();
    ed.save(&pdb_path).unwrap();
    // rekordbox 7 exports the same collection to OneLibrary as well.
    common::add_onelibrary(&root);

    let mut d = Device::open_export(&root).unwrap();
    let plan = clean::plan(&d, &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks.len(), 2);
    assert_eq!(plan.files.len(), 6, "{:?}", plan.files);
    assert_eq!(plan.lookup_rows, 4, "album and genre, in each database");
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    let report = clean::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!((report.tracks_removed, report.files_deleted), (2, 6));

    common::sqlcipher_reads(&root);
    for lib in [d.library().unwrap(), common::plus_library(&root)] {
        assert_eq!(lib.tracks.len(), 13);
        assert!(lib.tracks.iter().any(|t| t.title == "Keeper"));
        assert!(lib.tracks.iter().all(|t| t.title != "House 1" && t.title != "Techno 1"));
        assert_eq!(lib.find_playlist("aab").unwrap().track_ids.len(), 12);
    }
    let one = OneLibrary::open_root(&root).unwrap();
    let conn = one.connection();
    let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    let aab = count("SELECT playlist_id FROM playlist WHERE name = 'aab'");
    let seq: Vec<i64> = conn
        .prepare("SELECT sequenceNo FROM playlist_content WHERE playlist_id = ?1 ORDER BY sequenceNo")
        .unwrap()
        .query_map([aab], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(seq, (1..=12).collect::<Vec<_>>(), "entries renumbered without gaps");
    assert_eq!(count("SELECT count(*) FROM album WHERE name = 'GROOVE CIRCUIT FACTORY SAMPLES'"), 0);
    assert_eq!(count("SELECT count(*) FROM genre WHERE name = 'Loop Samples'"), 0);
    assert_eq!(count("SELECT count(*) FROM artist WHERE name = 'rekordbox'"), 1);
    assert_eq!(count("SELECT numberOfContents FROM property"), 13);

    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, report.backup.as_deref(), false).unwrap();
    assert_eq!(common::plus_library(&root).tracks.len(), 15);
    assert_eq!(d.library().unwrap().tracks.len(), 15);
}

/// Regression: apply made its backup and marked the drive as interrupted before editing, so a
/// clean that then failed left both behind. Here OneLibrary refuses to delete tracks (a table
/// rekordbox 7 doesn't have refers to them), after export.pdb was cleaned in memory.
#[test]
fn an_apply_failing_after_planning_leaves_the_drive_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    export(&root, "rekordbox", "House 1", None, &format!("{SAMPLES}/House1.wav"), 1);
    common::add_onelibrary(&root);
    let one = OneLibrary::open_root(&root).unwrap();
    one.connection()
        .execute_batch("CREATE TABLE futureCue (futureCue_id integer primary key, content_id integer)")
        .unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let before = common::Before::take(&root);
    let mut d = Device::open_export(&root).unwrap();
    let plan = clean::plan(&d, &Ctx::silent()).unwrap();
    let err = clean::apply(&plan, &mut d, &Ctx::silent()).unwrap_err();
    before.untouched(&root).unwrap();
    assert!(err.to_string().contains("futureCue"), "{err}");
}

#[test]
fn clean_a_onelibrary_only_drive() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let file = format!("{SAMPLES}/Breaks1.wav");
    export(&root, "rekordbox", "Breaks 1", None, &file, 1);
    common::onelibrary_only(&root);

    let mut d = Device::open_export(&root).unwrap();
    let plan = clean::plan(&d, &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks.len(), 1);
    clean::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    common::sqlcipher_reads(&root);
    let lib = d.library().unwrap();
    assert_eq!(lib.tracks.len(), 12);
    assert!(!d.path(&paths::key(&file)).exists());
    assert!(!root.join(paths::EXPORT_PDB).exists(), "no export.pdb appears");
}
