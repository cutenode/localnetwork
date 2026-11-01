//! Properties of saving drives into the store as they're plugged in (`api::autopull`) and of the
//! terminal player's plans (`api::tui`): which drives are looked at follows a model of the
//! events seen, and what looking at a drive does follows from how it differs from the store.

use crate::common::{self, path};

use indexmap::IndexSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::api::{
    self, AutoPull, DriveRef, EjectDrives, PullRequest, WatchAction, WatchOptions, Watched, Watching,
};
use locallibrary::compare::Relation;
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::watch::{DriveChange, MountedDrive, VolumeWatcher, WatchEvent};
use locallibrary::{Error, fleet};

fn mounted(root: &Path, has_export: bool) -> MountedDrive {
    MountedDrive {
        root: root.to_path_buf(),
        id: None,
        name: root.file_name().unwrap().to_string_lossy().into(),
        has_export,
    }
}

fn kind(a: &WatchAction) -> String {
    serde_json::to_value(a).unwrap()["event"].as_str().unwrap().to_string()
}

struct Due {
    _dir: tempfile::TempDir,
    store_root: PathBuf,
    auto: AutoPull,
    /// A drive never linked to a library, one adopted but in no library, and a blank folder.
    drives: Vec<PathBuf>,
    /// The model: the drives due to be looked at.
    due: IndexSet<PathBuf>,
}

impl Due {
    /// What looking at a drive says, without a store: the one never adopted isn't linked; the
    /// adopted one finds no store; the blank one has nothing to look at.
    fn expected(&self, root: &Path) -> Option<&'static str> {
        match self.drives.iter().position(|d| d == root) {
            Some(0) => Some("not linked to a library in the store (pull it once to start one)"),
            Some(1) => Some("there's no store yet (pull a drive once to start one)"),
            _ => None,
        }
    }
}

#[hegel::state_machine]
impl Due {
    /// A drive plugged in, changed or unplugged, as a poll sees it (it may say it has an export
    /// when it hasn't any longer).
    #[rule]
    fn observe(&mut self, tc: TestCase) {
        let mut events = Vec::new();
        for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
            let root = tc.draw(gs::sampled_from(self.drives.clone()));
            let d = mounted(&root, tc.draw(gs::booleans()));
            let e = match tc.draw(gs::integers::<u8>().max_value(3)) {
                0 => WatchEvent::Attached(d),
                1 => WatchEvent::Detached(d),
                2 => WatchEvent::Changed { drive: d, changed: vec![DriveChange::Databases] },
                _ => WatchEvent::Changed { drive: d, changed: vec![DriveChange::State] },
            };
            match &e {
                WatchEvent::Attached(d) | WatchEvent::Changed { drive: d, .. } if !d.has_export => {}
                WatchEvent::Attached(d) => {
                    self.due.insert(d.root.clone());
                }
                WatchEvent::Changed { drive, changed } if changed.contains(&DriveChange::Databases) => {
                    self.due.insert(drive.root.clone());
                }
                WatchEvent::Detached(d) => {
                    self.due.shift_remove(&d.root);
                }
                _ => {}
            }
            events.push(e);
        }
        self.auto.observe(&events);
    }

    /// Watching starts with these drives mounted.
    #[rule]
    fn check_now(&mut self, tc: TestCase) {
        let drives: Vec<MountedDrive> = self.drives.iter().map(|r| mounted(r, tc.draw(gs::booleans()))).collect();
        self.due.extend(drives.iter().filter(|d| d.has_export).map(|d| d.root.clone()));
        self.auto.check_now(&drives);
    }

    /// Looking at the drives due, among those mounted, says what the model says of each, in
    /// path order, and leaves none of them due; until they've settled, nothing is looked at.
    #[rule]
    fn run(&mut self, tc: TestCase) {
        let on: Vec<PathBuf> = tc.draw(gs::vecs(gs::sampled_from(self.drives.clone())).max_size(3).unique(true));
        let mounted: Vec<MountedDrive> = on.iter().map(|r| mounted(r, true)).collect();
        let mut did = Vec::new();
        self.auto.run(&self.store_root, &mounted, &Ctx::silent(), &mut |a| did.push(a.clone())).unwrap();
        let said: Vec<(PathBuf, String)> = did
            .iter()
            .map(|a| match a {
                WatchAction::Skipped { drive, reason } => (drive.root.clone(), reason.clone()),
                other => panic!("{other:?}"),
            })
            .collect();
        if !self.auto.options.settle.is_zero() {
            assert!(said.is_empty(), "looked at before they settled: {said:?}");
            return;
        }
        let mut ready: Vec<PathBuf> = self.due.drain(..).collect();
        ready.sort();
        let expected: Vec<(PathBuf, String)> = ready
            .iter()
            .filter(|r| on.contains(r))
            .filter_map(|r| self.expected(r).map(|why| (r.clone(), why.to_string())))
            .collect();
        assert_eq!(said, expected);
    }

    #[invariant]
    fn idle_when_nothing_is_due(&mut self, _: TestCase) {
        assert_eq!(self.auto.is_idle(), self.due.is_empty());
    }
}

