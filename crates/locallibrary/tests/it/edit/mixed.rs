//! One plan mixing playlist, track, My Tag and cue edits: one backup, one journal entry, one undo,
//! and a replay that makes all of it again.

use crate::common;

use std::path::Path;

use locallibrary::cue_edit::{self, CueChange, CuePosition, CueRef, HotCue, SetCue, UpdateCue};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, TrackFields};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::tags::{self, TagChange};
use locallibrary::{backup, paths};

/// What the edits change on the first track: its My Tags, hot cues, the playlist and its rating.
fn state(root: &Path, file: &str) -> (Vec<String>, Vec<Option<String>>, Option<usize>, u8) {
    let d = Device::open_export(root).unwrap();
    let lib = d.library().unwrap();
    let t = lib.tracks.iter().find(|t| t.file_path == file).unwrap();
    let tags = tags::by_track(&d).unwrap().remove(&paths::key(file).to_lowercase()).unwrap_or_default();
    let cues = cue_edit::list(&d, &[t.id]).unwrap().remove(0).cues.into_iter().map(|c| c.hot_cue).collect();
    (tags, cues, lib.find_playlist("Mixed").map(|p| p.track_ids.len()), t.rating)
}

#[test]
fn a_plan_mixing_my_tag_cue_playlist_and_track_edits_is_one_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let first = d.library().unwrap().tracks[0].file_path.clone();
    let exported = common::library_files(&root);
    let before = state(&root, &first);
    let one = || TrackSelector::paths([first.clone()]);
    let hot_a = SetCue {
        hot_cue: Some(HotCue(0)),
        at: CuePosition::Beat(1),
        snap: false,
        loop_beats: None,
        name: None,
        color: None,
    };
    let edits = [
        Edit::CreatePlaylist { path: "Mixed".into(), folder: false },
        Edit::AddTracks { playlist: "Mixed".into(), tracks: one(), position: None, sort: None, allow_repeats: false },
        Edit::MyTag(TagChange::Add { tag: "Genre/Deep".into(), tracks: vec![first.clone()] }),
        Edit::Cues { tracks: vec![first.clone()], change: CueChange::Set(hot_a) },
        Edit::EditTracks { tracks: one(), fields: TrackFields { rating: Some(5), ..Default::default() } },
    ];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    // The My Tag and cue edits come first, as they're made.
    assert_eq!(
        plan.changes,
        [
            "add My Tag Genre/Deep to 1 track(s)",
            "set hot cue A on 1 track(s)",
            "create playlist Mixed",
            "add 1 track(s) to Mixed",
            "set rating 5 on 1 track(s)",
        ]
    );
    assert!(matches!(&plan.recorded[..2], [Edit::MyTag(_), Edit::Cues { .. }]), "{:?}", plan.recorded);
    let backups = backup::list(&d).unwrap().len();
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let edited = state(&root, &first);
    assert_eq!(edited, (vec!["Genre/Deep".to_string()], vec![Some("A".to_string())], Some(1), 5));
    let d = Device::open_export(&root).unwrap();
    assert_eq!(backup::list(&d).unwrap().len(), backups + 1, "one backup");
    let journal = edit::journal::list(&d).unwrap();
    assert_eq!(journal.len(), 1, "one journal entry");
    assert_eq!((journal[0].edits.clone(), journal[0].backup.clone()), (plan.recorded.clone(), report.backup));

    // One undo takes all of it back, and off the journal.
    backup::restore(&mut Device::open_export(&root).unwrap(), None, false).unwrap();
    assert_eq!(state(&root, &first), before);
    assert!(edit::journal::list(&Device::open_export(&root).unwrap()).unwrap().is_empty());

    // Made again, then rekordbox exports: a replay makes all of it again.
    common::apply(&root, &edits);
    for (key, bytes) in &exported {
        std::fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    assert_eq!(state(&root, &first), before);
    common::replay(&root);
    assert_eq!(state(&root, &first), edited);
}

/// Planning the edits as given, a cue edit that can't be made stops the plan before the edits
/// after it, saying how far it got. Having written nothing, it leaves no backup; once it has,
/// undoing takes that back.
#[test]
fn a_mixed_edit_that_fails_as_it_is_made_stops_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let first = d.library().unwrap().tracks[0].file_path.clone();
    let before = common::Before::take(&root);
    // The track has no hot cue H to rename.
    let rename = UpdateCue { cue: CueRef::Hot(HotCue(7)), name: Some("Drop".into()), color: None };
    let fails = Edit::Cues { tracks: vec![first.clone()], change: CueChange::Update(rename) };
    let after = Edit::CreatePlaylist { path: "After".into(), folder: false };
    let plan = edit::plan(&d, &[fails.clone(), after.clone()], &EditOptions::default(), &Ctx::silent()).unwrap();
    let err = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap_err().to_string();
    let nothing = err.starts_with("the plan stopped after 0 of its edits: ") && err.ends_with("; it changed nothing");
    assert!(nothing, "{err}");
    before.untouched(&root).unwrap();

    // The tag it made is recorded, so a replay after rekordbox exports makes it again.
    let tag = Edit::MyTag(TagChange::Add { tag: "Genre/Deep".into(), tracks: vec![first.clone()] });
    let plan = edit::plan(&d, &[tag.clone(), fails, after], &EditOptions::default(), &Ctx::silent()).unwrap();
    let err = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().starts_with("the plan stopped after 1 of its edits: "), "{err}");
    assert_eq!(state(&root, &first).0, ["Genre/Deep"]);
    let journal = edit::journal::list(&d).unwrap();
    assert_eq!(journal.len(), 1);
    assert_eq!((journal[0].edits.clone(), journal[0].changes.clone()), (vec![tag], vec![plan.changes[0].clone()]));
    backup::restore(&mut Device::open_export(&root).unwrap(), None, false).unwrap();
    assert_eq!(common::changed_files(&before.files, &common::drive_files(&root)), Vec::<String>::new());
}

/// A write that fails once it changed the drive (here just after its swap, as a full drive can
/// while it deletes or records what it did) keeps the backup made before the plan, and says so:
/// undoing puts the drive back.
#[test]
fn a_mixed_edit_that_fails_after_changing_the_drive_keeps_its_backup() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let first = d.library().unwrap().tracks[0].file_path.clone();
    let before = common::Before::take(&root);
    let tag = Edit::MyTag(TagChange::Add { tag: "Genre/Deep".into(), tracks: vec![first.clone()] });
    let plan = edit::plan(&d, &[tag], &EditOptions::default(), &Ctx::silent()).unwrap();
    locallibrary::testing::fail_after_swap(true);
    let err = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap_err().to_string();
    locallibrary::testing::fail_after_swap(false);
    assert!(
        err.starts_with("the plan stopped after 0 of its edits: ") && err.ends_with("Undoing takes back what it made"),
        "{err}"
    );
    assert_eq!(state(&root, &first).0, ["Genre/Deep"]);
    let backups = backup::list(&Device::open_export(&root).unwrap()).unwrap();
    assert_eq!(backups.len(), 1, "{backups:?}");
    backup::restore(&mut Device::open_export(&root).unwrap(), None, false).unwrap();
    assert_eq!(common::changed_files(&before.files, &common::drive_files(&root)), Vec::<String>::new());
    assert!(state(&root, &first).0.is_empty());
}
