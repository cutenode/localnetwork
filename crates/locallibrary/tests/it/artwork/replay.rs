//! Artwork set as an edit: recorded with its image staged on the drive, made again by a replay
//! after rekordbox exports, carried to a synced copy with the journal, and let go with it.

use crate::common::{self, media::png};

use std::path::{Path, PathBuf};

use base64::prelude::*;
use locallibrary::api;
use locallibrary::artwork::{self, ImageSource};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Hash, paths};

/// What players show for the first two tracks, from each database: the medium images.
fn shown(root: &Path) -> Vec<Option<Vec<u8>>> {
    let d = Device::open_export(root).unwrap();
    let (pdb, plus) = (d.library().unwrap(), common::plus_library(root));
    let mut out = Vec::new();
    for t in &pdb.tracks[..2] {
        out.push(artwork::read_from_drive(&d, &pdb, t.id).unwrap());
        let other = plus.tracks.iter().find(|o| o.file_path == t.file_path).unwrap();
        out.push(artwork::read_from_drive(&d, &plus, other.id).unwrap());
    }
    out
}

fn staged(root: &Path, hash: &Hash) -> PathBuf {
    root.join(paths::STATE_DIR).join("artwork").join(format!("{hash}.jpg"))
}

#[test]
fn artwork_set_as_an_edit_is_recorded_replayed_synced_and_let_go() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = common::isolate(dir.path(), None);
    let root = common::bigger_drive(&volumes, "Main");
    common::add_onelibrary(&root);
    let exported = common::library_files(&root);
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let files: Vec<String> = lib.tracks[..2].iter().map(|t| t.file_path.clone()).collect();
    let tracks = TrackSelector { ids: vec![lib.tracks[0].id], ..Default::default() };
    let garbled = Edit::SetArtwork { tracks, image: ImageSource::Base64("not base64!".into()) };
    let err = edit::plan(&d, &[garbled], &EditOptions::default(), &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "image", "{err}");
    let image = ImageSource::Base64(BASE64_STANDARD.encode(png(300, 200, 5)));
    let set = Edit::SetArtwork {
        tracks: TrackSelector { ids: vec![lib.tracks[0].id, lib.tracks[1].id], ..Default::default() },
        image,
    };
    let plan = edit::plan(&d, &[set], &EditOptions::default(), &Ctx::silent()).unwrap();
    // Recorded with the tracks by file and the image by its hash.
    let [Edit::SetArtwork { tracks, image: ImageSource::Staged(hash) }] = &plan.recorded[..] else {
        panic!("{:?}", plan.recorded)
    };
    assert_eq!(tracks.paths, files);
    assert!(!staged(&root, hash).exists(), "nothing on the drive until it's applied");
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let bytes = std::fs::read(staged(&root, hash)).unwrap();
    assert_eq!(Hash::of(&bytes), *hash);
    let edited = shown(&root);
    assert!(edited.iter().all(|s| s.as_ref() == Some(&bytes)), "both databases show the staged image");

    // rekordbox exports again: the art is gone, and a replay sets the same picture again.
    for (key, bytes) in &exported {
        std::fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    assert!(shown(&root).iter().all(Option::is_none));
    common::replay(&root);
    assert_eq!(shown(&root), edited);

    // The staged image goes with the journal to a drive synced from this one, where a replay
    // after an export sets it too.
    let copy = common::blank(&volumes, "Copy");
    api::sync_apply(None, &common::sync_pair(&root, &copy), &Ctx::silent()).unwrap();
    assert_eq!(std::fs::read(staged(&copy, hash)).unwrap(), bytes);
    for (key, bytes) in &exported {
        std::fs::write(paths::to_fs(&copy, key), bytes).unwrap();
    }
    common::replay(&copy);
    assert_eq!(shown(&copy), edited);

    // Named by its hash, the staged image is set like any other; once the journal no longer
    // names it, it's gone.
    let again = Edit::SetArtwork { tracks: TrackSelector::paths(files), image: ImageSource::Staged(*hash) };
    common::apply(&root, std::slice::from_ref(&again));
    let d = Device::open_export(&root).unwrap();
    edit::journal::clear(&d, None).unwrap();
    assert!(!staged(&root, hash).exists());
    let err = edit::plan(&d, &[again], &EditOptions::default(), &Ctx::silent()).unwrap_err();
    assert_eq!(err.to_string(), format!("not found: the staged image {} on Main", hash.short()));
}

/// Give the tracks of `files` the picture of `shade` as an edit, planned on `d`.
fn set(d: &Device, files: &[String], shade: u8) -> edit::EditPlan {
    let image = ImageSource::Base64(BASE64_STANDARD.encode(png(300, 200, shade)));
    let set = Edit::SetArtwork { tracks: TrackSelector::paths(files.iter().cloned()), image };
    edit::plan(d, &[set], &EditOptions::default(), &Ctx::silent()).unwrap()
}