/// Whatever drives a poll sees plugged in, changed and unplugged, the drives looked at are
/// those plugged in or whose databases changed since, and not unplugged, once they've settled.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn drives_are_looked_at_as_the_model_says(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let never = common::demo_drive(dir.path(), "A");
    let adopted = common::demo_drive(dir.path(), "B");
    Device::open(&adopted).unwrap().adopt(None).unwrap();
    let blank = common::blank(dir.path(), "C");
    let drives = vec![never, adopted, blank];
    let store_root = dir.path().join("no store");
    let settle = if tc.draw(gs::weighted_booleans(0.8)) { Duration::ZERO } else { Duration::from_secs(3600) };
    let auto = AutoPull::new(WatchOptions { settle, ..WatchOptions::default() });
    let machine = Due { _dir: dir, store_root, auto, drives, due: IndexSet::new() };
    hegel::stateful::machine(machine).steps(10).run(tc);
}

/// What watching did about the drive at `root`.
fn about<'a>(did: &'a [WatchAction], root: &Path) -> Vec<&'a WatchAction> {
    did.iter().filter(|a| a.drive().root == root).collect()
}

/// A drive of a library in the store stays plugged in while, between looks, players record
/// history lists on it and rekordbox exports to it; each look (pulling or not, the store held by
/// another operation or not, syncing its group or not) says what a model of what the store lacks
/// says: up to date when it holds nothing new, else pulled (or, with `pull` off, pending) with the
/// history lists the store lacked, or waiting while the store is held (and failing at once with
/// no patience). Beside it, a drive never pulled is skipped, and a clone back from a set after a
/// newer export is saved beside the library's newest snapshot, then up to date. An archive or
/// retired drive is left alone. The terminal player's pull plan agrees with each look.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn plugged_in_drives_are_saved_as_they_differ(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let volumes = dir.path().join("volumes");
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let main = common::demo_drive(dir.path(), "Main");
    api::pull(&store, &PullRequest { drive: path(&main), ..Default::default() }, &Ctx::silent()).unwrap();
    let id = Device::open(&main).unwrap().id().unwrap();
    let ids: Vec<u32> = Device::open(&main).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect();
    let plug = |root: &Path| {
        let to = volumes.join(root.file_name().unwrap());
        std::fs::rename(root, &to).unwrap();
        dunce_canonicalize(&to)
    };
    // A clone played at a set while Main got a new export, which was pulled.
    let backup = common::blank(dir.path(), "Backup");
    api::sync_apply(Some(&store), &common::sync_pair(&main, &backup), &Ctx::silent()).unwrap();
    common::add_history(&backup, "BACKUP 001", &ids[..1]);
    common::export_new_track(&main, "Artist", "Fresh");
    api::pull(&store, &PullRequest { drive: path(&main), ..Default::default() }, &Ctx::silent()).unwrap();
    let backup = plug(&backup);
    let friend = plug(&common::demo_drive(dir.path(), "Friend"));
    let main = plug(&main);
    let (mut unsaved, mut changed, mut backup_saved) = (Vec::<String>::new(), false, false);
    let mut lists = 0;
    // Each look in turn without pulling, pulling while the store is held, and pulling.
    let first_mode = tc.draw(gs::integers::<usize>().max_value(2));
    for round in 0..3 {
        let mode = (first_mode + round) % 3;
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(1)) {
            lists += 1;
            let name = format!("HISTORY {lists:03}");
            common::add_history(
                &main,
                &name,
                &tc.draw(gs::vecs(gs::sampled_from(ids.clone())).min_size(1).max_size(2)),
            );
            unsaved.push(name);
            changed = true;
        }
        if tc.draw(gs::weighted_booleans(0.25)) {
            common::export_new_track(&main, "Artist", &format!("Fresh {round}"));
            changed = true;
        }
        let plan =
            api::pull_plan(store.root(), &PullRequest { drive: path(&main), ..Default::default() }, &Ctx::silent())
                .unwrap();
        let relation = if changed { Relation::RightNewer } else { Relation::Identical };
        assert_eq!((plan.plan.relation, plan.noop, plan.plan.new_library), (Some(relation), !changed, false));

        let (pull, held, sync_set) = (mode != 0, mode == 1, tc.draw(gs::booleans()));
        let impatient = held && tc.draw(gs::booleans());
        let snapshots = store.snapshot_ids().unwrap().len();
        let lock = held.then(|| store.lock().unwrap());
        let mut w = Watching::new(
            VolumeWatcher::in_folders(vec![volumes.clone()]),
            WatchOptions { pull, sync_set, settle: Duration::ZERO },
        );
        w.every = Duration::from_millis(1);
        if impatient {
            w.auto.patience = Some(Duration::ZERO);
        }
        let mut did = Vec::new();
        let mut on = |x: Watched<'_>| {
            if let Watched::Did(a) = x {
                did.push(a.clone());
            }
        };
        let looked = match (held, tc.draw(gs::integers::<u8>().max_value(2))) {
            (false, 0) => w.until_idle(store.root(), &Ctx::silent(), &mut on),
            (_, 1) => w.run(store.root(), &Ctx::silent(), &|| true, &mut on),
            _ => w.once(store.root(), &Ctx::silent(), &mut on),
        };
        drop(lock);
        let needs_lock = |c: bool| c && pull;
        let refused = impatient && (needs_lock(changed) || (!backup_saved && pull));
        match looked {
            Ok(()) => assert!(!refused, "gave up on no store"),
            Err(e) => assert!(refused && matches!(e, Error::Locked(_)), "{e}"),
        }
        {
            let f = &friend;
            assert!(
                matches!(about(&did, f)[..], [WatchAction::Skipped { reason, .. }] if reason.contains("not linked"))
                    || refused,
                "{did:?}"
            );
        }
        let mine = about(&did, &main);
        let first = mine.first().map(|a| kind(a));
        let pulled = match (changed, pull, held) {
            _ if refused && mine.is_empty() => false,
            (false, ..) => {
                assert_eq!(first.as_deref(), Some("watch.up_to_date"), "{mine:?}");
                false
            }
            (true, false, _) => {
                let [WatchAction::Pending { history_lists, beside: false, .. }] = &mine[..] else { panic!("{mine:?}") };
                assert_eq!(history_lists, &unsaved);
                false
            }
            (true, true, true) => {
                assert_eq!(mine.iter().map(|a| kind(a)).collect::<Vec<_>>(), ["watch.waiting"]);
                false
            }
            (true, true, false) => {
                let WatchAction::Pulled { history_lists, beside: false, snapshot, .. } = mine[0] else {
                    panic!("{mine:?}")
                };
                assert_eq!(history_lists, &unsaved);
                assert_eq!(store.find_library("Main").unwrap().unwrap().head, Some(snapshot.id), "the newest snapshot");
                true
            }
        };
        // Syncing the group after it says so; without it, nothing more.
        for a in mine.iter().skip(1) {
            let synced = matches!(a, WatchAction::Synced { .. })
                || matches!(a, WatchAction::Skipped { reason, .. } if reason.starts_with("not syncing its group"));
            assert!(sync_set && synced, "{mine:?}");
        }
        if pulled {
            let f = &friend;
            let order: Vec<&std::path::PathBuf> =
                did.iter().map(|a| &a.drive().root).filter(|r| *r == f || **r == main).collect();
            assert_eq!(order.first(), Some(&&main), "an export is saved first");
        }
        let mut beside = false;
        {
            let theirs = about(&did, &backup);
            match (backup_saved, pull, held) {
                _ if refused && theirs.is_empty() => {}
                (true, ..) => {
                    assert_eq!(theirs.first().map(|a| kind(a)).as_deref(), Some("watch.up_to_date"), "{theirs:?}")
                }
                (false, false, _) => {
                    assert!(matches!(theirs[..], [WatchAction::Pending { beside: true, .. }]), "{theirs:?}")
                }
                (false, true, true) => {
                    assert_eq!(theirs.iter().map(|a| kind(a)).collect::<Vec<_>>(), ["watch.waiting"])
                }
                (false, true, false) => {
                    let [WatchAction::Pulled { beside: true, history_lists, .. }] = &theirs[..] else {
                        panic!("{theirs:?}")
                    };
                    assert_eq!(history_lists, &["BACKUP 001"]);
                    (backup_saved, beside) = (true, true);
                }
            }
        }
        assert_eq!(store.snapshot_ids().unwrap().len(), snapshots + usize::from(pulled) + usize::from(beside));
        if pulled {
            (unsaved, changed) = (Vec::new(), false);
        }
    }
    // Archive and retired drives are left alone.
    let role = tc.draw(gs::integers::<u8>().max_value(2));
    match role {
        0 => drop(fleet::set_role(&store, id, Some(fleet::Role::Archive)).unwrap()),
        1 => drop(fleet::retire(&store, id, false).unwrap()),
        _ => return,
    }
    let mut w = Watching::new(
        VolumeWatcher::in_folders(vec![volumes.clone()]),
        WatchOptions { pull: true, sync_set: false, settle: Duration::ZERO },
    );
    let mut did = Vec::new();
    w.once(store.root(), &Ctx::silent(), &mut |x| {
        if let Watched::Did(a) = x {
            did.push(a.clone());
        }
    })
    .unwrap();
    let why = if role == 0 { "an archive drive, kept as it is" } else { "retired" };
    assert!(matches!(about(&did, &main)[..], [WatchAction::Skipped { reason, .. }] if reason == why), "{did:?}");
}

