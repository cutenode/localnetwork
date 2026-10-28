//! Edits refused when planning or applying, and history lists deleted as the journal names them.

use crate::common;

use std::fs;

use locallibrary::cue_edit::{CueChange, CueRef};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, EditPlan, HistoryList};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::tags::TagChange;
use locallibrary::{Error, paths};

fn plan(root: &std::path::Path, edits: &[Edit], lenient: bool) -> locallibrary::Result<EditPlan> {
    let opts = EditOptions { lenient, no_journal: lenient };
    edit::plan(&Device::open_export(root).unwrap(), edits, &opts, &Ctx::silent())
}

/// A My Tag or cue edit naming a file no track has is refused when planned; a replay plans it, to
/// make it without that track.
#[test]
fn my_tag_and_cue_edits_naming_a_file_no_track_has_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let gone = "/Contents/Gone/gone.mp3".to_string();
    let first = Device::open_export(&root).unwrap().library().unwrap().tracks[0].file_path.clone();
    let edits = [
        Edit::Cues { tracks: vec![gone.clone()], change: CueChange::Delete(CueRef::All) },
        Edit::Cues { tracks: vec![first], change: CueChange::Copy { from: gone.clone() } },
        Edit::MyTag(TagChange::Add { tag: "Genre/Deep".into(), tracks: vec![gone.clone()] }),
    ];
    for e in edits {
        let err = plan(&root, std::slice::from_ref(&e), false).unwrap_err();
        assert_eq!(err.to_string(), Error::NotFound(format!("no track at {gone}")).to_string());
        assert!(!plan(&root, &[e], true).unwrap().is_noop());
    }
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

/// A track goes with every row of its audio file, so a track replacing another of its own file
/// is refused, and deleting a playlist with its tracks leaves one whose file a track kept
/// elsewhere shares.
#[test]
fn rows_sharing_a_file_go_together_or_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let file = lib.tracks[0].file_path.clone();
    let twin = common::add_track(&root, rekordbox_pdb::NewTrack::new("Twin", file.clone()), b"audio");
    let ids = |id: u32| TrackSelector { ids: vec![id], ..Default::default() };
    let replace = Edit::ReplaceTrack { duplicate: ids(twin), keep: ids(lib.tracks[0].id), keep_duplicate: false };
    let err = plan(&root, &[replace], false).unwrap_err();
    assert_eq!(err.to_string(), "a track can't replace itself, or another of its own file");

    let mut d = Device::open_export(&root).unwrap();
    let edits = [
        Edit::CreatePlaylist { path: "Twins".into(), folder: false },
        Edit::AddTracks {
            playlist: "Twins".into(),
            tracks: ids(twin),
            position: None,
            sort: None,
            allow_repeats: false,
        },
    ];
    edit::apply(&plan(&root, &edits, false).unwrap(), &mut d, &Ctx::silent()).unwrap();
    let delete = Edit::DeletePlaylist { playlist: "Twins".into(), with_tracks: true };
    let p = plan(&root, &[delete], false).unwrap();
    assert!(p.tracks_removed.is_empty() && p.files.is_empty(), "{:?} {:?}", p.tracks_removed, p.files);
    edit::apply(&p, &mut d, &Ctx::silent()).unwrap();
    let titles: Vec<String> = d.library().unwrap().tracks.into_iter().map(|t| t.title).collect();
    assert!(titles.contains(&"Twin".to_string()) && titles.len() == 13, "{titles:?}");
    assert!(root.join(paths::key(&file)).is_file());
}
