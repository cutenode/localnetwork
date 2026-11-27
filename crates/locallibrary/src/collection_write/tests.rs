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

/// The track's cues as the collection reads them, every track: what a property compares.
type Shown = Vec<(u32, Vec<(Option<u8>, u32, Option<u32>, String, Option<[u8; 3]>)>)>;

fn shown(cues: &indexmap::IndexMap<u32, Vec<loclib_model::analysis::Cue>>) -> Shown {
    let mut out: Shown = cues
        .iter()
        .map(|(id, l)| {
            let mut l: Vec<_> =
                l.iter().map(|c| (c.hot_cue, c.start_ms, c.end_ms, c.comment.clone(), c.color)).collect();
            l.sort();
            (*id, l)
        })
        .filter(|(_, l)| !l.is_empty())
        .collect();
    out.sort();
    out
}

impl Lib {
    /// The collection's cues with what's waiting over them: what the player shows.
    fn shown(&self) -> Shown {
        let mut c = masterdb::open(&self.db, DEFAULT_KEY).unwrap();
        let waiting = staged(&self.store, &self.db).unwrap();
        let edits = waiting.iter().filter_map(|s| match &s.change {
            Change::Cues(e) => Some(e),
            Change::Undo { .. } => None,
        });
        crate::cue_edit::collection::overlay(&mut c.cues, &c.content_ids, edits);
        shown(&c.cues)
    }
}

/// Any run of cue edits of the collection, some made while rekordbox runs: none changes master.db
/// while it runs, `localUpdateCount` only rises, what's written once rekordbox quits is what the
/// player showed meanwhile, and undoing every edit puts every cue back as it was.
/// Each case writes a few encrypted copies, so they're few.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_collection_cue_edits_land_as_shown_and_undo_puts_them_back(tc: hegel::TestCase) {
    use crate::cue_edit::{AutoCues, CueRef, CueStyle, UpdateCue};
    use hegel::generators as gs;

    let lib = Lib::new(&writable());
    let original = lib.shown();
    let mut count = UPDATE_COUNT;
    let mut rose = |w: &CollectionWrite| {
        if let Some((was, now)) = w.count {
            assert!(was >= count && now > was, "localUpdateCount went {count} then {was} -> {now}");
            count = now;
        }
    };
    let steps = tc.draw(gs::integers::<usize>().min_value(1).max_value(5));
    for _ in 0..steps {
        let running = tc.draw(gs::booleans());
        RUNNING.with(|r| r.store(running, Ordering::Relaxed));
        let before = std::fs::read(&lib.db).unwrap();
        let hot = HotCue(tc.draw(gs::integers::<u8>().max_value(7)));
        let ms = tc.draw(gs::sampled_from(vec![0u32, 500, 1000, 2500, 4000]));
        let set = |hot_cue| SetCue {
            hot_cue,
            at: CuePosition::Ms(ms),
            snap: false,
            loop_beats: None,
            name: None,
            color: None,
        };
        let op = match tc.draw(gs::integers::<u8>().max_value(8)) {
            0 => Some(CueOp::Set(set(Some(hot)))),
            1 => Some(CueOp::Set(set(None))),
            2 => Some(CueOp::Update(UpdateCue { cue: CueRef::Hot(hot), name: Some("Named".into()), color: None })),
            3 => {
                let colour = tc.draw(gs::sampled_from(vec!["red", "aqua", "none"]));
                Some(CueOp::Update(UpdateCue { cue: CueRef::Hot(hot), name: None, color: Some(colour.into()) }))
            }
            4 => Some(CueOp::Delete { cue: if tc.draw(gs::booleans()) { CueRef::All } else { CueRef::Hot(hot) } }),
            5 => Some(CueOp::Copy { from: TrackSelector { ids: vec![TRACK], ..Default::default() } }),
            6 => Some(CueOp::Auto(AutoCues { style: CueStyle::Rekordbox, overwrite: false })),
            7 => {
                let plan = undo_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap();
                let r = plan.apply(Some(&lib.store), &Ctx::silent()).unwrap();
                rose(&r.write);
                None
            }
            _ => {
                let plan = apply_staged_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap();
                rose(&plan.apply(Some(&lib.store), &Ctx::silent()).unwrap());
                None
            }
        };
        if let Some(op) = op {
            let mut req = lib.edit(op);
            if matches!(req.edit, CueOp::Copy { .. }) {
                req.tracks.ids = vec![205356547];
            }
            // An edit that doesn't apply (no such cue) is refused when planned.
            if let Ok(plan) = api::cues_plan(&req, &Ctx::silent()) {
                let r = plan.apply(Some(&lib.store), &Ctx::silent()).unwrap();
                rose(r.collection.as_ref().unwrap_or(&CollectionWrite::default()));
            }
        }
        if running {
            assert_eq!(std::fs::read(&lib.db).unwrap(), before, "master.db changed while rekordbox ran");
        }
    }

    // rekordbox quits: what's waiting is written as it was shown.
    RUNNING.with(|r| r.store(false, Ordering::Relaxed));
    let undo_waits = staged(&lib.store, &lib.db).unwrap().iter().any(|s| matches!(s.change, Change::Undo { .. }));
    let showing = lib.shown();
    rose(&flush(&lib.store, &lib.db, DEFAULT_KEY, &Ctx::silent()).unwrap());
    if !undo_waits {
        assert_eq!(lib.shown(), showing);
    }

    // Every edit undone, newest first: the cues as they were.
    for _ in 0..20 {
        let plan = undo_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap();
        if plan.noop {
            break;
        }
        let r = plan.apply(Some(&lib.store), &Ctx::silent()).unwrap();
        assert!(r.write.warnings.is_empty(), "{:?}", r.write.warnings);
        rose(&r.write);
    }
    assert_eq!(lib.shown(), original);

    // Giving every track with cues a list changes no cue, and isn't undone.
    let plan = repair_plan(&lib.db, DEFAULT_KEY, true).unwrap();
    if !plan.noop {
        rose(&plan.apply(Some(&lib.store), &Ctx::silent()).unwrap());
    }
    assert!(repair_plan(&lib.db, DEFAULT_KEY, true).unwrap().noop);
    assert!(undo_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap().noop);
    assert_eq!(lib.shown(), original);
}