/// Pulling into a store that isn't there yet plans to start a library and makes no store; the
/// plan applied makes it, after which there's nothing to pull. A folder isn't ejected, a blank
/// one isn't pulled, and My Tags by track are read as `tags` reads them.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn pull_plans_start_a_store_and_folders_are_not_ejected(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let root = common::demo_drive(dir.path(), "USB");
    let store_root = dir.path().join("store");
    let library = tc.draw(gs::optional(gs::sampled_from(vec!["Main", "Club"]).map(String::from)));
    let req = PullRequest { drive: path(&root), library: library.clone(), new_library: true, ..Default::default() };
    let plan = api::pull_plan(&store_root, &req, &Ctx::silent()).unwrap();
    assert!(!store_root.exists(), "planning made the store");
    assert!(plan.plan.new_library && plan.plan.relation.is_none() && !plan.noop);
    assert_eq!(plan.plan.library, library.clone().unwrap_or_else(|| "USB".into()));
    assert!(plan.summary[0].contains("starting the library"), "{:?}", plan.summary);
    let report = plan.apply(None, &Ctx::silent()).unwrap();
    assert_eq!(report.library, library.unwrap_or_else(|| "USB".into()));
    let again =
        api::pull_plan(&store_root, &PullRequest { drive: path(&root), ..Default::default() }, &Ctx::silent()).unwrap();
    assert!(again.noop && again.plan.relation == Some(Relation::Identical) && !again.plan.new_library);

    // A clone made now falls behind when rekordbox exports a track that's saved (through the
    // plan, with the store it names): the store is newer, and its plan warns a pull is refused.
    let store = Store::open(&store_root).unwrap();
    let clone = common::blank(dir.path(), "Clone");
    api::sync_apply(Some(&store), &common::sync_pair(&root, &clone), &Ctx::silent()).unwrap();
    common::export_new_track(&root, "Artist", "Fresh");
    let plan =
        api::pull_plan(&store_root, &PullRequest { drive: path(&root), ..Default::default() }, &Ctx::silent()).unwrap();
    assert_eq!(plan.plan.relation, Some(Relation::RightNewer));
    assert!(!plan.apply(Some(&store), &Ctx::silent()).unwrap().up_to_date);
    let behind =
        api::pull_plan(&store_root, &PullRequest { drive: path(&clone), ..Default::default() }, &Ctx::silent())
            .unwrap();
    assert_eq!(behind.plan.relation, Some(Relation::LeftNewer));
    assert!(behind.warnings.iter().any(|w| w.contains("newer state")), "{:?}", behind.warnings);
    // A library the store has no snapshot of yet: the first.
    store.create_library("Empty").unwrap();
    let empty = PullRequest { drive: path(&root), library: Some("Empty".into()), ..Default::default() };
    let first = api::pull_plan(&store_root, &empty, &Ctx::silent()).unwrap();
    assert!(!first.plan.new_library && first.plan.relation.is_none() && first.plan.library == "Empty");
    assert!(first.summary[0].contains("first snapshot of Empty"), "{:?}", first.summary);

    let blank = common::blank(dir.path(), "BLANK");
    let err = api::pull_plan(&store_root, &PullRequest { drive: path(&blank), ..Default::default() }, &Ctx::silent())
        .unwrap_err();
    assert!(matches!(err, Error::NotAnExport(_)), "{err}");
    let eject = EjectDrives { drives: vec![path(&root)] };
    assert!(api::drive_eject_plan(Some(&store_root), &eject).unwrap_err().to_string().contains("nothing to eject"));
    assert_eq!(api::drive_eject_plan(None, &EjectDrives { drives: Vec::new() }).unwrap_err().code(), "invalid");
    let by_track = api::tags_by_track(&DriveRef { drive: path(&root) }).unwrap();
    assert_eq!(by_track, locallibrary::tags::by_track(&Device::open(&root).unwrap()).unwrap());
}