/// What players show for the track of `file` (the medium image), from export.pdb.
fn shows(root: &Path, file: &str) -> Option<Vec<u8>> {
    let d = Device::open_export(root).unwrap();
    let lib = d.library().unwrap();
    let t = lib.tracks.iter().find(|t| t.file_path == file).unwrap();
    artwork::read_from_drive(&d, &lib, t.id).unwrap()
}

/// A plan holds the image it was given, so the drive's copy going (the journal cleared) before
/// it's applied loses nothing; one given only the staged image's hash fails then, before writing.
#[test]
fn a_plan_keeps_its_image_and_one_naming_a_staged_image_that_went_fails_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let mut d = Device::open_export(&root).unwrap();
    let files: Vec<String> = d.library().unwrap().tracks[..3].iter().map(|t| t.file_path.clone()).collect();
    edit::apply(&set(&d, &files[..1], 5), &mut d, &Ctx::silent()).unwrap();
    let plan = set(&d, &files[1..2], 5);
    edit::journal::clear(&d, None).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let [Edit::SetArtwork { image: ImageSource::Staged(hash), .. }] = &plan.recorded[..] else { panic!() };
    assert_eq!(shows(&root, &files[1]), Some(std::fs::read(staged(&root, hash)).unwrap()));

    let named =
        Edit::SetArtwork { tracks: TrackSelector::paths([files[2].clone()]), image: ImageSource::Staged(*hash) };
    let plan = edit::plan(&d, &[named], &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::journal::clear(&d, None).unwrap();
    let before = common::Before::take(&root);
    let err = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap_err().to_string();
    assert!(err.ends_with(&format!("not found: the staged image {} on A; it changed nothing", hash.short())), "{err}");
    before.untouched(&root).unwrap();
}

/// A track deleted in the plan that sets its artwork takes the new images along.
#[test]
fn a_track_deleted_after_its_artwork_is_set_goes_with_its_new_images() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let mut d = Device::open_export(&root).unwrap();
    let file = d.library().unwrap().tracks[0].file_path.clone();
    let one = || TrackSelector::paths([file.clone()]);
    let image = ImageSource::Base64(BASE64_STANDARD.encode(png(300, 200, 9)));
    let edits = [Edit::SetArtwork { tracks: one(), image }, Edit::DeleteTracks { tracks: one(), keep_files: false }];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let lib = d.library().unwrap();
    assert!(!lib.tracks.iter().any(|t| t.file_path == file));
    let images = walkdir::WalkDir::new(root.join("PIONEER/Artwork")).into_iter().filter_map(|e| e.ok());
    assert_eq!(images.filter(|e| e.file_type().is_file()).count(), 0, "no image is left that no track uses");
}

/// Replaying, an artwork edit finds its tracks as it's made: after a recorded pointing of one at
/// another file, and without one the new export lacks, which a warning names.
#[test]
fn a_replayed_artwork_edit_finds_its_tracks_as_it_is_made() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let exported = common::library_files(&root);
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let (moved, other) = (lib.tracks[0].clone(), lib.tracks[1].file_path.clone());
    let copy = "/Contents/Moved/copy.mp3".to_string();
    let to = paths::to_fs(&root, &paths::key(&copy));
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(paths::to_fs(&root, &paths::key(&moved.file_path)), &to).unwrap();
    let point = Edit::SetPath {
        track: TrackSelector::paths([moved.file_path.clone()]),
        path: copy.clone(),
        file: None,
        shift_ms: 0,
    };
    common::apply(&root, &[point]);
    edit::apply(&set(&d, &[copy.clone(), other.clone()], 3), &mut d, &Ctx::silent()).unwrap();
    let (art, other_art) = (shows(&root, &copy), shows(&root, &other));
    assert!(art.is_some() && other_art == art);

    // rekordbox exports again: the track is at its old file, and the replay points it again.
    for (key, bytes) in &exported {
        std::fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    common::replay(&root);
    assert_eq!((shows(&root, &copy), shows(&root, &other)), (art.clone(), art.clone()));

    // Exported again, without the other track: the rest of the edit is made, with a warning.
    for (key, bytes) in &exported {
        std::fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    ed.remove_tracks(&[lib.tracks[1].id]).unwrap();
    ed.save(&pdb_path).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    let opts = EditOptions { lenient: true, no_journal: true };
    let plan = edit::plan(&d, &edit::journal::edits(&d).unwrap(), &opts, &Ctx::silent()).unwrap();
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let gone = format!(
        "1 track(s) aren't on the drive any more (e.g. {other}), so \"set the artwork of 2 track(s)\" is replayed without them"
    );
    assert!(report.warnings.contains(&gone), "{:?}", report.warnings);
    assert_eq!(shows(&root, &copy), art);
}
