use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::api::{self, CollectionRef, CueOp, EditCues};
use crate::cue_edit::{CuePosition, HotCue, SetCue};
use crate::query::TrackSelector;
use loclib_rekordbox_db::masterdb::DEFAULT_KEY;
use loclib_rekordbox_db::test_util::{Fixture, UPDATE_COUNT, codec, writable};

const TRACK: u32 = 84935471;

thread_local! {
    static RUNNING: AtomicBool = const { AtomicBool::new(false) };
}

fn running() -> Running {
    Running { rekordbox: RUNNING.with(|r| r.load(Ordering::Relaxed)), agent: false }
}

/// A collection (`f`'s, encrypted, in WAL mode as rekordbox keeps it) and a store, in temporary
/// folders; rekordbox closed and its agent's log saying sync is off, until a test says otherwise.
struct Lib {
    _dir: tempfile::TempDir,
    db: PathBuf,
    store: Store,
}

impl Lib {
    fn new(f: &Fixture) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let rekordbox = dir.path().join("rekordbox");
        std::fs::create_dir(&rekordbox).unwrap();
        let mut bytes = f.plain();
        bytes[18..20].copy_from_slice(&[2, 2]);
        let db = rekordbox.join("master.db");
        std::fs::write(&db, codec(DEFAULT_KEY).encrypt(&bytes).unwrap()).unwrap();
        let log = dir.path().join("log.log");
        std::fs::write(&log, "[261126T012213.828-0600] I/sync mode: null => standalone\n").unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        TEST_TARGET.with(|t| *t.borrow_mut() = Some((running, Some(log))));
        RUNNING.with(|r| r.store(false, Ordering::Relaxed));
        crate::api::THREAD_STORE_ROOT.with(|r| *r.borrow_mut() = Some(store.root().to_path_buf()));
        Lib { _dir: dir, db, store }
    }

    fn reference(&self) -> CollectionRef {
        CollectionRef { db: Some(self.db.clone()), key: None }
    }

    fn edit(&self, op: CueOp) -> EditCues {
        EditCues {
            drive: String::new(),
            collection: Some(self.reference()),
            tracks: TrackSelector { ids: vec![TRACK], ..Default::default() },
            edit: op,
        }
    }

    fn set(&self, hot: u8, ms: u32) -> crate::cue_edit::CueReport {
        let set = SetCue {
            hot_cue: Some(HotCue(hot)),
            at: CuePosition::Ms(ms),
            snap: false,
            loop_beats: None,
            name: None,
            color: None,
        };
        let plan = api::cues_plan(&self.edit(CueOp::Set(set)), &Ctx::silent()).unwrap();
        plan.apply(Some(&self.store), &Ctx::silent()).unwrap()
    }

    /// The track's cues as the collection reads them: (hot cue, start).
    fn cues(&self) -> Vec<(Option<u8>, u32)> {
        let c = masterdb::open(&self.db, DEFAULT_KEY).unwrap();
        c.cues.get(&TRACK).map(|l| l.iter().map(|c| (c.hot_cue, c.start_ms)).collect()).unwrap_or_default()
    }
}

#[test]
fn a_cue_edit_is_written_over_a_backup_in_the_store() {
    let lib = Lib::new(&writable());
    let before = lib.cues();
    assert_eq!(before, [(None, 500), (Some(0), 1000), (Some(1), 2000)]);
    let original = std::fs::read(&lib.db).unwrap();

    let report = lib.set(2, 3000);
    let w = report.collection.unwrap();
    assert_eq!((w.written, w.waiting, report.tracks_changed), (1, 0, 1));
    assert_eq!(w.count.unwrap().0, UPDATE_COUNT);
    assert!(w.count.unwrap().1 > UPDATE_COUNT);
    // The other cues are as they were.
    assert_eq!(lib.cues(), [(None, 500), (Some(0), 1000), (Some(1), 2000), (Some(2), 3000)]);
    // The backup is in the store, the file as it was.
    let backup = w.backup.unwrap();
    assert!(backup.starts_with(lib.store.root().join("collection/backups")));
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    let done = written(&lib.store, &lib.db).unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].what, "set hot cue C on 1 track(s)");
    assert!(staged(&lib.store, &lib.db).unwrap().is_empty());
}

#[test]
fn edits_made_while_rekordbox_runs_wait_and_are_shown_until_written() {
    let lib = Lib::new(&writable());
    let original = std::fs::read(&lib.db).unwrap();
    RUNNING.with(|r| r.store(true, Ordering::Relaxed));
    let w = lib.set(2, 3000).collection.unwrap();
    assert_eq!((w.written, w.waiting), (0, 1));
    assert!(w.why.unwrap().contains("rekordbox is open"));
    // A second one waits after it, planned on the cues as they'll be with the first.
    lib.set(3, 4000);
    assert_eq!(std::fs::read(&lib.db).unwrap(), original);
    let waiting = staged(&lib.store, &lib.db).unwrap();
    assert_eq!(waiting.len(), 2);

    // What the player shows meanwhile.
    let c = masterdb::open(&lib.db, DEFAULT_KEY).unwrap();
    let mut cues = c.cues.clone();
    let edits = waiting.iter().filter_map(|s| match &s.change {
        Change::Cues(e) => Some(e),
        Change::Undo { .. } => None,
    });
    crate::cue_edit::collection::overlay(&mut cues, &c.content_ids, edits);
    let shown: Vec<_> = cues[&TRACK].iter().map(|c| (c.hot_cue, c.start_ms)).collect();
    assert_eq!(shown, [(None, 500), (Some(0), 1000), (Some(1), 2000), (Some(2), 3000), (Some(3), 4000)]);

    // Once rekordbox has quit, both are written in one go, in order.
    RUNNING.with(|r| r.store(false, Ordering::Relaxed));
    let w = flush(&lib.store, &lib.db, DEFAULT_KEY, &Ctx::silent()).unwrap();
    assert_eq!((w.written, w.waiting), (2, 0));
    assert_eq!(lib.cues(), [(None, 500), (Some(0), 1000), (Some(1), 2000), (Some(2), 3000), (Some(3), 4000)]);
    assert_eq!(written(&lib.store, &lib.db).unwrap().len(), 2);
}

