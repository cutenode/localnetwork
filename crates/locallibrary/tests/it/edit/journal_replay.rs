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

/// Regression: the backup an undo takes didn't carry the journal, so redoing the undo (putting
/// that backup back) left the drive with the edit and the journal without it, and `edits replay`
/// wouldn't make it again. Restoring an older backup and then a newer one was the same. Backups
/// carry the journal, and putting one back puts it back as it was.
#[test]
fn a_redo_puts_the_undone_edits_back_on_the_journal() {
    use locallibrary::api::{UndoRequest, undo_plan};
    use locallibrary::backup;
    use locallibrary::edit::Edit;
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    for name in ["Friday", "Saturday"] {
        common::apply(&root, &[Edit::CreatePlaylist { path: name.into(), folder: false }]);
    }
    let changes =
        |d: &Device| -> Vec<Vec<String>> { edit::journal::list(d).unwrap().into_iter().map(|e| e.changes).collect() };
    let has = |d: &Device, name: &str| d.library().unwrap().find_playlist(name).is_some();
    let mut d = Device::open_export(&root).unwrap();
    let both = changes(&d);
    assert_eq!(both.len(), 2);

    // Undo Saturday, then redo it by putting back the backup the undo took.
    let undone = backup::restore(&mut d, None, false).unwrap();
    assert!(!has(&d, "Saturday"));
    assert_eq!(changes(&d), both[..1]);
    let redo = backup::list(&d).unwrap().into_iter().find(|b| b.undoes.as_deref() == Some(&undone.id)).unwrap();
    let req = UndoRequest { drive: Some(root.display().to_string()), backup: Some(redo.id.clone()), force: false };
    let plan = undo_plan(&req).unwrap();
    assert!(plan.summary.contains(&"1 recorded edit(s) go back on the journal".to_string()), "{:?}", plan.summary);
    assert!(plan.journal.is_empty(), "nothing comes off: {:?}", plan.journal);
    backup::restore(&mut d, Some(&redo.id), false).unwrap();
    assert!(has(&d, "Saturday"));
    assert_eq!(changes(&d), both, "the redone edit is back on the journal");

    // Restore the oldest backup (before Friday), then the one before Saturday: the journal
    // follows the drive both times.
    let backups = backup::list(&d).unwrap();
    let made: Vec<_> = backups.iter().filter(|b| b.undoes.is_none()).collect();
    let (before_saturday, before_friday) = (made[0].id.clone(), made[1].id.clone());
    backup::restore(&mut d, Some(&before_friday), false).unwrap();
    assert!(!has(&d, "Friday") && !has(&d, "Saturday"));
    assert!(changes(&d).is_empty());
    backup::restore(&mut d, Some(&before_saturday), false).unwrap();
    assert!(has(&d, "Friday") && !has(&d, "Saturday"));
    assert_eq!(changes(&d), both[..1], "Friday is back on the journal");
    let edits = edit::journal::edits(&d).unwrap();
    let replay = edit::plan(&d, &edits, &EditOptions { lenient: true, no_journal: true }, &Ctx::silent()).unwrap();
    assert!(replay.is_noop(), "the drive has every recorded edit: {:?}", replay.changes);
}

/// Regression: the backup an undo takes didn't carry the player name and colour the drive
/// remembers, so redoing an undone `drive player` left the drive remembering none, and `refresh`
/// wouldn't put the name back after rekordbox's next export.
#[test]
fn a_redo_remembers_the_player_name_again() {
    use locallibrary::{backup, player};
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut d = Device::open(&root).unwrap();
    d.adopt(None).unwrap();
    player::set(&mut d, Some("MAIN"), Some(3)).unwrap();
    let remembered = |d: &Device| Device::open(d.root()).unwrap().state().unwrap().player.clone();
    let set = remembered(&d).unwrap();

    let undone = backup::restore(&mut d, None, false).unwrap();
    assert_eq!(remembered(&d), None, "undone, the drive remembers no name");
    let redo = backup::list(&d).unwrap().into_iter().find(|b| b.undoes.as_deref() == Some(&undone.id)).unwrap();
    backup::restore(&mut d, Some(&redo.id), false).unwrap();
    assert_eq!(player::db_identity(&root).unwrap().name.as_deref(), Some("MAIN"));
    assert_eq!(remembered(&d), Some(set), "redone, it remembers MAIN again");
}

/// Regression: a replay cut off part way (the app killed) left its backup without what its change
/// left, so once the replay was run again to finish it, taking it all back needed `undo --force`.
/// The backup notes what each step left: two undos (the second run's, then the first's) put the
/// drive back as rekordbox exported it, neither forced.
#[test]
fn a_replay_cut_off_part_way_can_be_undone_without_forcing() {
    use locallibrary::backup;
    use locallibrary::progress::{CancelToken, Event, Phase};
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let exported = common::drive_files(&root);
    // A playlist edit first, which a replay makes after the cue edits: the drive cut off after
    // the first of those is like no state an edit left.
    common::apply(&root, &[locallibrary::edit::Edit::CreatePlaylist { path: "Friday".into(), folder: false }]);
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    for track in &lib.tracks[..2] {
        let set = CueEdit::Set(SetCue {
            hot_cue: Some(HotCue(1)),
            at: CuePosition::Beat(9),
            snap: false,
            loop_beats: None,
            name: None,
            color: None,
        });
        cue_edit::apply(&cue_edit::plan(&d, &[track.id], &set, &Ctx::silent()).unwrap(), &mut d, &Ctx::silent())
            .unwrap();
    }
    // rekordbox exports again, without the cues.
    for (key, bytes) in &exported {
        std::fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    assert!(common::changed_files(&exported, &common::drive_files(&root)).is_empty());

    // The replay is killed as its second step starts copying, its first step made.
    let killed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let kill = |e: &Event| {
            let started = matches!(e, Event::PhaseStart { phase: Phase::Copy, .. });
            if started && !common::changed_files(&exported, &common::drive_files(&root)).is_empty() {
                panic!("killed");
            }
        };
        let ctx = Ctx::new(&kill, CancelToken::new());
        let mut d = Device::open_export(&root).unwrap();
        let edits = edit::journal::edits(&d).unwrap();
        let plan = edit::plan(&d, &edits, &EditOptions { lenient: true, no_journal: true }, &ctx).unwrap();
        edit::apply(&plan, &mut d, &ctx)
    }));
    assert!(killed.is_err(), "the replay was killed");
    let cut_off = common::drive_files(&root);
    assert!(!common::changed_files(&exported, &cut_off).is_empty(), "the first step was made");

    common::replay(&root);
    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, None, false).unwrap();
    assert!(common::changed_files(&cut_off, &common::drive_files(&root)).is_empty(), "back to where it was cut off");
    let mut d = Device::open_export(&root).unwrap();
    let undone = backup::restore(&mut d, None, false).unwrap_or_else(|e| panic!("undone unforced: {e}"));
    assert_eq!(undone.reason, "before replaying edits");
    assert!(common::changed_files(&exported, &common::drive_files(&root)).is_empty(), "back to the export");
}
