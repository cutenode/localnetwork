//! Upgrading against a folder with more in it than good lossless files: a file that won't decode
//! is reported and skipped, an `.m4a` that isn't Apple Lossless isn't a candidate, and a match the
//! drive's players can't play is listed with why rather than used.

use crate::common;
use crate::upgrade::{recording, store};

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, UpgradeRequest};
use locallibrary::progress::Ctx;
use locallibrary::upgrade::UpgradeOptions;
use rekordbox_pdb::NewTrack;

/// Seconds of audio: enough to tell recordings apart and line them up, little to decode.
const SECS: f64 = 15.0;

#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(hi_res = true, broken = true, odd_m4a = true)]
fn only_good_lossless_files_a_player_takes_are_used(tc: TestCase) {
    let hi_res = tc.draw(gs::booleans());
    let broken = tc.draw(gs::booleans());
    let odd_m4a = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let _store = store(dir.path());
    // The twelve-track drive, plus an "MP3" (a WAV named so) whose lossless file is in the folder.
    let root = common::bigger_drive(dir.path(), "Main");
    let track = NewTrack {
        artist: Some("Someone".into()),
        duration: SECS as u16,
        bitrate: 320,
        sample_rate: 44_100,
        ..NewTrack::new("Song", "/Contents/Sound/Song.mp3")
    };
    let song = common::add_track(&root, track, &recording(7919, SECS, 44_100, 16, 26));
    let lossless = dir.path().join("Lossless");
    fs::create_dir_all(lossless.join("Album")).unwrap();
    let rate = if hi_res { 96_000 } else { 48_000 };
    fs::write(lossless.join("Album/Song.wav"), recording(7919, SECS, rate, 24, 0)).unwrap();
    if broken {
        fs::write(lossless.join("Album/Broken.flac"), b"fLaC not really").unwrap();
    }
    if odd_m4a {
        fs::write(lossless.join("Album/Odd.m4a"), b"not an mpeg-4 file").unwrap();
    }
    let options =
        UpgradeOptions { folders: vec![lossless.clone()], players: vec!["CDJ-2000NXS".into()], ..Default::default() };
    let plan = api::upgrade_plan(&UpgradeRequest { drive: common::path(&root), options }, &Ctx::silent()).unwrap();
    let undecoded: Vec<&String> = plan.warnings.iter().filter(|w| w.contains("couldn't be decoded")).collect();
    match broken {
        true => {
            let [w] = undecoded.as_slice() else { panic!("{:?}", plan.warnings) };
            assert!(w.starts_with("1 file(s) couldn't be decoded") && w.contains("Broken.flac"), "{w}");
        }
        false => assert!(undecoded.is_empty(), "{:?}", plan.warnings),
    }
    assert!(
        plan.warnings.iter().all(|w| !w.contains("Odd.m4a")),
        "not lossless, so never decoded: {:?}",
        plan.warnings
    );
    assert_eq!(plan.unmatched, 0, "the drive's other tracks don't decode, so they're never compared");
    match hi_res {
        true => {
            assert!(plan.upgrades.is_empty());
            let [n] = plan.not_upgraded.as_slice() else { panic!("{:?}", plan.not_upgraded) };
            assert_eq!(
                (n.track.id, n.why.as_str()),
                (song, "the CDJ-2000NXS doesn't play it (WAV at 96000 Hz, 24 bit)")
            );
            assert!(n.file.ends_with("Album/Song.wav"));
        }
        false => {
            let [u] = plan.upgrades.as_slice() else { panic!("{:?}", plan.not_upgraded) };
            assert_eq!((u.track.id, u.to.as_str()), (song, "Contents/Sound/Song.wav"));
        }
    }
}
