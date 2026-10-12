//! Syncs, pulls and pushes around what the target holds beyond its library: a file of the DJ's
//! where the source keeps audio outside `Contents/` (taken as it is when it's the same, copied
//! again when a cut-off run left it, never overwritten otherwise), files that turn up or turn into
//! folders while a sync runs, a snapshot too big for the drive, a drive whose state no store
//! saved, and pulls that land a drive in the library it names, beside the head, or as a backup.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use filetime::FileTime;
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::{Device, LineageOp};
use locallibrary::error::Reason;
use locallibrary::fleet::{self, Hardware, Role};
use locallibrary::paths;
use locallibrary::progress::{CancelToken, Ctx, Event, Phase};
use locallibrary::store::Store;
use locallibrary::sync::{self, CopyReason, PullOptions, PushOptions, SyncOptions};
use locallibrary::{Error, Library};
use rekordbox_pdb::{NewTrack, PdbEditor};

const SET: &str = "Music/Set.mp3";
const AUDIO: &[u8] = b"A's audio";
/// When the source's file outside `Contents/` was written: long before any run.
const OLD: i64 = 1_600_000_000;

/// The twelve-track drive with one more track, whose audio rekordbox left at `Music/Set.mp3`.
fn with_audio_outside_contents(dir: &Path) -> PathBuf {
    let root = common::bigger_drive(dir, "A");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    ed.add_track(&NewTrack { file_size: AUDIO.len() as u32, ..NewTrack::new("Set", "/Music/Set.mp3") }).unwrap();
    ed.save(&pdb_path).unwrap();
    let set = root.join(SET);
    fs::create_dir_all(set.parent().unwrap()).unwrap();
    fs::write(&set, AUDIO).unwrap();
    filetime::set_file_mtime(&set, FileTime::from_unix_time(OLD, 0)).unwrap();
    root
}

/// What the target has at `Music/Set.mp3`.
#[derive(Debug, Clone, Copy)]
enum There {
    Nothing,
    Folder,
    /// A file: A's bytes or others (of the same length or not), written when A's was or now.
    File {
        bytes: u8,
        old: bool,
    },
}

/// A file outside the library at the path the source keeps audio: nothing there, it's copied; a
/// folder or a file of the DJ's, the sync is refused naming it; the same file (by content when
/// verifying, else by size and time) is taken as it is; and one a run cut off on this drive wrote
/// is copied again.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_file_outside_the_library_is_copied_kept_or_refused_as_it_is(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(with_audio_outside_contents(dir.path())).unwrap();
    let mut cases = vec![There::Nothing, There::Folder];
    for bytes in 0..3 {
        cases.push(There::File { bytes, old: tc.draw(gs::booleans()) });
    }
    for (n, there) in cases.into_iter().enumerate() {
        let verify = tc.draw(gs::booleans());
        let pending = tc.draw(gs::booleans());
        let root = common::blank(dir.path(), &format!("B{n}"));
        let mut b = Device::open(&root).unwrap();
        if pending {
            b.adopt(None).unwrap();
            b.begin(LineageOp::Sync, "sync from A", None).unwrap();
        }
        let at = root.join(SET);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        let mine: Vec<u8> = match there {
            There::Nothing => {
                fs::remove_dir(at.parent().unwrap()).unwrap();
                Vec::new()
            }
            There::Folder => {
                fs::create_dir(&at).unwrap();
                Vec::new()
            }
            There::File { bytes, old } => {
                let mine = match bytes {
                    0 => AUDIO.to_vec(),
                    1 => b"B's audio".to_vec(),
                    _ => b"mine".to_vec(),
                };
                fs::write(&at, &mine).unwrap();
                if old {
                    filetime::set_file_mtime(&at, FileTime::from_unix_time(OLD, 0)).unwrap();
                }
                mine
            }
        };
        let opts = SyncOptions { verify, ..SyncOptions::default() };
        let planned = sync::plan_drive_to_drive(&a, &b, &opts, &Ctx::silent()).map(|(p, _)| p);
        // What the sync rules say: a file written since the cut-off run started is that run's.
        let expect = match there {
            There::Nothing => Ok(Some((CopyReason::New, None))),
            There::Folder => Err(()),
            There::File { old, .. } => {
                let ours = pending && !old;
                let same = mine.len() == AUDIO.len() && if verify { mine == AUDIO } else { !ours && old };
                match (same, ours) {
                    (true, _) => Ok(None),
                    (false, true) => Ok(Some((CopyReason::Changed, Some(mine.len() as u64)))),
                    (false, false) => Err(()),
                }
            }
        };
        match (expect, planned) {
            (Err(()), Err(e)) => {
                assert_eq!(e.reason(), Some(Reason::InTheWay), "{there:?}: {e}");
                assert!(e.to_string().contains(SET), "{e}");
                assert!(!b.has_export(), "refused before anything is written");
            }
            (Ok(copy), Ok(plan)) => {
                let set = plan.copies.iter().find(|c| c.path == SET).map(|c| (c.reason, c.target_size));
                assert_eq!(set, copy, "{there:?}, verify {verify}, pending {pending}");
                if copy.is_some() {
                    sync::sync_drive_to_drive(&mut a, &mut b, &opts, &Ctx::silent()).unwrap();
                    assert_eq!(fs::read(&at).unwrap(), AUDIO);
                    assert!(sync::plan_drive_to_drive(&a, &b, &opts, &Ctx::silent()).unwrap().0.is_noop());
                }
            }
            (want, got) => panic!("{there:?} (verify {verify}, pending {pending}): want {want:?}, got {got:?}"),
        }
        if !matches!(there, There::Nothing) && fs::read(&at).ok().is_some_and(|b| b != AUDIO) {
            assert_eq!(fs::read(&at).unwrap(), mine, "the DJ's file is never overwritten");
        }
    }
}

