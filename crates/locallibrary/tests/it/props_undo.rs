//! Undoing edits on a drive: each undo takes back the newest change still on it until there's
//! none, an interrupted write's backup is the one put back, and putting a backup back is refused
//! (with the way to force it) over a drive a sync changed since, or when the library it holds
//! names audio that's gone.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::backup;
use locallibrary::device::{Device, LineageOp};
use locallibrary::edit::Edit;
use locallibrary::error::Reason;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::sync::{self, SyncOptions};

fn retries_with_force(e: &locallibrary::Error) -> bool {
    e.detail().actions.iter().any(|a| a.params.get("force") == Some(&serde_json::json!(true)))
}

#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn undo_takes_back_whats_still_there_and_nothing_else(tc: TestCase) {
    for kind in 0..4 {
        undo_after(&tc, kind);
    }
}

/// Edits on a drive, then what `kind` says happens before the undo.
#[hegel::test_helper]
fn undo_after(tc: &TestCase, kind: u8) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let edits = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    for n in 0..edits {
        common::apply(&root, &[Edit::CreatePlaylist { path: format!("Set {n}"), folder: false }]);
    }
    let playlists = |root: &std::path::Path| common::library(root).playlists.len();
    let start = playlists(&root) - edits;
    match kind {
        // Undo, undo, ...: one change at a time, then nothing more.
        0 => {
            for n in (0..edits).rev() {
                backup::restore(&mut Device::open_export(&root).unwrap(), None, false).unwrap();
                assert_eq!(playlists(&root), start + n);
            }
            let err = backup::restore(&mut Device::open_export(&root).unwrap(), None, false).unwrap_err();
            assert!(err.to_string().contains("the changes its backups were taken for are undone already"), "{err}");
        }
        // A sync copies another library over it: undoing would take that back too.
        1 => {
            let mut other = Device::open_export(common::demo_drive(dir.path(), "Other")).unwrap();
            let mut d = Device::open_export(&root).unwrap();
            sync::sync_drive_to_drive(&mut other, &mut d, &SyncOptions::default(), &Ctx::silent()).unwrap();
            let err = backup::restore(&mut Device::open_export(&root).unwrap(), None, false).unwrap_err();
            assert_eq!(err.reason(), Some(Reason::ChangedSince), "{err}");
            assert!(err.to_string().contains("after a sync on"), "{err}");
            assert!(retries_with_force(&err));
            // Named outright, an older backup is refused too, unless forced.
            let oldest = backup::list(&Device::open_export(&root).unwrap()).unwrap().last().unwrap().id.clone();
            let err = backup::restore(&mut Device::open_export(&root).unwrap(), Some(&oldest), false).unwrap_err();
            assert_eq!(err.reason(), Some(Reason::ChangedSince), "{err}");
            backup::restore(&mut Device::open_export(&root).unwrap(), Some(&oldest), true).unwrap();
            assert_eq!(playlists(&root), start);
        }
        // A write cut off after its backup: undo puts that backup back.
        2 => {
            let newest = backup::list(&Device::open_export(&root).unwrap()).unwrap()[0].id.clone();
            let older = backup::list(&Device::open_export(&root).unwrap()).unwrap().last().unwrap().id.clone();
            let mut d = Device::open_export(&root).unwrap();
            d.begin(LineageOp::Edit, "an edit cut off", Some(older.clone())).unwrap();
            assert_eq!(backup::choose(&Device::open_export(&root).unwrap(), None, false).unwrap().id, older);
            assert!(edits == 1 || newest != older);
        }
        // The audio of a track the backed-up library holds is gone from the drive.
        _ => {
            let lib = common::library(&root);
            let gone = &lib.tracks[tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1))];
            fs::remove_file(paths::to_fs(&root, &paths::key(&gone.file_path))).unwrap();
            let err = backup::restore(&mut Device::open_export(&root).unwrap(), None, false).unwrap_err();
            assert_eq!(err.reason(), Some(Reason::MissingAudio), "{err}");
            assert!(retries_with_force(&err));
            backup::restore(&mut Device::open_export(&root).unwrap(), None, true).unwrap();
            assert_eq!(playlists(&root), start + edits - 1);
        }
    }
}
