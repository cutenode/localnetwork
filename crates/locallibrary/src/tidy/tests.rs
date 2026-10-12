use super::*;

#[test]
fn finds_and_removes_clutter_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let w = |p: &str, b: &[u8]| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b).unwrap();
    };
    let mut ad = APPLE_DOUBLE_MAGIC.to_vec();
    ad.extend_from_slice(&[0; 60]);
    w("Contents/a.mp3", b"audio");
    w("Contents/._a.mp3", &ad);
    w("Contents/._not-apple-double", b"real data");
    w(".DS_Store", b"x");
    w(".Spotlight-V100/Store-V2/index", &[0; 100]);
    w(".Trashes/501/old.mp3", &[0; 1000]);
    w(".locallibrary/._state", &ad);
    w("System Volume Information/IndexerVolumeGuid", b"g");
    let d = Device::open(root).unwrap();
    let ctx = Ctx::silent();

    let p = plan(&d, &TidyOptions::default(), &ctx).unwrap();
    let paths: Vec<&str> = p.items.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(paths, ["Contents/._a.mp3", ".DS_Store", ".Spotlight-V100"]);
    assert_eq!(p.trash_bytes, 1000);

    let with_trash = plan(&d, &TidyOptions { empty_trash: true }, &ctx).unwrap();
    assert_eq!(with_trash.items.len(), 4);

    let r = apply(&with_trash, &d, &ctx).unwrap();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert!(root.join("Contents/a.mp3").exists());
    assert!(root.join("Contents/._not-apple-double").exists());
    assert!(!root.join("Contents/._a.mp3").exists());
    assert!(!root.join(".Trashes").exists());
    assert!(root.join(".locallibrary/._state").exists());
    assert!(root.join("System Volume Information/IndexerVolumeGuid").exists());
    assert!(plan(&d, &TidyOptions { empty_trash: true }, &ctx).unwrap().is_noop());
}

#[test]
fn finds_what_cut_off_writes_left() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for p in ["Contents/A/.locallibrary-tmp-x1y2", "PIONEER/rekordbox/.locallibrary-tmp-ab", "Contents/A/a.mp3"] {
        std::fs::create_dir_all(root.join(p).parent().unwrap()).unwrap();
        std::fs::write(root.join(p), b"half a file").unwrap();
    }
    let d = Device::open(root).unwrap();
    let p = plan(&d, &TidyOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(p.by_kind(), [(TidyKind::TempFiles, 2, 22)]);
    remove_leftovers(&d).unwrap();
    assert!(plan(&d, &TidyOptions::default(), &Ctx::silent()).unwrap().is_noop());
    assert!(root.join("Contents/A/a.mp3").is_file());
}

/// A plan is checked again item by item: a path that leaves the drive is refused, and what
/// is no longer the clutter it was is left.
#[test]
fn applying_leaves_what_changed_since_and_refuses_odd_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    for p in ["Thumbs.db", "Contents/.DS_Store", "Contents/notes.txt"] {
        std::fs::write(root.join(p), b"x").unwrap();
    }
    let d = Device::open(root).unwrap();
    let mut p = plan(&d, &TidyOptions::default(), &Ctx::silent()).unwrap();
    let found: Vec<(&str, TidyKind)> = p.items.iter().map(|i| (i.path.as_str(), i.kind)).collect();
    assert_eq!(found, [("Contents/.DS_Store", TidyKind::DsStore), ("Thumbs.db", TidyKind::Thumbnails)]);
    // The .DS_Store is now a folder the DJ made.
    std::fs::remove_file(root.join("Contents/.DS_Store")).unwrap();
    std::fs::create_dir(root.join("Contents/.DS_Store")).unwrap();
    let odd =
        |path: &str| TidyItem { path: path.into(), kind: TidyKind::Thumbnails, is_dir: false, files: 1, bytes: 1 };
    p.items.extend([odd("../Thumbs.db"), odd("Contents//Thumbs.db"), odd("Contents\\Thumbs.db")]);

    let r = apply(&p, &d, &Ctx::silent()).unwrap();
    assert_eq!(r.removed, 1);
    let failed: Vec<(&str, &str)> = r.failed.iter().map(|f| (f.path.as_str(), f.error.as_str())).collect();
    assert_eq!(
        failed,
        [
            ("../Thumbs.db", "unexpected path"),
            ("Contents//Thumbs.db", "unexpected path"),
            ("Contents\\Thumbs.db", "unexpected path")
        ]
    );
    assert!(!root.join("Thumbs.db").exists() && root.join("Contents/.DS_Store").is_dir());
    assert!(root.join("Contents/notes.txt").is_file());
}
