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
            locallibrary::testing::sparse(&root.join("Contents/a/Song.mp3"), 4 << 30).unwrap();
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

/// Which of the drive's tracks a drawn tempo finding is about.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TempoTrack {
    /// A grid at its stored tempo.
    Grid,
    /// A grid at 400 BPM, which can be halved but not doubled (800 BPM is past what a grid holds).
    Fast,
    /// No analysis files, so no grid.
    NoGrid,
    /// A grid at 120 BPM, stored at 180: doubling or halving it wouldn't fix the stored tempo.
    OffTempo,
    /// Not on the drive at all.
    Missing,
}

/// `doctor --fix-tempo` on any findings: each half or double one the library doesn't favour as
/// stored becomes its track's grid doubled or halved, a grid that can't be is left with a note
/// saying why, a track with no grid or one at another tempo than it's stored at is counted as
/// such, and a finding for a track that isn't on the drive is refused. A folder with no library
/// has nothing to fix. Planned through the API (`doctor_tempo_plan`), the plan holds those edits
/// and warns with those notes, or is refused alike.
#[hegel::test(test_cases = 20, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_tempo_fixes_follow_each_findings_track(tc: TestCase) {
    use locallibrary::analysis_edit::NewAnalysis;
    use locallibrary::anlz::{self, Beat};
    use locallibrary::edit::{self, Edit, EditOptions};
    use locallibrary::model::Track;
    use locallibrary::sound::{Plausible, TempoFinding, TempoRelation};

    let dir = tempfile::tempdir().unwrap();
    let blank = common::blank(dir.path(), "Blank");
    assert!(doctor::tempo_fixes(&Device::open(&blank).unwrap(), &[]).is_err());
    // One drive for every case: planning fixes leaves it as it is.
    static DRIVE: std::sync::OnceLock<(tempfile::TempDir, std::path::PathBuf)> = std::sync::OnceLock::new();
    let root = &DRIVE
        .get_or_init(|| {
            let dir = tempfile::tempdir().unwrap();
            let root = common::demo_drive(dir.path(), "A");
            let no_grid = NewTrack::new("No Grid", "/Contents/n/NoGrid.mp3");
            common::add_track(&root, no_grid, &common::placeholder_audio("ng"));
            let lib = Device::open_export(&root).unwrap().library().unwrap();
            let fast = lib.tracks.iter().find(|t| t.title == "Demo Track 2").unwrap();
            let beats: Vec<Beat> = (0..16)
                .map(|i| Beat { beat_number: i % 4 + 1, bpm: 400.0, time_ms: 1_000 + u32::from(i) * 150 })
                .collect();
            let set = Edit::SetAnalysis {
                track: fast.file_path.clone(),
                analysis: NewAnalysis { beats: Some(beats), ..Default::default() },
            };
            let off = NewTrack {
                analyze_path: "/PIONEER/USBANLZ/P0EE/0000EE01/ANLZ0000.DAT".into(),
                ..NewTrack::new("Off Tempo", "/Contents/n/OffTempo.mp3")
            };
            let off = common::add_track(&root, off, &common::placeholder_audio("off"));
            let off = Device::open_export(&root).unwrap().library().unwrap().track(off).unwrap().file_path.clone();
            let steady: Vec<Beat> = (0..16)
                .map(|i| Beat { beat_number: i % 4 + 1, bpm: 120.0, time_ms: 1_000 + u32::from(i) * 500 })
                .collect();
            let grid = Edit::SetAnalysis {
                track: off.clone(),
                analysis: NewAnalysis { beats: Some(steady), ..Default::default() },
            };
            let bpm = Edit::EditTracks {
                tracks: locallibrary::query::TrackSelector::paths([off]),
                fields: locallibrary::edit::TrackFields { bpm: Some(180.0), ..Default::default() },
            };
            let d = Device::open_export(&root).unwrap();
            let p = edit::plan(&d, &[set, grid, bpm], &EditOptions::default(), &Ctx::silent()).unwrap();
            edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
            (dir, root)
        })
        .1;
    let track = |title: &str| {
        let lib = Device::open_export(root).unwrap().library().unwrap();
        lib.tracks.into_iter().find(|t| t.title == title).unwrap()
    };
    let d = Device::open_export(root).unwrap();
    let missing = Track { id: 999_999, title: "Gone".into(), ..track("Demo Track 1") };
    let tracks = [
        (TempoTrack::Grid, track("Demo Track 1")),
        (TempoTrack::Fast, track("Demo Track 2")),
        (TempoTrack::NoGrid, track("No Grid")),
        (TempoTrack::Missing, missing),
        (TempoTrack::OffTempo, track("Off Tempo")),
    ];
    let relations = vec![
        TempoRelation::Agrees,
        TempoRelation::Half,
        TempoRelation::Double,
        TempoRelation::TwoThirds,
        TempoRelation::ThreeHalves,
        TempoRelation::Disagrees,
    ];
    // Each kind of finding or not, wherever drawn among drawn others; then all of them with one
    // for a track that isn't on the drive.
    let mut drawn = tc.draw(
        gs::vecs(gs::tuples!(
            gs::sampled_from(vec![0usize, 1, 2, 4]),
            gs::sampled_from(relations),
            gs::sampled_from(vec![None, Some(Plausible::Stored), Some(Plausible::Estimate)]),
        ))
        .max_size(4),
    );
    let each = [
        (0, TempoRelation::Half, None),
        (1, TempoRelation::Half, None),
        (1, TempoRelation::Double, Some(Plausible::Estimate)),
        (2, TempoRelation::Double, None),
        (4, TempoRelation::Half, None),
        (0, TempoRelation::Double, Some(Plausible::Stored)),
    ];
    for f in each {
        if tc.draw(gs::booleans()) {
            let at = tc.draw(gs::integers::<usize>().max_value(drawn.len()));
            drawn.insert(at, f);
        }
    }
    let mut with_missing = drawn.clone();
    let at = tc.draw(gs::integers::<usize>().max_value(drawn.len()));
    with_missing.insert(at, (3, tc.draw(gs::sampled_from(vec![TempoRelation::Half, TempoRelation::Double])), None));
    'drawn: for drawn in [drawn, with_missing] {
        let findings: Vec<TempoFinding> = drawn
            .iter()
            .map(|&(i, relation, likelier)| TempoFinding {
                track: (&tracks[i].1).into(),
                stored: tracks[i].1.bpm,
                estimate: 0.0,
                confidence: None,
                relation,
                likelier,
            })
            .collect();
        let result = doctor::tempo_fixes(&d, &findings);
        let planned = locallibrary::api::doctor_tempo_plan(&common::path(root), &findings, &Ctx::silent());

        let (mut edits, mut notes, mut favoured, mut no_grid, mut off) = (Vec::new(), Vec::new(), 0, 0, 0);
        for &(i, relation, likelier) in &drawn {
            let (kind, t) = &tracks[i];
            let scaled: fn(&[Beat]) -> Vec<Beat> = match relation {
                TempoRelation::Half => anlz::doubled_grid,
                TempoRelation::Double => anlz::halved_grid,
                _ => continue,
            };
            if likelier == Some(Plausible::Stored) {
                favoured += 1;
                continue;
            }
            match kind {
                TempoTrack::Missing => {
                    assert_eq!(result.unwrap_err().code(), "not_found", "{drawn:?}");
                    assert_eq!(planned.map(|p| p.noop).unwrap_err().code(), "not_found", "{drawn:?}");
                    continue 'drawn;
                }
                TempoTrack::NoGrid => no_grid += 1,
                TempoTrack::OffTempo => off += 1,
                TempoTrack::Fast if relation == TempoRelation::Half => notes
                    .push(format!("{} is left as it is: beat 1's tempo, 800 BPM, isn't 0.01-655.35", t.display_name())),
                TempoTrack::Grid | TempoTrack::Fast => {
                    let beats = anlz::checked_grid(&scaled(&anlz::load_grid(&d, t).beats)).unwrap();
                    let analysis = NewAnalysis { beats: Some(beats), ..Default::default() };
                    edits.push(Edit::SetAnalysis { track: t.file_path.clone(), analysis });
                }
            }
        }
        if favoured > 0 {
            notes.push(format!(
                "{favoured} track(s) left as they are: the library's other tempos favour the stored one"
            ));
        }
        if no_grid > 0 {
            notes.push(format!("{no_grid} track(s) left as they are: no beat grid to double or halve"));
        }
        if off > 0 {
            notes.push(format!("{off} track(s) left as they are: their beat grid isn't at their stored tempo"));
        }
        if !edits.is_empty() {
            notes.push("only the beat grid (PQTZ) is doubled or halved: the .EXT's extended grid (PQT2) isn't decoded, so it isn't rescaled".into());
        }
        assert_eq!(result.unwrap(), (edits.clone(), notes.clone()), "{drawn:?}");
        // Planned through the API, the fixes are an edit plan warning of what they leave out.
        let planned = planned.unwrap();
        assert!(notes.iter().all(|n| planned.warnings.contains(n)), "{:?}", planned.warnings);
        assert_eq!(planned.noop, edits.is_empty(), "{drawn:?}");
    }
}

