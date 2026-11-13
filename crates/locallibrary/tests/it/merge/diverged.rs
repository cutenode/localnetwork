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
use locallibrary::progress::Ctx;
use locallibrary::sync::{self, SyncOptions};
use locallibrary::{Track, paths, tags};

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

/// A merge's plan lists a track both drives have whose My Tags differ among those that differ,
/// and the tracks only the source's OneLibrary lists, which it doesn't bring across. With one of
/// those, it doesn't take in the source, so `sync` asks which way rather than drop it from the
/// source; with only a tag differing it does, and the sync that follows takes the listed tag.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(tagged = true, plus_only = false)]
#[hegel::explicit_test_case(tagged = false, plus_only = true)]
fn a_merge_lists_the_my_tags_and_onelibrary_tracks_a_sync_would_drop(tc: TestCase) {
    let tagged = tc.draw(gs::booleans());
    let plus_only = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let a_root = common::bigger_drive(dir.path(), "A");
    common::adopt(None, &a_root, "A");
    let shared = common::export_new_track(&a_root, "Both", "Shared");
    common::add_onelibrary(&a_root);
    let b_root = common::blank(dir.path(), "B");
    let mut a = Device::open_export(&a_root).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut Device::open(&b_root).unwrap(), &SyncOptions::default(), &Ctx::silent())
        .unwrap();
    common::export_new_track(&b_root, "Only", "On B");
    if plus_only {
        // OneLibrary gets a track export.pdb doesn't list, as a CDJ-3000X's export leaves it.
        let mut lib = Device::open_export(&a_root).unwrap().library().unwrap();
        let only = Track {
            id: lib.tracks.iter().map(|t| t.id).max().unwrap() + 1,
            title: "Only Plus".into(),
            file_path: "/Contents/Booth/Only Plus.mp3".into(),
            ..lib.tracks[0].clone()
        };
        lib.tracks.push(only);
        common::populate_library(&a_root, &lib);
        std::fs::write(a_root.join(paths::LIBRARY_PLUS_DB), common::onelibrary_for(&lib).to_encrypted().unwrap())
            .unwrap();
    }
    if tagged {
        let mut a = Device::open_export(&a_root).unwrap();
        let plan = tags::plan(&a, &tags::Edit::Add { tag: "Genre/Deep".into(), tracks: vec![shared] }).unwrap();
        tags::apply(&plan, &mut a, &Ctx::silent()).unwrap();
    }
    if !tagged && !plus_only {
        common::export_new_track(&a_root, "Only", "On A");
    }
    assert_eq!(relation(&b_root, &a_root), Relation::Diverged);

    let a = Device::open_export(&a_root).unwrap();
    let mut b = Device::open_export(&b_root).unwrap();
    let plan = merge::plan(&a, &b, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let tag_changes: Vec<(&str, &str, &str)> = plan
        .differing_tracks
        .iter()
        .flat_map(|t| {
            t.fields.iter().filter(|f| f.field == "my_tags").map(|f| (t.track.name.as_str(), &*f.old, &*f.new))
        })
        .collect();
    assert_eq!(tag_changes, if tagged { vec![("Both - Shared", "Genre/Deep", "")] } else { vec![] });
    let left: Vec<&str> = plan.source_onelibrary_only.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(left.len(), usize::from(plus_only), "{left:?}");
    assert!(left.iter().all(|name| name.ends_with(" - Only Plus")), "{left:?}");
    assert_eq!(plan.takes_in_source, !plus_only);

    merge::apply(&plan, &a, &mut b, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let sync = api::sync_plan(&common::sync_pair(&b_root, &a_root), &Ctx::silent());
    if plus_only {
        assert_eq!(sync.err().and_then(|e| e.reason()), Some(Reason::Diverged));
        return;
    }
    sync.unwrap().apply(None, &Ctx::silent()).unwrap();
    let a = Device::open_export(&a_root).unwrap();
    let shared = a.library().unwrap().tracks.into_iter().find(|t| t.title == "Shared").unwrap();
    assert!(tags::by_track(&a).unwrap().of(&shared).is_empty(), "the tag the plan listed went");
}
