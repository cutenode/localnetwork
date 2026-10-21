//! More faults for the check before a set: audio cut short, artwork gone, a file too big for
//! FAT32, a database that can't be read, a drive holding only OneLibrary, history lists with no
//! store to keep them, and a second drive that went its own way or holds another library. Each
//! fault put in is named, at its level.

use crate::common::{self, media};
use crate::drawn;

use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::check::{self, CheckOptions, CheckReport, Level};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{paths, pdb};
use rekordbox_pdb::NewTrack;

fn said(r: &CheckReport, drive: &str, check: &str, level: Level, what: &str) -> bool {
    r.items.iter().any(|i| i.drive == drive && i.check == check && i.level == level && i.message.contains(what))
}

/// A 30-second WAV track, its file cut to two fifths as an interrupted copy leaves it.
fn cut_track(root: &Path) {
    let file = "/Contents/Cut/Short.wav";
    let whole = media::wav_of(8_000, 16, 1, 30.0);
    let track = NewTrack { duration: 30, ..NewTrack::new("Short", file) };
    common::add_track(root, track, &whole);
    fs::write(paths::to_fs(root, &paths::key(file)), &whole[..whole.len() * 2 / 5]).unwrap();
}

/// A track whose audio is 4 GiB (sparse: nothing is written), more than FAT32 holds.
fn big_track(root: &Path) {
    let file = "/Contents/Big/Long Mix.wav";
    common::add_track(root, NewTrack::new("Long Mix", file), b"RIFF");
    let f = fs::OpenOptions::new().write(true).open(paths::to_fs(root, &paths::key(file))).unwrap();
    f.set_len(4 << 30).unwrap();
}

#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn each_fault_put_in_is_named(tc: TestCase) {
    for variant in 0..4 {
        each_fault_put_in_is_named_with(&tc, variant);
    }
}

/// One case of [`each_fault_put_in_is_named`], `variant` choosing what's drawn.
#[hegel::test_helper]
fn each_fault_put_in_is_named_with(tc: &TestCase, variant: u8) {
    let dir = tempfile::tempdir().unwrap();
    let (a, _) = drawn::drive(tc, dir.path(), "A");
    common::add_history(&a, "HISTORY 001", &[1]);
    // Which of the drive's databases can be read, and what's wrong with its files.
    let databases = variant;
    let (cut, art_gone, big) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::weighted_booleans(0.3)));
    if cut {
        cut_track(&a);
    }
    if art_gone {
        media::set_art(&a, &[1]);
        let art =
            pdb::read_library(&a.join(paths::EXPORT_PDB)).unwrap().track(1).unwrap().artwork_path.clone().unwrap();
        fs::remove_file(paths::to_fs(&a, &paths::key(&art))).unwrap();
    }
    if big {
        big_track(&a);
    }
    match databases {
        0 => {}
        1 => common::onelibrary_only(&a),
        2 => {
            common::add_onelibrary(&a);
            fs::write(a.join(paths::LIBRARY_PLUS_DB), b"not a database at all").unwrap();
        }
        _ => fs::write(a.join(paths::EXPORT_PDB), vec![7u8; 8192]).unwrap(),
    }
    // A store that keeps the history lists, or none. (Never with the 4 GiB file, which pulling
    // would copy into the store.)
    let store = tc.draw(gs::booleans()).then(|| Store::open_or_init(dir.path().join("store")).unwrap());
    let pulled = store.is_some() && !big && databases != 3;
    if let (Some(store), true) = (&store, pulled) {
        sync::pull(store, &mut Device::open_export(&a).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
    }
    // A second drive: a copy that then changed as A did (both moved on), or another library.
    let second = match databases {
        0 => tc.draw(gs::optional(gs::integers::<u8>().max_value(1))),
        _ => None,
    };
    let mut devices = vec![Device::open(&a).unwrap()];
    if let Some(kind) = second {
        let b = dir.path().join("B");
        match kind {
            0 => {
                common::copy_dir(&a, &b);
                let _ = fs::remove_dir_all(b.join(paths::STATE_DIR));
                common::export_new_track(&b, "New", "On B");
                common::export_new_track(&a, "New", "On A");
            }
            _ => drop(common::demo_drive(dir.path(), "B")),
        }
        devices.push(Device::open(&b).unwrap());
    }
    let r = check::check(&devices, store.as_ref(), &CheckOptions::default(), &Ctx::silent()).unwrap();
    let notes = format!("{:#?}", r.items);

    match databases {
        3 => {
            assert!(said(&r, "A", "export", Level::Problem, "the library can't be read"), "{notes}");
            return;
        }
        2 => assert!(said(&r, "A", "databases", Level::Problem, "OneLibrary can't be read"), "{notes}"),
        1 => assert!(said(&r, "A", "databases", Level::Info, "OneLibrary only"), "{notes}"),
        _ => {}
    }
    assert_eq!(said(&r, "A", "files", Level::Problem, "stop short of their length"), cut, "{notes}");
    assert_eq!(said(&r, "A", "files", Level::Warning, "have no artwork image"), art_gone, "{notes}");
    assert_eq!(said(&r, "A", "large_files", Level::Warning, "won't fit on a FAT32 drive"), big, "{notes}");
    assert_eq!(said(&r, "A", "history", Level::Warning, "aren't in the store"), !pulled, "{notes}");
    match second {
        Some(0) => assert!(said(&r, "B", "same_library", Level::Problem, "have both changed"), "{notes}"),
        Some(_) => assert!(said(&r, "B", "same_library", Level::Problem, "holds a different library"), "{notes}"),
        None => {}
    }
}
