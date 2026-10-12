//! Where rekordbox's timeline starts in an MP3, told by its waveform: the cues and grid of a track
//! upgraded to a lossless file move by the offset between the files' audio, and a frame (1,152
//! samples) more when rekordbox times the MP3 from a frame early.

use crate::common;
use crate::upgrade::{DELAY_MS, append, setup, signal, store, waveform};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, UpgradeRequest};
use locallibrary::device::Device;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::upgrade::UpgradeOptions;

#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn cues_move_by_where_rekordboxs_timeline_starts(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, lossless, song) = setup(dir.path());
    let _store = store(dir.path());
    let lead = tc.draw(gs::sampled_from(vec![0usize, 1152]));
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let dat = paths::to_fs(&root, &paths::key(&lib.track(song).unwrap().analyze_path));
    append(&dat.with_extension("EXT"), &waveform(&signal(7919, 40.0, 44_100, DELAY_MS), lead));
    let options = UpgradeOptions { folders: vec![lossless], ..Default::default() };
    let plan = api::upgrade_plan(&UpgradeRequest { drive: common::path(&root), options }, &Ctx::silent()).unwrap();
    let frame_ms = if lead == 1152 { 26 } else { 0 };
    assert_eq!(plan.upgrades[0].shift_ms, -DELAY_MS - frame_ms, "timeline {lead} samples early");
}