/// Another track of [`writable`]'s, with no cues.
const OTHER: u32 = 205356547;

fn set_cue(hot: Option<u8>, ms: u32) -> SetCue {
    SetCue { hot_cue: hot.map(HotCue), at: CuePosition::Ms(ms), snap: false, loop_beats: None, name: None, color: None }
}

impl Lib {
    fn collection(&self) -> masterdb::Collection {
        masterdb::open(&self.db, DEFAULT_KEY).unwrap()
    }

    /// [`TRACK`] analysed: the demo track's analysis files, beat grid and phrases, where its
    /// `AnalysisDataPath` says in rekordbox's `share` folder.
    fn analysed(&self) {
        for file in ["ANLZ0000.DAT", "ANLZ0000.EXT"] {
            let at = Path::new("PIONEER/USBANLZ/P016/0000875E").join(file);
            let to = self.db.parent().unwrap().join("share").join(&at);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks");
            std::fs::copy(fixtures.join(at), to).unwrap();
        }
    }
}

#[test]
fn a_plan_refuses_what_it_cant_do_and_passes_over_what_it_cant_place() {
    use crate::cue_edit::collection::plan;
    use crate::cue_edit::{AutoCues, CueEdit, CueStyle};

    // TRACK's grid runs past its end, cut short to 5s; OTHER has no grid.
    let f = writable();
    f.exec("UPDATE djmdContent SET Length = 5 WHERE ID = '84935471';");
    let lib = Lib::new(&f);
    lib.analysed();
    let c = lib.collection();
    let set = CueEdit::Set(set_cue(Some(2), 3000));
    assert!(plan(&c, &[], &set).unwrap_err().to_string().contains("no tracks selected"));
    assert!(plan(&c, &[TRACK, OTHER], &set).unwrap_err().to_string().contains("2 tracks selected; pick one"));

    let auto = CueEdit::Auto(AutoCues { style: CueStyle::Rekordbox, overwrite: false });
    let p = plan(&c, &[TRACK, OTHER], &auto).unwrap();
    assert!(p.tracks.is_empty(), "{:?}", p.tracks);
    assert_eq!(p.warnings.len(), 2, "{:?}", p.warnings);
    assert!(p.warnings.iter().any(|w| w.contains("past the end")), "{:?}", p.warnings);
    assert!(p.warnings.iter().any(|w| w.contains("has no beat grid")), "{:?}", p.warnings);

    // A copy leaves out the track it's from.
    let p = plan(&c, &[TRACK, OTHER], &CueEdit::Copy { from: TRACK }).unwrap();
    assert_eq!(p.tracks.len(), 1);

    // A memory cue's colour isn't written, and the plan says so.
    let coloured = SetCue { color: Some("red".into()), ..set_cue(None, 3000) };
    for edit in [CueEdit::Set(coloured.clone()), CueEdit::SetMany(vec![coloured])] {
        let p = plan(&c, &[TRACK], &edit).unwrap();
        assert!(p.warnings.iter().any(|w| w.contains("memory cue's colour")), "{:?}", p.warnings);
    }
}

