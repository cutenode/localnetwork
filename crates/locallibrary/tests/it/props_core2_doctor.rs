//! The doctor on what the main properties don't draw: copies that are one group only through
//! another copy (two rows for one file, and a third row with the same tags in a file of its own),
//! a file too big for FAT32, and orphans that a track came to use between planning and removing.

use crate::common;

use std::collections::BTreeSet;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::doctor::{self, DoctorOptions, DuplicateReason, Section};
use locallibrary::paths;
use locallibrary::progress::Ctx;
use rekordbox_pdb::NewTrack;

/// The six orders three rows can be exported in.
const ORDERS: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];

/// On the twelve-track drive, two rows for one file and a row with the same artist and title in another file are one group,
/// whatever order they were exported in, with both reasons and nothing else in it. A file of
/// 4 GiB or more is listed as too big for FAT32, with its track.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn copies_linked_through_another_copy_are_one_group(tc: TestCase) {
    let first_big = tc.draw(gs::booleans());
    for (n, order) in ORDERS.into_iter().enumerate() {
        // Every other order with the big file, so each run sees both.
        let big = first_big ^ (n % 2 == 1);
        let dir = tempfile::tempdir().unwrap();
        let root = common::bigger_drive(dir.path(), "A");
        let rows = [
            NewTrack {
                artist: Some("Copy Artist".into()),
                date_added: "2026-01-01".into(),
                ..NewTrack::new("Copy Title", "/Contents/a/Song.mp3")
            },
            NewTrack { artist: Some("Someone".into()), ..NewTrack::new("Other Title", "/Contents/b/Song.mp3") },
            NewTrack { artist: Some("Copy Artist".into()), ..NewTrack::new("Copy Title", "/Contents/b/Song.mp3") },
        ];
        let mut ids = [0u32; 3];
        for i in order {
            ids[i] = common::add_track(&root, rows[i].clone(), &common::placeholder_audio(&rows[i].file_path));
        }
        if big {
            let f = fs::OpenOptions::new().write(true).open(root.join("Contents/a/Song.mp3")).unwrap();
            f.set_len(4 << 30).unwrap();
        }
        let d = Device::open_export(&root).unwrap();
        let only = vec![Section::Duplicates, Section::LargeFiles];
        let r = doctor::run(&d, &DoctorOptions { only, ..Default::default() }, &Ctx::silent()).unwrap();
        let groups = r.duplicates.as_ref().unwrap();
        let group = groups.iter().find(|g| g.tracks.iter().any(|t| t.track.id == ids[0])).unwrap();
        let members: BTreeSet<u32> = group.tracks.iter().map(|t| t.track.id).collect();
        assert_eq!(members, ids.into_iter().collect(), "{order:?}: {groups:#?}");
        let reasons: BTreeSet<DuplicateReason> = group.reasons.iter().copied().collect();
        assert_eq!(reasons, BTreeSet::from([DuplicateReason::SameFile, DuplicateReason::SameMetadata]), "{order:?}");
        // Alike in everything else, the copy with a date added is kept over those without.
        assert_eq!(group.keep, ids[0], "{order:?}: {group:#?}");
        let large = r.large_files.as_ref().unwrap();
        match big {
            true => {
                assert_eq!(large.len(), 1);
                assert_eq!(large[0].size, 4 << 30);
                assert_eq!(large[0].track.as_ref().map(|t| t.id), Some(ids[0]));
            }
            false => assert!(large.is_empty()),
        }
        assert!(r.has_problems());
    }
}

/// A folder that isn't an export, or a drive with only a OneLibrary database that can't be read,
/// isn't checked: there's nothing to check it against.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn drives_without_a_readable_database_are_not_checked(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let blank = common::blank(dir.path(), "Blank");
    fs::create_dir_all(blank.join("Contents")).unwrap();
    let root = common::demo_drive(dir.path(), "A");
    common::onelibrary_only(&root);
    let garbage = tc.draw(gs::sampled_from(vec![&b"not a database"[..], b"", b"SQLite format 3\0 and then nothing"]));
    fs::write(root.join(paths::LIBRARY_PLUS_DB), garbage).unwrap();
    for section in [Section::Missing, Section::Orphans, Section::Duplicates] {
        let opts = DoctorOptions { only: vec![section], ..Default::default() };
        let e = doctor::run(&Device::open(&blank).unwrap(), &opts, &Ctx::silent()).unwrap_err();
        assert_eq!(e.code(), "not_an_export", "{e}");
        assert!(doctor::run(&Device::open(&root).unwrap(), &opts, &Ctx::silent()).is_err());
    }
    assert!(doctor::plan_orphans(&Device::open(&root).unwrap(), &Ctx::silent()).is_err());
}

/// Orphans planned for removal that a track uses by the time they'd be removed aren't removed:
/// the plan is stale, and the drive is left as it was.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn orphans_a_track_came_to_use_are_not_removed(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let strays = ["Contents/Stray/one.mp3", "Contents/Stray/two.flac", "Contents/Stray/three.wav"];
    for k in strays {
        let p = paths::to_fs(&root, k);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, common::placeholder_audio(k)).unwrap();
    }
    let mut d = Device::open_export(&root).unwrap();
    let plan = doctor::plan_orphans(&d, &Ctx::silent()).unwrap();
    let planned: BTreeSet<String> = plan.files.iter().map(|f| f.path.to_lowercase()).collect();
    assert_eq!(planned, strays.iter().map(|s| s.to_lowercase()).collect());
    let used = tc.draw(gs::sampled_from(strays.to_vec()));
    let mut ed = rekordbox_pdb::PdbEditor::from_file(root.join(paths::EXPORT_PDB)).unwrap();
    ed.add_track(&NewTrack::new("Found Again", format!("/{used}"))).unwrap();
    ed.save(root.join(paths::EXPORT_PDB)).unwrap();
    let before = common::drive_files(&root);
    let err = doctor::apply_orphans(&plan, &mut d, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty());
}
