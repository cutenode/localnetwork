//! Planned operations: a plan made from a drawn drive is refused once anything it was made from
//! changes behind its back (its databases, its identity, the files it names), and only then; a
//! fresh plan applies, and says what it is when serialised. Sizes read as a person would.

use crate::{common, drawn};

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, EditRequest};
use locallibrary::device::Device;
use locallibrary::edit::{Edit, EditOptions};
use locallibrary::op::{self, AnyPlanned, Probe, Touched};
use locallibrary::paths;
use locallibrary::progress::Ctx;
use rekordbox_pdb::PdbEditor;

#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn a_plan_is_refused_exactly_when_what_it_was_made_from_changes(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, lib) = drawn::drive(&tc, &common::isolate(dir.path(), None), "A");
    if tc.draw(gs::booleans()) {
        Device::open(&root).unwrap().adopt(Some("A")).unwrap();
    }
    let audio = paths::key(&lib.tracks[0].file_path);
    let d = Device::open_export(&root).unwrap();
    let touched = [
        Touched::library(&d, true),
        Touched::drive(&d, true, vec![Probe::Content(vec![audio.clone()])]),
        Touched::drive(&d, false, vec![Probe::Stamp(vec![audio.clone()]), Probe::Listing("Contents".into())]),
    ];
    let req = EditRequest {
        drive: common::path(&root),
        edits: vec![Edit::CreatePlaylist { path: "Planned".into(), folder: false }],
        options: EditOptions::default(),
    };
    let planned = api::edit_plan(&req, &Ctx::silent()).unwrap();
    let json = planned.to_json();
    for field in ["plan_id", "operation", "summary", "warnings", "destructive", "noop", "drives", "planned_at"] {
        assert!(json.get(field).is_some(), "{field} in {json}");
    }
    assert_eq!((planned.operation(), planned.is_noop(), planned.destructive()), ("edit", false, false));
    assert_eq!(planned.drives().len(), 1);

    // What changes behind the plan's back.
    let change = tc.draw(gs::integers::<u8>().max_value(4));
    match change {
        0 => {}
        // rekordbox exports again.
        1 => {
            let mut ed = PdbEditor::from_file(root.join(paths::EXPORT_PDB)).unwrap();
            ed.create_playlist("Exported", 0, false).unwrap();
            ed.save(root.join(paths::EXPORT_PDB)).unwrap();
        }
        // The audio file is rewritten, size and all.
        2 => fs::write(paths::to_fs(&root, &audio), b"re-encoded").unwrap(),
        // A file appears beside the audio (not in its folder's listing: in Contents').
        3 => fs::write(paths::to_fs(&root, "Contents/new.mp3"), b"new").unwrap(),
        // Another drive mounted where this one was: a new identity.
        _ => {
            let _ = fs::remove_dir_all(root.join(paths::STATE_DIR));
            Device::open(&root).unwrap().adopt(Some("Other")).unwrap();
        }
    }
    let changed: Vec<bool> = touched.iter().map(Touched::changed).collect();
    assert_eq!(changed[0], change == 1 || change == 4, "databases and identity");
    assert_eq!(changed[1], change == 2, "the file's content");
    assert_eq!(changed[2], change == 2 || change == 3, "its stamp and the folder's listing");
    let fresh = change == 0 || change == 2 || change == 3;
    match planned.check(None) {
        Ok(()) => {
            assert!(fresh, "a stale plan passed");
            let report = planned.apply_json(None, &Ctx::silent()).unwrap();
            assert!(report.get("changes").is_some(), "{report}");
            assert!(Device::open_export(&root).unwrap().library().unwrap().find_playlist("Planned").is_some());
        }
        Err(e) => {
            assert!(!fresh, "{e}");
            assert_eq!(e.code(), "stale_plan");
            assert!(planned.apply_json(None, &Ctx::silent()).is_err(), "applying checks too");
        }
    }
    locallibrary::testing::volumes_on_this_thread(None);
}

/// A size reads back as about as many bytes, in the largest unit under a thousand.
#[hegel::test(test_cases = 300)]
fn sizes_read_as_a_person_would(tc: TestCase) {
    let n = tc.draw(hegel::one_of!(
        gs::integers::<u64>().max_value(2_000_000),
        gs::integers::<u64>().max_value(1 << 44),
        gs::integers::<u64>(),
        gs::sampled_from(&[999, 1000, 999_949, 999_950, 999_999, 1_000_000, 999_960_000][..]),
    ));
    let s = op::bytes(n);
    let (v, unit) = s.split_once(' ').unwrap();
    let v: f64 = v.parse().unwrap();
    let scale = match unit {
        "B" => 1.0,
        "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        other => panic!("{other}"),
    };
    assert!(unit == "TB" || v < 1000.0, "{s}");
    assert!(unit == "B" || v >= 1.0, "{s}");
    if unit == "B" {
        assert_eq!(v as u64, n);
    } else {
        assert!(((v * scale) - n as f64).abs() <= 0.05 * scale + 1.0, "{s} for {n}");
    }
}
