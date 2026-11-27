use locallibrary::collection_write::Staged;
use locallibrary::cue_edit::{CollectionCueEdit, CollectionTrack, CueChange, CueRef};

use super::*;
use crate::tui::app::background::Msg;
use crate::tui::app::drives::{CollectionStamp, collection_root};
use crate::tui::app::testing::*;
use crate::tui::edits::Applied;

/// The first track of `writable`'s collection, by its `djmdContent.ID`.
const TRACK: u32 = 84935471;

/// rekordbox 7's collection as a cue edit finds it (`writable`, decrypted), as `master.db` in `dir`.
fn master_db(dir: &Path) -> PathBuf {
    let db = dir.join("rekordbox/master.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    std::fs::write(&db, loclib_rekordbox_db::test_util::writable().plain()).unwrap();
    db
}

/// `changes` waiting in the app's store to be written to `db`.
fn waiting(app: &App, db: &Path, changes: Vec<Change>) {
    let store = Store::open_or_init(&app.store_root).unwrap();
    let staged: Vec<Staged> = (changes.into_iter().enumerate())
        .map(|(i, change)| Staged { id: format!("s{i}"), at: chrono::Utc::now(), db: db.to_path_buf(), change })
        .collect();
    std::fs::create_dir_all(store.root().join("collection")).unwrap();
    std::fs::write(store.root().join("collection/staged.json"), serde_json::to_vec(&staged).unwrap()).unwrap();
}

/// Every cue of [`TRACK`] deleted.
fn deleted() -> Change {
    let track = CollectionTrack { id: TRACK.to_string(), name: "Daft Punk - One More Time".into(), after: Vec::new() };
    Change::Cues(CollectionCueEdit { tracks: vec![track], change: CueChange::Delete(CueRef::All), from: None })
}

fn showing_collection(app: &App) -> bool {
    app.loading.is_none() && app.loaded.as_ref().is_some_and(|l| l.is_collection())
}

/// The collection open in the player, with `changes` waiting; nothing looked at in the background
/// until a test says so.
fn opened_with(changes: Vec<Change>) -> (App, tempfile::TempDir, PathBuf) {
    let (mut app, dir) = blank(View::Library);
    let db = master_db(dir.path());
    app.collection_db = Some(db.clone());
    app.collection_writes.in_flight = true;
    waiting(&app, &db, changes);
    app.handle_msg(Msg::Drives(Vec::new()));
    wait(&mut app, showing_collection);
    (app, dir, db)
}

/// What's waiting shows (an undo waiting shows nothing yet); a look writes it or says why not, and
/// one refused isn't tried again until master.db changes.
#[test]
fn waiting_edits_show_are_looked_at_and_a_refused_one_waits_for_master_db_to_change() {
    let undo = Change::Undo { write: "w1".into(), what: "set hot cue A on 1 track(s)".into() };
    let (mut app, _dir, db) = opened_with(vec![deleted(), undo]);
    let l = app.loaded.as_ref().unwrap();
    assert!(l.collection_cues.as_ref().unwrap().get(&TRACK).is_none_or(Vec::is_empty), "shown as deleted");

    // A look, in the background: written, or (on a computer with no rekordbox agent's log to say
    // Cloud Library Sync is off, or rekordbox running) not.
    app.collection_writes.in_flight = false;
    app.watch_collection_writes();
    assert!(app.collection_writes.in_flight);
    wait(&mut app, |a| !a.collection_writes.in_flight);

    // Refused as master.db is now: not looked at again.
    app.collection_writes.refused_at = Some(CollectionStamp::of(&db));
    app.collection_writes.last = None;
    app.watch_collection_writes();
    assert!(!app.collection_writes.in_flight);
}

/// A write that passed over a track says so, and the status line counts what waits.
#[test]
fn a_write_says_what_it_passed_over_and_the_status_line_counts_what_waits() {
    let (mut app, _dir, db) = opened_with(Vec::new());
    let write = Ok(CollectionWrite { written: 2, warnings: vec!["a track was gone".into()], ..Default::default() });
    app.looked_at_collection(Looked { db: db.clone(), write });
    assert_eq!(
        last_message(&app),
        format!("wrote 2 cue edits to rekordbox's collection{UNDO_HINT} · a track was gone")
    );
    wait(&mut app, showing_collection);

    let why = Some("rekordbox's agent is running: its collection is written only once it has quit".to_string());
    app.looked_at_collection(Looked { db, write: Ok(CollectionWrite { waiting: 3, why, ..Default::default() }) });
    let s = crate::tui::ui::screen(&mut app, 160, 30);
    assert!(s.contains("3 cue edits waiting for rekordbox's agent to quit"), "{s}");
}

/// `u` on the collection drops the edit waiting, after showing what that does.
#[test]
fn undo_on_the_collection_drops_the_edit_waiting() {
    let (mut app, _dir, db) = opened_with(vec![deleted()]);
    let before = std::fs::read(&db).unwrap();
    app.undo_on(collection_root());
    let (operation, summary) = confirming(&mut app);
    assert_eq!(operation, "collection.undo");
    assert!(summary.contains("(waiting, not written yet)"), "{summary}");
    assert_eq!(say_yes(&mut app), "dropped the waiting cue edit: it was never written");
    assert_eq!(app.collection_writes.waiting, 0);
    assert_eq!(std::fs::read(&db).unwrap(), before);
}

/// What a cue edit or undo of the collection came to, as the footer says it.
#[test]
fn says_what_a_collection_write_came_to() {
    let (mut app, _dir, _db) = opened_with(Vec::new());
    let mut applied = |operation: &str, report: serde_json::Value| {
        let result = Ok(Box::new(Applied { operation: operation.into(), report }));
        app.handle_msg(Msg::Applied { job: 1, root: collection_root(), result });
        (last_message(&app).to_string(), app.collection_writes.waiting)
    };
    let kept = serde_json::json!({ "collection": { "written": 0, "waiting": 2, "why": "rekordbox is open" } });
    assert_eq!(
        applied("cues", kept),
        ("kept: 2 cue edits waiting for rekordbox and its agent to quit, then written to its collection".into(), 2)
    );
    let undone = serde_json::json!({ "dropped": false, "write": { "written": 1, "waiting": 0 } });
    assert_eq!(
        applied("collection.undo", undone).0,
        "undone in rekordbox's collection: the cues as they were, written as a new change"
    );
    let wrote = serde_json::json!({ "collection": { "written": 1, "waiting": 0, "warnings": ["a track was gone"] } });
    assert_eq!(
        applied("cues", wrote).0,
        format!("wrote 1 cue edit to rekordbox's collection{UNDO_HINT} · a track was gone")
    );
}
