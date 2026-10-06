//! A plan is applied as it was confirmed: one whose drives changed since is refused with
//! `stale_plan`, and writes to archive and retired drives are refused unless allowed.

use crate::common;

use locallibrary::api::{self, Direction, EditRequest, SyncRequest};
use locallibrary::edit::{Edit, EditOptions};
use locallibrary::fleet::{self, Role};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Device, Error, Store, op, paths, sync};

fn delete_track(root: &std::path::Path, id: u32) -> EditRequest {
    EditRequest {
        drive: root.display().to_string(),
        edits: vec![Edit::DeleteTracks {
            tracks: TrackSelector { ids: vec![id], ..Default::default() },
            keep_files: false,
        }],
        options: EditOptions::default(),
    }
}

#[test]
fn a_confirmed_edit_is_refused_once_the_drive_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let planned = api::edit_plan(&delete_track(&root, 1), &Ctx::silent()).unwrap();
    assert_eq!(planned.operation, "edit");
    assert!(planned.destructive && !planned.noop);
    let json = serde_json::to_value(&planned).unwrap();
    assert_eq!(json["plan_id"], planned.id.as_str());
    assert_eq!(json["tracks_removed"].as_array().unwrap().len(), 1, "the plan's own fields are there too");

    // rekordbox exports again in between: the plan no longer says what applying would do.
    common::export_new_track(&root, "Artist", "Late addition");
    let before = common::Before::take(&root);
    let err = planned.apply(None, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
    assert_eq!(err.code(), "stale_plan");
    before.untouched(&root).unwrap();

    // Planned again, it applies.
    let planned = api::edit_plan(&delete_track(&root, 1), &Ctx::silent()).unwrap();
    planned.apply(None, &Ctx::silent()).unwrap();
    assert!(Device::open(&root).unwrap().library().unwrap().track(1).is_none());
}

#[test]
fn a_sync_copies_exactly_what_was_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let b = dir.path().join("B");
    std::fs::create_dir(&b).unwrap();
    let req = SyncRequest {
        left: a.display().to_string(),
        right: b.display().to_string(),
        direction: Direction::Auto,
        allow_low_confidence: false,
        verify: false,
        keep_extra: false,
        delete_first: false,
        overwrite_archive: false,
        keep_player_changes: false,
    };
    // The source exports again after the plan was made.
    let planned = api::sync_plan(&req, &Ctx::silent()).unwrap();
    common::export_new_track(&a, "Artist", "Late addition");
    let err = planned.apply(None, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
    assert!(!b.join(paths::EXPORT_PDB).exists(), "nothing was copied");

    // A file appears on the target that the sync would delete: the databases are the same, but
    // the plan under the lock isn't the one confirmed.
    let planned = api::sync_plan(&req, &Ctx::silent()).unwrap();
    let stray = b.join("Contents/stray.mp3");
    std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
    std::fs::write(&stray, b"not in the source").unwrap();
    let err = planned.apply(None, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
    assert!(stray.is_file());

    let planned = api::sync_plan(&req, &Ctx::silent()).unwrap();
    let result = planned.apply(None, &Ctx::silent()).unwrap();
    assert!(result.report.unwrap().copied > 0);
    assert!(!stray.exists());
}

#[test]
fn archive_drives_are_refused_unless_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    let root = common::bigger_drive(dir.path(), "Vault");
    let mut d = Device::open(&root).unwrap();
    d.adopt(Some("Vault")).unwrap();
    sync::register_drive(&store, &d, None).unwrap();
    fleet::set_role(&store, d.id().unwrap(), Some(Role::Archive)).unwrap();

    let err = api::edit_plan(&delete_track(&root, 1), &Ctx::silent()).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(m) if m.contains("archive")), "{err}");

    op::allow_archive_writes(true);
    let planned = api::edit_plan(&delete_track(&root, 1), &Ctx::silent());
    op::allow_archive_writes(false);
    // A plan made to write to an archive drive keeps that permission.
    planned.unwrap().apply(Some(&store), &Ctx::silent()).unwrap();
}

/// What a plan adds to its JSON doesn't hide the plan's own fields.
#[test]
fn a_plans_own_fields_survive_in_its_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let req = api::TagsEdit {
        drive: root.display().to_string(),
        action: api::TagAction::Add {
            tag: "Genre/Dark".into(),
            tracks: TrackSelector { ids: vec![1], ..Default::default() },
        },
    };
    let planned = api::tags_plan(&req).unwrap();
    let json = serde_json::to_value(&planned).unwrap();
    assert!(json["created"].as_array().is_some_and(|c| !c.is_empty()), "TagPlan's own `created`: {json}");
    assert!(json["planned_at"].is_string());
    for key in ["plan_id", "operation", "summary", "warnings", "destructive", "noop", "drives"] {
        assert!(!json[key].is_null(), "{key}: {json}");
    }
}
