//! A library's clones stay clones: after any run of writes to the library (playlists made and
//! deleted, tracks rated, the last change undone), each addressed to the library or to one of
//! its clones, every writable clone holds the same library, nothing said it couldn't copy, and
//! the archive drive is untouched.

use std::path::{Path, PathBuf};

use crate::clones::home;
use crate::common::{self, Warnings, path};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api;
use locallibrary::compare::Side;
use locallibrary::device::Device;
use locallibrary::edit::{Edit, EditOptions, TrackFields};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;

const PLAYLISTS: &[&str] = &["Friday", "Saturday", "Warmup", "Peak/Late"];

/// An edit a DJ makes to the library on `root`: a playlist made or deleted, or a track rated.
fn edit(tc: &TestCase, root: &Path) -> Edit {
    let library = Device::open(root).unwrap().library().unwrap();
    let pick = |n: usize| tc.draw(gs::integers::<usize>().max_value(n - 1));
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        1 if !library.playlists.is_empty() => Edit::DeletePlaylist {
            playlist: library.playlists[pick(library.playlists.len())].name.clone(),
            with_tracks: false,
        },
        2 if !library.tracks.is_empty() => {
            let tracks =
                TrackSelector { ids: vec![library.tracks[pick(library.tracks.len())].id], ..Default::default() };
            let rating = Some(tc.draw(gs::integers::<u8>().max_value(5)));
            Edit::EditTracks { tracks, fields: TrackFields { rating, ..Default::default() } }
        }
        _ => Edit::CreatePlaylist { path: tc.draw(gs::sampled_from(PLAYLISTS)).to_string(), folder: false },
    }
}

fn fingerprint(root: &Path) -> String {
    let side = Side::from_device(&Device::open(root).unwrap(), &Ctx::silent()).unwrap();
    side.fingerprint.map(|f| f.to_string()).unwrap_or_default()
}

#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn writes_to_a_library_keep_its_clones_identical(tc: TestCase) {
    let h = home();
    let clones = [&h.main, &h.backup, &h.sd];
    let archive = common::drive_files(&h.archive);
    // The drive the last write was made on, which has its backup to undo.
    let mut written: Option<PathBuf> = None;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        let warnings = Warnings::default();
        match &written {
            Some(root) if tc.draw(gs::weighted_booleans(0.3)) => {
                let req = api::UndoRequest { drive: Some(path(root)), ..Default::default() };
                let plan = api::undo_plan(&req).unwrap();
                assert_eq!(plan.clones.len(), 2, "{:?}", plan.clones);
                plan.apply(Some(&h.store), &warnings.ctx()).unwrap();
                written = None;
            }
            _ => {
                // Mostly the library by name; sometimes one clone by its mount path.
                let target = match tc.draw(gs::integers::<usize>().max_value(5)) {
                    i @ 0..=2 => path(clones[i]),
                    _ => "Home".to_string(),
                };
                let lead = api::resolve_drive(&target).unwrap().root().to_path_buf();
                let edits = vec![edit(&tc, &lead)];
                let plan = api::edit_plan(
                    &api::EditRequest { drive: target, edits, options: EditOptions::default() },
                    &Ctx::silent(),
                )
                .unwrap();
                if plan.noop {
                    continue;
                }
                assert_eq!(plan.copied_onto().map(|_| plan.clones.len()), Some(2), "{:?}", plan.clones);
                plan.apply(Some(&h.store), &warnings.ctx()).unwrap();
                written = Some(lead);
            }
        }
        let copies: Vec<String> = warnings.said().into_iter().filter(|w| w.contains("copied")).collect();
        assert!(copies.is_empty(), "{copies:?}");
        let held: Vec<String> = clones.iter().map(|c| fingerprint(c)).collect();
        assert!(held.iter().all(|f| *f == held[0]), "the clones differ: {held:?}");
    }
    assert!(common::drive_files(&h.archive) == archive, "the archive was written");
}
