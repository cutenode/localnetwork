use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::tui::app::background::Msg;
use crate::tui::app::testing::*;
use crate::tui::edits::{COLLECTION_READ_ONLY, Modal};
use crate::tui::keymap::Act;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
             INSERT INTO djmdSongPlaylist VALUES ('a', '11', '1', 1), ('b', '11', '2', 2);
             CREATE TABLE djmdCue (ID TEXT PRIMARY KEY, ContentID TEXT, InMsec INTEGER, OutMsec INTEGER,
                 Kind INTEGER, ColorTableIndex INTEGER, Comment TEXT, rb_local_deleted INTEGER DEFAULT 0);
             INSERT INTO djmdCue (ID, ContentID, InMsec, OutMsec, Kind, ColorTableIndex, Comment)
             VALUES ('c1', '1', 1500, -1, 1, 0, 'Start'), ('c2', '1', 500, -1, 0, 0, '');",
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
    let info = drive_info(&drive);
    let drive = info.root.clone();
    app.handle_msg(Msg::Drives(vec![info]));
    // A drive with an export is there, and still the collection opens first.
    wait(&mut app, showing_collection);
    assert_eq!((app.source_rows(), app.source_row()), (2, Some(0)));

    app.focus = Focus::Drives;
    app.move_selection(1);
    assert_eq!((app.collection_row, app.drive_state.selected()), (false, Some(0)));
    app.activate();
    wait(&mut app, |a| a.loading.is_none() && a.loaded.as_ref().is_some_and(|l| l.root() == drive));
    assert!(!app.loaded.as_ref().unwrap().is_collection());

    app.focus = Focus::Drives;
    app.move_selection(-1);
    assert_eq!(app.source_row(), Some(0), "up from the drive is the collection");
    app.activate();
    wait(&mut app, showing_collection);
    assert_eq!(app.focus, Focus::Playlists);

    // `r` reads the collection again, not a drive at `/`.
    app.refresh();
    wait(&mut app, showing_collection);
}

#[test]
fn a_drive_open_at_the_last_quit_opens_again_over_the_collection() {
    let (mut app, dir) = blank(View::Library);
    app.collection_db = Some(master_db(dir.path()));
    let (_drive_dir, drive) = demo_drive();
    let info = drive_info(&drive);
    let drive = info.root.clone();
    app.preferred_root = Some(drive.clone());
    app.handle_msg(Msg::Drives(vec![info]));
    wait(&mut app, |a| a.loading.is_none() && a.loaded.as_ref().is_some_and(|l| l.root() == drive));
    assert_eq!(app.source_row(), Some(1));
}

/// Mark the drive at `drive` as holding a copy of the collection at `db` (`collection export`),
/// of the playlists named (none: all of it), made on November 13.
fn copied_to(drive: &Path, db: &Path, playlists: &[&str]) {
    let state = Device::open(drive).unwrap().state_dir();
    std::fs::create_dir_all(&state).unwrap();
    let rec = serde_json::json!({ "collection": db, "playlists": playlists, "at": "2026-11-13T15:00:00Z" });
    std::fs::write(state.join("collection.json"), rec.to_string()).unwrap();
}

#[test]
fn a_copy_of_the_collection_open_at_the_last_quit_gives_way_to_the_collection_and_says_its_a_copy() {
    let (mut app, dir) = blank(View::Library);
    let db = master_db(dir.path());
    app.collection_db = Some(db.clone());
    let (_drive_dir, drive) = demo_drive();
    copied_to(&drive, &db, &[]);
    let info = drive_info(&drive);
    let root = info.root.clone();
    app.preferred_root = Some(root.clone());
    app.handle_msg(Msg::Drives(vec![info.clone()]));
    wait(&mut app, showing_collection);
    assert!(app.collection_copies.contains_key(&root));
    app.focus = Focus::Drives;
    let s = crate::tui::ui::screen(&mut app, 120, 30);
    let row = s.lines().find(|l| l.contains(&info.volume_name)).unwrap_or_else(|| panic!("no drive in:\n{s}"));
    assert!(row.contains("copy, Nov 13"), "{row}");

    // A copy of some playlists is a drive of its own: open last time, it opens again.
    let (mut app, dir) = blank(View::Library);
    let db = master_db(dir.path());
    app.collection_db = Some(db.clone());
    copied_to(&drive, &db, &["Shows/Friday"]);
    app.preferred_root = Some(root.clone());
    app.handle_msg(Msg::Drives(vec![info]));
    wait(&mut app, |a| a.loading.is_none() && a.loaded.as_ref().is_some_and(|l| l.root() == root));
    assert!(app.collection_copies.is_empty());
}

