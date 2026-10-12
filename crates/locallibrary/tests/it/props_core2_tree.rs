//! Scanning a drive's files: names stored decomposed (as macOS wrote them) are keyed composed and
//! still found on disk; extra keys are added once, as audio, only when they're regular files;
//! sizes add up; and a folder that can't be read stops the scan, naming it.

use std::collections::BTreeSet;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::paths::FileClass;
use locallibrary::progress::Ctx;
use locallibrary::tree::{self, ScanOptions};

#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn scans_key_files_as_the_library_names_them(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("USB");
    let decomposed = tc.draw(gs::booleans());
    let name = if decomposed { "Cafe\u{301}.mp3" } else { "Caf\u{e9}.mp3" };
    let files = [
        (format!("Contents/A/{name}"), 3usize),
        ("Contents/A/b.flac".to_string(), 5),
        ("PIONEER/rekordbox/export.pdb".to_string(), 7),
        ("Elsewhere/outside.mp3".to_string(), 11),
    ];
    let mut total = 0;
    for (rel, size) in &files {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, vec![1u8; *size]).unwrap();
        total += size;
    }
    let mut extra = vec!["/Contents/A/b.flac".to_string(), "/Elsewhere/missing.mp3".to_string()];
    let outside = tc.draw(gs::booleans());
    if outside {
        extra.push("/Elsewhere/outside.mp3".to_string());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("Elsewhere/outside.mp3"), root.join("Elsewhere/link.mp3")).unwrap();
        extra.push("/Elsewhere/link.mp3".to_string());
    }

    let t = tree::scan(&root, &ScanOptions { extra_keys: extra }, &Ctx::silent()).unwrap();
    let keys: BTreeSet<&str> = t.entries.keys().map(String::as_str).collect();
    let mut want = BTreeSet::from(["Contents/A/Caf\u{e9}.mp3", "Contents/A/b.flac", "PIONEER/rekordbox/export.pdb"]);
    if outside {
        want.insert("Elsewhere/outside.mp3");
    }
    assert_eq!(keys, want);
    if outside {
        assert_eq!(t.get("Elsewhere/outside.mp3").unwrap().class, FileClass::Audio);
    }
    let on_disk = t.fs_path("Contents/A/Caf\u{e9}.mp3");
    assert_eq!(fs::read(&on_disk).unwrap().len(), 3, "{}", on_disk.display());
    assert_eq!(t.total_bytes(), (total - if outside { 0 } else { 11 }) as u64);
}

#[cfg(unix)]
#[hegel::test(test_cases = 4)]
fn a_folder_that_cant_be_read_stops_the_scan(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("USB");
    let top = tc.draw(gs::sampled_from(vec!["Contents", "PIONEER"]));
    let locked = root.join(top).join("Locked");
    fs::create_dir_all(locked.join("inner")).unwrap();
    fs::write(locked.join("inner/x.mp3"), b"x").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let scanned = tree::scan(&root, &ScanOptions::default(), &Ctx::silent());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    let e = scanned.unwrap_err();
    assert_eq!(e.code(), "io", "{e}");
    assert!(e.to_string().contains("Locked"), "{e}");
}
