//! The jobs past their usual path: a refresh that also copies the drive onto the other drives
//! of its library (or doesn't, when one of them is newer), and getting drives ready for a set
//! when they can't be put in order or ejected.

use crate::common::{self, path};

use std::path::{Path, PathBuf};

use locallibrary::Error;
use locallibrary::api::{self, StepKind};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

fn titles(root: &Path) -> Vec<String> {
    Device::open(root).unwrap().library().unwrap().tracks.into_iter().map(|t| t.title).collect()
}

/// The reason a step of `plan` doesn't run, if it doesn't.
fn reason(plan: &api::JobPlan, kind: StepKind) -> Option<String> {
    plan.steps.iter().find(|s| s.step == kind).and_then(|s| s.reason.clone())
}

/// A store named for this thread, drives looked for in `dir`, and Main with Backup synced from
/// it, both saved in the library Home.
fn two_drives(dir: &Path) -> (Store, PathBuf, PathBuf) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.to_path_buf()]));
    let ctx = Ctx::silent();
    let main = common::demo_drive(dir, "Main");
    common::adopt(Some(&store), &main, "Main");
    let pull =
        api::PullRequest { drive: path(&main), library: Some("Home".into()), new_library: true, ..Default::default() };
    api::pull(&store, &pull, &ctx).unwrap();
    let backup = common::blank(dir, "Backup");
    api::sync_apply(Some(&store), &common::sync_pair(&main, &backup), &ctx).unwrap();
    common::adopt(Some(&store), &backup, "Backup");
    (store, main, backup)
}

/// Refreshing with `sync` saves the new export and copies it onto the library's other mounted
/// drive; when that drive is newer, it isn't copied over, and the plan says why.
#[test]
fn a_refresh_copies_the_drive_onto_its_librarys_others() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main, backup) = two_drives(dir.path());
    let ctx = Ctx::silent();
    common::export_new_track(&main, "Artist", "Fresh");
    let refresh = |skip: Vec<StepKind>| api::RefreshRequest { drive: Some(path(&main)), skip, sync: true };

    let skipped = api::refresh_plan(&store, &refresh(vec![StepKind::Pull, StepKind::Sync]), &ctx).unwrap();
    assert_eq!(reason(&skipped, StepKind::Pull).as_deref(), Some("skipped"));
    assert_eq!(reason(&skipped, StepKind::Sync).as_deref(), Some("skipped"));

    let plan = api::refresh_plan(&store, &refresh(Vec::new()), &ctx).unwrap();
    let running: Vec<StepKind> = plan.running().map(|s| s.step).collect();
    assert_eq!(running, [StepKind::Pull, StepKind::Sync]);
    let report = api::refresh_apply(&store, &refresh(Vec::new()), &ctx).unwrap();
    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    assert!(titles(&backup).contains(&"Fresh".to_string()));

    common::export_new_track(&backup, "Artist", "Later");
    let plan = api::refresh_plan(&store, &refresh(Vec::new()), &ctx).unwrap();
    let sync = plan.steps.iter().find(|s| s.step == StepKind::Sync).unwrap();
    assert_eq!(sync.reason.as_deref(), Some("Backup is newer than Main"));
    assert!(sync.warnings[0].contains("it isn't copied over"), "{:?}", sync.warnings);
}

/// Only a drive holding an export can be refreshed or taken to a set, and each only once.
#[test]
fn drives_for_a_job_hold_an_export_and_are_named_once() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main, _) = two_drives(dir.path());
    let blank = dir.path().join("Blank");
    std::fs::create_dir(&blank).unwrap();
    let ctx = Ctx::silent();

    let refresh = api::RefreshRequest { drive: Some(path(&blank)), ..Default::default() };
    assert!(matches!(api::refresh_plan(&store, &refresh, &ctx), Err(Error::NotAnExport(_))));
    let set = |drives: &[&Path]| api::SetJobRequest {
        drives: drives.iter().map(|d| path(d)).collect(),
        eject: false,
        ..Default::default()
    };
    let err = api::set_job_plan(&store, &set(&[&main, &main]), &ctx).err().expect("refused");
    assert!(err.to_string().contains("listed twice"), "{err}");
    assert!(matches!(api::set_job_plan(&store, &set(&[&main, &blank]), &ctx), Err(Error::NotAnExport(_))));
}

/// Drives that both changed since they matched aren't synced before a set: the plan says it
/// can't tell which is newest, and the rest of the job still runs.
#[test]
fn drives_that_both_changed_arent_synced_before_a_set() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main, backup) = two_drives(dir.path());
    let ctx = Ctx::silent();
    common::export_new_track(&main, "Artist", "Here");
    common::export_new_track(&backup, "Artist", "There");
    let req = api::SetJobRequest { drives: vec![path(&main), path(&backup)], eject: false, ..Default::default() };

    let plan = api::set_job_plan(&store, &req, &ctx).unwrap();
    let sync = plan.steps.iter().find(|s| s.step == StepKind::Sync).unwrap();
    assert_eq!(sync.reason.as_deref(), Some("no drive is newer than all the others"));
    assert!(sync.warnings[0].starts_with("can't tell which drive holds the newest library: "), "{:?}", sync.warnings);
    let report = api::set_job_apply(&store, &req, &ctx).unwrap();
    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    let done: Vec<StepKind> = report.steps.iter().map(|s| s.step).collect();
    assert_eq!(done, [StepKind::Pull, StepKind::Check]);
    assert!(report.check.is_some());
    assert!(titles(&main).contains(&"Here".to_string()) && !titles(&main).contains(&"There".to_string()));
}

/// A drive that isn't a volume of its own can't be ejected after the check: the job says it
/// stopped there.
#[test]
fn a_set_job_stops_at_an_eject_that_cant_be_done() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main, _) = two_drives(dir.path());
    let req = api::SetJobRequest { drives: vec![path(&main)], ..Default::default() };
    let plan = api::set_job_plan(&store, &req, &Ctx::silent()).unwrap();
    assert_eq!(plan.steps.last().unwrap().summary, ["eject Main once the check passes"]);
    let report = plan.apply(Some(&store), &Ctx::silent()).unwrap();
    let stopped = report.stopped.expect("not ejected");
    assert_eq!(stopped.step, "eject");
    assert!(stopped.message.contains("nothing to eject"), "{}", stopped.message);
}

/// A request that doesn't say ejects the drives once the check passes.
#[test]
fn a_set_job_ejects_unless_told_not_to() {
    let req: api::SetJobRequest = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(req.eject);
    assert!(req.drives.is_empty() && !req.audio);
}
