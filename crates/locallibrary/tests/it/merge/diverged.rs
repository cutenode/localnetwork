//! Merging a drive into another when both changed since they were last in sync: once the merge
//! brings in everything the source has, the target counts as newer and `sync` copies it over the
//! source, tracks the plan listed as differing included. A merge only planned, one that leaves
//! some of the source out, or one taken back, leaves them diverged.

use crate::common;

use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, ComparePair, UndoRequest};
use locallibrary::compare::Relation;
use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::merge::{self, MergeOptions, PlaylistMode};
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::sync::{self, SyncOptions};

#[derive(Debug, Clone, Copy, PartialEq)]
enum How {
    PlannedOnly,
    Whole,
    OnePlaylist,
    TracksOnly,
    Undone,
}

fn relation(left: &Path, right: &Path) -> Relation {
    let pair = ComparePair { left: common::path(left), right: common::path(right) };
    api::compare(&pair, &Ctx::silent()).unwrap().relation
}

/// Drives A and B, A synced onto B and then both changed: a new export each, and with `edited`,
/// A's details and analysis (a beat grid of another size) for "Shared", a track B has too.
fn diverged(dir: &Path, edited: bool) -> (PathBuf, PathBuf) {
    let a_root = common::bigger_drive(dir, "A");
    common::adopt(None, &a_root, "A");
    // The fixture's own tracks keep their analysis in .PIONEER/, which no scan reads.
    let shared = common::export_new_track(&a_root, "Both", "Shared");
    let b_root = common::blank(dir, "B");
    let mut a = Device::open_export(&a_root).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut Device::open(&b_root).unwrap(), &SyncOptions::default(), &Ctx::silent())
        .unwrap();
    common::export_new_track(&a_root, "Only", "On A");
    common::export_new_track(&b_root, "Only", "On B");
    if edited {
        let mut ed = rekordbox_pdb::PdbEditor::from_file(a_root.join(paths::EXPORT_PDB)).unwrap();
        ed.set_track_strings(shared, &[("comment", "edited on A")]).unwrap();
        ed.save(a_root.join(paths::EXPORT_PDB)).unwrap();
        let tracks = Device::open_export(&a_root).unwrap().library().unwrap().tracks;
        let t = tracks.into_iter().find(|t| t.id == shared).unwrap();
        let regridded = common::synthetic_anlz(&format!("{}.longer", t.file_path));
        std::fs::write(paths::to_fs(&a_root, &paths::key(&t.analyze_path)), regridded).unwrap();
    }
    assert_eq!(relation(&b_root, &a_root), Relation::Diverged);
    (a_root, b_root)
}

#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn a_merge_that_takes_in_the_source_lets_sync_go_ahead(tc: TestCase) {
    let edited = tc.draw(gs::booleans());
    for how in [How::PlannedOnly, How::Whole, How::OnePlaylist, How::TracksOnly, How::Undone] {
        let dir = tempfile::tempdir().unwrap();
        let (a_root, b_root) = diverged(dir.path(), edited);
        let a = Device::open_export(&a_root).unwrap();
        let mut b = Device::open_export(&b_root).unwrap();
        let lib = a.library().unwrap();
        let opts = match how {
            How::OnePlaylist => MergeOptions {
                only_playlists: vec![lib.playlist_path(lib.playlists.iter().find(|p| !p.is_folder).unwrap().id)],
                ..Default::default()
            },
            How::TracksOnly => MergeOptions { playlists: PlaylistMode::None, ..Default::default() },
            _ => MergeOptions::default(),
        };
        let plan = merge::plan(&a, &b, &opts, &Ctx::silent()).unwrap();
        let whole = matches!(how, How::PlannedOnly | How::Whole | How::Undone);
        assert_eq!(plan.takes_in_source, whole, "{how:?}");
        if whole {
            let named: Vec<(&str, Vec<&str>)> = plan
                .differing_tracks
                .iter()
                .map(|t| (t.track.name.as_str(), t.fields.iter().map(|f| f.field.as_str()).collect()))
                .collect();
            assert_eq!(named, if edited { vec![("Both - Shared", vec!["comment", "analysis"])] } else { vec![] });
        }
        if how != How::PlannedOnly {
            merge::apply(&plan, &a, &mut b, &opts, &Ctx::silent()).unwrap();
        }
        if how == How::Undone {
            let undo = UndoRequest { drive: Some(common::path(&b_root)), ..Default::default() };
            api::undo_plan(&undo).unwrap().apply(None, &Ctx::silent()).unwrap();
        }

        let sync = api::sync_plan(&common::sync_pair(&b_root, &a_root), &Ctx::silent());
        if how != How::Whole {
            assert_eq!(relation(&b_root, &a_root), Relation::Diverged, "{how:?}");
            assert_eq!(sync.err().and_then(|e| e.reason()), Some(Reason::Diverged), "{how:?}");
            continue;
        }
        assert_eq!(relation(&b_root, &a_root), Relation::LeftNewer);
        sync.unwrap().apply(None, &Ctx::silent()).unwrap();
        assert_eq!(common::library_files(&a_root), common::library_files(&b_root), "A gets the merged library");
        assert_eq!(relation(&b_root, &a_root), Relation::Identical);
    }
}
