//! Cleaning drives whose files or databases aren't all as rekordbox left them, and plans that no
//! longer hold.

use crate::common::{self, Warnings};

use std::fs;
use std::path::Path;

use locallibrary::clean::{self, CleanPlan};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::{Error, paths};

/// A drive with the twelve tracks and one of rekordbox's demo samples.
fn with_sample(dir: &Path) -> std::path::PathBuf {
    let root = common::bigger_drive(dir, "Main");
    common::export_new_track(&root, "rekordbox", "House 1");
    root
}

fn plan(root: &Path) -> CleanPlan {
    clean::plan(&Device::open_export(root).unwrap(), &Ctx::silent()).unwrap()
}

#[test]
fn a_drive_without_an_export_has_nothing_to_clean() {
    let dir = tempfile::tempdir().unwrap();
    let err = clean::plan(&Device::open(dir.path()).unwrap(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::NotAnExport(_)), "{err}");
}

#[test]
fn audio_outside_contents_is_left_where_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let sample = rekordbox_pdb::NewTrack {
        artist: Some("rekordbox".into()),
        file_size: 5,
        ..rekordbox_pdb::NewTrack::new("House 2", "/Music/House2.wav")
    };
    ed.add_track(&sample).unwrap();
    ed.save(&pdb_path).unwrap();
    fs::create_dir_all(root.join("Music")).unwrap();
    fs::write(root.join("Music/House2.wav"), b"audio").unwrap();

    let p = plan(&root);
    assert_eq!(p.tracks.len(), 1);
    assert!(p.files.is_empty(), "{:?}", p.files);
    let warning = "1 audio file(s) outside Contents/ are left on the drive (e.g. /Music/House2.wav); locallibrary never \
                   deletes files there";
    assert!(p.warnings.iter().any(|w| w == warning), "{:?}", p.warnings);
    clean::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    assert!(root.join("Music/House2.wav").is_file());
    assert_eq!(Device::open_export(&root).unwrap().library().unwrap().tracks.len(), 12);
}

#[test]
fn a_plan_that_doesnt_fit_or_whose_tracks_changed_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = with_sample(dir.path());
    let p = plan(&root);
    let mut d = Device::open_export(&root).unwrap();
    let before = common::Before::take(&root);

    let mut full = p.clone();
    full.fits = false;
    let err = clean::apply(&full, &mut d, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::NoSpace { .. }), "{err}");
    before.untouched(&root).unwrap();
    // rekordbox exports the drive again, with another sample in the planned one's place.
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    ed.remove_tracks(&[13]).unwrap();
    ed.save(&pdb_path).unwrap();
    common::export_new_track(&root, "rekordbox", "House 2");
    let err = clean::apply(&p, &mut d, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    let titles: Vec<String> = d.library().unwrap().tracks.into_iter().map(|t| t.title).collect();
    assert_eq!(titles.len(), 13);
    assert!(titles.contains(&"House 2".to_string()), "nothing was removed: {titles:?}");
}

#[test]
fn a_onelibrary_that_cant_be_read_is_left_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let root = with_sample(dir.path());
    common::add_onelibrary(&root);
    fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();

    let p = plan(&root);
    assert!(p.warnings.iter().any(|w| w.contains(". Only export.pdb will be changed")), "{:?}", p.warnings);
    clean::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    assert_eq!(Device::open_export(&root).unwrap().library().unwrap().tracks.len(), 12);
    assert_eq!(fs::read(root.join(paths::LIBRARY_PLUS_DB)).unwrap(), b"not a database");
}

#[test]
fn an_exportext_pdb_that_cant_be_read_is_left_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let root = with_sample(dir.path());
    fs::write(root.join(paths::EXPORT_EXT_PDB), b"not a database").unwrap();
    let warnings = Warnings::default();

    let r = clean::apply(&plan(&root), &mut Device::open_export(&root).unwrap(), &warnings.ctx()).unwrap();
    assert_eq!(r.tracks_removed, 1);
    let warnings = warnings.said();
    assert!(warnings.iter().any(|w| w.starts_with("My Tags in exportExt.pdb weren't updated: ")), "{warnings:?}");
    assert_eq!(fs::read(root.join(paths::EXPORT_EXT_PDB)).unwrap(), b"not a database");
}
