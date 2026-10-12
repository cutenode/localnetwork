//! Undoing an edit after the drive changed again: what the plan says changes, what's lost, and
//! the library it puts back.

use crate::common::{self, path};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, UndoRequest};
use locallibrary::edit::Edit;
use locallibrary::paths;
use locallibrary::progress::Ctx;

/// The demo export, edited (a playlist made, which keeps a backup), then maybe exported to again,
/// played on (a history list) and given a OneLibrary database: undoing the edit, forced as the
/// drive changed since, puts export.pdb back, removes the database the backup didn't have, and
/// warns of the history list it drops; the library is then as it was before the edit.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(exported = true, played = true, onelibrary = true)]
fn undoing_puts_back_the_backup_and_says_what_goes(tc: TestCase) {
    let exported = tc.draw(gs::booleans());
    let played = tc.draw(gs::booleans());
    let onelibrary = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let volumes = common::isolate(dir.path(), None);
    let root = common::demo_drive(&volumes, "Main");
    let before = common::library(&root);
    common::apply(&root, &[Edit::CreatePlaylist { path: "Friday".into(), folder: false }]);
    if exported {
        common::export_new_track(&root, "Artist", "Since");
    }
    if played {
        let ids: Vec<u32> = common::library(&root).tracks.iter().map(|t| t.id).collect();
        common::add_history(&root, "HISTORY 001", &ids[..1]);
    }
    if onelibrary {
        common::add_onelibrary(&root);
    }
    let req = UndoRequest { drive: Some(path(&root)), backup: None, force: true };
    let plan = api::undo_plan(&req).unwrap();
    let change = |key: &str| plan.plan.files.iter().find(|f| f.path == key).map(|f| f.change);
    assert_eq!(change(paths::EXPORT_PDB), Some("replaced"));
    assert_eq!(change(paths::LIBRARY_PLUS_DB), onelibrary.then_some("removed"));
    let dropped = "1 history list(s) players recorded since are dropped: HISTORY 001";
    assert_eq!(plan.warnings.iter().any(|w| w == dropped), played, "{:?}", plan.warnings);
    assert_eq!(plan.plan.history_lists.len(), usize::from(played));
    plan.apply(None, &Ctx::silent()).unwrap();
    let after = common::library(&root);
    assert_eq!((after.tracks, after.playlists), (before.tracks, before.playlists));
    assert!(!root.join(paths::LIBRARY_PLUS_DB).exists(), "only what the backup had");
}
