//! What the doctor finds on drives with one thing wrong each, orphan plans that no longer hold
//! when they're applied, and the doctor through the service layer.

use crate::common::{self, path};

use std::fs;
use std::path::Path;

use locallibrary::api::{self, DriveRef};
use locallibrary::device::Device;
use locallibrary::doctor::{self, DoctorOptions, Problem, Section};
use locallibrary::progress::Ctx;
use locallibrary::{Error, paths};
use rekordbox_pdb::{NewTrack, PdbEditor};

/// Add `tracks` to the drive's export.pdb (without audio). Returns their ids.
fn add(root: &Path, tracks: &[NewTrack]) -> Vec<u32> {
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    let ids = tracks.iter().map(|t| ed.add_track(t).unwrap()).collect();
    ed.save(&pdb).unwrap();
    ids
}

fn only(sections: &[Section]) -> DoctorOptions {
    DoctorOptions { only: sections.to_vec(), ..Default::default() }
}

#[test]
fn a_healthy_drive_has_no_problems() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open_export(common::demo_drive(dir.path(), "DNA")).unwrap();
    let r = doctor::run(&d, &DoctorOptions::default(), &Ctx::silent()).unwrap();
    assert!(!r.has_problems(), "{r:#?}");
}

/// A database path that climbs out of the drive is reported as such, never looked for outside
/// it.
#[test]
fn a_path_that_leaves_the_drive_is_reported_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "DNA");
    fs::write(dir.path().join("outside.mp3"), b"not the drive's").unwrap();
    let [id] = add(&root, &[NewTrack::new("Escape", "/Contents/../../outside.mp3")])[..] else { panic!() };
    let d = Device::open_export(&root).unwrap();
    let r = doctor::run(&d, &only(&[Section::Missing]), &Ctx::silent()).unwrap();
    let found: Vec<(u32, Problem)> = r.missing.as_ref().unwrap().iter().map(|m| (m.track.id, m.problem)).collect();
    assert_eq!(found, [(id, Problem::UnsafePath)]);
    assert!(r.has_problems());
}

/// Of two copies in no playlist at the same bitrate, the one with a date added stays.
#[test]
fn the_copy_kept_is_the_one_with_a_date() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "DNA");
    let track = |path: &str, date: &str| NewTrack {
        artist: Some("Someone".into()),
        duration: 200,
        bitrate: 320,
        date_added: date.into(),
        ..NewTrack::new("Same Song", path)
    };
    let ids = add(&root, &[track("/Contents/a/song.mp3", ""), track("/Contents/b/song.mp3", "2020-01-01")]);
    let d = Device::open_export(&root).unwrap();
    let r = doctor::run(&d, &only(&[Section::Duplicates]), &Ctx::silent()).unwrap();
    let groups = r.duplicates.unwrap();
    assert_eq!(groups.len(), 1, "{groups:#?}");
    assert_eq!(groups[0].keep, ids[1]);
}

/// A drive whose only database can't be read can't be checked at all; one that isn't an
/// export isn't read.
#[test]
fn databases_that_cant_be_read_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "DNA");
    common::onelibrary_only(&root);
    fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not sqlcipher").unwrap();
    let check = |d: &Device| doctor::run(d, &DoctorOptions::default(), &Ctx::silent());
    assert!(matches!(check(&Device::open_export(&root).unwrap()), Err(Error::OneLibrary(_))));
    let empty = dir.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert!(matches!(check(&Device::open(&empty).unwrap()), Err(Error::NotAnExport(_))));
}

/// An orphan plan is only applied while it holds: nothing to do is nothing done, a backup that
/// won't fit stops it, and a file a database uses since is no orphan any more.
#[test]
fn orphan_plans_are_applied_only_while_they_hold() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "DNA");
    let mut d = Device::open_export(&root).unwrap();
    let noop = doctor::plan_orphans(&d, &Ctx::silent()).unwrap();
    assert!(noop.is_noop());
    assert_eq!(doctor::apply_orphans(&noop, &mut d, &Ctx::silent()).unwrap().files_deleted, 0);

    let key = "Contents/Later/song.mp3";
    fs::create_dir_all(root.join("Contents/Later")).unwrap();
    fs::write(paths::to_fs(&root, key), b"audio").unwrap();
    let plan = doctor::plan_orphans(&d, &Ctx::silent()).unwrap();
    assert_eq!(plan.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), [key]);
    let full = doctor::OrphanPlan { fits: false, free_space: Some(1), ..plan.clone() };
    assert!(matches!(doctor::apply_orphans(&full, &mut d, &Ctx::silent()), Err(Error::NoSpace { available: 1, .. })));

    // rekordbox exports a track using the file meanwhile.
    add(&root, &[NewTrack::new("Later", format!("/{key}"))]);
    let mut d = Device::open_export(&root).unwrap();
    let err = doctor::apply_orphans(&plan, &mut d, &Ctx::silent()).unwrap_err();
    assert!(matches!(err.inner(), Error::StalePlan(_)), "{err}");
    assert!(paths::to_fs(&root, key).is_file());
}

/// A file in the drive's Contents that no track uses is reported, planned for deletion, deleted
/// (backed up), and then there's nothing left to delete.
#[test]
fn an_orphaned_file_is_reported_and_deleted_as_planned() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    std::fs::create_dir_all(root.join("Contents/Stray")).unwrap();
    std::fs::write(root.join("Contents/Stray/stray.mp3"), b"nobody's audio").unwrap();
    let drive = DriveRef { drive: path(&root) };

    let req = api::DoctorRequest { drive: path(&root), options: Default::default() };
    let report = api::doctor(&req, &Ctx::silent()).unwrap();
    let orphans: Vec<String> = report.orphans.unwrap().into_iter().map(|o| o.path).collect();
    assert_eq!(orphans, ["Contents/Stray/stray.mp3"]);
    let plan = api::doctor_orphans_plan(&drive, &Ctx::silent()).unwrap();
    assert!(plan.destructive);
    assert_eq!(plan.summary, ["delete 1 orphaned file(s) (14 B) from Main"]);
    let cleaned = api::doctor_orphans_apply(&drive, &Ctx::silent()).unwrap();
    assert_eq!(cleaned.files_deleted, 1);
    assert!(cleaned.backup.is_some());
    assert!(api::doctor_orphans_plan(&drive, &Ctx::silent()).unwrap().noop);
}
