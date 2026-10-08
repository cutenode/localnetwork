//! Backups undo won't put back, and playlists a restore can't find, and what each says about why.

use crate::common::{self, apply, path};

use std::fs;
use std::path::Path;

use locallibrary::api::{self, PlaylistRestoreRequest, UndoRequest};
use locallibrary::backup;
use locallibrary::device::Device;
use locallibrary::edit::Edit;
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, SyncOptions};
use locallibrary::{Error, paths};

fn create(name: &str) -> Edit {
    Edit::CreatePlaylist { path: name.into(), folder: false }
}

#[test]
fn a_drive_without_backups_has_nothing_to_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    let err = backup::restore(&mut d, None, false).unwrap_err();
    assert_eq!(err.to_string(), Error::NotFound("anything to undo on Main: it has no backups".into()).to_string());
}

#[test]
fn a_named_backup_is_refused_once_the_drive_changed_since() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    apply(&root, &[create("First")]);
    let mut d = Device::open(&root).unwrap();
    let id = backup::list(&d).unwrap().remove(0).id;
    common::export_new_track(&root, "New", "Track");

    let err = backup::restore(&mut d, Some(&id), false).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::ChangedSince), "{err}");
    assert!(err.to_string().contains("perhaps by a rekordbox export, a sync or players recording history"), "{err}");
}

#[test]
fn a_drive_a_whole_library_was_synced_onto_names_the_sync() {
    let dir = tempfile::tempdir().unwrap();
    // Both databases, so the backup holds three files the sync replaces.
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    apply(&root, &[create("First")]);
    let other = common::bigger_drive(dir.path(), "Other");
    common::add_onelibrary(&other);
    let mut other = Device::open_export(other).unwrap();
    let mut d = Device::open(&root).unwrap();
    sync::sync_drive_to_drive(&mut other, &mut d, &SyncOptions::default(), &Ctx::silent()).unwrap();

    let err = backup::restore(&mut d, None, false).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::ChangedSince), "{err}");
    let message = err.to_string();
    assert!(message.contains(" more file(s) changed, after a sync on "), "{message}");
}

/// A backup listing a track whose audio is gone from the drive is refused, naming the track and
/// offering to force it; forced, the plan says what has no audio, and it's put back.
#[test]
fn a_backup_whose_audio_is_gone_is_put_back_only_when_forced() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    apply(&root, &[create("First")]);
    let mut d = Device::open(&root).unwrap();
    let gone = d.library().unwrap().tracks[0].clone();
    fs::remove_file(paths::to_fs(&root, &paths::key(&gone.file_path))).unwrap();
    let undo = |force| api::undo_plan(&UndoRequest { drive: Some(path(&root)), backup: None, force });

    let err = backup::restore(&mut d, None, false).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::MissingAudio), "{err}");
    assert!(err.to_string().starts_with("1 track(s) in backup "), "{err}");
    assert!(err.to_string().contains(&format!("(e.g. {})", gone.display_name())), "{err}");
    let err = undo(false).expect_err("refused");
    assert_eq!(err.detail().fields["missing"], serde_json::json!([gone.display_name()]));

    let forced = undo(true).unwrap();
    assert_eq!(forced.missing_audio, [gone.display_name()]);
    let warned = forced.warnings.iter().any(|w| w.starts_with("1 track(s) in the backup have no audio"));
    assert!(warned, "{:?}", forced.warnings);
    backup::restore(&mut d, None, true).unwrap();
    assert!(d.library().unwrap().find_playlist("First").is_none());
}

fn restore(drive: &Path, playlist: &str) -> PlaylistRestoreRequest {
    PlaylistRestoreRequest { drive: path(drive), playlist: playlist.into(), snapshot: None, backup: None }
}

/// A restore must name what to restore and at most one source, for a drive the store can place;
/// a playlist no snapshot of its library has is refused, offering the drive's backups.
#[test]
fn a_playlist_restore_must_say_what_and_from_where() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let main = common::demo_drive(dir.path(), "Main");
    let plan = |req: &PlaylistRestoreRequest| {
        api::playlist_restore_plan(Some(&store), req, &Ctx::silent()).expect_err("refused")
    };

    assert!(plan(&restore(&main, " / ")).to_string().contains("give the playlist or folder"));
    let both = PlaylistRestoreRequest {
        snapshot: Some("abcd".into()),
        backup: Some("efgh".into()),
        ..restore(&main, "Friday")
    };
    assert!(plan(&both).to_string().contains("not both"));
    assert!(plan(&restore(&main, "Friday")).to_string().contains("isn't linked to a library in the store"));

    api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &Ctx::silent()).unwrap();
    let err = plan(&restore(&main, "Friday"));
    assert!(matches!(err.inner(), Error::NotFound(m) if m.contains("in any snapshot of \"Main\"")), "{err}");
    let offered: Vec<Option<String>> = err.detail().actions.into_iter().map(|a| a.method).collect();
    assert_eq!(offered, [Some("drive.backups".to_string())]);
}
