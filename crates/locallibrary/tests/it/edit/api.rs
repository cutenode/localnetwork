//! Editing a drive through the service layer: edits applied, listed and made again after an
//! export replaced them; tracklists refused; history lists deleted once they're saved; and
//! rule-based playlists kept and written.

use crate::common::{self, path};

use std::path::Path;

use locallibrary::api::{self, DriveRef};
use locallibrary::device::Device;
use locallibrary::edit::Edit;
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::{Error, paths};

fn playlist_names(root: &Path) -> Vec<String> {
    Device::open(root).unwrap().library().unwrap().playlists.into_iter().map(|p| p.name).collect()
}

/// An edit is recorded in the drive's journal; when an export puts the old database back, the
/// recorded edits are made again.
#[test]
fn edits_are_recorded_and_made_again_after_an_export() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let drive = DriveRef { drive: path(&root) };
    let exported = std::fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    let req = api::EditRequest {
        drive: path(&root),
        edits: vec![Edit::CreatePlaylist { path: "Friday".into(), folder: false }],
        options: Default::default(),
    };
    assert_eq!(api::edit_apply(&req, &Ctx::silent()).unwrap().changes, 1);
    let journal = api::edits_list(&drive).unwrap();
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].edits.len(), 1);

    std::fs::write(root.join(paths::EXPORT_PDB), exported).unwrap();
    assert!(!playlist_names(&root).contains(&"Friday".to_string()));
    api::edits_replay_apply(&drive, &Ctx::silent()).unwrap();
    assert!(playlist_names(&root).contains(&"Friday".to_string()));
}

/// History lists are deleted only once the store holds them (or when forced); a name the drive
/// doesn't have is refused.
#[test]
fn history_lists_are_deleted_once_saved() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let ids: Vec<u32> = Device::open(&root).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect();
    common::add_history(&root, "HISTORY 001", &ids);
    let delete = |names: &[&str], force| api::DeleteHistoryRequest {
        drive: path(&root),
        names: names.iter().map(|n| n.to_string()).collect(),
        force,
    };

    let err = api::delete_history_plan(None, &delete(&["HISTORY 999"], false), &Ctx::silent()).expect_err("refused");
    assert!(matches!(err, Error::NotFound(_)), "{err}");
    let err = api::delete_history_plan(Some(&store), &delete(&[], false), &Ctx::silent()).expect_err("refused");
    assert_eq!(err.reason(), Some(Reason::NotSaved), "{err}");
    api::pull(&store, &api::PullRequest { drive: path(&root), ..Default::default() }, &Ctx::silent()).unwrap();
    let plan = api::delete_history_plan(Some(&store), &delete(&["history 001"], false), &Ctx::silent()).unwrap();
    assert!(!plan.noop);
    api::delete_history_apply(Some(&store), &delete(&["history 001"], false), &Ctx::silent()).unwrap();
    assert!(Device::open(&root).unwrap().library().unwrap().history.is_empty());
}

/// Rule-based playlists are kept in the store (for some drives, or all), written to a drive they
/// apply to, and removed; a drive none applies to is told so.
#[test]
fn rule_based_playlists_are_kept_written_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
    let main = common::demo_drive(dir.path(), "Main");
    let other = common::demo_drive(dir.path(), "Other");
    let rule = api::SetSmartPlaylist {
        path: "/Everything/".into(),
        filter: "bpm:>=1".into(),
        sort: None,
        limit: None,
        drives: vec![path(&main)],
    };
    let rules = api::smart_set(&store, &rule).unwrap();
    assert_eq!(rules.playlists[0].path, "Everything");
    assert_eq!(api::smart_list(&store).unwrap().playlists.len(), 1);

    let refused = api::smart_apply_plan(&store, &DriveRef { drive: path(&other) }, &Ctx::silent()).err();
    assert!(matches!(refused, Some(Error::NotFound(_))));
    api::smart_apply(&store, &DriveRef { drive: path(&main) }, &Ctx::silent()).unwrap();
    assert!(playlist_names(&main).contains(&"Everything".to_string()));
    let left = api::smart_remove(&store, &api::RemoveSmartPlaylist { path: "everything".into() }).unwrap();
    assert!(left.playlists.is_empty());
}
