use chrono::{TimeZone, Utc};
use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::api::{DriveStatus, FleetDrive, FleetLibrary, FleetStatus};
use locallibrary::fleet::{CheckEntry, Duplicate};
use locallibrary::scrub::{FileProblem, RepairSource};
use locallibrary::space::{DriveFit, Forecast, LooseFile, SpaceCategory};
use serde_json::json;
use uuid::Uuid;

use super::*;

/// What the terminal shows of a styled string: the text, without colour codes.
fn plain(s: &str) -> String {
    console::strip_ansi_codes(s).into_owned()
}

/// A mounted drive (an empty folder) and what's known of it, drawn: write-protected or not,
/// nearly full or not, exported over recorded edits or not, and its last check, if any, failed
/// or not.
#[hegel::test_helper]
fn fleet_drive(tc: &TestCase, dir: &std::path::Path, name: &str) -> FleetDrive {
    let root = dir.join(name);
    std::fs::create_dir_all(&root).unwrap();
    let info = locallibrary::Device::open(&root).unwrap().info();
    let drive = locallibrary::DeviceInfo { name: Some(name.into()), ..info };
    let status = DriveStatus {
        drive,
        library: None,
        relation: None,
        head: None,
        back_from_set: None,
        settings_drift: 0,
        error: None,
    };
    let mut d = FleetDrive {
        status,
        role: tc.draw(gs::optional(gs::sampled_from(&Role::ALL[..]))),
        read_only: tc.draw(gs::booleans()),
        low_space: tc.draw(gs::booleans()),
        log: None,
        same_identity_as: Vec::new(),
        exported_over_edits: tc.draw(gs::integers::<usize>().max_value(3)),
    };
    if tc.draw(gs::booleans()) {
        let at = Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
        let ok = tc.draw(gs::booleans());
        let last_scrub = Some(CheckEntry { at, ok, summary: "2 damaged".into() });
        d.log = Some(DriveLog { last_scrub, ..DriveLog::default() });
    }
    d
}

/// A drive's notes say each thing worth knowing of it, once and in order: write protection,
/// little space left, an export over recorded edits (with the command that makes them again,
/// naming the drive), and a check that failed (never one that passed). The fleet's listing shows
/// them with every drive, in a library or in none.
#[hegel::test(test_cases = 100)]
fn a_drives_notes_say_what_else_to_know(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let drives: Vec<FleetDrive> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
        .map(|i| fleet_drive(&tc, dir.path(), &format!("D{i}")))
        .collect();
    for d in &drives {
        let name = drive_name(&d.status.drive);
        let notes: Vec<String> = drive_notes(d, &name).iter().map(|n| plain(n)).collect();
        let failed = d.log.as_ref().and_then(|l| l.last_scrub.as_ref()).is_some_and(|c| !c.ok);
        let expected = [
            (d.read_only, "write-protected"),
            (d.low_space, "nearly full"),
            (d.exported_over_edits > 0, "recorded edit(s)"),
            (failed, "last check: 2 damaged"),
        ];
        let wanted: Vec<&str> = expected.iter().filter(|(on, _)| *on).map(|(_, w)| *w).collect();
        assert_eq!(notes.len(), wanted.len(), "{notes:?}");
        for (note, w) in notes.iter().zip(&wanted) {
            assert!(note.contains(w), "{note:?} should say {w:?}");
        }
        if d.exported_over_edits > 0 {
            assert!(notes.iter().any(|n| n.contains(&format!("locallibrary refresh {name:?}"))), "{notes:?}");
        }
    }
    // Some in a library (with or without a snapshot, the same or not), the rest in none.
    let in_library = tc.draw(gs::integers::<usize>().max_value(drives.len()));
    let library = FleetLibrary {
        id: Uuid::from_u128(1),
        name: "Main".into(),
        head: None,
        mounted: drives[..in_library].iter().map(|d| d.status.drive.root.clone()).collect(),
        offline: Vec::new(),
        identical: tc.draw(gs::sampled_from(&[None, Some(true), Some(false)][..])),
        unsaved: tc.draw(gs::booleans()),
    };
    let duplicates = match tc.draw(gs::booleans()) {
        true => vec![Duplicate {
            id: Uuid::from_u128(2),
            name: "D0".into(),
            roots: vec![drives[0].status.drive.root.clone()],
            original: tc.draw(gs::booleans()).then(|| drives[0].status.drive.root.clone()),
        }],
        false => Vec::new(),
    };
    let status = FleetStatus { mounted: drives, offline: Vec::new(), duplicates, libraries: vec![library] };
    fleet_status(&status);
    fleet_status(&FleetStatus::default());
}

