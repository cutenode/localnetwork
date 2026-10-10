//! The files a merge copies besides audio: artwork into each of the target's databases, and
//! analysis files, renamed or left as they are when they can't be read.

use crate::common::{self, Warnings};

use std::fs;
use std::path::{Path, PathBuf};

use locallibrary::artwork;
use locallibrary::device::Device;
use locallibrary::merge::{self, MergeOptions, MergePlan, MergeReport, PlaylistMode};
use locallibrary::{Library, Track, anlz, paths};

/// The demo drive with one image set on both of its tracks.
fn with_artwork(dir: &Path, name: &str) -> PathBuf {
    let root = common::demo_drive(dir, name);
    common::media::set_art(&root, &[1, 2]);
    root
}

/// Plan and apply a merge of `src` into `dst`: the plan, the report and the warnings it gave.
fn merge_into(src: &Path, dst: &Path, opts: &MergeOptions) -> (MergePlan, MergeReport, Vec<String>) {
    let (src, mut dst) = (Device::open_export(src).unwrap(), Device::open_export(dst).unwrap());
    let warnings = Warnings::default();
    let plan = merge::plan(&src, &dst, opts, &warnings.ctx()).unwrap();
    let report = merge::apply(&plan, &src, &mut dst, opts, &warnings.ctx()).unwrap();
    (plan, report, warnings.said())
}

fn read(root: &Path, device_path: &str) -> Vec<u8> {
    fs::read(paths::to_fs(root, &paths::key(device_path))).unwrap()
}

fn guests(lib: &Library) -> Vec<&Track> {
    lib.tracks.iter().filter(|t| t.artist == "Loopmasters").collect()
}

#[test]
fn merge_copies_artwork_into_both_of_the_targets_databases() {
    let dir = tempfile::tempdir().unwrap();
    let src = with_artwork(dir.path(), "Guest");
    let source_art = locallibrary::pdb::read_library(&src.join(paths::EXPORT_PDB)).unwrap().tracks[0].clone();
    let art_path = source_art.artwork_path.unwrap();
    let dst = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&dst);

    let (plan, report, warnings) = merge_into(&src, &dst, &MergeOptions::default());
    // The two tracks share one image, which is copied once, with its larger variant.
    assert_eq!(plan.artwork_files, 1);
    let art_bytes = read(&src, &art_path).len() + read(&src, &artwork::medium_variant(&art_path)).len();
    let audio_and_analysis: u64 = plan.new_tracks.iter().map(|t| t.size).sum::<u64>();
    assert!(plan.copy_bytes >= audio_and_analysis + art_bytes as u64, "{plan:?}");
    assert_eq!(report.artwork_added, 1);
    assert!(warnings.iter().all(|w| !w.contains("artwork")), "{warnings:?}");

    common::sqlcipher_reads(&dst);
    let pdb = Device::open_export(&dst).unwrap().library().unwrap();
    let plus = common::plus_library(&dst);
    for lib in [&pdb, &plus] {
        let merged = guests(lib);
        assert_eq!(merged.len(), 2);
        let images: Vec<&String> = merged.iter().map(|t| t.artwork_path.as_ref().unwrap()).collect();
        assert_eq!(images[0], images[1], "one row for the shared image");
        assert_eq!(read(&dst, images[0]), read(&src, &art_path));
        assert_eq!(read(&dst, &artwork::medium_variant(images[0])), read(&src, &artwork::medium_variant(&art_path)));
    }
}

#[test]
fn merge_into_a_onelibrary_only_drive_counts_the_artwork_it_copies() {
    let dir = tempfile::tempdir().unwrap();
    let src = with_artwork(dir.path(), "Guest");
    let dst = common::bigger_drive(dir.path(), "Main");
    common::onelibrary_only(&dst);

    let (_, report, _) = merge_into(&src, &dst, &MergeOptions::default());
    assert_eq!((report.tracks_added, report.artwork_added), (2, 1));
    common::sqlcipher_reads(&dst);
    let plus = common::plus_library(&dst);
    let image = guests(&plus)[0].artwork_path.clone().unwrap();
    assert!(image.contains("/b"), "OneLibrary's own name for it: {image}");
    assert!(!read(&dst, &image).is_empty());
}

