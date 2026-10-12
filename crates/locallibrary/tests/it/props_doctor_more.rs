//! The doctor on drives the main property doesn't draw: a OneLibrary that can't be read (only
//! export.pdb is checked, and orphans can't be told), an operation left interrupted (orphans
//! aren't removed until it's finished), a database path that leaves the drive, artwork the
//! databases use beside images nothing does, and the audio itself read to its end.

use crate::common::{self, media};
use crate::drawn;

use std::collections::BTreeSet;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::{Device, LineageOp};
use locallibrary::doctor::{self, DoctorOptions, Problem};
use locallibrary::error::Reason;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use rekordbox_pdb::NewTrack;

#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn the_doctor_says_what_it_couldnt_check_and_holds_back_what_it_shouldnt_do(tc: TestCase) {
    for variant in 0..3 {
        the_doctor_says_what_it_couldnt_check_and_holds_back_what_it_shouldnt_do_with(&tc, variant);
    }
}

/// One case of [`the_doctor_says_what_it_couldnt_check_and_holds_back_what_it_shouldnt_do`], `variant` choosing what's drawn.
#[hegel::test_helper]
fn the_doctor_says_what_it_couldnt_check_and_holds_back_what_it_shouldnt_do_with(tc: &TestCase, variant: u8) {
    let dir = tempfile::tempdir().unwrap();
    let (root, lib) = drawn::drive(tc, dir.path(), "A");
    // Artwork the databases use, and an image nothing does.
    let art = tc.draw(gs::booleans());
    if art {
        media::set_art(&root, &[lib.tracks[0].id]);
    }
    let stray = "PIONEER/Artwork/00009/a999.jpg";
    fs::create_dir_all(paths::to_fs(&root, "PIONEER/Artwork/00009")).unwrap();
    fs::write(paths::to_fs(&root, stray), b"jpeg").unwrap();
    // A row whose path leaves the drive.
    let unsafe_path = tc.draw(gs::booleans());
    if unsafe_path {
        common::add_track(&root, NewTrack::new("Escaped", "/../outside.mp3"), b"ID3");
    }
    let plus = variant;
    match plus {
        0 => {}
        1 => common::add_onelibrary(&root),
        _ => {
            common::add_onelibrary(&root);
            fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
        }
    }
    let interrupted = tc.draw(gs::booleans());
    if interrupted {
        Device::open(&root).unwrap().begin(LineageOp::Sync, "sync from B", None).unwrap();
    }
    let audio = tc.draw(gs::booleans());
    let d = Device::open_export(&root).unwrap();
    let r = doctor::run(&d, &DoctorOptions { audio, ..Default::default() }, &Ctx::silent()).unwrap();
    let says = |what: &str| r.warnings.iter().any(|w| w.contains(what));
    assert_eq!(says("only export.pdb was checked"), plus == 2, "{:?}", r.warnings);
    assert_eq!(says("orphaned files weren't looked for"), plus == 2, "{:?}", r.warnings);
    assert_eq!(says("an interrupted operation may have left files"), interrupted && plus != 2, "{:?}", r.warnings);
    let missing = r.missing.as_ref().unwrap();
    assert_eq!(missing.iter().any(|m| m.problem == Problem::UnsafePath), unsafe_path, "{missing:?}");
    if plus != 2 {
        let orphans: BTreeSet<String> = r.orphans.as_ref().unwrap().iter().map(|o| o.path.to_lowercase()).collect();
        assert_eq!(orphans, BTreeSet::from([stray.to_lowercase()]), "the images the databases use aren't orphans");
    }
    assert_eq!(r.audio.is_some(), audio);
    if let Some(a) = &r.audio {
        assert!(a.checked > 0, "{a:?}");
    }

    // Removing orphans: refused while something's interrupted, or without OneLibrary to say what's
    // used; otherwise it takes the stray image only, and not while it's gone to the databases.
    let mut d = Device::open_export(&root).unwrap();
    match doctor::plan_orphans(&d, &Ctx::silent()) {
        Err(e) if interrupted => assert_eq!(e.reason(), Some(Reason::Interrupted), "{e}"),
        Err(e) => {
            assert_eq!(plus, 2, "{e}");
            assert!(e.to_string().contains("which files are orphaned can't be known"), "{e}");
        }
        Ok(plan) => {
            assert!(!interrupted && plus != 2);
            let mut full = plan.clone();
            full.fits = false;
            assert!(matches!(
                doctor::apply_orphans(&full, &mut d, &Ctx::silent()),
                Err(locallibrary::Error::NoSpace { .. })
            ));
            // A track takes the stray image's path as its audio: the plan is out of date.
            if tc.draw(gs::booleans()) {
                common::add_track(&root, NewTrack::new("Claims it", format!("/{stray}")), b"jpeg");
                let err = doctor::apply_orphans(&plan, &mut Device::open_export(&root).unwrap(), &Ctx::silent());
                assert!(err.is_err_and(|e| matches!(e, locallibrary::Error::StalePlan(_))));
                return;
            }
            let done = doctor::apply_orphans(&plan, &mut d, &Ctx::silent()).unwrap();
            assert_eq!(done.files_deleted, 1);
            assert!(!paths::to_fs(&root, stray).exists());
        }
    }
}
