//! Scrubbing and repairing drives with damage drawn into them: flash rot (a byte flipped, size and
//! modification time kept) and files gone. A scrub finds exactly the files damaged, finds them
//! again until they're repaired, and a repair from the store or a sibling drive gives every byte
//! back.

use crate::{common, drawn};

use std::collections::BTreeSet;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::paths::{self, FileClass};
use locallibrary::progress::Ctx;
use locallibrary::scrub::{self, RepairSource, ScrubOptions, ScrubReport};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, SyncOptions};

fn folded<'a>(paths: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    paths.into_iter().map(str::to_lowercase).collect()
}

fn scrub_with(d: &Device, store: Option<&Store>, siblings: &[Device]) -> ScrubReport {
    scrub::scrub(d, store, siblings, &ScrubOptions::default(), &Ctx::silent()).unwrap()
}

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn scrub_finds_exactly_the_damage_and_repair_undoes_it(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let (root, lib) = drawn::drive(&tc, dir.path(), "Main");
    // Files older than FAT's racy window, so the hash cache vouches for them.
    let old = filetime::FileTime::from_unix_time(filetime::FileTime::now().unix_seconds() - 120, 0);
    for e in walkdir::WalkDir::new(&root).into_iter().map(Result::unwrap).filter(|e| e.file_type().is_file()) {
        filetime::set_file_mtime(e.path(), old).unwrap();
    }
    let mut main = Device::open_export(&root).unwrap();
    main.adopt(Some("Main")).unwrap();
    sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap();
    // From the store, or (without one) from a mirror of the drive.
    let from_store = tc.draw(gs::booleans());
    let siblings = match from_store {
        true => Vec::new(),
        false => {
            let mut mirror = Device::open(common::blank(dir.path(), "Mirror")).unwrap();
            sync::sync_drive_to_drive(&mut main, &mut mirror, &SyncOptions::default(), &Ctx::silent()).unwrap();
            vec![mirror]
        }
    };
    let store_used = from_store.then_some(&store);
    let clean = scrub_with(&main, store_used, &siblings);
    assert!(clean.is_clean(), "{clean:?}");
    let intact = common::library_files(&root);

    // Rot some audio and analysis files, and take some away.
    let rottable: Vec<String> = intact
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| matches!(paths::classify(k), Some(FileClass::Audio | FileClass::Analysis)))
        .collect();
    // Without a store nothing says what a file that's gone held, so only rot is drawn there.
    let deletable = if from_store { rottable.clone() } else { Vec::new() };
    let (rotted, gone) = drawn::damage(&tc, &root, &rottable, &deletable);

    let r = scrub_with(&main, store_used, &siblings);
    assert_eq!(folded(r.damaged.iter().map(|p| p.path.as_str())), folded(rotted.iter().map(String::as_str)), "{r:#?}");
    assert_eq!(folded(r.missing.iter().map(|p| p.path.as_str())), folded(gone.iter().map(String::as_str)), "{r:#?}");
    assert!(r.unreadable.is_empty());
    assert_eq!(r.is_clean(), rotted.is_empty() && gone.is_empty());
    assert_eq!(r.verified + r.unverified + r.damaged.len(), r.files, "{r:#?}");
    for p in r.problems() {
        let want = if from_store {
            RepairSource::Store
        } else {
            RepairSource::Drive { name: "Mirror".into(), root: siblings[0].root().to_path_buf() }
        };
        assert_eq!(p.repair.as_ref(), Some(&want), "{p:?}");
        assert!(p.expected.is_some() && p.expected != p.actual);
    }
    // The tracks named are those whose audio or analysis is hit, with their playlists.
    let (tracks, playlists) = scrub::affected(&lib, r.problems().map(|p| p.path.as_str()));
    assert_eq!(r.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), tracks.iter().map(|t| t.id).collect::<Vec<_>>());
    assert_eq!(r.playlists, playlists);
    for k in rotted.iter().chain(&gone).filter(|k| paths::classify(k) == Some(FileClass::Audio)) {
        assert!(r.tracks.iter().any(|t| paths::key(&t.file_path).eq_ignore_ascii_case(k)), "{k} names its track");
    }

    // A second scrub doesn't take the damage for the new normal.
    let again = scrub_with(&main, store_used, &siblings);
    assert_eq!(
        folded(again.damaged.iter().map(|p| p.path.as_str())),
        folded(r.damaged.iter().map(|p| p.path.as_str()))
    );

    let problems: Vec<_> = r.problems().cloned().collect();
    let rep = scrub::repair(&mut main, store_used, &siblings, &problems, &Ctx::silent()).unwrap();
    assert!(rep.failed.is_empty(), "{rep:?}");
    assert_eq!(rep.repaired.len(), problems.len(), "{rep:?}");
    assert_eq!(common::library_files(&root), intact, "every byte is back");
    assert!(scrub_with(&main, store_used, &siblings).is_clean());

    // Repairing what's intact again writes nothing.
    let rep = scrub::repair(&mut main, store_used, &siblings, &problems, &Ctx::silent()).unwrap();
    assert!(rep.repaired.is_empty());
    assert_eq!(rep.skipped.len(), problems.len());
}