#[test]
fn a_waiting_edit_of_a_track_the_collection_lacks_shows_nothing() {
    use crate::cue_edit::{CollectionCueEdit, CollectionTrack, CueChange, CueRef};

    let lib = Lib::new(&writable());
    let c = lib.collection();
    let mut cues = c.cues.clone();
    let gone = CollectionTrack { id: "gone".into(), name: "Gone".into(), after: Vec::new() };
    let edit = CollectionCueEdit { tracks: vec![gone], change: CueChange::Delete(CueRef::All), from: None };
    crate::cue_edit::collection::overlay(&mut cues, &c.content_ids, [&edit]);
    assert_eq!(shown(&cues), shown(&c.cues));
}

/// Each kind of edit waiting is made again on the cues as they are when it's written; one that no
/// longer changes anything is passed over, and one that no longer applies is said.
#[test]
fn waiting_edits_are_made_on_the_cues_as_they_are_when_written() {
    use crate::cue_edit::{AutoCues, CollectionCueEdit, CollectionTrack, CueChange, CueRef, CueStyle, UpdateCue};

    let lib = Lib::new(&writable());
    let on = |id: u32, change: CueChange, from: Option<&str>| {
        let track = CollectionTrack { id: id.to_string(), name: format!("track {id}"), after: Vec::new() };
        Change::Cues(CollectionCueEdit { tracks: vec![track], change, from: from.map(str::to_string) })
    };
    let many = CueChange::SetMany(vec![set_cue(Some(2), 3000), set_cue(Some(3), 4000)]);
    let named = CueChange::Update(UpdateCue { cue: CueRef::Hot(HotCue(2)), name: Some("Named".into()), color: None });
    let carry = CueChange::Carry { from: String::new(), shift_ms: 10 };
    let auto = CueChange::Auto(AutoCues { style: CueStyle::Rekordbox, overwrite: false });
    let copy = CueChange::Copy { from: String::new() };
    let forgotten = Change::Undo { write: "forgotten".into(), what: "set hot cue A on 1 track(s)".into() };
    for change in [
        on(TRACK, many.clone(), None),
        on(TRACK, named, None),
        on(OTHER, carry, Some(&TRACK.to_string())),
        // No grid on OTHER: nothing placed.
        on(OTHER, auto, None),
        // Made already.
        on(TRACK, many, None),
        // From a track that's gone.
        on(TRACK, copy, Some("gone")),
        forgotten,
    ] {
        stage(&lib.store, &lib.db, change).unwrap();
    }
    let w = flush(&lib.store, &lib.db, DEFAULT_KEY, &Ctx::silent()).unwrap();
    assert_eq!((w.written, w.waiting), (7, 0));
    assert_eq!(w.warnings.len(), 2, "{:?}", w.warnings);
    assert!(
        w.warnings[0].contains("on track 84935471: not found: track gone in rekordbox's collection"),
        "{:?}",
        w.warnings
    );
    assert!(w.warnings[1].contains("that write isn't remembered any more"), "{:?}", w.warnings);

    let c = lib.collection();
    let track = &c.cues[&TRACK];
    assert!(track.iter().any(|c| c.hot_cue == Some(2) && c.start_ms == 3000 && c.comment == "Named"), "{track:?}");
    assert!(track.iter().any(|c| c.hot_cue == Some(3) && c.start_ms == 4000), "{track:?}");
    let carried: Vec<u32> = c.cues[&OTHER].iter().map(|c| c.start_ms).collect();
    assert!(carried.contains(&510) && carried.contains(&3010), "{carried:?}");
}

