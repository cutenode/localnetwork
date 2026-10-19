//! Tracks told apart by their sound: drawn lossy tracks (WAVs named `.mp3`, as in
//! `upgrade.rs`) and a folder of lossless files, some of the same recordings (one or two copies)
//! and some of others as long. Exactly the tracks whose recording is in the folder are upgraded,
//! and the doctor puts tracks of one recording in one group.

use crate::common;
use crate::upgrade::recording;

use indexmap::IndexMap;
use std::collections::BTreeSet;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, UpgradeRequest};
use locallibrary::check;
use locallibrary::device::Device;
use locallibrary::doctor::{self, DoctorOptions, DuplicateReason, Section};
use locallibrary::progress::Ctx;
use locallibrary::sound;
use locallibrary::store::Store;
use locallibrary::upgrade::UpgradeOptions;
use locallibrary::{paths, pdb, testing};
use rekordbox_pdb::NewTrack;

/// Recordings by seed: each as long as the others, so only their sound tells them apart.
const SEEDS: [u32; 3] = [7919, 3 * 7919, 101];
const SECS: f64 = 30.0;

#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn only_the_same_recording_is_found(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    let root = common::bigger_drive(dir.path(), "Main");
    // The drive's lossy tracks: one per recording drawn, perhaps two of one.
    let mut tracks: IndexMap<u32, u32> = IndexMap::new();
    let mut n = 0;
    for (i, seed) in SEEDS.iter().enumerate() {
        for copy in 0..tc.draw(gs::integers::<u32>().max_value(if i == 0 { 2 } else { 1 })) {
            let path = format!("/Contents/Sound/Song {i}-{copy}.mp3");
            let track = NewTrack {
                artist: Some(format!("Artist {n}")),
                duration: SECS as u16,
                bitrate: 320,
                sample_rate: 44_100,
                key: tc.draw(gs::optional(gs::sampled_from(vec!["8A", "9A", "8B", "3A", "C"]))).map(String::from),
                ..NewTrack::new(format!("Song {n}"), path.clone())
            };
            tracks.insert(common::add_track(&root, track, &recording(*seed, SECS, 44_100, 16, 0)), *seed);
            n += 1;
        }
    }
    // The folder: lossless files of some of them.
    let lossless = dir.path().join("Lossless");
    fs::create_dir_all(&lossless).unwrap();
    let mut found: BTreeSet<u32> = BTreeSet::new();
    for (i, seed) in SEEDS.iter().enumerate() {
        if tc.draw(gs::booleans()) {
            let rate = tc.draw(gs::sampled_from(vec![44_100u32, 48_000]));
            fs::write(lossless.join(format!("Lossless {i}.wav")), recording(*seed, SECS, rate, 24, 0)).unwrap();
            // At times a second copy of it: either may be taken, but one of them is.
            if tc.draw(gs::booleans()) {
                fs::write(lossless.join(format!("Again {i}.wav")), recording(*seed, SECS, 44_100, 16, 0)).unwrap();
            }
            found.insert(*seed);
        }
    }

    // The doctor groups tracks of one recording.
    let d = Device::open_export(&root).unwrap();
    let r = doctor::run(&d, &DoctorOptions { by_sound: true, sure_only: true, ..Default::default() }, &Ctx::silent())
        .unwrap();
    let groups = r.duplicates.unwrap();
    for g in &groups {
        let seeds: BTreeSet<Option<&u32>> = g.tracks.iter().map(|t| tracks.get(&t.track.id)).collect();
        if g.reasons.contains(&DuplicateReason::SameRecording) {
            assert_eq!(seeds.len(), 1, "one recording in a group: {g:?}");
        }
    }
    let twice = tracks.values().filter(|s| **s == SEEDS[0]).count() > 1;
    assert_eq!(groups.iter().any(|g| g.reasons.contains(&DuplicateReason::SameRecording)), twice);

    // Analysed, every track is either measured or fails to decode (the placeholder audio); asked
    // again, the cache answers. The check before a set then has their levels.
    let analyse = DoctorOptions { only: vec![Section::Analysis], ..Default::default() };
    let first = doctor::run(&d, &analyse, &Ctx::silent()).unwrap().analysis.unwrap();
    let all = d.library().unwrap().tracks.len();
    assert_eq!(first.analyzed + first.failed.len(), all);
    assert_eq!(first.analyzed, tracks.len(), "the synthetic recordings decode");
    assert_eq!(first.decoded, first.analyzed);
    let again = doctor::run(&d, &analyse, &Ctx::silent()).unwrap().analysis.unwrap();
    assert_eq!((again.analyzed, again.decoded), (first.analyzed, 0), "from the cache");
    // How sure each stored key is, from those cached analyses alone: a track is doubted exactly
    // when it has a key and an analysis whose estimate disagrees with it, by how far.
    let lib = d.library().unwrap();
    let cached = sound::Cache::in_store(&store).cached(&d, lib.tracks.iter());
    let doubted = sound::key_confidences(&d, lib.tracks.iter());
    for t in &lib.tracks {
        let estimate = cached.get(&t.id).and_then(|a| a.estimate.as_ref());
        let want = estimate.and_then(|e| sound::key_confidence(&t.key, e));
        assert_eq!(doubted.get(&t.id).copied(), want, "{} {:?}", t.key, estimate.map(|e| e.key));
        assert!(estimate.is_some() || !doubted.contains_key(&t.id));
    }
    let checked =
        check::check(&[Device::open_export(&root).unwrap()], Some(&store), &Default::default(), &Ctx::silent())
            .unwrap();
    assert_eq!(checked.items.iter().any(|i| i.check == "loudness"), !tracks.is_empty());

    let req = UpgradeRequest {
        drive: common::path(&root),
        options: UpgradeOptions { folders: vec![lossless.clone()], ..Default::default() },
    };
    let plan = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    let upgraded: BTreeSet<u32> = plan.upgrades.iter().map(|u| u.track.id).collect();
    let refused: BTreeSet<u32> = plan.not_upgraded.iter().map(|u| u.track.id).collect();
    let want: BTreeSet<u32> = tracks.iter().filter(|(_, s)| found.contains(s)).map(|(id, _)| *id).collect();
    assert_eq!(upgraded.union(&refused).copied().collect::<BTreeSet<_>>(), want, "{:?}", plan.not_upgraded);
    assert_eq!(plan.unmatched, tracks.len() - want.len());
    if plan.upgrades.is_empty() {
        testing::store_root_on_this_thread(None);
        return;
    }
    plan.apply(None, &Ctx::silent()).unwrap();
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    for u in &plan.upgrades {
        let t = lib.track(u.track.id).unwrap();
        assert_eq!(paths::key(&t.file_path), u.to);
        assert_eq!(fs::read(paths::to_fs(&root, &u.to)).unwrap(), fs::read(&u.file).unwrap(), "copied whole");
    }
    // Upgraded, nothing is left to upgrade.
    let again = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    assert!(again.upgrades.is_empty());
    testing::store_root_on_this_thread(None);
}