#[test]
fn artwork_missing_on_the_source_is_left_out_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let src = with_artwork(dir.path(), "Guest");
    fs::remove_dir_all(src.join("PIONEER/Artwork")).unwrap();
    let pdb_only = common::bigger_drive(dir.path(), "Main");
    let plus_only = common::bigger_drive(dir.path(), "Plus");
    common::onelibrary_only(&plus_only);

    for (dst, library) in [(&pdb_only, paths::EXPORT_PDB), (&plus_only, paths::LIBRARY_PLUS_DB)] {
        let (_, report, warnings) = merge_into(&src, dst, &MergeOptions::default());
        assert_eq!((report.tracks_added, report.artwork_added), (2, 0), "{library}");
        assert!(warnings.iter().any(|w| w.contains("missing on the source")), "{library}: {warnings:?}");
        let lib = Device::open_export(dst).unwrap().library().unwrap();
        assert!(guests(&lib).iter().all(|t| t.artwork_path.is_none()), "{library}");
    }
}

#[test]
fn analysis_files_get_a_folder_of_their_own_when_the_targets_is_taken() {
    let dir = tempfile::tempdir().unwrap();
    // Both drives export a 13th track, so both put its analysis in the same folder.
    let src = common::bigger_drive(dir.path(), "Src");
    common::export_new_track(&src, "Extra", "Bonus");
    let dst = common::bigger_drive(dir.path(), "Dst");
    common::export_new_track(&dst, "Other", "Thing");
    let theirs = |root: &Path| {
        let lib = Device::open_export(root).unwrap().library().unwrap();
        lib.tracks.into_iter().find(|t| t.title == "Thing").unwrap()
    };
    let (thing, thing_dat) = (theirs(&dst), read(&dst, &theirs(&dst).analyze_path));
    let bonus = locallibrary::pdb::read_library(&src.join(paths::EXPORT_PDB)).unwrap();
    let bonus = bonus.tracks.iter().find(|t| t.title == "Bonus").unwrap();
    assert_eq!(bonus.analyze_path, thing.analyze_path);

    let opts = MergeOptions { playlists: PlaylistMode::None, ..Default::default() };
    let (plan, _, _) = merge_into(&src, &dst, &opts);
    assert_eq!(plan.new_tracks.len(), 1);
    let lib = Device::open_export(&dst).unwrap().library().unwrap();
    let merged = lib.tracks.iter().find(|t| t.title == "Bonus").unwrap();
    assert_ne!(merged.analyze_path, thing.analyze_path);
    assert!(merged.analyze_path.starts_with("/PIONEER/USBANLZ/P"), "{}", merged.analyze_path);
    let dat = read(&dst, &merged.analyze_path);
    assert_eq!(anlz::read_path(&dat).unwrap().as_deref(), Some(merged.file_path.as_str()));
    assert_eq!(read(&dst, &thing.analyze_path), thing_dat, "the target's own analysis is untouched");
}

#[test]
fn analysis_files_that_cant_be_read_are_copied_as_they_are() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::bigger_drive(dir.path(), "Src");
    common::export_new_track(&src, "Extra", "Bonus");
    let lib = locallibrary::pdb::read_library(&src.join(paths::EXPORT_PDB)).unwrap();
    let bonus = lib.tracks.iter().find(|t| t.title == "Bonus").unwrap();
    fs::write(paths::to_fs(&src, &paths::key(&bonus.analyze_path)), b"not an analysis file").unwrap();
    let dst = common::bigger_drive(dir.path(), "Dst");
    common::add_onelibrary(&dst);

    let (_, report, warnings) = merge_into(&src, &dst, &MergeOptions::default());
    assert_eq!(report.tracks_added, 1);
    assert!(warnings.iter().any(|w| w.contains("unrecognised analysis format")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("no cues in OneLibrary")), "{warnings:?}");
    let lib = Device::open_export(&dst).unwrap().library().unwrap();
    let merged = lib.tracks.iter().find(|t| t.title == "Bonus").unwrap();
    assert_eq!(read(&dst, &merged.analyze_path), b"not an analysis file");
}

#[test]
fn tracks_whose_audio_is_missing_on_the_source_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::demo_drive(dir.path(), "Guest");
    let lib = locallibrary::pdb::read_library(&src.join(paths::EXPORT_PDB)).unwrap();
    fs::remove_file(paths::to_fs(&src, &paths::key(&lib.tracks[0].file_path))).unwrap();
    let dst = common::bigger_drive(dir.path(), "Main");

    let (plan, report, _) = merge_into(&src, &dst, &MergeOptions::default());
    assert_eq!(plan.new_tracks.iter().map(|t| t.source_id).collect::<Vec<_>>(), [lib.tracks[1].id]);
    let warning = format!(
        "{}: audio file {} is missing on the source; skipped",
        lib.tracks[0].display_name(),
        lib.tracks[0].file_path
    );
    assert!(plan.warnings.contains(&warning), "{:?}", plan.warnings);
    assert_eq!(report.tracks_added, 1);
}
