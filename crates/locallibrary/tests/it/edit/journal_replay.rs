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
        "1 track(s) aren't on the drive any more (e.g. {}), so \"set hot cue A on 1 track(s)\" is replayed without them",
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
        "1 track(s) aren't on the drive any more (e.g. {}), so \"{what}\" is replayed without them",
        first.file_path
    );
    let skipped = format!("skipped an edit that no longer applies ({what}): ");
    assert!(report.warnings.contains(&gone), "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.starts_with(&skipped)), "{:?}", report.warnings);
}

/// rekordbox can export over one database and leave the other: replaying makes the edits again in
/// the one that lost them, though the other still has them, and isn't taken for a plan that
/// changes nothing.
#[test]
fn a_replay_makes_edits_again_in_the_one_database_that_lost_them() {
    use locallibrary::edit::Edit;
    use locallibrary::onelibrary::OneLibrary;
    use locallibrary::pdb;
    use locallibrary::query::TrackSelector;
    use std::path::Path;
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let lenient = EditOptions { lenient: true, no_journal: true };
    for lost in [paths::LIBRARY_PLUS_DB, paths::EXPORT_PDB] {
        let before = std::fs::read(root.join(lost)).unwrap();
        let ids = TrackSelector { ids: vec![1, 2], ..Default::default() };
        let name = format!("Set {}", lost.len());
        let set = Edit::SetTracks { playlist: name.clone(), tracks: ids, sort: None, limit: None, create: true };
        common::apply(&root, &[set]);
        std::fs::write(root.join(lost), &before).unwrap();
        let has = |root: &Path| {
            let pdb = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
            let plus = OneLibrary::open_root(root).unwrap().library().unwrap();
            (pdb.find_playlist(&name).is_some(), plus.find_playlist(&name).is_some())
        };
        assert_ne!(has(&root), (true, true), "{lost} lost it");

        let mut d = Device::open_export(&root).unwrap();
        let plan = edit::plan(&d, &edit::journal::edits(&d).unwrap(), &lenient, &Ctx::silent()).unwrap();
        assert!(!plan.is_noop(), "{lost}: {plan:?}");
        let said = plan.changes.iter().any(|c| c.ends_with("exportLibrary.db, which lacks them"));
        assert_eq!(said, lost == paths::LIBRARY_PLUS_DB, "{:?}", plan.changes);
        edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
        assert_eq!(has(&root), (true, true), "{lost}");
        let d = Device::open_export(&root).unwrap();
        let again = edit::plan(&d, &edit::journal::edits(&d).unwrap(), &lenient, &Ctx::silent()).unwrap();
        assert!(again.is_noop(), "{lost}: {:?}", again.changes);
    }
}
