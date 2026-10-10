//! Cue edits refused when planning or applying, and the warnings a plan gives for tracks only one
//! database or analysis file can hold cues for.

use crate::common;

use std::path::{Path, PathBuf};

use locallibrary::cue_edit::{self, CueEdit, CuePlan, CuePosition, HotCue, SetCue};
use locallibrary::progress::Ctx;
use locallibrary::{Device, Error, paths};

fn set(hot: Option<u8>, at: CuePosition) -> SetCue {
    SetCue { hot_cue: hot.map(HotCue), at, snap: false, loop_beats: None, name: None, color: None }
}

fn plan(root: &Path, ids: &[u32], edit: &CueEdit) -> locallibrary::Result<CuePlan> {
    cue_edit::plan(&Device::open(root).unwrap(), ids, edit, &Ctx::silent())
}

fn invalid(r: locallibrary::Result<CuePlan>) -> String {
    match r {
        Err(Error::Invalid(m)) => m,
        other => panic!("not refused as invalid: {other:?}"),
    }
}

/// The demo drive with a third track whose analysis file has no beat grid, at `tempo` (BPM x 100).
fn with_gridless_track(dir: &Path, tempo: u32) -> (PathBuf, u32) {
    let root = common::demo_drive(dir, "USB");
    let id = common::export_new_track(&root, "No", "Grid");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    ed.set_track_field(id, "tempo", tempo).unwrap();
    ed.save(&pdb_path).unwrap();
    (root, id)
}

#[test]
fn an_edit_needs_an_export_and_the_tracks_it_takes() {
    let dir = tempfile::tempdir().unwrap();
    let edit = CueEdit::Set(set(Some(0), CuePosition::Ms(0)));
    let err = plan(dir.path(), &[1], &edit).unwrap_err();
    assert!(matches!(err, Error::NotAnExport(_)), "{err}");
    let root = common::demo_drive(dir.path(), "USB");
    assert_eq!(invalid(plan(&root, &[], &edit)), "no tracks selected");
    assert_eq!(invalid(plan(&root, &[1, 2], &edit)), "2 tracks selected; pick one");
    assert_eq!(invalid(plan(&root, &[1], &CueEdit::Set(set(Some(8), CuePosition::Ms(0))))), "hot cues are A-H");
}

#[test]
fn a_track_needs_safe_analysis_files_and_a_grid_or_tempo() {
    let dir = tempfile::tempdir().unwrap();
    let (root, gridless) = with_gridless_track(dir.path(), 0);
    let snapped = CueEdit::Set(SetCue { snap: true, ..set(None, CuePosition::Ms(1000)) });
    assert_eq!(invalid(plan(&root, &[gridless], &snapped)), "the track has no beat grid; give a time instead");
    let looped = CueEdit::Set(SetCue { loop_beats: Some(4.0), ..set(None, CuePosition::Ms(1000)) });
    assert_eq!(
        invalid(plan(&root, &[gridless], &looped)),
        "the track's tempo isn't known, so a loop can't be sized in beats"
    );

    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let unanalysed = ed.add_track(&rekordbox_pdb::NewTrack::new("Raw", "/Contents/raw.mp3")).unwrap();
    let elsewhere = ed
        .add_track(&rekordbox_pdb::NewTrack {
            analyze_path: "/Contents/odd.DAT".into(),
            ..rekordbox_pdb::NewTrack::new("Odd", "/Contents/odd.mp3")
        })
        .unwrap();
    ed.save(&pdb_path).unwrap();
    let edit = CueEdit::Set(set(Some(0), CuePosition::Ms(0)));
    assert!(
        invalid(plan(&root, &[unanalysed], &edit))
            .ends_with("has no analysis files (rekordbox hasn't analysed it), so it can't hold cues")
    );
    assert!(invalid(plan(&root, &[elsewhere], &edit)).starts_with("refusing analysis path \"/Contents/odd.DAT\""));
}

#[test]
fn a_track_only_export_pdb_lists_is_warned_about() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    common::add_onelibrary(&root);
    let id = common::export_new_track(&root, "Only", "In export.pdb");
    let p = plan(&root, &[id], &CueEdit::Set(set(Some(0), CuePosition::Ms(500)))).unwrap();
    let name = Device::open(&root).unwrap().library().unwrap().track(id).unwrap().display_name();
    let warning = format!("{name} isn't in exportLibrary.db; players that read OneLibrary won't see its new cues");
    assert!(p.warnings.contains(&warning), "{:?}", p.warnings);
    assert!(!p.tracks[0].onelibrary);
}

#[test]
fn copying_cues_leaves_the_track_they_come_from_and_a_plan_that_doesnt_fit_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let cue = plan(&root, &[1], &CueEdit::Set(set(Some(0), CuePosition::Beat(1)))).unwrap();
    cue_edit::apply(&cue, &mut Device::open(&root).unwrap(), &Ctx::silent()).unwrap();
    let p = plan(&root, &[1, 2], &CueEdit::Copy { from: 1 }).unwrap();
    assert_eq!(p.tracks.iter().map(|t| t.track.id).collect::<Vec<_>>(), [2]);

    let before = common::Before::take(&root);
    let mut full = plan(&root, &[2], &CueEdit::Set(set(Some(0), CuePosition::Beat(1)))).unwrap();
    full.fits = false;
    let err = cue_edit::apply(&full, &mut Device::open(&root).unwrap(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::NoSpace { .. }), "{err}");
    before.untouched(&root).unwrap();
}
