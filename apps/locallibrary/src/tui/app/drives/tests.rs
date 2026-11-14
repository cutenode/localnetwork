use std::path::{Path, PathBuf};

use crate::tui::app::background::Msg;
use crate::tui::app::testing::*;
use crate::tui::edits::COLLECTION_READ_ONLY;

use super::*;

/// A plain (decrypted) `master.db` in `dir`: two tracks, one whose audio file is there and one
/// whose isn't, and a folder "Shows" with a playlist "Friday" holding both.
fn master_db(dir: &Path) -> PathBuf {
    let here = dir.join("Music/Here.mp3");
    std::fs::create_dir_all(here.parent().unwrap()).unwrap();
    std::fs::write(&here, b"mp3").unwrap();
    let db = dir.join("rekordbox/master.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(&format!(
            "CREATE TABLE djmdProperty (DBID TEXT, DBVersion TEXT);
             INSERT INTO djmdProperty VALUES ('1234567890', '7000');
             CREATE TABLE djmdArtist (ID TEXT PRIMARY KEY, Name TEXT);
             INSERT INTO djmdArtist VALUES ('1', 'Floorplan');
             CREATE TABLE djmdContent (ID TEXT PRIMARY KEY, FolderPath TEXT, FileNameL TEXT, Title TEXT,
                 ArtistID TEXT, BPM INTEGER, Length INTEGER, FileSize INTEGER, AnalysisDataPath TEXT,
                 rb_local_deleted INTEGER DEFAULT 0);
             INSERT INTO djmdContent (ID, FolderPath, FileNameL, Title, ArtistID, BPM, Length, FileSize, AnalysisDataPath)
             VALUES
                 ('1', '{}', 'Here.mp3', 'Here', '1', 12400, 300, 3, '/PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT'),
                 ('2', '{}', 'Gone.mp3', 'Gone', '1', 12800, 200, 3, '');
             CREATE TABLE djmdPlaylist (ID TEXT PRIMARY KEY, Seq INTEGER, Name TEXT, Attribute INTEGER, ParentID TEXT);
             INSERT INTO djmdPlaylist VALUES ('10', 1, 'Shows', 1, 'root'), ('11', 1, 'Friday', 0, '10');
             CREATE TABLE djmdSongPlaylist (ID TEXT PRIMARY KEY, PlaylistID TEXT, ContentID TEXT, TrackNo INTEGER);
             INSERT INTO djmdSongPlaylist VALUES ('a', '11', '1', 1), ('b', '11', '2', 2);",
            here.display(),
            dir.join("Music/Gone.mp3").display(),
        ))
        .unwrap();
    db
}

fn showing_collection(app: &App) -> bool {
    app.loading.is_none() && app.loaded.as_ref().is_some_and(|l| l.is_collection())
}

#[test]
fn with_no_drive_the_collection_opens_read_only_with_its_files_where_they_are() {
    let (mut app, dir) = blank(View::Library);
    let db = master_db(dir.path());
    app.collection_db = Some(db.clone());
    // The first read of the drives answers with none: the collection opens.
    app.handle_msg(Msg::Drives(Vec::new()));
    wait(&mut app, showing_collection);

    let l = app.loaded.as_ref().unwrap();
    assert_eq!((l.root(), l.collection.as_deref()), (collection_root().as_path(), Some(db.as_path())));
    assert_eq!(l.library.tracks.len(), 2);
    let anlz = l.library.tracks.iter().find(|t| t.title == "Here").unwrap().analyze_path.clone();
    assert_eq!(PathBuf::from(anlz), dir.path().join("rekordbox/share/PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT"));
    assert!(l.tree.iter().any(|n| n.name == "Shows"), "its folders and playlists are the tree");
    assert_eq!(app.drive_label(l.root()), COLLECTION_NAME);
    assert_eq!((app.source_rows(), app.source_row()), (1, Some(0)));

    // Its audio is found where the collection says it is.
    wait(&mut app, |a| a.loaded.as_ref().is_some_and(|l| l.problems.missing.is_some()));
    let l = app.loaded.as_ref().unwrap();
    let missing: Vec<(&str, bool)> =
        (l.library.tracks.iter().map(|t| t.title.as_str())).zip(l.problems.missing.clone().unwrap()).collect();
    assert!(missing.contains(&("Here", false)) && missing.contains(&("Gone", true)), "{missing:?}");

    // Nothing is planned on it, and nothing on disk changes.
    let before = std::fs::read(&db).unwrap();
    app.plan_edits(collection_root(), Vec::new());
    assert!(app.job.is_none());
    assert_eq!(last_message(&app), COLLECTION_READ_ONLY);
    assert_eq!(std::fs::read(&db).unwrap(), before);

    // Polling for the open drive leaves it alone: it can't be unplugged.
    app.last_volume_poll = Instant::now() - Duration::from_secs(5);
    app.tick();
    wait(&mut app, |a| !a.volumes_in_flight);
    assert!(!app.loaded.as_ref().unwrap().stale);
}

#[test]
fn the_sources_go_from_the_collection_to_a_drive_and_back() {
    let (mut app, dir) = blank(View::Library);
    app.collection_db = Some(master_db(dir.path()));
    let (_drive_dir, drive) = demo_drive();
    let info =
        locallibrary::api::drive_info(&locallibrary::api::DriveRef { drive: drive.display().to_string() }).unwrap();
    let drive = info.root.clone();
    app.handle_msg(Msg::Drives(vec![info]));
    // A drive with an export is there: it opens first, as before.
    wait(&mut app, |a| a.loading.is_none() && a.loaded.as_ref().is_some_and(|l| l.root() == drive));
    assert_eq!((app.source_rows(), app.source_row()), (2, Some(1)));

    app.focus = Focus::Drives;
    app.move_selection(-1);
    assert_eq!(app.source_row(), Some(0), "up from the drive is the collection");
    app.activate();
    wait(&mut app, showing_collection);
    assert_eq!(app.focus, Focus::Playlists);

    // `r` reads the collection again, not a drive at `/`.
    app.refresh();
    wait(&mut app, showing_collection);

    app.focus = Focus::Drives;
    app.move_selection(1);
    assert_eq!((app.collection_row, app.drive_state.selected()), (false, Some(0)));
    app.activate();
    wait(&mut app, |a| a.loading.is_none() && a.loaded.as_ref().is_some_and(|l| l.root() == drive));
    assert!(!app.loaded.as_ref().unwrap().is_collection());
}

#[test]
fn without_rekordbox_the_pane_is_the_drives_as_before() {
    let (mut app, _dir) = blank(View::Library);
    app.handle_msg(Msg::Drives(Vec::new()));
    assert!(app.loaded.is_none() && app.loading.is_none());
    assert_eq!((app.source_rows(), app.source_row()), (0, None));
    app.focus = Focus::Drives;
    app.move_selection(1);
    assert!(!app.collection_row);
}
