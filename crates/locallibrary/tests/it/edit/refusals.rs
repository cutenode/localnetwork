//! Edits refused when planning or applying, and history lists deleted as the journal names them.

use crate::common;

use std::fs;

use locallibrary::cue_edit::{CueChange, CueRef};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, EditPlan, HistoryList};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Error, paths};

fn plan(root: &std::path::Path, edits: &[Edit], lenient: bool) -> locallibrary::Result<EditPlan> {
    let opts = EditOptions { lenient, no_journal: lenient };
    edit::plan(&Device::open_export(root).unwrap(), edits, &opts, &Ctx::silent())
}

#[test]
fn cue_edits_are_made_with_cues_not_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let cues = Edit::Cues { tracks: vec![], change: CueChange::Delete(CueRef::All) };
    let err = plan(&root, &[cues], false).unwrap_err();
    let refused = "My Tag and cue edits are made with `tags` and `cues` (a replay makes them again)";
    assert_eq!(err.to_string(), Error::Invalid(refused.into()).to_string());
}

#[test]
fn history_lists_are_deleted_only_with_the_entries_they_were_recorded_with() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_history(&root, "HISTORY 001", &[1, 2]);
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let path = |id: u32| lib.track(id).unwrap().file_path.clone();
    let list = |tracks: Vec<String>| HistoryList { name: "HISTORY 001".into(), tracks };
    let other_entries = Edit::DeleteHistoryLists { lists: vec![list(vec![path(1)])] };

    let err = plan(&root, std::slice::from_ref(&other_entries), false).unwrap_err();
    assert_eq!(
        err.to_string(),
        Error::NotFound("history list \"HISTORY 001\" with those 1 entries".into()).to_string()
    );
    // Replaying, the edit is skipped with a warning.
    let replay = plan(&root, &[other_entries], true).unwrap();
    assert!(replay.is_noop());
    let skipped = "skipped an edit that no longer applies: not found: the history lists to delete";
    assert_eq!(replay.warnings, [skipped]);

    let exact = Edit::DeleteHistoryLists { lists: vec![list(vec![path(1), path(2)])] };
    common::apply(&root, &[exact]);
    assert!(Device::open_export(&root).unwrap().library().unwrap().history.is_empty());
}

#[test]
fn deleting_tracks_leaves_audio_outside_contents_on_the_drive() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let set = ed
        .add_track(&rekordbox_pdb::NewTrack { file_size: 5, ..rekordbox_pdb::NewTrack::new("Set", "/Music/Set.mp3") })
        .unwrap();
    ed.save(&pdb_path).unwrap();
    fs::create_dir_all(root.join("Music")).unwrap();
    fs::write(root.join("Music/Set.mp3"), b"audio").unwrap();

    let delete =
        Edit::DeleteTracks { tracks: TrackSelector { ids: vec![set], ..Default::default() }, keep_files: false };
    let p = common::apply(&root, &[delete]);
    let warning = "1 audio file(s) outside Contents/ are left on the drive (e.g. /Music/Set.mp3); locallibrary never \
                   deletes files there";
    assert!(p.warnings.iter().any(|w| w == warning), "{:?}", p.warnings);
    assert!(root.join("Music/Set.mp3").is_file());
    assert!(Device::open_export(&root).unwrap().library().unwrap().track(set).is_none());
}

#[test]
fn a_plan_that_doesnt_fit_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let mut p = plan(&root, &[Edit::CreatePlaylist { path: "New".into(), folder: false }], false).unwrap();
    p.fits = false;
    let before = common::Before::take(&root);
    let err = edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::NoSpace { .. }), "{err}");
    before.untouched(&root).unwrap();
}