#[test]
fn undo_moves_forward_and_drops_what_waits() {
    let lib = Lib::new(&writable());
    let before = lib.cues();
    let first = lib.set(2, 3000).collection.unwrap().count.unwrap();

    let plan = undo_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap();
    assert!(!plan.noop && !plan.waiting);
    let r = plan.apply(Some(&lib.store), &Ctx::silent()).unwrap();
    assert_eq!(lib.cues(), before);
    // A new change, counted after the first.
    let undo = r.write.count.unwrap();
    assert!(undo.0 == first.1 && undo.1 > first.1, "{first:?} then {undo:?}");
    // Nothing more to undo: an undo isn't undone, the first is marked undone.
    assert!(undo_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap().noop);

    // One waiting is only dropped.
    RUNNING.with(|r| r.store(true, Ordering::Relaxed));
    lib.set(4, 5000);
    let plan = undo_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap();
    assert!(plan.waiting);
    assert!(plan.apply(Some(&lib.store), &Ctx::silent()).unwrap().dropped);
    assert!(staged(&lib.store, &lib.db).unwrap().is_empty());
}

#[test]
fn an_undo_leaves_a_track_rekordbox_changed_since() {
    let lib = Lib::new(&writable());
    lib.set(2, 3000);
    // As if rekordbox changed the track's cues since: they aren't what the write left.
    let mut w = written(&lib.store, &lib.db).unwrap();
    w[0].after[0].1.clear();
    remember(&lib.store, w).unwrap();
    let r = undo_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap().apply(Some(&lib.store), &Ctx::silent()).unwrap();
    assert_eq!(r.write.warnings.len(), 1, "{:?}", r.write.warnings);
    assert!(r.write.warnings[0].contains("changed since"));
    assert!(lib.cues().contains(&(Some(2), 3000)));
}

#[test]
fn cloud_library_sync_refuses_and_keeps_nothing() {
    let lib = Lib::new(&writable());
    TEST_TARGET.with(|t| *t.borrow_mut() = Some((running, None)));
    let original = std::fs::read(&lib.db).unwrap();
    let set = SetCue {
        hot_cue: Some(HotCue(2)),
        at: CuePosition::Ms(3000),
        snap: false,
        loop_beats: None,
        name: None,
        color: None,
    };
    let plan = api::cues_plan(&lib.edit(CueOp::Set(set)), &Ctx::silent()).unwrap();
    let e = plan.apply(Some(&lib.store), &Ctx::silent()).unwrap_err();
    assert!(e.to_string().contains("Cloud Library Sync"), "{e}");
    assert_eq!(std::fs::read(&lib.db).unwrap(), original);
    assert!(staged(&lib.store, &lib.db).unwrap().is_empty());
}

/// With rekordbox open, an edit with Cloud Library Sync on is refused now rather than kept: it'd
/// show on the deck as made and never be written.
#[test]
fn cloud_library_sync_refuses_an_edit_that_would_wait_too() {
    let lib = Lib::new(&writable());
    let log = lib.db.parent().unwrap().parent().unwrap().join("log.log");
    std::fs::write(&log, "[261126T012213.828-0600] I/sync mode: standalone => cloud\n").unwrap();
    RUNNING.with(|r| r.store(true, Ordering::Relaxed));
    let set = SetCue {
        hot_cue: Some(HotCue(2)),
        at: CuePosition::Ms(3000),
        snap: false,
        loop_beats: None,
        name: None,
        color: None,
    };
    let plan = api::cues_plan(&lib.edit(CueOp::Set(set)), &Ctx::silent()).unwrap();
    let e = plan.apply(Some(&lib.store), &Ctx::silent()).unwrap_err();
    assert!(e.to_string().contains("Cloud Library Sync"), "{e}");
    assert!(staged(&lib.store, &lib.db).unwrap().is_empty());
}

#[test]
fn repair_makes_the_lists_agree() {
    let lib = Lib::new(&writable());
    let plan = repair_plan(&lib.db, DEFAULT_KEY, false).unwrap();
    assert!(plan.noop, "no track has a list to disagree yet");
    let plan = repair_plan(&lib.db, DEFAULT_KEY, true).unwrap();
    assert_eq!(plan.tracks, [format!("{TRACK}  Daft Punk - One More Time")]);
    let w = plan.apply(Some(&lib.store), &Ctx::silent()).unwrap();
    assert_eq!(w.written, 1);
    assert!(repair_plan(&lib.db, DEFAULT_KEY, true).unwrap().noop);
    // A repair isn't undone.
    assert!(undo_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap().noop);
}

#[test]
fn only_the_latest_backups_are_kept() {
    let lib = Lib::new(&writable());
    let dir = folder(&lib.store).join("backups");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..BACKUPS_KEPT + 3 {
        std::fs::write(dir.join(format!("master.2020-01-01T0000{i:02}.000Z.db")), b"old").unwrap();
    }
    lib.set(2, 3000);
    let names: Vec<String> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names.len(), BACKUPS_KEPT);
    // The one just made is among them.
    assert!(names.iter().any(|n| !n.starts_with("master.2020")));
}
