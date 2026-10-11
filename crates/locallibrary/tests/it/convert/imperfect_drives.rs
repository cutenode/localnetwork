//! Converting drives that aren't quite what rekordbox leaves: missing images, unreadable
//! databases and analysis files, orphaned playlists, My Tags with nowhere to go.

use crate::common::{self, Warnings};

use std::fs;
use std::path::{Path, PathBuf};

use locallibrary::convert::{self, ConvertOptions, ConvertPlan, ConvertReport, Target};
use locallibrary::device::Device;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{Error, paths};

fn opts(to: Target, replace: bool) -> ConvertOptions {
    ConvertOptions { to, replace }
}

/// The twelve-track drive with one image on its first two tracks.
fn with_artwork(dir: &Path) -> PathBuf {
    let root = common::bigger_drive(dir, "A");
    common::media::set_art(&root, &[1, 2]);
    root
}

/// Plan and apply a conversion: the plan, the report, and the warnings applying it gave.
fn convert(root: &Path, opts: &ConvertOptions) -> (ConvertPlan, ConvertReport, Vec<String>) {
    let mut d = Device::open_export(root).unwrap();
    let warnings = Warnings::default();
    let plan = convert::plan(&d, opts, &Ctx::silent()).unwrap();
    let report = convert::apply(&plan, &mut d, opts, &warnings.ctx()).unwrap();
    (plan, report, warnings.said())
}

/// Give a OneLibrary database My Tag "Mood/Dark" on its first track.
fn tag_first_track(root: &Path) {
    let one = OneLibrary::open_root(root).unwrap();
    one.connection()
        .execute_batch(
            "INSERT INTO myTag VALUES (1, 0, 'Mood', 1, 0), (2, 0, 'Dark', 0, 1);
             INSERT INTO myTag_content VALUES (2, 1);",
        )
        .unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
}

#[test]
fn a_drive_without_the_database_to_convert_from_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let err = convert::plan(&d, &opts(Target::DeviceLibrary, true), &Ctx::silent()).unwrap_err();
    assert_eq!(err.to_string(), format!("{} has no exportLibrary.db to convert from", d.name()));
}

#[test]
fn a_plan_that_no_longer_fits_or_matches_the_drive_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let mut d = Device::open_export(&root).unwrap();
    let before = common::Before::take(&root);
    let to = opts(Target::OneLibrary, false);
    let plan = convert::plan(&d, &to, &Ctx::silent()).unwrap();

    let full = ConvertPlan { fits: false, ..plan.clone() };
    let err = convert::apply(&full, &mut d, &to, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::NoSpace { needed, .. } if needed == plan.needed_bytes), "{err}");
    let stale = ConvertPlan { tracks: plan.tracks + 1, ..plan };
    let err = convert::apply(&stale, &mut d, &to, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    before.untouched(&root).unwrap();
}

#[test]
fn artwork_missing_on_the_drive_is_left_out_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let root = with_artwork(dir.path());
    fs::remove_dir_all(root.join("PIONEER/Artwork")).unwrap();

    let (plan, report, _) = convert(&root, &opts(Target::OneLibrary, false));
    assert_eq!(plan.artwork, 0);
    let warning = "1 artwork image(s) the tracks point at are missing on the drive; those tracks get no artwork";
    assert!(plan.warnings.iter().any(|w| w == warning), "{:?}", plan.warnings);
    assert_eq!((report.tracks, report.artwork), (12, 0));
    assert!(common::plus_library(&root).tracks.iter().all(|t| t.artwork_path.is_none()));
}

#[test]
fn rebuilding_onelibrary_keeps_the_images_it_had() {
    let dir = tempfile::tempdir().unwrap();
    let root = with_artwork(dir.path());
    common::add_onelibrary(&root);
    let images = |root: &Path| -> Vec<Option<String>> {
        common::plus_library(root).tracks.into_iter().map(|t| t.artwork_path).collect()
    };
    let before = images(&root);
    assert!(before[0].as_deref().is_some_and(|p| p.contains("/b")), "{before:?}");

    let (_, report, _) = convert(&root, &opts(Target::OneLibrary, true));
    assert_eq!(report.artwork, 0, "no image is copied again");
    assert_eq!(images(&root), before);
}

#[test]
fn a_database_that_cant_be_read_is_rebuilt_from_scratch() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();

    let (plan, report, _) = convert(&root, &opts(Target::OneLibrary, true));
    assert!(plan.replaces);
    let warning = "the current exportLibrary.db can't be read; it's rebuilt from scratch";
    assert!(plan.warnings.iter().any(|w| w == warning), "{:?}", plan.warnings);
    assert_eq!(report.tracks, 12);
    assert_eq!(common::plus_library(&root).tracks.len(), 12);
    common::sqlcipher_reads(&root);
}

#[test]
fn analysis_files_that_cant_be_read_give_no_cues_and_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    for t in &lib.tracks {
        fs::write(paths::to_fs(&root, &paths::key(&t.analyze_path)), b"garbage").unwrap();
    }

    let (_, report, warnings) = convert(&root, &opts(Target::OneLibrary, false));
    assert_eq!((report.tracks, report.cues), (2, 0));
    let warning = format!(
        "2 track(s) have analysis files that can't be read (e.g. {}); they get no cues in OneLibrary",
        lib.tracks[0].display_name()
    );
    assert!(warnings.contains(&warning), "{warnings:?}");
}

