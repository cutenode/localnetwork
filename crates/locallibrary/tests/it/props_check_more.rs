//! More faults for the check before a set: audio cut short, artwork gone, a file too big for
//! FAT32, a database that can't be read, a drive holding only OneLibrary, history lists with no
//! store to keep them, and a second drive that went its own way or holds another library. Each
//! fault put in is named, at its level, with what fixes it when one does.

use crate::common::{self, media};
use crate::drawn;

use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::check::{self, CheckOptions, CheckReport, Level};
use locallibrary::device::Device;
use locallibrary::plugin::Fix;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{paths, pdb};
use loclib_plugin::{DriveAction, HostAction};
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
    locallibrary::testing::sparse(&paths::to_fs(root, &paths::key(file)), 4 << 30).unwrap();
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
    // Of these, only history lists the store lacks have a fix: pulling the drive. No plugin finds
    // cover art here, and drives that both changed or hold another library can't be synced.
    for i in &r.items {
        let pull = Fix::Host(HostAction::Pull(DriveAction { drive: i.drive.clone() }));
        let fixed = i.check == "history" && i.level == Level::Warning;
        assert_eq!(i.fix.as_ref(), fixed.then_some(&pull), "{notes}");
    }
}

/// A WAV track whose row leaves out its sample rate and bit depth is judged by the file's: each
/// player given that reads export.pdb holds it against the track exactly when it doesn't play
/// that rate and depth, once, with the rate and depth read from the file. One whose file is gone
/// isn't held to any.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_formats_a_row_leaves_out_are_read_from_the_file(tc: TestCase) {
    use locallibrary::equipment::{Codec, MODELS};
    let players: Vec<&str> = MODELS.iter().filter(|m| !m.is_mixer() && m.export_pdb.value).map(|m| m.name).collect();
    let player = tc.draw(gs::sampled_from(players));
    let rate = tc.draw(gs::sampled_from(vec![44_100u32, 48_000, 96_000, 192_000]));
    let depth = tc.draw(gs::sampled_from(vec![16u16, 24]));
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let file = "/Contents/Rates/Unsaid.wav";
    let row = NewTrack { sample_rate: 0, sample_depth: 0, ..NewTrack::new("Unsaid", file) };
    let id = common::add_track(&root, row, &media::wav_of(rate, depth, 1, 0.05));
    // A row that leaves them out too, whose file is gone: nothing to read them from, so they
    // aren't held against it.
    let gone = "/Contents/Rates/Gone.wav";
    let gone_id = common::add_track(&root, NewTrack { sample_rate: 0, ..NewTrack::new("Gone", gone) }, b"RIFF");
    fs::remove_file(paths::to_fs(&root, &paths::key(gone))).unwrap();
    let opts = CheckOptions { players: vec![player.to_string()], ..Default::default() };
    let r = check::check(&[Device::open(&root).unwrap()], None, &opts, &Ctx::silent()).unwrap();
    let held: Vec<_> = r.unloadable.iter().filter(|u| u.tracks.iter().any(|t| t.id == id)).collect();
    assert!(r.unloadable.iter().all(|u| u.tracks.iter().all(|t| t.id != gone_id)), "{:#?}", r.unloadable);
    let model = locallibrary::equipment::find(player).unwrap();
    match model.plays(Codec::Wav, rate, depth).result {
        Ok(()) => assert!(held.is_empty(), "{held:#?}"),
        Err(reason) => {
            assert_eq!(held.len(), 1, "{held:#?}");
            assert_eq!((held[0].player.as_str(), held[0].reason, held[0].format.as_str()), (player, reason, "WAV"));
            assert!(held[0].sample_rate.is_none_or(|r| r == rate) && held[0].bit_depth.is_none_or(|d| d == depth));
        }
    }
}

/// History lists pulled into the store count as kept while the drive is as it was then, and
/// after it changes too, list by list: only a list added since isn't in the store.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_history_lists_pulled_before_the_drive_changed_are_kept(tc: TestCase) {
    let played = tc.draw(gs::integers::<u32>().min_value(1).max_value(12));
    for change in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let a = common::bigger_drive(dir.path(), "A");
        common::add_history(&a, "HISTORY 001", &[played]);
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        sync::pull(&store, &mut Device::open_export(&a).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
        match change {
            0 => {}
            1 => drop(common::export_new_track(&a, "New", "Since")),
            _ => common::add_history(&a, "HISTORY 002", &[played]),
        }
        let devices = [Device::open(&a).unwrap()];
        let r = check::check(&devices, Some(&store), &CheckOptions::default(), &Ctx::silent()).unwrap();
        let history: Vec<_> = r.items.iter().filter(|i| i.check == "history").collect();
        assert_eq!(history.len(), 1, "{history:#?}");
        let (level, message) = (history[0].level, history[0].message.as_str());
        match change {
            0 => {
                assert!(level == Level::Info && message.starts_with("1 history list(s) saved in snapshot"), "{message}")
            }
            1 => assert_eq!((level, message), (Level::Info, "1 history list(s), all in the store")),
            _ => {
                let kept = message.starts_with("1 of 2 history list(s) aren't in the store");
                assert!(level == Level::Warning && kept, "{message}");
            }
        }
    }
}
