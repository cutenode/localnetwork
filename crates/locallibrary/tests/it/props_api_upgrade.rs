//! Upgrading a lossy track to its lossless file through the service layer, planned first or in
//! one go, keeping the old file or not.

use crate::common;
use crate::upgrade::{MP3, setup, store};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, UpgradeRequest};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::upgrade::UpgradeOptions;

/// The song as an MP3 on the drive and as a lossless WAV in a folder: the plan upgrades that
/// track, deleting the MP3 (after backing it up) only when asked, which makes it destructive;
/// applied, planned first or in one go, the track plays the WAV, and the MP3 is gone only when
/// asked. Planned again, there's nothing to upgrade.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(remove_old = true, in_one_go = true)]
#[hegel::explicit_test_case(remove_old = false, in_one_go = false)]
fn a_lossy_track_upgrades_to_its_lossless_file(tc: TestCase) {
    let remove_old = tc.draw(gs::booleans());
    let in_one_go = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let (root, lossless, song) = setup(dir.path());
    let _store = store(dir.path());
    let req = UpgradeRequest {
        drive: common::path(&root),
        options: UpgradeOptions { folders: vec![lossless], remove_old, ..Default::default() },
    };
    let plan = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    assert_eq!(plan.plan.upgrades.iter().map(|u| u.track.id).collect::<Vec<_>>(), [song]);
    assert!(plan.summary[0].starts_with("upgrade 1 track(s) on "), "{:?}", plan.summary);
    let deletes = plan.summary.iter().any(|l| l == "delete 1 lossy file(s), after backing them up");
    assert_eq!((deletes, plan.destructive), (remove_old, remove_old), "{:?}", plan.summary);
    match in_one_go {
        true => drop(api::upgrade_apply(&req, &Ctx::silent()).unwrap()),
        false => drop(plan.apply(None, &Ctx::silent()).unwrap()),
    }
    let lib = Device::open(&root).unwrap().library().unwrap();
    assert_eq!(lib.track(song).unwrap().file_path, "/Contents/Sound/Song.wav");
    assert_eq!(root.join(&MP3[1..]).exists(), !remove_old);
    let again = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    assert!(again.noop && again.plan.upgrades.is_empty(), "{:?}", again.summary);
}
