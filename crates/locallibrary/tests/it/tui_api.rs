//! The plans and reads the terminal player added to the api: saving a drive in the store,
//! ejecting, and My Tags by track.

use crate::common;

use locallibrary::api::{self, DriveRef, EjectDrives, PullRequest};
use locallibrary::compare::Relation;
use locallibrary::op::AnyPlanned;
use locallibrary::progress::Ctx;
use locallibrary::{Device, Store, tags};

#[test]
fn a_pull_is_planned_then_made_and_then_has_nothing_to_do() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let store_root = dir.path().join("store");
    let req = PullRequest { drive: root.display().to_string(), ..Default::default() };

    // No store yet: planning makes none, and says the pull starts a library.
    let plan = api::pull_plan(&store_root, &req, &Ctx::silent()).unwrap();
    assert!(!store_root.exists(), "planning writes nothing");
    assert!(plan.new_library && plan.relation.is_none() && !plan.noop);
    assert_eq!(plan.operation, "pull");
    assert!(plan.summary[0].contains("starting the library"), "{:?}", plan.summary);
    let any: Box<dyn AnyPlanned> = Box::new(plan);
    let report = any.apply_json(None, &Ctx::silent()).unwrap();
    assert_eq!(report["up_to_date"], false);
    let store = Store::open(&store_root).unwrap();
    assert_eq!(store.libraries().unwrap().len(), 1);

    // Saved: the next plan has nothing to do.
    let again = api::pull_plan(&store_root, &req, &Ctx::silent()).unwrap();
    assert_eq!(again.relation, Some(Relation::Identical));
    assert!(again.noop && !again.new_library);
    assert!(again.summary[0].contains("saved in the store already"), "{:?}", again.summary);

    // A library the store doesn't know is refused, unless it's to be started.
    let named = PullRequest { library: Some("Elsewhere".into()), ..req.clone() };
    assert!(api::pull_plan(&store_root, &named, &Ctx::silent()).is_err());
    let new = PullRequest { new_library: true, ..named };
    assert!(api::pull_plan(&store_root, &new, &Ctx::silent()).unwrap().new_library);

    // Not a rekordbox drive.
    let blank = dir.path().join("BLANK");
    std::fs::create_dir_all(&blank).unwrap();
    let req = PullRequest { drive: blank.display().to_string(), ..Default::default() };
    assert!(api::pull_plan(&store_root, &req, &Ctx::silent()).is_err());
}

/// A folder holding an export isn't a volume: there's nothing to eject (and the commands would
/// eject the disk the folder is on, here the one the tests run on).
#[test]
fn ejecting_a_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let req = EjectDrives { drives: vec![root.display().to_string()] };
    let err = api::drive_eject_plan(None, &req).unwrap_err();
    assert!(err.to_string().contains("nothing to eject"), "{err}");
    assert!(api::drive_eject_plan(None, &EjectDrives { drives: Vec::new() }).is_err());
    // Ejecting without a plan is refused alike.
    let err = api::drive_eject(None, &req).unwrap_err();
    assert!(err.to_string().contains("nothing to eject"), "{err}");
}

#[test]
fn my_tags_by_track() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let mut d = Device::open(&root).unwrap();
    let track = d.library().unwrap().tracks[0].clone();
    let edit = tags::Edit::Add { tag: "Genre/Deep".into(), tracks: vec![track.id] };
    let plan = tags::plan(&d, &edit).unwrap();
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let by = api::tags_by_track(&DriveRef { drive: root.display().to_string() }).unwrap();
    let key = locallibrary::paths::file_key(&track.file_path);
    assert_eq!(by.get(&key).map(Vec::as_slice), Some(&["Genre/Deep".to_string()][..]));
}

/// Changing a volume label drives the operating system's tools, untested on Linux and Windows:
/// the plan says so, for the sidecar and the terminal player as for the CLI.
#[test]
fn a_volume_label_change_warns_where_it_is_untested() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let req = |label| api::RenameDrive { drive: root.display().to_string(), name: "Club".into(), label };
    locallibrary::testing::os_on_this_thread(Some("windows"));
    let warned = api::drive_rename_plan(&req(true)).unwrap().warnings.clone();
    let plain = api::drive_rename_plan(&req(false)).unwrap().warnings.clone();
    locallibrary::testing::os_on_this_thread(Some("macos"));
    let on_macos = api::drive_rename_plan(&req(true)).unwrap().warnings.clone();
    locallibrary::testing::os_on_this_thread(None);
    assert_eq!(warned, ["changing the volume label hasn't been tested on Windows yet; check the result"]);
    assert!(plain.is_empty(), "{plain:?}");
    assert!(on_macos.is_empty(), "tested on macOS: {on_macos:?}");
}
