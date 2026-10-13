use super::*;
use std::fs;

#[test]
fn scan_classifies_and_ignores_clutter() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (p, body) in [
        ("PIONEER/rekordbox/export.pdb", "db"),
        ("PIONEER/MYSETTING.DAT", "s"),
        ("PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT", "a"),
        ("Contents/A/x.mp3", "audio"),
        ("Contents/A/._x.mp3", "appledouble"),
        ("Contents/.DS_Store", "x"),
        ("Music/other.mp3", "unmanaged"),
    ] {
        let path = root.join(p);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    let tree = scan(root, &ScanOptions::default(), &Ctx::silent()).unwrap();
    let keys: Vec<&str> = tree.entries.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "Contents/A/x.mp3",
            "PIONEER/MYSETTING.DAT",
            "PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT",
            "PIONEER/rekordbox/export.pdb"
        ]
    );
    assert_eq!(tree.get("PIONEER/MYSETTING.DAT").unwrap().class, FileClass::Settings);
}

/// Audio the database keeps outside `Contents/` is scanned when asked for, once, and only as
/// a regular file on the drive.
#[test]
fn audio_outside_contents_is_scanned_when_asked_for() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (p, body) in [("Contents/A/x.mp3", "audio"), ("Music/set.mp3", "more audio")] {
        let path = root.join(p);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    // A link to audio on the drive is still not a file on it (on Windows there's no link).
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("Music/set.mp3"), root.join("Music/link.mp3")).unwrap();
    let extra_keys =
        ["Music/set.mp3", "/Contents/A/x.mp3", "Music/gone.mp3", "Music/link.mp3"].map(String::from).into();
    let tree = scan(root, &ScanOptions { extra_keys }, &Ctx::silent()).unwrap();
    let keys: Vec<&str> = tree.entries.keys().map(String::as_str).collect();
    assert_eq!(keys, ["Contents/A/x.mp3", "Music/set.mp3"]);
    assert_eq!(tree.get("Music/set.mp3").unwrap().class, FileClass::Audio);
    assert_eq!(tree.total_bytes(), 15);
    assert_eq!(tree.fs_path("Music/set.mp3"), root.join("Music/set.mp3"));
}

#[test]
fn hash_index_avoids_rehashing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("Contents")).unwrap();
    fs::write(root.join("Contents/a.mp3"), b"hello").unwrap();
    // Old enough that the cache entry isn't "racy".
    filetime::set_file_mtime(root.join("Contents/a.mp3"), filetime::FileTime::from_unix_time(1_600_000_000, 0))
        .unwrap();
    let mut index = HashIndex::default();
    let mut tree = scan(root, &ScanOptions::default(), &Ctx::silent()).unwrap();
    tree.ensure_hashes(&mut index, |_| true, &Ctx::silent()).unwrap();
    let h = tree.get("Contents/a.mp3").unwrap().hash.unwrap();
    assert_eq!(h, Hash::of(b"hello"));

    // A rescan reuses the cached hash; a size change invalidates it.
    let mut tree2 = scan(root, &ScanOptions::default(), &Ctx::silent()).unwrap();
    let entry = tree2.entries.get_mut("Contents/a.mp3").unwrap();
    assert_eq!(index.lookup(entry), Some(h));
    entry.size += 1;
    assert_eq!(index.lookup(entry), None);
}

/// Links are left out, and a folder that can't be read stops the scan with an error naming
/// it, as cancelling stops it.
#[cfg(unix)]
#[test]
fn scan_skips_links_and_stops_at_an_unreadable_folder() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for p in ["Contents/A/x.mp3", "Contents/B/y.mp3", "PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT"] {
        fs::create_dir_all(root.join(p).parent().unwrap()).unwrap();
        fs::write(root.join(p), p).unwrap();
    }
    symlink(root.join("Contents/A"), root.join("Contents/L")).unwrap();
    symlink(root.join("Contents/A/x.mp3"), root.join("Contents/A/z.mp3")).unwrap();
    let tree = scan(root, &ScanOptions::default(), &Ctx::silent()).unwrap();
    let keys: Vec<&str> = tree.entries.keys().map(String::as_str).collect();
    assert_eq!(keys, ["Contents/A/x.mp3", "Contents/B/y.mp3", "PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT"]);

    let locked = root.join("Contents/B");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let err = scan(root, &ScanOptions::default(), &Ctx::silent()).unwrap_err();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(err, Error::Io { path, .. } if path == locked));

    let cancel = crate::progress::CancelToken::new();
    cancel.cancel();
    let err = scan(root, &ScanOptions::default(), &Ctx::new(&crate::progress::NoProgress, cancel)).unwrap_err();
    assert!(matches!(err, Error::Cancelled));
}
