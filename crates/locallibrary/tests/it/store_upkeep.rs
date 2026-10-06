//! Looking after the store: libraries, snapshot labels, pruning.

use crate::common;

use std::path::Path;

use locallibrary::Error;
use locallibrary::api::{self, KeepReason, LabelRequest, PruneOptions, PullRequest, RenameLibrary};
use locallibrary::device::Device;
use locallibrary::edit::Edit;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PushOptions};

fn path(p: &Path) -> String {
    p.display().to_string()
}

#[test]
fn pull_refuses_an_unknown_library_unless_asked_to_start_one() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let ctx = Ctx::silent();
    let a = common::demo_drive(dir.path(), "A");

    // The first pull without --library starts one named after the drive, as before.
    let r = api::pull(&store, &PullRequest { drive: path(&a), ..Default::default() }, &ctx).unwrap();
    assert_eq!(r.library, "A");

    // A typo is refused, naming the libraries there are; nothing is created.
    let typo = PullRequest { drive: path(&a), library: Some("Aa".into()), ..Default::default() };
    match api::pull(&store, &typo, &ctx) {
        Err(e) => {
            assert!(matches!(e.inner(), Error::NotFound(m) if m.contains("\"A\"")), "{e}");
            assert_eq!(e.detail().actions[0].params["new_library"], true, "{e}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(store.libraries().unwrap().len(), 1);
    // A known name in any case is fine.
    let known = PullRequest { drive: path(&a), library: Some("a".into()), ..Default::default() };
    assert!(api::pull(&store, &known, &ctx).unwrap().up_to_date);

    // With new_library it starts one.
    let b = common::bigger_drive(dir.path(), "B");
    let new = PullRequest { drive: path(&b), library: Some("Club".into()), new_library: true, ..Default::default() };
    assert_eq!(api::pull(&store, &new, &ctx).unwrap().library, "Club");

    // Libraries are listed with their drives, and renamed in place.
    let libs = api::libraries_list(&store).unwrap();
    let names: Vec<(&str, usize, &[String])> =
        libs.iter().map(|l| (l.name.as_str(), l.snapshots, l.drives.as_slice())).collect();
    assert_eq!(names, [("A", 1, &["A".to_string()][..]), ("Club", 1, &["B".to_string()][..])]);
    let club = libs[1].id;
    let taken = api::library_rename(&store, &RenameLibrary { library: "club".into(), name: "a".into() });
    assert!(taken.is_err_and(|e| matches!(e.inner(), Error::Conflict(_))));
    let renamed =
        api::library_rename(&store, &RenameLibrary { library: "club".into(), name: " Shows ".into() }).unwrap();
    assert_eq!((renamed.id, renamed.name.as_str()), (club, "Shows"));
    let again = PullRequest { drive: path(&b), library: Some("Shows".into()), ..Default::default() };
    assert!(api::pull(&store, &again, &ctx).unwrap().up_to_date);
    assert!(matches!(
        api::library_rename(&store, &RenameLibrary { library: "Club".into(), name: "X".into() }),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn labels_name_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let ctx = Ctx::silent();
    let a = common::demo_drive(dir.path(), "A");
    let labelled = PullRequest { drive: path(&a), label: Some("before NYE".into()), ..Default::default() };
    let first = api::pull(&store, &labelled, &ctx).unwrap().snapshot.unwrap();
    assert_eq!(first.labels, ["before NYE"]);

    // The label is taken: another pull can't have it, and nothing is pulled.
    common::export_new_track(&a, "New", "Track");
    assert!(api::pull(&store, &labelled, &ctx).is_err_and(|e| matches!(e.inner(), Error::Conflict(_))));
    let second = api::pull(&store, &PullRequest { drive: path(&a), ..Default::default() }, &ctx).unwrap();
    let second = second.snapshot.unwrap();
    assert!(second.labels.is_empty());

    // Shown in the history, and good for a snapshot id (any case).
    let lib = store.find_library("A").unwrap().unwrap();
    let hist = store.history(&lib, None).unwrap();
    assert_eq!(hist.iter().map(|s| s.labels.clone()).collect::<Vec<_>>(), [vec![], vec!["before NYE".to_string()]]);
    assert_eq!(store.resolve_snapshot("Before nye").unwrap(), first.id);
    let d = Device::open_export(&a).unwrap();
    let opts = PushOptions { snapshot: Some("before NYE".into()), ..Default::default() };
    assert_eq!(sync::resolve_push_snapshot(&store, &d, &opts).unwrap().id, first.id);

    // Labels are added by id prefix or another label, and taken off by name.
    let add = |label: &str, snapshot: &str| {
        api::snapshot_label(
            &store,
            &LabelRequest { label: label.into(), snapshot: Some(snapshot.into()), remove: false },
        )
    };
    assert_eq!(add("latest", &second.short_id).unwrap().snapshot, second.id);
    assert_eq!(add("Latest", &second.short_id).unwrap().name, "latest", "labelling again does nothing");
    assert!(add("latest", "before NYE").is_err_and(|e| matches!(e.inner(), Error::Conflict(_))));
    assert!(matches!(add(" ", &second.short_id), Err(Error::Invalid(_))));
    assert!(matches!(add("x", "ffff"), Err(Error::NotFound(_))));
    let off = LabelRequest { label: "LATEST".into(), snapshot: None, remove: true };
    assert_eq!(api::snapshot_label(&store, &off).unwrap().snapshot, second.id);
    assert!(matches!(api::snapshot_label(&store, &off), Err(Error::NotFound(_))));
    assert_eq!(store.labels().unwrap().len(), 1);
}

#[test]
fn prune_keeps_heads_labels_drives_and_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let ctx = Ctx::silent();
    let a = common::demo_drive(dir.path(), "A");
    let pull = |message: &str| {
        let req = PullRequest { drive: path(&a), message: Some(message.into()), ..Default::default() };
        api::pull(&store, &req, &ctx).unwrap().snapshot.unwrap()
    };
    let s1 = pull("one");
    // B is a copy of the first state, and was last seen holding it.
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    let first = store.load_snapshot(&s1.id).unwrap();
    sync::push(&store, &first, &mut b, &PushOptions::default(), &ctx).unwrap();
    common::export_new_track(&a, "New", "One");
    let s2 = pull("two");
    // A player records a set; rekordbox takes it off the drive at the next export.
    let ids: Vec<u32> = Device::open_export(&a).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect();
    common::add_history(&a, "HISTORY 001", &ids);
    let s3 = pull("three");
    common::apply(&a, &[Edit::DeleteHistory { names: vec![], all: true }]);
    common::export_new_track(&a, "New", "Two");
    let s4 = pull("four");

    // A dry run says what would go, and removes nothing.
    let opts = PruneOptions { keep_last: Some(1), dry_run: true, ..Default::default() };
    let dry = api::store_prune(&store, &opts, &ctx).unwrap();
    assert_eq!(dry.removed_snapshots, 1);
    assert!(dry.removed_objects > 0 && dry.freed_bytes > 0);
    assert!(store.load_snapshot(&s2.id).is_ok());
    let lib = &dry.libraries[0];
    assert_eq!(lib.removed.iter().map(|s| s.id).collect::<Vec<_>>(), [s2.id]);
    let kept: Vec<_> = lib.kept.iter().map(|k| (k.snapshot.id, k.reasons.clone())).collect();
    assert_eq!(
        kept,
        [
            (s4.id, vec![KeepReason::Latest, KeepReason::Drive, KeepReason::Last]),
            (s3.id, vec![KeepReason::History]),
            (s1.id, vec![KeepReason::Drive]),
        ]
    );
    assert_eq!(lib.kept[0].drives, ["A"]);
    assert_eq!(lib.kept[1].history, ["HISTORY 001"]);
    assert_eq!(lib.kept[2].drives, ["B"]);

    // The real thing frees what the dry run said.
    let opts = PruneOptions { dry_run: false, ..opts };
    let report = api::store_prune(&store, &opts, &ctx).unwrap();
    assert_eq!(
        (report.removed_snapshots, report.removed_objects, report.freed_bytes),
        (1, dry.removed_objects, dry.freed_bytes)
    );
    assert!(matches!(store.resolve_snapshot(&s2.short_id), Err(Error::NotFound(m)) if m.contains("pruned")));

    // History walks through the gap, and nothing the rest use is missing.
    let library = store.find_library("A").unwrap().unwrap();
    let hist: Vec<_> = store.history(&library, None).unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(hist, [s4.id, s3.id, s1.id]);
    assert!(store.is_ancestor(&s1.id, &s4.id).unwrap());
    let verified = store.verify(&ctx).unwrap();
    assert!(verified.missing.is_empty() && verified.corrupt.is_empty());
    assert_eq!(store.gc(false).unwrap().removed_objects, 0);
    let archive =
        api::history_archive(&store, &api::HistoryArchiveRequest { library: None, drive: None }, &ctx).unwrap();
    assert_eq!(archive.lists.len(), 1, "the history list is still in the store");

    // Pruning again changes nothing; a policy is needed, and the library must exist.
    assert_eq!(api::store_prune(&store, &opts, &ctx).unwrap().removed_snapshots, 0);
    assert!(matches!(api::store_prune(&store, &PruneOptions::default(), &ctx), Err(Error::Invalid(_))));
    let other = PruneOptions { library: Some("Other".into()), ..opts };
    assert!(matches!(api::store_prune(&store, &other, &ctx), Err(Error::NotFound(_))));
}

/// Regression: gc kept only what a library's latest snapshot or a label reaches, so `store gc`,
/// and the gc after every prune, deleted the snapshot a drive back from a set was saved as
/// beside the latest, which the prune's own plan listed as kept.
#[test]
fn prune_and_gc_keep_a_drive_saved_beside_the_latest() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let ctx = Ctx::silent();
    let a = common::demo_drive(dir.path(), "A");
    let pull = |message: &str| {
        let req = PullRequest { drive: path(&a), message: Some(message.into()), ..Default::default() };
        api::pull(&store, &req, &ctx).unwrap().snapshot.unwrap()
    };
    let s1 = pull("one");
    // B goes to a set with the first state; A gets a newer export meanwhile.
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    sync::push(&store, &store.load_snapshot(&s1.id).unwrap(), &mut b, &PushOptions::default(), &ctx).unwrap();
    common::export_new_track(&a, "New", "One");
    pull("two");
    common::export_new_track(&a, "New", "Two");
    let s3 = pull("three");
    let ids: Vec<u32> = b.library().unwrap().tracks.iter().map(|t| t.id).collect();
    common::add_history(b.root(), "HISTORY 001", &ids);
    let mut b = Device::open(dir.path().join("B")).unwrap();
    let beside = sync::pull_beside(&store, &mut b, &sync::PullOptions::default(), s1.id, &ctx).unwrap();
    let beside = beside.snapshot.unwrap();
    assert_eq!(store.find_library("A").unwrap().unwrap().head, Some(s3.id), "the latest stays the latest");

    // Nothing uses an extra file: gc says so, and keeps every snapshot.
    let stray = store.import_bytes(b"nothing uses this").unwrap();
    let gc = store.gc(true).unwrap();
    assert_eq!((gc.removed_snapshots, gc.removed_objects), (0, 1));

    // Pruning keeps it (B was last seen holding it), says what it frees, and frees just that.
    let opts = PruneOptions { keep_last: Some(1), ..Default::default() };
    let plan = api::store_prune_plan(&store, &opts, &ctx).unwrap();
    let lib = &plan.libraries[0];
    let kept = lib.kept.iter().find(|k| k.snapshot.id == beside.id).expect("kept");
    assert!(kept.reasons.contains(&KeepReason::Drive) && kept.drives == ["B"], "{:?}", kept.reasons);
    assert_eq!(lib.removed.iter().map(|s| s.id).collect::<Vec<_>>().len(), 2, "the first two pulls");
    let report = plan.apply(Some(&store), &ctx).unwrap();
    assert_eq!(
        (report.removed_snapshots, report.removed_objects, report.freed_bytes),
        (plan.removed_snapshots, plan.removed_objects, plan.freed_bytes)
    );
    assert!(!store.has_object(&stray));
    assert!(store.load_snapshot(&beside.id).is_ok(), "the prune kept it");
    let gc = store.gc(false).unwrap();
    assert_eq!((gc.removed_snapshots, gc.removed_objects), (0, 0));
    assert!(store.load_snapshot(&beside.id).is_ok(), "gc kept it");
    let archive =
        api::history_archive(&store, &api::HistoryArchiveRequest { library: None, drive: None }, &ctx).unwrap();
    assert!(archive.lists.iter().any(|l| l.list.name == "HISTORY 001"));
}

/// Regression: `store prune` asked about one plan and then worked out the policy again, so a
/// pull while it asked changed what was removed.
#[test]
fn prune_applies_the_plan_it_showed() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let ctx = Ctx::silent();
    let a = common::demo_drive(dir.path(), "A");
    let pull = || api::pull(&store, &PullRequest { drive: path(&a), ..Default::default() }, &ctx).unwrap();
    pull();
    common::export_new_track(&a, "New", "One");
    pull();
    let opts = PruneOptions { keep_last: Some(1), ..Default::default() };
    let plan = api::store_prune_plan(&store, &opts, &ctx).unwrap();
    assert_eq!(plan.removed_snapshots, 1);
    assert!(plan.destructive && !plan.noop);

    // A pull while it's shown: the plan is stale, and nothing is removed.
    common::export_new_track(&a, "New", "Two");
    pull();
    let err = plan.apply(Some(&store), &ctx).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
    assert_eq!(store.snapshot_ids().unwrap().len(), 3);

    // Planned again, it removes what it lists.
    let plan = api::store_prune_plan(&store, &opts, &ctx).unwrap();
    let removed: Vec<_> = plan.libraries[0].removed.iter().map(|s| s.id).collect();
    assert_eq!(removed.len(), 2);
    plan.apply(Some(&store), &ctx).unwrap();
    let left = store.snapshot_ids().unwrap();
    assert!(removed.iter().all(|id| !left.contains(id)) && left.len() == 1);
}