#[test]
fn the_open_collection_is_read_again_once_rekordbox_has_saved() {
    let (mut app, dir) = blank(View::Library);
    let db = master_db(dir.path());
    app.collection_db = Some(db.clone());
    app.handle_msg(Msg::Drives(Vec::new()));
    wait(&mut app, showing_collection);
    assert_eq!(app.loaded.as_ref().unwrap().library.tracks.len(), 2);
    let a_while_ago = || Instant::now().checked_sub(Duration::from_secs(10)).unwrap();

    // Nothing changed: nothing's read.
    app.last_collection_poll = a_while_ago();
    app.watch_collection();
    assert!(app.loading.is_none() && app.collection_change.is_none());

    // rekordbox saves: the change is seen, and left alone until it's settled.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            "INSERT INTO djmdContent (ID, FolderPath, FileNameL, Title, ArtistID, BPM, Length, FileSize, AnalysisDataPath)
             VALUES ('3', '/nowhere/New.mp3', 'New.mp3', 'New', '1', 13000, 240, 3, '');",
        )
        .unwrap();
    app.last_collection_poll = a_while_ago();
    app.watch_collection();
    assert!(app.loading.is_none() && app.collection_change.is_some());
    app.last_collection_poll = a_while_ago();
    app.watch_collection();
    assert!(app.loading.is_none(), "not settled yet");

    // Settled: read again, with the new track.
    app.collection_change.as_mut().unwrap().1 = a_while_ago();
    app.last_collection_poll = a_while_ago();
    app.watch_collection();
    assert!(app.loading.is_some());
    wait(&mut app, showing_collection);
    assert_eq!(app.loaded.as_ref().unwrap().library.tracks.len(), 3);
    assert!(app.collection_change.is_none());
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

/// The drive at `drive`, as a read of the drives answers it.
fn drive_info(drive: &Path) -> locallibrary::DeviceInfo {
    locallibrary::api::drive_info(&locallibrary::api::DriveRef { drive: drive.display().to_string() }).unwrap()
}

/// The collection's row in the Library view's sources pane, as drawn on a 120 by 30 screen.
fn collection_row(app: &mut App) -> String {
    let s = crate::tui::ui::screen(app, 120, 30);
    let r = app.areas.drives;
    let mut pane = (s.lines().skip(r.y as usize).take(r.height as usize))
        .map(|l| l.chars().skip(r.x as usize).take(r.width as usize).collect::<String>());
    pane.find(|l| l.contains(COLLECTION_NAME)).unwrap_or_else(|| panic!("no collection in {r:?}:\n{s}"))
}

#[test]
fn the_sources_pane_draws_the_collection_above_the_drives_and_a_double_click_opens_it() {
    let (mut app, dir) = blank(View::Library);
    app.collection_db = Some(master_db(dir.path()));
    let (_drive_dir, drive) = demo_drive();
    let info = drive_info(&drive);
    let drive = info.root.clone();
    // The drive was open at the last quit: it opens again, and the collection isn't read yet.
    app.preferred_root = Some(drive.clone());
    app.handle_msg(Msg::Drives(vec![info]));
    wait(&mut app, |a| a.loading.is_none() && a.loaded.as_ref().is_some_and(|l| l.root() == drive));

    // Unread, it has no track count yet, and the pane holds sources rather than only drives.
    app.focus = Focus::Drives;
    let s = crate::tui::ui::screen(&mut app, 120, 30);
    assert!(s.contains("Sources") && s.contains("2/2"), "the footer counts the sources, the drive second:\n{s}");
    let row = collection_row(&mut app);
    assert!(row.trim_end_matches(['│', ' ']).ends_with(COLLECTION_NAME), "{row:?}");

    point(&mut app, LEFT, COLLECTION_NAME);
    assert_eq!(app.source_row(), Some(0), "a click selects it");
    assert!(crate::tui::ui::screen(&mut app, 120, 30).contains("1/2"));
    quick_again(&mut app);
    point(&mut app, LEFT, COLLECTION_NAME);
    wait(&mut app, showing_collection);
    assert_eq!(app.focus, Focus::Playlists, "opening it goes on to its playlists");

    // Open, it shows its track count.
    app.focus = Focus::Drives;
    let row = collection_row(&mut app);
    assert!(row.trim_end_matches(['│', ' ']).ends_with(" 2"), "{row:?}");

    // My Tags are an edit too: none on the collection.
    app.focus = Focus::Playlists;
    app.perform(crate::tui::keymap::Act::Tag, None);
    assert_eq!(last_message(&app), COLLECTION_READ_ONLY);
    assert!(app.modal.is_none() && app.job.is_none());
}

