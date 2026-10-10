//! Replaying recorded edits on a drive rekordbox exported again without some of their tracks:
//! what no longer applies is skipped with a warning, and the rest is made.

use crate::common;

use locallibrary::cue_edit::{self, CueEdit, CuePosition, HotCue, SetCue};
use locallibrary::device::Device;
use locallibrary::edit::{self, EditOptions};
use locallibrary::paths;
use locallibrary::progress::Ctx;

#[test]
fn cue_edits_on_tracks_that_are_gone_are_skipped_with_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let (first, second) = (lib.tracks[0].clone(), lib.tracks[1].clone());
    let set = CueEdit::Set(SetCue {
        hot_cue: Some(HotCue(0)),
        at: CuePosition::Beat(1),
        snap: false,
        loop_beats: None,
        name: None,
        color: None,
    });
    for (ids, edit) in [(vec![first.id], set), (vec![second.id], CueEdit::Copy { from: first.id })] {
        cue_edit::apply(&cue_edit::plan(&d, &ids, &edit, &Ctx::silent()).unwrap(), &mut d, &Ctx::silent()).unwrap();
    }
    // rekordbox exports the drive again, without the first track.
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    ed.remove_tracks(&[first.id]).unwrap();
    ed.save(&pdb_path).unwrap();

    let mut d = Device::open_export(&root).unwrap();
    let edits = edit::journal::edits(&d).unwrap();
    assert_eq!(edits.len(), 2);
    let opts = EditOptions { lenient: true, no_journal: true };
    let plan = edit::plan(&d, &edits, &opts, &Ctx::silent()).unwrap();
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let gone = format!(
        "1 track(s) the cue edit names aren't on the drive any more (e.g. {}); the rest are replayed",
        first.file_path
    );
    let skipped = |what: &str| format!("skipped an edit that no longer applies ({what}): ");
    for want in [
        gone,
        skipped("set hot cue A on 1 track(s)"),
        skipped(&format!("copy the cues of {} to 1 track(s)", first.file_path)),
    ] {
        assert!(report.warnings.iter().any(|w| w.starts_with(&want)), "{want}: {:?}", report.warnings);
    }
}

#[test]
fn my_tag_edits_on_tracks_that_are_gone_are_skipped_with_warnings() {
    use locallibrary::tags;
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let first = d.library().unwrap().tracks[0].clone();
    let add = tags::Edit::Add { tag: "Genre/Deep".into(), tracks: vec![first.id] };
    tags::apply(&tags::plan(&d, &add).unwrap(), &mut d, &Ctx::silent()).unwrap();
    // rekordbox exports the drive again, without the track.
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    ed.remove_tracks(&[first.id]).unwrap();
    ed.save(&pdb_path).unwrap();

    let mut d = Device::open_export(&root).unwrap();
    let edits = edit::journal::edits(&d).unwrap();
    let opts = EditOptions { lenient: true, no_journal: true };
    let report = edit::apply(&edit::plan(&d, &edits, &opts, &Ctx::silent()).unwrap(), &mut d, &Ctx::silent()).unwrap();
    let what = "add My Tag Genre/Deep to 1 track(s)";
    let gone = format!(
        "1 track(s) {what} names aren't on the drive any more (e.g. {}); the rest are replayed",
        first.file_path
    );
    let skipped = format!("skipped an edit that no longer applies ({what}): ");
    assert!(report.warnings.contains(&gone), "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.starts_with(&skipped)), "{:?}", report.warnings);
}
