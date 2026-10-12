//! A track's analysis files when it's pointed at another file: each one that names the old file
//! is rewritten to name the new one (its times moved with the audio), whatever state the others
//! are in.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{anlz, paths};

/// An analysis file of no sections: one that names no audio file.
fn no_path() -> Vec<u8> {
    let mut f = b"PMAI".to_vec();
    f.extend(28u32.to_be_bytes());
    f.extend(28u32.to_be_bytes());
    f.extend([0; 16]);
    f
}

/// Pointing a track at a copy of its audio rewrites its `.DAT`, `.EXT` and `.2EX` to name the copy,
/// each drawn as it is, already naming the copy, not an analysis file, missing, a folder, or
/// naming no file. One already naming it, or naming none, is left; one that isn't an analysis file is left with a warning, or, when the
/// copy's times are later, refuses the edit (cues move everywhere or nowhere); a folder refuses it.
/// A refused edit leaves the drive as it was.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(dat = 0u8, ext = 1u8, ex2 = 2u8, shift = false)]
#[hegel::explicit_test_case(dat = 0u8, ext = 4u8, ex2 = 0u8, shift = false)]
#[hegel::explicit_test_case(dat = 2u8, ext = 0u8, ex2 = 3u8, shift = true)]
#[hegel::explicit_test_case(dat = 0u8, ext = 3u8, ex2 = 1u8, shift = true)]
#[hegel::explicit_test_case(dat = 5u8, ext = 0u8, ex2 = 5u8, shift = false)]
fn pointing_a_track_rewrites_the_analysis_files_that_name_its_old_file(tc: TestCase) {
    let dat = tc.draw(gs::integers::<u8>().max_value(5));
    let ext = tc.draw(gs::integers::<u8>().max_value(5));
    let ex2 = tc.draw(gs::integers::<u8>().max_value(5));
    let shift = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let t = common::library(&root).tracks[0].clone();
    let new_path = format!("/Contents/Moved/{}", t.file_path.rsplit('/').next().unwrap());
    let new_file = paths::to_fs(&root, &paths::key(&new_path));
    fs::create_dir_all(new_file.parent().unwrap()).unwrap();
    fs::copy(paths::to_fs(&root, &paths::key(&t.file_path)), &new_file).unwrap();
    let stem = paths::key(&t.analyze_path).strip_suffix(".DAT").unwrap().to_string();
    let modes = [("DAT", dat), ("EXT", ext), ("2EX", ex2)];
    for (suffix, mode) in modes {
        let p = paths::to_fs(&root, &format!("{stem}.{suffix}"));
        match mode {
            1 => fs::write(&p, anlz::rewrite_path(&fs::read(&p).unwrap(), &new_path).unwrap()).unwrap(),
            2 => fs::write(&p, b"not an analysis file").unwrap(),
            3 => fs::remove_file(&p).unwrap(),
            4 => {
                fs::remove_file(&p).unwrap();
                fs::create_dir_all(p.join("inside")).unwrap();
            }
            // An analysis file that names no audio file.
            5 => fs::write(&p, no_path()).unwrap(),
            _ => {}
        }
    }
    let before = common::Before::take(&root);
    let shift_ms = if shift { 26 } else { 0 };
    let e = Edit::SetPath {
        track: TrackSelector::paths([t.file_path.clone()]),
        path: new_path.clone(),
        file: None,
        shift_ms,
    };
    let mut d = Device::open_export(&root).unwrap();
    let plan = edit::plan(&d, &[e], &EditOptions::default(), &Ctx::silent()).unwrap();
    let result = edit::apply(&plan, &mut d, &Ctx::silent());
    let folder = modes.iter().any(|(_, m)| *m == 4);
    let garbage = modes.iter().filter(|(_, m)| *m == 2).count();
    tc.note(&format!("{modes:?} shift {shift}: {result:?}"));
    if folder || (shift && garbage > 0) {
        let err = result.unwrap_err().to_string();
        assert!(folder || err.contains("its cues can't be moved"), "{err}");
        before.untouched(&root).unwrap();
        return;
    }
    let report = result.unwrap();
    let unrecognised = report.warnings.iter().filter(|w| w.contains("unrecognised analysis format")).count();
    assert_eq!(unrecognised, garbage, "{:?}", report.warnings);
    for (suffix, mode) in modes {
        let key = format!("{stem}.{suffix}");
        let now = fs::read(paths::to_fs(&root, &key)).ok();
        let was = before.files.get(&key);
        match mode {
            0 | 1 => assert_eq!(
                anlz::read_path(now.as_deref().unwrap()).unwrap().as_deref(),
                Some(new_path.as_str()),
                "{key}"
            ),
            3 => assert_eq!(now, None, "{key}"),
            _ => {}
        }
        if matches!(mode, 1 | 2 | 5) {
            assert_eq!(now.as_ref(), was, "{key} left as it was");
        }
    }
}
