//! Relocating where one file would do for two tracks (it's neither's, and both are listed), where
//! a track's file belongs outside Contents/ (found on the drive it's repointed, found elsewhere
//! it's left for the DJ to copy), and where the drive's OneLibrary can't be read (its own files
//! aren't looked through).

use crate::common::{self, media};

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::relocate::{self, RelocateOptions};
use rekordbox_pdb::NewTrack;

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_file_two_tracks_would_take_is_neithers(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let secs = tc.draw(gs::integers::<u16>().min_value(1).max_value(3));
    let wav = media::wav_of(8_000, 16, 1, f64::from(secs));
    let add = |title: &str, path: &str| {
        let id = common::add_track(&root, NewTrack { duration: secs, ..NewTrack::new(title, path) }, &wav);
        fs::remove_file(paths::to_fs(&root, &paths::key(path))).unwrap();
        id
    };
    let (a, b) = (add("A", "/Contents/A/Song.wav"), add("B", "/Contents/B/Song.wav"));
    let outside = add("Out", "/Elsewhere/Other.wav");
    // Where the files are: loose on the drive, or in a folder on the computer.
    let on_drive = tc.draw(gs::booleans());
    let music = dir.path().join("Music");
    let at = if on_drive { root.join("Contents/Loose") } else { music.clone() };
    fs::create_dir_all(&at).unwrap();
    fs::create_dir_all(&music).unwrap();
    for name in ["Song.wav", "Other.wav"] {
        fs::write(at.join(name), &wav).unwrap();
    }
    let unreadable = tc.draw(gs::booleans());
    if unreadable {
        common::add_onelibrary(&root);
        fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
    }

    let d = Device::open_export(&root).unwrap();
    let plan = relocate::plan(&d, &RelocateOptions { folders: vec![music] }, &Ctx::silent()).unwrap();
    let said = |what: &str| plan.warnings.iter().any(|w| w.contains(what));
    assert_eq!(said("weren't looked through"), unreadable, "{:?}", plan.warnings);
    let found = !(on_drive && unreadable);
    let listed: Vec<u32> = plan.ambiguous.iter().map(|x| x.track.id).collect();
    let lost: Vec<u32> = plan.not_found.iter().map(|t| t.id).collect();
    for id in [a, b] {
        assert_eq!(listed.contains(&id), found, "#{id}: {plan:#?}");
        assert_eq!(lost.contains(&id), !found, "#{id}");
    }
    assert_eq!(said("could be the file of several tracks"), found);
    let repointed = plan.repointed.iter().any(|r| r.track.id == outside);
    assert_eq!(repointed, found && on_drive);
    assert_eq!(said("belongs outside Contents/"), found && !on_drive, "{:?}", plan.warnings);
    assert!(plan.copied.is_empty(), "nothing is copied: {:?}", plan.copied);
}