/// How Main stands to the store it was saved in, when its pull is planned.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Standing {
    /// As saved.
    Unchanged,
    /// rekordbox exported to it since.
    Changed,
    /// A copy of it was saved since, with a track exported to the copy.
    Older,
    /// That, and a track exported to Main too.
    Diverged,
    /// Not Main: a drive holding the demo export that was never pulled.
    Unlinked,
}

/// A pull planned for Main as it stands to the store (as saved, changed since, older than a copy
/// saved since, or diverged from it; or a drive never pulled), into its own library, a library
/// named that the store hasn't got (refused unless starting it is asked for, as pulling refuses
/// it), or the default: the plan says which library and whether it's new, how the drive and
/// the library's latest snapshot compare, and warns of a pull that will be refused; applied, it
/// pulls as planned (nothing new for a drive saved already) or is refused as warned.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(standing = Standing::Diverged, library = None::<&str>, new_library = false)]
#[hegel::explicit_test_case(standing = Standing::Unlinked, library = None::<&str>, new_library = false)]
fn a_pull_plan_says_what_pulling_does(tc: TestCase) {
    let standing = tc.draw(gs::sampled_from(vec![
        Standing::Unchanged,
        Standing::Changed,
        Standing::Older,
        Standing::Diverged,
        Standing::Unlinked,
    ]));
    let library = tc.draw(gs::sampled_from(vec![None, Some("main"), Some("Nope")]));
    let new_library = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = crate::props_api_drives::saved_copy(dir.path());
    let ctx = Ctx::silent();
    if matches!(standing, Standing::Older | Standing::Diverged) {
        let copy = main.with_file_name("Copy");
        crate::props_api_drives::saved_demo().1.write(&copy);
        crate::props_api_jobs::own_identity(&store, &mut Device::open(&copy).unwrap(), "Copy");
        common::export_new_track(&copy, "Artist", "At the club");
        api::pull(&store, &PullRequest { drive: path(&copy), ..Default::default() }, &ctx).unwrap();
    }
    if matches!(standing, Standing::Changed | Standing::Diverged) {
        common::export_new_track(&main, "Artist", "At home");
    }
    let (drive, name) = match standing {
        Standing::Unlinked => (common::demo_drive(main.parent().unwrap(), "Fresh"), "Fresh"),
        _ => (main.clone(), "Main"),
    };
    // A drive never pulled into Main isn't compared with it: only its own library or a new one.
    let library = library.filter(|l| standing != Standing::Unlinked || *l != "main");
    let req = PullRequest { drive: path(&drive), library: library.map(Into::into), new_library, ..Default::default() };
    let plan = api::pull_plan(store.root(), &req, &ctx);
    if library == Some("Nope") && !new_library {
        let want = "library \"Nope\" in the store; pull with new_library to start it";
        assert!(matches!(&plan, Err(Error::NotFound(m)) if m == want), "{:?}", plan.map(|p| p.noop));
        assert!(api::pull(&store, &req, &ctx).is_err(), "pulling refuses it too");
        return;
    }
    let plan = plan.unwrap();
    let starts = library == Some("Nope") || standing == Standing::Unlinked;
    let into = match (library, standing) {
        (Some("Nope"), _) => "Nope",
        (_, Standing::Unlinked) => "Fresh",
        _ => "Main",
    };
    assert_eq!((plan.plan.library.as_str(), plan.plan.new_library), (into, starts));
    let relation = match standing {
        _ if starts => None,
        Standing::Unchanged => Some(Relation::Identical),
        Standing::Changed => Some(Relation::RightNewer),
        Standing::Older => Some(Relation::LeftNewer),
        _ => Some(Relation::Diverged),
    };
    assert_eq!(plan.plan.relation, relation);
    assert_eq!(plan.noop, relation == Some(Relation::Identical));
    let summary = match relation {
        None => format!("save {name} in the store, starting the library {into}"),
        Some(Relation::Identical) => format!("{name} is saved in the store already ({into})"),
        Some(Relation::RightNewer) => format!("save {name}'s new changes as a snapshot of {into}"),
        Some(_) => format!("save {name} as a snapshot of {into}"),
    };
    assert_eq!(plan.summary, [summary]);
    let warned = |what: &str| plan.warnings.iter().any(|w| w.contains(what));
    assert_eq!(warned("holds a newer state"), relation == Some(Relation::LeftNewer), "{:?}", plan.warnings);
    assert_eq!(warned("both changed since"), relation == Some(Relation::Diverged), "{:?}", plan.warnings);

    let pulled = plan.apply(Some(&store), &ctx);
    match relation {
        Some(Relation::LeftNewer | Relation::Diverged) => {
            assert!(matches!(&pulled, Err(e) if e.code() == "conflict"), "{:?}", pulled.map(|r| r.up_to_date))
        }
        _ => {
            let report = pulled.unwrap();
            assert_eq!((report.library.as_str(), report.up_to_date), (into, relation == Some(Relation::Identical)));
            let again = api::pull_plan(store.root(), &PullRequest { drive: path(&drive), ..Default::default() }, &ctx);
            assert!(again.unwrap().noop, "saved now");
        }
    }
}