#[test]
fn the_collection_open_at_the_last_quit_opens_first_next_time() {
    let (mut app, dir) = blank(View::Library);
    let db = master_db(dir.path());
    app.collection_db = Some(db.clone());
    app.handle_msg(Msg::Drives(Vec::new()));
    wait(&mut app, showing_collection);
    let saved = app.saved_state();

    // The next run: a drive with an export is there, and still the collection opens.
    let mut next = App::blank(app.store_root.clone(), None);
    next.view = View::Library;
    next.apply_saved(&saved, true);
    next.collection_db = Some(db);
    let (_drive_dir, drive) = demo_drive();
    next.handle_msg(Msg::Drives(vec![drive_info(&drive)]));
    wait(&mut next, showing_collection);
    assert_eq!(next.source_row(), Some(0));
}

#[test]
fn a_collection_that_cant_be_read_says_why_and_opens_nothing() {
    let (mut app, dir) = blank(View::Library);
    let db = dir.path().join("master.db");
    rusqlite::Connection::open(&db).unwrap().execute_batch("CREATE TABLE notes (body TEXT);").unwrap();
    app.collection_db = Some(db);
    app.handle_msg(Msg::Drives(Vec::new()));
    wait(&mut app, |a| a.loading.is_none());
    assert!(app.loaded.is_none());
    assert!(last_message(&app).contains("not a rekordbox collection database"), "{}", last_message(&app));

    // With no collection on this computer, asking for it says so.
    app.collection_db = None;
    app.load_collection();
    assert!(app.loading.is_none());
    assert_eq!(last_message(&app), "rekordbox's collection isn't on this computer");
}

/// Moving through the sources stays on a row there is, one at a time, the collection first, and
/// selecting a row past the last changes nothing.
#[hegel::test(test_cases = 100)]
fn prop_moving_through_the_sources_stays_on_them(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, dir) = blank(View::Library);
    app.focus = Focus::Drives;
    let (_drive_dir, drive) = demo_drive();
    let info = drive_info(&drive);
    let drives = tc.draw(gs::integers::<usize>().max_value(4));
    let root = |i: usize| dir.path().join(i.to_string());
    app.drives = (0..drives).map(|i| locallibrary::DeviceInfo { root: root(i), ..info.clone() }).collect();
    let collection = tc.draw(gs::booleans());
    app.collection_db = collection.then(|| dir.path().join("master.db"));
    let rows = usize::from(collection) + drives;
    assert_eq!(app.source_rows(), rows);
    let mut at = None;
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        if tc.draw(gs::booleans()) {
            let delta = tc.draw(gs::sampled_from(&[-3isize, -1, 1, 2][..]));
            app.move_selection(delta);
            if rows > 0 {
                at = Some((at.unwrap_or(0) as isize + delta).clamp(0, rows as isize - 1) as usize);
            }
        } else {
            let row = tc.draw(gs::integers::<usize>().max_value(rows + 1));
            app.select_source_row(row);
            if row < rows {
                at = Some(row);
            }
        }
        assert_eq!(app.source_row(), at);
        assert_eq!(app.collection_row, collection && at == Some(0));
    }
}

/// rekordbox keeps the collection's cues in master.db, not in its analysis files: a track played
/// from the collection has them (and auto cue starts it at the memory cue).
#[test]
fn a_track_from_the_collection_plays_with_its_cues_from_master_db() {
    let (mut app, dir) = blank(View::Library);
    app.collection_db = Some(master_db(dir.path()));
    // Audio the player can open (the name doesn't decide how it's read).
    std::fs::write(dir.path().join("Music/Here.mp3"), crate::tui::audio::tests::wav_of(8000, &[0; 8000])).unwrap();
    app.handle_msg(Msg::Drives(Vec::new()));
    wait(&mut app, showing_collection);
    let l = app.loaded.as_ref().unwrap();
    let (track, root) = (l.library.tracks.iter().find(|t| t.title == "Here").unwrap().clone(), l.root().to_path_buf());
    // Auto cue starts it at its memory cue, which is master.db's too.
    let (audio, _out) = crate::tui::audio::Audio::offline(8000);
    (app.audio, app.deck.auto_cue) = (Some(audio), true);
    let now = app.open(track, root, true);
    app.now = Some(now);
    let cues = |a: &App| a.now.as_ref().map(|n| n.analysis.cues.clone()).unwrap_or_default();
    wait(&mut app, |a| !cues(a).is_empty());
    let got: Vec<_> = cues(&app).iter().map(|c| (c.hot_cue, c.start_ms, c.comment.clone())).collect();
    assert_eq!(got, [(None, 500, String::new()), (Some(0), 1500, "Start".into())]);
    wait(&mut app, |a| a.now.as_ref().is_some_and(|n| !n.opening));
    assert_eq!(app.position(), Duration::from_millis(500), "auto cue: the memory cue");
}

