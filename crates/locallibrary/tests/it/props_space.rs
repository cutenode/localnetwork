//! What uses a drive's space: drawn files of every kind around a library, each counted once in
//! its kind, every byte accounted for, and loose audio listed largest first.

use crate::{common, drawn};

use indexmap::IndexMap;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::space::{self, SpaceKind};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};

/// Files a drive may hold besides its library, and what they count as.
const EXTRAS: &[(&str, SpaceKind)] = &[
    ("PIONEER REC/REC001.WAV", SpaceKind::Recordings),
    ("ALPHATHETA REC/Set/REC002.WAV", SpaceKind::Recordings),
    (".Trashes/501/old.mp3", SpaceKind::Trash),
    ("$RECYCLE.BIN/S-1-5/x.mp3", SpaceKind::Trash),
    (".DS_Store", SpaceKind::OsClutter),
    ("Contents/._Track.mp3", SpaceKind::OsClutter),
    (".Spotlight-V100/Store-V2/store.db", SpaceKind::OsClutter),
    ("Music/.fseventsd/uuid", SpaceKind::OsClutter),
    ("Music/loose.mp3", SpaceKind::LooseAudio),
    ("Loose/Track.FLAC", SpaceKind::LooseAudio),
    ("notes.txt", SpaceKind::Other),
    ("System Volume Information/IndexerVolumeGuid", SpaceKind::Other),
    ("PIONEER/MYSETTING.DAT", SpaceKind::Settings),
    ("PIONEER/DeviceLibBackup/rbDevLibBaInfo_1.json", SpaceKind::RekordboxOther),
    (".locallibrary/backups/1/export.pdb", SpaceKind::LocallibraryBackups),
    (".locallibrary/notes.json", SpaceKind::Locallibrary),
    ("PIONEER/Artwork/00001/a5.jpg", SpaceKind::Artwork),
];

/// The kind a library file of the fixture drives is.
fn library_kind(key: &str) -> SpaceKind {
    match key.split('/').take(2).collect::<Vec<_>>()[..] {
        ["Contents", ..] => SpaceKind::Audio,
        ["PIONEER", "USBANLZ"] => SpaceKind::Analysis,
        ["PIONEER", "rekordbox"] => SpaceKind::Databases,
        // The twelve-track export names some analysis files under `.PIONEER/`, which isn't
        // rekordbox's folder.
        _ => SpaceKind::Other,
    }
}

#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn every_byte_is_counted_once_in_its_kind(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = drawn::drive(&tc, dir.path(), "A");
    let mut want: IndexMap<SpaceKind, (usize, u64)> = IndexMap::new();
    for (key, bytes) in common::drive_files(&root) {
        let slot = want.entry(library_kind(&key)).or_default();
        slot.0 += 1;
        slot.1 += bytes.len() as u64;
    }
    let mut loose = Vec::new();
    for (key, kind) in drawn::some(&tc, EXTRAS) {
        let size = tc.draw(gs::integers::<usize>().max_value(3000));
        let p = root.join(key);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, vec![7u8; size]).unwrap();
        let slot = want.entry(kind).or_default();
        slot.0 += 1;
        slot.1 += size as u64;
        if kind == SpaceKind::LooseAudio {
            loose.push(size as u64);
        }
    }

    let r = space::breakdown(&Device::open(&root).unwrap(), &Ctx::silent()).unwrap();
    let got: IndexMap<SpaceKind, (usize, u64)> = r.categories.iter().map(|c| (c.kind, (c.files, c.bytes))).collect();
    let mut a: Vec<_> = got.iter().collect();
    let mut b: Vec<_> = want.iter().collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
    for (kind, (_, bytes)) in &want {
        assert_eq!(r.bytes(*kind), *bytes);
    }
    let sizes: Vec<u64> = r.categories.iter().map(|c| c.bytes).collect();
    assert!(sizes.is_sorted_by(|x, y| x >= y), "largest first: {sizes:?}");
    loose.sort_unstable_by(|x, y| y.cmp(x));
    assert_eq!(r.loose_audio.iter().map(|f| f.size).collect::<Vec<_>>(), loose);
    assert_eq!(r.unreadable, 0);
}

/// The library fits on another drive of its set exactly when its size and the players' headroom
/// fit in the room that drive had, and no forecast is made from one day's snapshots.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn fitting_on_the_set_goes_by_each_drives_room(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    main.adopt(Some("Main")).unwrap();
    sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap();
    let copy = dir.path().join("Copy");
    common::copy_dir(main.root(), &copy);
    let mut copy = Device::open(&copy).unwrap();
    locallibrary::fleet::reidentify(Some(&store), &mut copy, Some("Backup")).unwrap();
    let capacity = store.drives().unwrap()[&copy.id().unwrap()].capacity.unwrap();
    let room = capacity / 100 * 97;
    let bytes = tc.draw(gs::integers::<u64>().max_value(room.saturating_sub(space::PLAYER_HEADROOM) * 2));
    let fits = space::fits_on_set(&store, &main, bytes, std::slice::from_ref(&copy)).unwrap();
    assert_eq!(fits.len(), 1, "only the other drive");
    assert_eq!((fits[0].name.as_str(), fits[0].mounted), ("Backup", true));
    assert_eq!(fits[0].fits, bytes + space::PLAYER_HEADROOM <= room);
    assert!(space::forecast(&store, &main, Some(1 << 40)).unwrap().is_none());
}