/// Every kind of space is named, and no two alike; a space report shows each part of it: the
/// forecast whichever way the library goes, the drives it fits on and those it doesn't.
#[hegel::test(test_cases = 100)]
fn space_reports_name_every_kind(tc: TestCase) {
    let kinds = [
        SpaceKind::Audio,
        SpaceKind::Analysis,
        SpaceKind::Artwork,
        SpaceKind::Databases,
        SpaceKind::Settings,
        SpaceKind::RekordboxOther,
        SpaceKind::LocallibraryBackups,
        SpaceKind::Locallibrary,
        SpaceKind::Recordings,
        SpaceKind::OsClutter,
        SpaceKind::Trash,
        SpaceKind::LooseAudio,
        SpaceKind::Other,
    ];
    let mut names: Vec<&str> = kinds.iter().map(|k| kind_name(*k)).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), kinds.len());
    let picked: Vec<usize> = tc.draw(gs::subsequences((0..kinds.len()).collect::<Vec<usize>>()));
    let report = SpaceReport {
        drive: "DJ".into(),
        root: "/Volumes/DJ".into(),
        total: tc.draw(gs::booleans()).then_some(1 << 34),
        free: tc.draw(gs::booleans()).then_some(1 << 30),
        cluster_size: 32768,
        categories: picked
            .into_iter()
            .map(|i| SpaceCategory {
                kind: kinds[i],
                files: 3,
                bytes: tc.draw(gs::integers::<u64>().max_value(1 << 32)),
            })
            .collect(),
        loose_audio: (0..tc.draw(gs::integers::<usize>().max_value(7)))
            .map(|i| LooseFile { path: format!("Music/{i}.mp3"), size: 1000 })
            .collect(),
        unreadable: tc.draw(gs::integers::<usize>().max_value(2)),
        forecast: tc.draw(gs::booleans()).then(|| Forecast {
            library: "Main".into(),
            library_bytes: 1 << 30,
            per_month: tc.draw(gs::integers::<i64>().min_value(-(1 << 30)).max_value(1 << 30)),
            snapshots: 4,
            span_days: 60,
            full_around: tc.draw(gs::booleans()).then(|| chrono::NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()),
        }),
        fits: (0..tc.draw(gs::integers::<usize>().max_value(2)))
            .map(|i| DriveFit {
                name: format!("B{i}"),
                capacity: 1 << 33,
                needed: 1 << 32,
                fits: tc.draw(gs::booleans()),
                mounted: false,
            })
            .collect(),
    };
    space_report(&report);
}

/// A check's report lists each damaged, unreadable and missing file with where a good copy is,
/// up to its cap, says how many more there are, and offers the repair only when there's a good
/// copy to repair from.
#[hegel::test(test_cases = 100)]
fn check_reports_list_each_problem(tc: TestCase) {
    let problem = |tc: &TestCase, i: usize| -> FileProblem {
        let mut p: FileProblem = serde_json::from_value(json!({
            "path": format!("Contents/{i}.mp3"), "class": "audio", "size": 10,
        }))
        .unwrap();
        p.error = tc.draw(gs::booleans()).then(|| "permission denied".into());
        p.repair = match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => None,
            1 => Some(RepairSource::Store),
            _ => Some(RepairSource::Drive { name: "Backup".into(), root: "/Volumes/Backup".into() }),
        };
        p
    };
    let list = |tc: &TestCase| -> Vec<FileProblem> {
        (0..tc.draw(gs::integers::<usize>().max_value(25))).map(|i| problem(tc, i)).collect()
    };
    let report = ScrubReport {
        drive: "DJ".into(),
        root: "/Volumes/DJ".into(),
        snapshot: tc.draw(gs::booleans()).then(|| locallibrary::Hash::of(b"state")),
        files: 40,
        bytes: 1 << 20,
        verified: 10,
        unverified: 2,
        damaged: list(&tc),
        unreadable: list(&tc),
        missing: list(&tc),
        tracks: Vec::new(),
        playlists: Vec::new(),
    };
    assert_eq!(report.is_clean(), report.problems().next().is_none());
    scrub_report(&report);
}