#[test]
fn folders_and_playlists_whose_folder_is_gone_come_across() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::onelibrary_only(&root);
    let mut one = OneLibrary::open_root(&root).unwrap();
    let sets = one.create_playlist("Sets", 0, true).unwrap();
    let warmup = one.create_playlist("Warmup", sets, false).unwrap();
    one.add_to_playlist(warmup, 3).unwrap();
    let lost = one.create_playlist("Lost", 999, false).unwrap();
    one.add_to_playlist(lost, 4).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    let (plan, report, _) = convert(&root, &opts(Target::DeviceLibrary, false));
    assert_eq!((plan.playlists, plan.folders), (3, 1));
    assert_eq!(report.playlists, 3);
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let titles = |path: &str| -> Vec<String> {
        let p = lib.find_playlist(path).unwrap();
        p.track_ids.iter().map(|id| lib.track(*id).unwrap().title.clone()).collect()
    };
    let plus = common::plus_library(&root);
    let title = |id: u32| plus.track(id).unwrap().title.clone();
    assert_eq!(titles("Sets/Warmup"), [title(3)]);
    assert_eq!(lib.find_playlist("Lost").unwrap().parent_id, None, "at the top level");
    assert_eq!(titles("Lost"), [title(4)]);
}

#[test]
fn my_tags_without_an_exportext_pdb_to_go_in_are_warned_about() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::onelibrary_only(&root);
    tag_first_track(&root);

    let (plan, report, _) = convert(&root, &opts(Target::DeviceLibrary, false));
    assert_eq!(plan.tagged_tracks, 0);
    assert!(plan.warnings.iter().any(|w| w.starts_with("1 track(s) have My Tags, but the drive has no exportExt.pdb")));
    assert_eq!(report.tagged_tracks, 0);
    assert!(!root.join(paths::EXPORT_EXT_PDB).exists(), "none is made");
}

#[test]
fn an_exportext_pdb_that_cant_be_read_is_left_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    // Into OneLibrary: its My Tags can't come along.
    let root = common::bigger_drive(dir.path(), "A");
    fs::write(root.join(paths::EXPORT_EXT_PDB), b"not a database").unwrap();
    let (plan, report, warnings) = convert(&root, &opts(Target::OneLibrary, false));
    assert!(plan.warnings.iter().any(|w| w.ends_with("; its My Tags aren't converted")), "{:?}", plan.warnings);
    assert!(warnings.iter().any(|w| w.ends_with("; its My Tags aren't converted")), "{warnings:?}");
    assert_eq!((report.tracks, report.tagged_tracks), (12, 0));

    // Into export.pdb: it can't be moved to the new tracks, so it stays as it was.
    let root = common::bigger_drive(dir.path(), "B");
    common::onelibrary_only(&root);
    tag_first_track(&root);
    fs::write(root.join(paths::EXPORT_EXT_PDB), b"not a database").unwrap();
    let (plan, report, warnings) = convert(&root, &opts(Target::DeviceLibrary, false));
    let left = "exportExt.pdb is left as it is, so its My Tags may name the wrong tracks";
    assert!(plan.warnings.iter().any(|w| w.ends_with(left)), "{:?}", plan.warnings);
    assert!(warnings.iter().any(|w| w.ends_with(left)), "{warnings:?}");
    assert_eq!(report.tagged_tracks, 0);
    assert_eq!(fs::read(root.join(paths::EXPORT_EXT_PDB)).unwrap(), b"not a database");

    // Into the export.pdb the drive has: the same, and the rest is added.
    let root = common::bigger_drive(dir.path(), "C");
    common::add_onelibrary(&root);
    tag_first_track(&root);
    fs::write(root.join(paths::EXPORT_EXT_PDB), b"not a database").unwrap();
    let (plan, report, warnings) = convert(&root, &opts(Target::DeviceLibrary, false));
    let left = "; its My Tags aren't converted";
    assert!(plan.adds && plan.warnings.iter().any(|w| w.ends_with(left)), "{:?}", plan.warnings);
    assert!(warnings.iter().any(|w| w.ends_with(left)), "{warnings:?}");
    assert_eq!(report.tagged_tracks, 0);
    assert_eq!(fs::read(root.join(paths::EXPORT_EXT_PDB)).unwrap(), b"not a database");
}

/// Rebuilding OneLibrary keeps a track's own row, My Tags included, though exportExt.pdb lacks
/// the tag.
#[test]
fn rebuilding_onelibrary_keeps_its_my_tags() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    tag_first_track(&root);
    let tags_of_first = |root: &Path| -> Vec<(String, String)> {
        let one = OneLibrary::open_root(root).unwrap();
        let first = one.library().unwrap().tracks[0].id;
        one.export_track(first).unwrap().unwrap().my_tags
    };
    assert_eq!(tags_of_first(&root), [("Mood".to_string(), "Dark".to_string())]);

    convert(&root, &opts(Target::OneLibrary, true));
    assert_eq!(tags_of_first(&root), [("Mood".to_string(), "Dark".to_string())]);
    common::sqlcipher_reads(&root);
}