/// What turns up on the target while a sync runs stops it, leaving the DJ's file alone: a file at
/// the path the source keeps audio outside `Contents/` (written once the first file is copied:
/// nothing is written there, and the old library stays), or a stale file that became a folder
/// before it's deleted (the new library is in by then; the folder stays, and the sync says which).
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_sync_stops_at_what_turns_up_on_the_target_while_it_runs(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(with_audio_outside_contents(dir.path())).unwrap();
    let lib = a.library().unwrap();

    // A file of the DJ's appears where A keeps audio.
    let root = common::blank(dir.path(), "B");
    let mut b = Device::open(&root).unwrap();
    let mine = tc.draw(gs::sampled_from(vec![b"mine".to_vec(), AUDIO.to_vec()]));
    let at = root.join(SET);
    let appear = |e: &Event| {
        if let Event::Item { phase: Phase::Copy, path, .. } = e
            && path != SET
            && !at.exists()
        {
            fs::create_dir_all(at.parent().unwrap()).unwrap();
            fs::write(&at, &mine).unwrap();
        }
    };
    let ctx = Ctx::new(&appear, CancelToken::new());
    let err = sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &ctx).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::InTheWay), "{err}");
    assert!(err.to_string().contains("appeared on"), "{err}");
    assert_eq!(fs::read(&at).unwrap(), mine);
    assert!(!b.has_export(), "the databases go in only after every file");
    assert!(Device::open(&root).unwrap().state().unwrap().pending.is_some(), "marked as cut off");

    // Stale files, one of which becomes a folder before it's deleted.
    let root = common::blank(dir.path(), "C");
    let mut c = Device::open(&root).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut c, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let stale: Vec<String> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
        .map(|n| format!("Contents/Gone/Old {n}.mp3"))
        .collect();
    for key in &stale {
        let p = root.join(key);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"stale").unwrap();
    }
    let folder = tc.draw(gs::sampled_from(stale.clone()));
    let turns = |e: &Event| {
        if let Event::Item { phase: Phase::Delete, path, .. } = e
            && *path == folder
        {
            let p = root.join(path);
            fs::remove_file(&p).unwrap();
            fs::create_dir_all(p.join("kept")).unwrap();
        }
    };
    let ctx = Ctx::new(&turns, CancelToken::new());
    let mut c = Device::open(&root).unwrap();
    let err = sync::sync_drive_to_drive(&mut a, &mut c, &SyncOptions::default(), &ctx).unwrap_err();
    assert!(matches!(err.inner(), Error::Io { path, .. } if path.ends_with(&folder)), "{err}");
    assert!(root.join(&folder).join("kept").is_dir());
    for key in stale.iter().filter(|k| **k < folder) {
        assert!(!root.join(key).exists(), "{key} went before it");
    }
    let held: Library = common::library(&root);
    assert_eq!(held.tracks.len(), lib.tracks.len(), "the library is in");
}

