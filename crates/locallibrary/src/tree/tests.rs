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

/// An entry recorded within moments of the file's mtime is racy: not looked up, and [`racy`]
/// says so while the file still shows that size and mtime; recorded clearly later, it isn't.
///
/// [`racy`]: HashIndex::racy
#[test]
fn entries_recorded_as_the_file_was_written_are_racy() {
    let now = chrono::Utc::now().timestamp();
    let entry = |mtime: i64| FileEntry {
        path: "Contents/a.mp3".into(),
        size: 5,
        mtime,
        class: FileClass::Audio,
        hash: Some(Hash::of(b"hello")),
    };
    // Racy entries' mtimes are ahead of now, so however slow the machine, they're recorded within
    // the window.
    for (mtime, racy) in [(now + 30, true), (now + 1, true), (now - 10, false), (1_600_000_000, false)] {
        let mut index = HashIndex::default();
        index.record(&entry(mtime));
        let seen = FileEntry { hash: None, ..entry(mtime) };
        assert_eq!(index.racy(&seen), racy, "mtime {} s from now", mtime - now);
        assert_eq!(index.lookup(&seen).is_none(), racy);
        assert!(!index.racy(&FileEntry { size: 6, ..seen.clone() }), "another size isn't the recorded file");
    }
}

/// What's saved answers lookups as the index recording every hash afresh did (only when entries
/// were recorded can differ), and recording what the cache holds already writes nothing.
#[hegel::test(test_cases = 100)]
fn the_hash_cache_is_written_only_when_its_answers_change(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("index.json");
    let now = chrono::Utc::now().timestamp();
    // Hashed now, files modified this long before are settled (see `fsutil::settled`); ones
    // modified this long after (negative) aren't, however long the test takes.
    let ages = [-1000, -5000, 1000, 5000];
    let entry = |path: usize, size: u64, age: usize, hash: u8| FileEntry {
        path: ["a", "b", "c"][path].to_string(),
        size,
        mtime: now - ages[age],
        class: FileClass::Analysis,
        hash: Some(Hash::of(&[hash])),
    };
    // What the index answered before (every hash recorded afresh): path -> (entry, settled).
    let mut model: FxHashMap<String, (FileEntry, bool)> = FxHashMap::default();
    // A cache saved earlier, its entries hashed at some time after the file changed.
    let mut index = HashIndex::default();
    for path in 0..3 {
        if tc.draw(gs::booleans()) {
            let mut e = entry(path, 0, tc.draw(gs::integers::<usize>().max_value(3)), 0);
            let after = tc.draw(gs::sampled_from(vec![0, 1, 3, 100]));
            let cached = IndexEntry { size: e.size, mtime: e.mtime, hash: e.hash.unwrap(), at: e.mtime + after };
            index.entries.insert(e.path.clone(), cached);
            e.mtime = cached.mtime;
            model.insert(e.path.clone(), (e, cached.settled()));
        }
    }
    index.dirty = true;
    index.save(&file).unwrap();
    let mut index = HashIndex::load(&file);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(30)) {
        let path = tc.draw(gs::integers::<usize>().max_value(2));
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                let size = tc.draw(gs::integers::<u64>().max_value(1));
                let age = tc.draw(gs::integers::<usize>().max_value(3));
                let e = entry(path, size, age, tc.draw(gs::integers::<u8>().max_value(1)));
                index.record(&e);
                model.insert(e.path.clone(), (e, ages[age] > 0));
            }
            1 => {
                index.forget(&entry(path, 0, 0, 0).path);
                model.remove(&entry(path, 0, 0, 0).path);
            }
            _ => {
                index.save(&file).unwrap();
                index = HashIndex::load(&file);
            }
        }
    }
    index.save(&file).unwrap();
    let saved = HashIndex::load(&file);
    let queries =
        (0..3).flat_map(|p| (0..2).flat_map(move |s| (0..4).flat_map(move |a| (0..2).map(move |h| (p, s, a, h)))));
    for (path, size, age, hash) in queries {
        let e = entry(path, size, age, hash);
        let expected = model.get(&e.path).filter(|(m, settled)| *settled && m.size == e.size && m.mtime == e.mtime);
        assert_eq!(saved.lookup(&e), expected.and_then(|(m, _)| m.hash), "{e:?}");
        assert_eq!(index.lookup(&e), saved.lookup(&e), "{e:?}");
    }
    // Hashing every file again, as a scan does for the database files, saves fresh answers;
    // doing it once more finds nothing new and writes nothing (the file, removed once loaded,
    // stays away).
    for pass in 0..2 {
        let mut again = HashIndex::load(&file);
        if pass == 1 {
            std::fs::remove_file(&file).unwrap();
        }
        for (m, _) in model.values() {
            again.forget(&m.path);
            again.record(m);
        }
        again.save(&file).unwrap();
        if pass == 0 {
            let saved = HashIndex::load(&file);
            for (m, _) in model.values() {
                assert_eq!(saved.lookup(m), m.hash.filter(|_| m.mtime < now), "{m:?}");
            }
        }
    }
    assert!(!file.exists(), "rewrote an unchanged cache");
}

/// The walk lists the managed files walkdir finds, in walkdir's order (depth first, each folder's
/// entries as it lists them), clutter and what's below a clutter folder left out.
#[hegel::test(test_cases = 50)]
fn the_walk_lists_what_walkdir_does(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let top = root.join("Contents");
    fs::create_dir_all(&top).unwrap();
    let folder = gs::sampled_from(vec!["a", "b", "c d", "Caf\u{e9}", ".Trashes"]);
    let file = gs::sampled_from(vec!["x.mp3", "y.flac", "notes.txt", "._x.mp3", ".DS_Store"]);
    let files: Vec<(Vec<&str>, &str)> = tc.draw(gs::vecs(gs::tuples!(gs::vecs(folder).max_size(5), file)).max_size(12));
    for (folders, name) in &files {
        let at = folders.iter().fold(top.clone(), |p, f| p.join(f));
        fs::create_dir_all(&at).unwrap();
        fs::write(at.join(name), name).unwrap();
    }
    let walked: Vec<PathBuf> = walk(root, &top, &Ctx::silent()).unwrap().into_iter().map(|(rel, ..)| rel).collect();
    let want: Vec<PathBuf> = walkdir::WalkDir::new(&top)
        .into_iter()
        .filter_entry(|e| e.file_name().to_str().is_none_or(|n| !paths::is_ignored_name(n)))
        .map(Result::unwrap)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(root).unwrap().to_path_buf())
        .filter(|rel| paths::classify(&paths::key(&rel.to_string_lossy())).is_some())
        .collect();
    assert_eq!(walked, want);
}

/// A drive whose folders nest as deep as its paths can go scans on threads with small stacks: the
/// walk doesn't recurse into each folder.
#[test]
fn deep_folders_scan_in_a_small_stack() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let deep = (0..300).fold(root.join("Contents"), |p, _| p.join("d"));
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("x.mp3"), "x").unwrap();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(2).stack_size(64 * 1024).build().unwrap();
    let tree = std::thread::scope(|s| {
        let scan = || pool.install(|| scan(root, &ScanOptions::default(), &Ctx::silent()));
        std::thread::Builder::new().stack_size(64 * 1024).spawn_scoped(s, scan).unwrap().join().unwrap()
    })
    .unwrap();
    let key = format!("Contents/{}x.mp3", "d/".repeat(300));
    assert_eq!(tree.entries.keys().collect::<Vec<_>>(), [&key]);
}