/// While rekordbox's agent runs, or rekordbox starts while the write is made, the edits wait.
#[test]
fn edits_wait_for_the_agent_and_for_a_rekordbox_that_starts_meanwhile() {
    thread_local! {
        static LOOKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    fn agent() -> Running {
        Running { rekordbox: false, agent: true }
    }
    /// rekordbox starts after the first look.
    fn starts() -> Running {
        let n = LOOKS.with(|l| l.replace(l.get() + 1));
        Running { rekordbox: n > 0, agent: false }
    }

    let lib = Lib::new(&writable());
    let original = std::fs::read(&lib.db).unwrap();
    stage(&lib.store, &lib.db, Change::Undo { write: "w".into(), what: "w".into() }).unwrap();
    let log = TEST_TARGET.with(|t| t.borrow().clone().unwrap().1);
    TEST_TARGET.with(|t| *t.borrow_mut() = Some((agent, log.clone())));
    let w = flush(&lib.store, &lib.db, DEFAULT_KEY, &Ctx::silent()).unwrap();
    assert_eq!((w.written, w.waiting), (0, 1));
    assert!(w.why.unwrap().contains("agent"));

    TEST_TARGET.with(|t| *t.borrow_mut() = Some((starts, log)));
    let w = flush(&lib.store, &lib.db, DEFAULT_KEY, &Ctx::silent()).unwrap();
    assert_eq!((w.written, w.waiting), (0, 1));
    assert!(w.why.unwrap().contains("rekordbox is open"));
    assert_eq!(std::fs::read(&lib.db).unwrap(), original);
}

#[test]
fn an_undo_of_nothing_does_nothing_and_one_written_meanwhile_is_stale() {
    let lib = Lib::new(&writable());
    let plan = undo_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap();
    assert!(plan.noop);
    let r = plan.apply(Some(&lib.store), &Ctx::silent()).unwrap();
    assert!(!r.dropped && r.write == CollectionWrite::default());

    RUNNING.with(|r| r.store(true, Ordering::Relaxed));
    lib.set(2, 3000);
    let plan = undo_plan(&lib.store, &lib.db, DEFAULT_KEY).unwrap();
    assert!(plan.waiting);
    RUNNING.with(|r| r.store(false, Ordering::Relaxed));
    flush(&lib.store, &lib.db, DEFAULT_KEY, &Ctx::silent()).unwrap();
    let e = plan.apply(Some(&lib.store), &Ctx::silent()).unwrap_err();
    assert!(e.to_string().contains("written meanwhile"), "{e}");
}

/// Writing needs a store for its backups: the one named when none is given.
#[test]
fn a_write_keeps_its_backups_in_the_store_named() {
    let lib = Lib::new(&writable());
    assert_eq!(store_for(None).unwrap().root(), lib.store.root());
    crate::api::THREAD_STORE_ROOT.with(|r| *r.borrow_mut() = None);
    if crate::api::store_root().is_none() {
        assert!(store_for(None).unwrap_err().to_string().contains("needs locallibrary's store"));
    }
}

/// The API's collection plans name the collection by a [`CollectionRef`].
#[test]
fn the_api_plans_repair_apply_and_undo_of_the_collection_named() {
    let lib = Lib::new(&writable());
    let r = lib.reference();
    assert_eq!(api::collection_repair_plan(&r, true).unwrap().tracks.len(), 1);
    RUNNING.with(|r| r.store(true, Ordering::Relaxed));
    lib.set(2, 3000);
    assert_eq!(api::collection_apply_plan(&lib.store, &r).unwrap().edits, ["set hot cue C on 1 track(s)"]);
    assert!(api::collection_undo_plan(&lib.store, &r).unwrap().waiting);
}

/// A cue edit of the collection is planned over what's waiting (an undo shows as nothing yet),
/// with or without a store, and a copy is from one track.
#[test]
fn collection_cue_edits_are_planned_over_what_waits() {
    use crate::cue_edit::{AutoCues, CueStyle};

    let lib = Lib::new(&writable());
    stage(&lib.store, &lib.db, Change::Undo { write: "w".into(), what: "w".into() }).unwrap();
    let auto = lib.edit(CueOp::Auto(AutoCues { style: CueStyle::FirstCue, overwrite: false }));
    let plan = api::cues_plan(&auto, &Ctx::silent()).unwrap();
    assert!(plan.tracks.is_empty() && plan.warnings[0].contains("no beat grid"), "{:?}", plan.warnings);

    let both = TrackSelector { ids: vec![TRACK, OTHER], ..Default::default() };
    let mut copy = lib.edit(CueOp::Copy { from: both });
    copy.tracks.ids = vec![OTHER];
    let e = api::cues_plan(&copy, &Ctx::silent()).unwrap_err();
    assert!(e.to_string().contains("copy from one track (2 selected)"), "{e}");

    crate::api::THREAD_STORE_ROOT.with(|r| *r.borrow_mut() = None);
    if crate::api::store_root().is_none() {
        let set = lib.edit(CueOp::Set(set_cue(Some(2), 3000)));
        assert_eq!(api::cues_plan(&set, &Ctx::silent()).unwrap().tracks.len(), 1);
    }
}