/// A snapshot too big for the drive is refused before anything is written, saying how much it
/// needs; the plan said it wouldn't fit.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_push_too_big_for_the_drive_is_refused_before_anything_is_written(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut a = Device::open_export(common::bigger_drive(dir.path(), "A")).unwrap();
    let pulled = sync::pull(&store, &mut a, &PullOptions::default(), &Ctx::silent()).unwrap();
    let mut snap = store.load_snapshot(&pulled.snapshot.unwrap().id).unwrap();
    let audio: Vec<usize> = (0..snap.files.len()).filter(|i| snap.files[*i].class == paths::FileClass::Audio).collect();
    let big = tc.draw(gs::sampled_from(audio));
    snap.files[big].size = 1 << 50;
    let root = common::blank(dir.path(), "B");
    let mut b = Device::open(&root).unwrap();
    let opts = PushOptions { force: tc.draw(gs::booleans()), ..Default::default() };
    let plan = sync::plan_push(&store, &snap, &b, &opts, &Ctx::silent()).unwrap();
    assert!(!plan.fits && plan.warnings.iter().any(|w| w.starts_with("needs ")), "{:?}", plan.warnings);
    let err = sync::push(&store, &snap, &mut b, &opts, &Ctx::silent()).unwrap_err();
    match err.inner() {
        Error::NoSpace { needed, available, .. } => {
            assert_eq!(*needed, plan.bytes_needed());
            assert!(available < needed);
        }
        other => panic!("{other}"),
    }
    assert!(!b.has_export() && common::drive_files(&root).is_empty(), "nothing written");
}

/// A drive holding a library no store saved isn't overwritten by a push unless forced, whichever
/// store's snapshot it would get; forced, it plans.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_drive_whose_library_no_store_saved_is_overwritten_only_when_forced(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (store, other) = (
        Store::open_or_init(dir.path().join("store")).unwrap(),
        Store::open_or_init(dir.path().join("other")).unwrap(),
    );
    let mut snaps = Vec::new();
    for (n, s) in [&store, &other].into_iter().enumerate() {
        let root = common::demo_drive(dir.path(), &format!("Saved {n}"));
        let mut d = Device::open_export(&root).unwrap();
        let pulled = sync::pull(s, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
        snaps.push(s.load_snapshot(&pulled.snapshot.unwrap().id).unwrap());
    }
    // The drive: the twelve-track export, never pulled, perhaps with one more track.
    let root = common::bigger_drive(dir.path(), "Unsaved");
    if tc.draw(gs::booleans()) {
        common::export_new_track(&root, "New", "Track");
    }
    let d = Device::open_export(&root).unwrap();
    for snap in &snaps {
        let err = sync::plan_push(&store, snap, &d, &PushOptions::default(), &Ctx::silent()).unwrap_err();
        assert_eq!(err.reason(), Some(Reason::NotSaved), "{err}");
        let forced = PushOptions { force: true, ..Default::default() };
        let plan = sync::plan_push(&store, snap, &d, &forced, &Ctx::silent()).unwrap();
        assert!(!plan.copies.is_empty(), "the demo library is copied over");
    }
}

/// Pulls land a drive where it says: into a store that lacks its library, a new library named
/// after it; beside the head, up to date while unchanged, a snapshot beside it otherwise, and the
/// head stays; into a library that has a primary already, the drive (a primary elsewhere) is its
/// backup. The registry keeps the disk the drive recorded.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn pulls_land_a_drive_in_the_library_it_says(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let pull = |s: &Store, root: &Path, opts: &PullOptions| {
        sync::pull(s, &mut Device::open_export(root).unwrap(), opts, &Ctx::silent()).unwrap()
    };
    let one = common::bigger_drive(dir.path(), "One");
    let hardware = tc.draw(gs::booleans()).then(|| Hardware {
        model: "Cruzer".into(),
        serial: Some("4C530001123456789012".into()),
        volume_uuid: None,
        size: 64_000_000_000,
    });
    let mut d = Device::open_export(&one).unwrap();
    d.adopt(None).unwrap();
    d.state_mut().unwrap().hardware = hardware.clone();
    d.save_state().unwrap();
    let first = pull(&store, &one, &PullOptions::default());
    let id = Device::open(&one).unwrap().id().unwrap();
    assert_eq!(store.drives().unwrap()[&id].hardware, hardware);

    // Beside the head.
    let saved = first.snapshot.clone().unwrap();
    let changed = tc.draw(gs::booleans());
    if changed {
        common::export_new_track(&one, "Later", "Beside");
    }
    let mut d = Device::open_export(&one).unwrap();
    let beside = sync::pull_beside(&store, &mut d, &PullOptions::default(), saved.id, &Ctx::silent()).unwrap();
    assert_eq!(beside.up_to_date, !changed, "{beside:?}");
    assert_eq!(beside.snapshot.as_ref().map(|s| s.id == saved.id), Some(!changed));
    assert_eq!(store.find_library(&first.library).unwrap().unwrap().head, Some(saved.id), "the head stays");

    // Another store lacks the drive's library: it starts one.
    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    let there = pull(&other, &one, &PullOptions::default());
    assert!(!there.up_to_date);
    assert_eq!(other.libraries().unwrap().iter().map(|l| l.name.clone()).collect::<Vec<_>>(), [there.library]);

    // A primary pulled into a library that has one becomes its backup.
    let two = common::demo_drive(dir.path(), "Two");
    let second = pull(&store, &two, &PullOptions::default());
    let two_id = Device::open(&two).unwrap().id().unwrap();
    for drive in [id, two_id] {
        fleet::set_role(&store, drive, Some(Role::Primary)).unwrap();
    }
    pull(&store, &one, &PullOptions { library: Some(second.library.clone()), force: true, ..Default::default() });
    let drives = store.drives().unwrap();
    assert_eq!((drives[&id].role, drives[&two_id].role), (Some(Role::Backup), Some(Role::Primary)));
    assert_eq!(drives[&id].library_id, drives[&two_id].library_id);
}

