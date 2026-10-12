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
