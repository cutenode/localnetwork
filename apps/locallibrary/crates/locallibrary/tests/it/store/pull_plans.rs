//! What a pull plan says about a drive against the library's latest snapshot: new changes to
//! save, a store that's ahead of the drive, or both changed. And an eject plan needs a drive.

use crate::common::{self, path};

use std::path::{Path, PathBuf};

use locallibrary::api::{self, PullRequest};
use locallibrary::compare::Relation;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

fn pull_req(root: &Path) -> PullRequest {
    PullRequest { drive: path(root), ..Default::default() }
}

/// Main, adopted and saved in the store, which drives are looked for beside.
fn saved(dir: &Path) -> (Store, PathBuf) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.to_path_buf()]));
    let main = common::demo_drive(dir, "Main");
    common::adopt(None, &main, "Main");
    api::pull(&store, &pull_req(&main), &Ctx::silent()).unwrap();
    (store, main)
}

/// A drive linked to its library with an export since is planned as new changes, and pulling
/// as planned saves them.
#[test]
fn new_changes_on_a_linked_drive_are_saved_as_planned() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved(dir.path());
    common::export_new_track(&main, "Artist", "Fresh");

    let plan = api::pull_plan(store.root(), &pull_req(&main), &Ctx::silent()).unwrap();
    assert_eq!((plan.library.as_str(), plan.relation), ("Main", Some(Relation::RightNewer)));
    assert_eq!(plan.summary, ["save Main's new changes as a snapshot of Main"]);
    let report = plan.apply(Some(&store), &Ctx::silent()).unwrap();
    assert!(report.snapshot.is_some());
    let again = api::pull_plan(store.root(), &pull_req(&main), &Ctx::silent()).unwrap();
    assert!(again.noop);
}

/// A drive behind the store's latest snapshot, or changed alongside it, is warned about before
/// a pull that may be refused.
#[test]
fn a_drive_behind_or_beside_the_store_is_warned_about() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved(dir.path());
    // A copy of Main, with an identity of its own, moves the library on.
    let copy = dir.path().join("Copy");
    common::copy_dir(&main, &copy);
    api::drive_reidentify(None, &api::Reidentify { drive: path(&copy), name: None, force: false }).unwrap();
    common::export_new_track(&copy, "Artist", "Fresh");
    api::pull(&store, &pull_req(&copy), &Ctx::silent()).unwrap();

    let behind = api::pull_plan(store.root(), &pull_req(&main), &Ctx::silent()).unwrap();
    assert_eq!(behind.relation, Some(Relation::LeftNewer));
    assert!(behind.warnings[0].starts_with("the store holds a newer state of Main"), "{:?}", behind.warnings);

    common::export_new_track(&main, "Artist", "Elsewhere");
    let beside = api::pull_plan(store.root(), &pull_req(&main), &Ctx::silent()).unwrap();
    assert_eq!(beside.relation, Some(Relation::Diverged));
    assert_eq!(beside.summary, ["save Main as a snapshot of Main"]);
    assert!(beside.warnings[0].contains("both changed"), "{:?}", beside.warnings);
}

/// Ejecting needs at least one drive named.
#[test]
fn an_eject_plan_needs_a_drive() {
    let err = api::drive_eject_plan(None, &api::EjectDrives { drives: Vec::new() }).expect_err("refused");
    assert!(err.to_string().contains("name a drive to eject"), "{err}");
}