/// Orphans whose backup wouldn't fit in the drive's free space: the plan says so and how much
/// it needs, and removing them is refused with the drive left as it was. Ones that fit are
/// removed.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_orphans_too_big_to_back_up_are_not_removed(tc: TestCase) {
    // 8 TiB is past any drive tests run on, and under ext4's and NTFS's largest file.
    let size = tc.draw(gs::sampled_from(vec![1u64 << 10, 1 << 43]));
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let stray = paths::to_fs(&root, "Contents/Stray/huge.wav");
    fs::create_dir_all(stray.parent().unwrap()).unwrap();
    locallibrary::testing::sparse(&stray, size).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    let plan = doctor::plan_orphans(&d, &Ctx::silent()).unwrap();
    assert_eq!((plan.files.len(), plan.bytes), (1, size));
    assert!(plan.needed_bytes >= size);
    let free = plan.free_space.unwrap();
    assert_eq!(plan.fits, plan.needed_bytes <= free);
    assert_eq!(plan.fits, size < 1 << 43, "{free} bytes free");
    // Read back without the stray: its 8 TiB would have to be read too.
    let pdb = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    match plan.fits {
        true => {
            assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
            assert_eq!(doctor::apply_orphans(&plan, &mut d, &Ctx::silent()).unwrap().files_deleted, 1);
            assert!(!stray.exists());
        }
        false => {
            let warned =
                format!("the backup needs about {} bytes but only {free} are free on the drive", plan.needed_bytes);
            assert_eq!(plan.warnings, [warned]);
            let e = doctor::apply_orphans(&plan, &mut d, &Ctx::silent()).unwrap_err();
            assert_eq!(e.code(), "no_space", "{e}");
            assert_eq!(fs::metadata(&stray).unwrap().len(), size);
            assert_eq!(fs::read(root.join(paths::EXPORT_PDB)).unwrap(), pdb);
        }
    }
}

