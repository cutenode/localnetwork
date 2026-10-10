//! Which source tracks a merge takes for the target's own, the merges it refuses, and a merge
//! made again through the service layer.

use crate::common::{self, Warnings, path};

use std::fs;
use std::path::Path;

use locallibrary::api;
use locallibrary::device::Device;
use locallibrary::merge::{self, MatchMode, MergeOptions, PlaylistMode};
use locallibrary::progress::Ctx;
use locallibrary::{Error, Library, Playlist, paths};

fn plan(src: &Device, dst: &Device, opts: &MergeOptions) -> locallibrary::Result<merge::MergePlan> {
    merge::plan(src, dst, opts, &Ctx::silent())
}

#[test]
fn tracks_match_by_artist_title_and_length_unless_matching_by_path_only() {
    let dir = tempfile::tempdir().unwrap();
    // The source's first track has another size in its row, so its path alone doesn't match.
    let src_root = common::bigger_drive(dir.path(), "Src");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(src_root.join(paths::EXPORT_PDB)).unwrap();
    ed.set_track_field(1, "file_size", 999).unwrap();
    ed.save(src_root.join(paths::EXPORT_PDB)).unwrap();
    let src = Device::open_export(&src_root).unwrap();
    let dst = Device::open_export(common::bigger_drive(dir.path(), "Dst")).unwrap();

    let tracks_only = MergeOptions { playlists: PlaylistMode::None, ..Default::default() };
    let by_metadata = plan(&src, &dst, &tracks_only).unwrap();
    assert_eq!((by_metadata.matched_tracks, by_metadata.new_tracks.len()), (12, 0));
    assert!(by_metadata.is_noop());

    let by_path = plan(&src, &dst, &MergeOptions { match_mode: MatchMode::Path, ..tracks_only }).unwrap();
    assert_eq!(by_path.matched_tracks, 11);
    let copy = &by_path.new_tracks[0];
    assert_eq!(copy.source_id, 1);
    assert!(copy.renamed && copy.audio.ends_with(" (2).mp3"), "{}", copy.audio);
    assert!(!by_path.is_noop());
}

/// A track the target has in export.pdb but not in OneLibrary still goes into the merged
/// playlists in export.pdb, and is left out of OneLibrary's with a warning.
#[test]
fn matched_tracks_onelibrary_lacks_are_left_out_of_its_playlists() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::bigger_drive(dir.path(), "Src");
    let bonus = common::export_new_track(&src_root, "Extra", "Bonus");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(src_root.join(paths::EXPORT_PDB)).unwrap();
    ed.add_to_playlist(1, bonus, None).unwrap();
    ed.save(src_root.join(paths::EXPORT_PDB)).unwrap();
    let dst_root = common::bigger_drive(dir.path(), "Dst");
    common::add_onelibrary(&dst_root);
    common::export_new_track(&dst_root, "Extra", "Bonus");
    let src = Device::open_export(&src_root).unwrap();
    let mut dst = Device::open_export(&dst_root).unwrap();

    let p = plan(&src, &dst, &MergeOptions::default()).unwrap();
    assert_eq!((p.matched_tracks, p.new_tracks.len()), (13, 0));
    let warnings = Warnings::default();
    merge::apply(&p, &src, &mut dst, &MergeOptions::default(), &warnings.ctx()).unwrap();
    let warnings = warnings.said();
    assert!(
        warnings.iter().any(|w| w.contains("1 matched track(s) are in the target's export.pdb but not")),
        "{warnings:?}"
    );
    let entries = |lib: &Library| lib.find_playlist("Src/aab").unwrap().track_ids.len();
    assert_eq!(entries(&dst.library().unwrap()), 13);
    assert_eq!(entries(&common::plus_library(&dst_root)), 12);
}

#[test]
fn a_drive_is_not_merged_into_itself() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let (a, b) = (Device::open_export(&root).unwrap(), Device::open_export(&root).unwrap());
    let err = plan(&a, &b, &MergeOptions::default()).unwrap_err();
    assert!(err.to_string().contains("same drive"), "{err}");
}

