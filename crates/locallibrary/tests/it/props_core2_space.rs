//! Where a drive's space goes, with a folder that can't be read (counted as unreadable, wherever
//! it is, the rest measured), a link (not followed or counted) and what players keep beside the
//! library (rekordbox's other files).

#![cfg(unix)]

use crate::common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::space::{self, SpaceKind};

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn space_is_measured_around_what_cant_be_read(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::blank(dir.path(), "USB");
    let files: &[(&str, usize, SpaceKind)] = &[
        ("Contents/A/a.mp3", 10, SpaceKind::Audio),
        ("PIONEER/CDJ/x.dat", 20, SpaceKind::RekordboxOther),
        ("Music/loose.mp3", 40, SpaceKind::LooseAudio),
        (".Trashes/501/old.mp3", 80, SpaceKind::Trash),
    ];
    for (rel, size, _) in files {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![0u8; *size]).unwrap();
    }
    std::os::unix::fs::symlink(root.join("Music/loose.mp3"), root.join("Music/link.mp3")).unwrap();
    // A folder that can't be read: among the files, or in the trash (measured whole).
    let at = tc.draw(gs::sampled_from(vec!["Junk/Locked", ".Trashes/Locked", "Contents/Locked"]));
    let locked = root.join(at);
    fs::create_dir_all(locked.join("inner")).unwrap();
    fs::write(locked.join("inner/hidden.mp3"), vec![0u8; 160]).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let r = space::breakdown(&Device::open(&root).unwrap(), &Ctx::silent());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    let r = r.unwrap();
    assert_eq!(r.unreadable, 1, "{r:?}");
    for (_, size, kind) in files {
        assert_eq!(r.bytes(*kind), *size as u64, "{kind:?}: {:?}", r.categories);
    }
    assert_eq!(r.categories.iter().map(|c| c.files).sum::<usize>(), files.len(), "the link isn't counted");
    assert_eq!(r.loose_audio.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["Music/loose.mp3"]);
}

/// A folder that can be listed but not looked into (readable, not searchable): each file in it
/// counts as unreadable, among the files or in the trash (measured whole), and the rest is
/// measured. A cancelled measure stops.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_files_that_cant_be_looked_at_are_unreadable(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::blank(dir.path(), "USB");
    let at = tc.draw(gs::sampled_from(vec!["Junk/Listed", ".Trashes/Listed"]));
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let listed = root.join(at);
    fs::create_dir_all(&listed).unwrap();
    for i in 0..n {
        fs::write(listed.join(format!("{i}.mp3")), vec![0u8; 10]).unwrap();
    }
    fs::create_dir_all(root.join("Music")).unwrap();
    fs::write(root.join("Music/loose.mp3"), vec![0u8; 40]).unwrap();
    fs::set_permissions(&listed, fs::Permissions::from_mode(0o444)).unwrap();

    let device = Device::open(&root).unwrap();
    let r = space::breakdown(&device, &Ctx::silent());
    let cancel = locallibrary::progress::CancelToken::new();
    cancel.cancel();
    let cancelled = space::breakdown(&device, &Ctx::new(&locallibrary::progress::NoProgress, cancel));
    fs::set_permissions(&listed, fs::Permissions::from_mode(0o755)).unwrap();
    let r = r.unwrap();
    assert_eq!(r.unreadable, n, "{r:?}");
    assert_eq!(r.bytes(SpaceKind::LooseAudio), 40);
    assert_eq!(r.bytes(SpaceKind::Trash), 0, "nothing in the trash could be measured");
    assert!(matches!(cancelled, Err(locallibrary::Error::Cancelled)), "{cancelled:?}");
}