/// The audio of copies in MPEG-4 files is lossless only when the file's header says Apple
/// Lossless; a file the header can't be read from, or with no extension, isn't. Rows for a file
/// with no extension are still one group, with one reason however many rows link it.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_copies_in_mpeg4_files_are_lossless_by_their_header(tc: TestCase) {
    let audio = |name: &str| fs::read(common::fixtures().join("audio").join(name)).unwrap();
    let (content, alac) = tc.draw(gs::sampled_from(vec![
        (audio("silence-alac.m4a"), true),
        (audio("silence.m4a"), false),
        (b"not an mpeg-4 file".to_vec(), false),
    ]));
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let row = |title: &str, file: &str| NewTrack { artist: Some(title.into()), ..NewTrack::new(title, file) };
    let m4a = [("One", "/Contents/m/Song.m4a"), ("Two", "/Contents/m/Song.m4a")];
    let bare = [("Three", "/Contents/m/Song"), ("Four", "/Contents/m/Song"), ("Five", "/Contents/m/Song")];
    let mut ids = Vec::new();
    for (title, file) in m4a.into_iter().chain(bare) {
        let bytes = if file.ends_with(".m4a") { content.clone() } else { common::placeholder_audio(file) };
        ids.push(common::add_track(&root, row(title, file), &bytes));
    }
    let d = Device::open_export(&root).unwrap();
    let only = vec![Section::Duplicates];
    let r = doctor::run(&d, &DoctorOptions { only, ..Default::default() }, &Ctx::silent()).unwrap();
    let groups = r.duplicates.unwrap();
    for (pair, lossless) in [(&ids[..2], alac), (&ids[2..], false)] {
        let g = groups.iter().find(|g| g.tracks.iter().any(|t| t.track.id == pair[0])).unwrap();
        let members: BTreeSet<u32> = g.tracks.iter().map(|t| t.track.id).collect();
        assert_eq!(members, pair.iter().copied().collect(), "{groups:#?}");
        assert_eq!(g.reasons, [DuplicateReason::SameFile]);
        assert!(g.tracks.iter().all(|t| t.lossless == lossless), "{g:#?}");
    }
}

/// A OneLibrary database beside export.pdb that can't be read, as a database or as a file (its
/// WAL a folder): the doctor checks export.pdb alone and says so, and orphans aren't planned,
/// saying why.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_orphans_arent_planned_beside_an_unreadable_onelibrary(tc: TestCase) {
    let unreadable_file = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    match unreadable_file {
        true => fs::create_dir_all(root.join(paths::LIBRARY_PLUS_WAL)).unwrap(),
        false => fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap(),
    }
    let d = Device::open_export(&root).unwrap();
    let r =
        doctor::run(&d, &DoctorOptions { only: vec![Section::Missing], ..Default::default() }, &Ctx::silent()).unwrap();
    assert!(r.warnings.iter().any(|w| w.ends_with("; only export.pdb was checked")), "{:?}", r.warnings);
    let e = doctor::plan_orphans(&d, &Ctx::silent()).unwrap_err();
    let message = e.to_string();
    assert!(message.contains("; without it, which files are orphaned can't be known"), "{message}");
    assert_eq!(message.contains("exportLibrary.db-wal"), unreadable_file, "{message}");
}