/// Watches a sync to several drives: what each target's copy reports, and the target whose
/// reporting falls over (a panic in its thread).
struct Watch {
    fail: Option<String>,
    copied: std::sync::Mutex<Vec<(Option<String>, String)>>,
}

impl locallibrary::progress::Progress for Watch {
    fn on_event(&self, _: &Event) {}

    fn on_scoped_event(&self, scope: &locallibrary::progress::Scope, event: &Event) {
        if let Event::Item { phase: Phase::Copy, path, .. } = event {
            self.copied.lock().unwrap().push((scope.target.clone(), path.clone()));
            if scope.target.is_some() && scope.target == self.fail {
                panic!("the progress bar fell over");
            }
        }
    }
}

/// A sync to several drives names and scopes what each target's copy reports to that drive, and
/// one that fails (its thread panics) says why without stopping the others, which end up holding
/// the source's library.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_sync_to_several_drives_goes_on_past_one_that_fails(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(3));
    let mut targets: Vec<Device> =
        (0..n).map(|i| Device::open(common::blank(dir.path(), &format!("B{i}"))).unwrap()).collect();
    let failing = tc.draw(gs::integers::<usize>().max_value(n - 1));
    let label = |d: &Device| d.root().display().to_string();
    let watch = Watch { fail: Some(label(&targets[failing])), copied: Default::default() };
    let ctx = Ctx::new(&watch, CancelToken::new());
    let results = sync::sync_drive_to_many(&mut a, &mut targets, &SyncOptions::default(), &ctx).unwrap();
    assert_eq!(results.len(), n);
    for (i, (r, t)) in results.iter().zip(&targets).enumerate() {
        match r {
            Err(e) => {
                assert_eq!(i, failing, "{e}");
                assert!(e.to_string().ends_with("a sync thread panicked: the progress bar fell over"), "{e}");
            }
            Ok(report) => {
                assert_ne!(i, failing);
                assert_eq!(report.copied, common::library_files(a.root()).len());
                assert_eq!(common::library_files(a.root()), common::library_files(t.root()));
            }
        }
    }
    for (target, path) in watch.copied.lock().unwrap().iter() {
        let t = targets.iter().find(|t| Some(label(t)) == *target).expect("scoped to a target");
        assert!(path.starts_with(&format!("{}: ", t.name())), "{path}");
    }
}

/// When the DJ's own files stand where the source keeps several tracks' audio outside
/// `Contents/`, the refusal names the first three and counts the rest, and lists every one.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn every_file_in_the_way_is_listed(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(6));
    let keys: Vec<String> = (0..n).map(|k| format!("Music/Set {k}.mp3")).collect();
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let b = common::blank(dir.path(), "B");
    for key in &keys {
        ed.add_track(&NewTrack { file_size: AUDIO.len() as u32, ..NewTrack::new(key, format!("/{key}")) }).unwrap();
        for (drive, bytes) in [(&root, AUDIO), (&b, &b"the DJ's"[..])] {
            fs::create_dir_all(drive.join("Music")).unwrap();
            fs::write(drive.join(key), bytes).unwrap();
        }
    }
    ed.save(&pdb_path).unwrap();
    let a = Device::open_export(&root).unwrap();
    let err =
        sync::plan_drive_to_drive(&a, &Device::open(&b).unwrap(), &SyncOptions::default(), &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::InTheWay), "{err}");
    let shown = keys.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    let more = if n > 3 { format!(" and {} more", n - 3) } else { String::new() };
    assert!(err.to_string().contains(&format!("({shown}{more})")), "{err}");
    assert_eq!(err.detail().fields["paths"], serde_json::json!(keys));
}