#[test]
fn a_drive_without_an_export_is_pointed_at_sync() {
    let dir = tempfile::tempdir().unwrap();
    let src = Device::open_export(common::demo_drive(dir.path(), "Guest")).unwrap();
    fs::create_dir(dir.path().join("Blank")).unwrap();
    let blank = Device::open(dir.path().join("Blank")).unwrap();
    let err = plan(&src, &blank, &MergeOptions::default()).unwrap_err();
    assert_eq!(err.reason(), Some(locallibrary::error::Reason::NoExport), "{err}");
    assert!(err.to_string().contains("sync copies the whole library instead"), "{err}");
}

#[test]
fn a_plan_that_doesnt_fit_is_refused_and_the_target_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let src = Device::open_export(common::demo_drive(dir.path(), "Guest")).unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let mut dst = Device::open_export(&root).unwrap();
    let mut p = plan(&src, &dst, &MergeOptions::default()).unwrap();
    let before = common::Before::take(&root);
    p.fits = false;
    let err = merge::apply(&p, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::NoSpace { needed, .. } if needed == p.needed_bytes), "{err}");
    before.untouched(&root).unwrap();
}

/// A source with only OneLibrary (which takes names of any length) holding `playlists` and
/// `history` over the demo drive's tracks.
fn onelibrary_source(root: &Path, playlists: &[&str], history: &[&str]) -> Device {
    let mut lib = locallibrary::pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let list = |(i, name): (usize, &&str)| Playlist {
        id: i as u32 + 1,
        parent_id: None,
        name: name.to_string(),
        is_folder: false,
        sort_order: i as u32,
        track_ids: vec![lib.tracks[0].id],
    };
    lib.playlists = playlists.iter().enumerate().map(list).collect();
    lib.history = history.iter().enumerate().map(list).collect();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), common::onelibrary_for(&lib).to_encrypted().unwrap()).unwrap();
    for key in [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB] {
        fs::remove_file(root.join(key)).unwrap();
    }
    Device::open_export(root).unwrap()
}

#[test]
fn names_too_long_for_the_targets_export_pdb_are_refused_when_planning() {
    let dir = tempfile::tempdir().unwrap();
    let dst = Device::open_export(common::bigger_drive(dir.path(), "Main")).unwrap();
    let long = "x".repeat(4100);

    let src = onelibrary_source(&common::demo_drive(dir.path(), "Lists"), &[&long], &[]);
    let err = plan(&src, &dst, &MergeOptions::default()).unwrap_err();
    assert!(err.to_string().starts_with(&format!("the name of {long} is too long")), "{err}");

    let src = onelibrary_source(&common::demo_drive(dir.path(), "History"), &[], &[&long]);
    assert!(plan(&src, &dst, &MergeOptions::default()).is_ok(), "history stays behind unless asked for");
    let err = plan(&src, &dst, &MergeOptions { history: true, ..Default::default() }).unwrap_err();
    assert!(err.to_string().contains("the name of a history list is too long"), "{err}");
}

/// Merging one drive into another says what it adds and adds it; merged again, its tracks are
/// all there already.
#[test]
fn a_merge_adds_what_it_planned_and_then_matches_it() {
    let dir = tempfile::tempdir().unwrap();
    let main = common::demo_drive(dir.path(), "Main");
    let club = common::bigger_drive(dir.path(), "Club");
    let req = api::MergeRequest { source: path(&club), target: path(&main), options: Default::default() };

    let plan = api::merge_plan(&req, &Ctx::silent()).unwrap();
    assert!(plan.summary[0].starts_with("add 12 track(s) ("), "{:?}", plan.summary);
    assert!(plan.summary[0].ends_with(") from Club to Main"), "{:?}", plan.summary);
    assert_eq!(plan.summary.len(), 2, "and the playlist: {:?}", plan.summary);
    let report = api::merge_apply(&req, &Ctx::silent()).unwrap();
    assert_eq!(report.tracks_added, 12);
    assert_eq!(Device::open(&main).unwrap().library().unwrap().tracks.len(), 14);
    // Merged again, every track is matched; only the folder of its playlists comes again.
    let again = api::merge_plan(&req, &Ctx::silent()).unwrap();
    assert_eq!((again.new_tracks.len(), again.matched_tracks), (0, 12));
    assert!(again.playlists.iter().all(|p| p.path.starts_with("Club (2)")), "{:?}", again.playlists);
}