/// The queue arranged from rekordbox's collection packs onto a drive as a playlist of its own,
/// named first: exported, as the collection is read-only here, even with nothing open in the
/// library.
#[test]
fn the_queue_from_the_collection_packs_onto_a_drive_as_a_playlist() {
    let (mut app, dir) = blank(View::Library);
    app.collection_db = Some(master_db(dir.path()));
    app.handle_msg(Msg::Drives(Vec::new()));
    wait(&mut app, showing_collection);
    let here = app.loaded.as_ref().unwrap().library.tracks.iter().find(|t| t.title == "Here").unwrap().clone();
    app.crate_list.add(&collection_root(), "rekordbox", [here]);
    let target = dir.path().join("SET");
    std::fs::create_dir_all(&target).unwrap();
    let target = locallibrary::Device::open(&target).unwrap();
    app.drives = vec![target.info()];
    // With the collection open, its playlists are exported too; the queue isn't saved on it.
    app.perform(Act::Pack, None);
    let Some(Modal::Pick(p)) = app.modal.take() else { panic!("no choice: {}", last_message(&app)) };
    let labels: Vec<&str> = p.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels[labels.len() - 2..],
        [
            "the queue's 1 track from rekordbox's collection (added as a playlist)",
            "what the drive was exported with last time"
        ]
    );
    assert!(!labels.iter().any(|l| l.contains("saved as a playlist first")), "{labels:?}");
    app.loaded = None;

    app.perform(Act::Pack, None);
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no choice: {}", last_message(&app)) };
    let labels: Vec<&str> = p.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["the queue's 1 track from rekordbox's collection (added as a playlist)"]);
    press(&mut app, "\n");
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no drives to pick") };
    assert_eq!(p.items.len(), 1);
    press(&mut app, "\n");
    let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt") };
    assert_eq!(p.label, "Name the queue's playlist on SET");
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    press(&mut app, "Sets/Friday\n");
    let (op, summary) = confirming(&mut app);
    assert_eq!(op, "collection export");
    assert!(summary.contains("Sets/Friday") && summary.contains("1 track(s)"), "{summary}");
    let said = say_yes(&mut app);
    assert!(said.starts_with("packed onto SET"), "{said}");
    let packed = locallibrary::Device::open_export(target.root()).unwrap().library().unwrap();
    let set = packed.find_playlist("Sets/Friday").unwrap();
    assert_eq!(set.track_ids.len(), 1);
    assert_eq!(packed.tracks.len(), 1);
    assert!(packed.find_playlist("Shows/Friday").is_none(), "only the queue");

    // Added again under another name, now that the drive has a library: nothing's copied twice,
    // and what it has stays.
    app.drives = vec![locallibrary::Device::open(target.root()).unwrap().info()];
    app.perform(Act::Pack, None);
    press(&mut app, "\n");
    let Some(Modal::Pick(p)) = &app.modal else { panic!("no drives to pick") };
    assert_eq!(p.title, "Add the queue to which drive? (what's on it stays)");
    press(&mut app, "\n");
    app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    press(&mut app, "Sets/Saturday\n");
    let (op, _) = confirming(&mut app);
    assert_eq!(op, "collection queue");
    let said = say_yes(&mut app);
    assert!(said.starts_with("added the queue to SET: 0 tracks copied, 1 already there"), "{said}");
    let added = locallibrary::Device::open_export(target.root()).unwrap().library().unwrap();
    assert_eq!(added.tracks.len(), 1);
    assert!(added.find_playlist("Sets/Friday").is_some() && added.find_playlist("Sets/Saturday").is_some());
}

#[test]
fn a_playlist_from_the_collection_packs_onto_a_drive_as_an_export() {
    let (mut app, dir) = blank(View::Library);
    let target = dir.path().join("SET");
    std::fs::create_dir_all(&target).unwrap();

    // Without the collection on this computer, nothing is planned and it says why.
    app.plan_collection_pack(target.clone(), Default::default());
    assert_eq!((last_message(&app), app.job.is_none()), ("rekordbox's collection isn't on this computer", true));
    app.toast = None;
    app.plan_queue_pack(target.clone(), "Sets/Friday".into(), vec![1]);
    assert_eq!((last_message(&app), app.job.is_none()), ("rekordbox's collection isn't on this computer", true));

    app.collection_db = Some(master_db(dir.path()));
    app.plan_pack(collection_root(), target, vec!["Shows/Friday".into()]);
    let (op, summary) = confirming(&mut app);
    assert_eq!(op, "collection export");
    assert!(summary.contains("Shows/Friday"), "{summary}");
}

#[test]
fn the_queues_playlist_needs_a_name() {
    let (mut app, dir) = blank(View::Library);
    let root = dir.path().join("SET");
    app.modal = Some(Modal::Prompt(crate::tui::edits::Prompt {
        label: "Name the queue's playlist on SET".into(),
        text: " / ".into(),
        purpose: crate::tui::edits::PromptFor::QueueForPack { tracks: vec![1] },
        root,
    }));
    press(&mut app, "\n");
    assert!(app.job.is_none(), "nothing is planned for a name with no parts");
}
