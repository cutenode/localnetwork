use super::*;

/// An analysis file copied for audio that starts later (pack's copy of a decoded lossy file)
/// records the new path, has its beat grid that much later, and is the size planned for it.
#[test]
fn an_analysis_file_gets_its_new_path_and_times() {
    let dat = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/rekordcrate/demo_tracks/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT");
    let audio = "/Contents/Artist/a longer name than before.aiff";
    let src = Source::analysis(dat.clone(), 0, audio, 14).unwrap();
    let written = analysis_bytes(&src.path, audio, src.shift_ms).unwrap();
    assert_eq!(src.size, written.len() as u64);
    assert_eq!(anlz::read_path(&written).unwrap().as_deref(), Some(audio));
    let times = |b: &[u8]| anlz::beat_grid(b).unwrap().iter().map(|b| b.time_ms).collect::<Vec<_>>();
    let before = times(&std::fs::read(&dat).unwrap());
    assert!(!before.is_empty());
    assert_eq!(times(&written), before.iter().map(|t| t + 14).collect::<Vec<_>>());
}

/// The demo export's analysis file, which a write copies with a new audio path.
fn demo_dat() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/rekordcrate/demo_tracks/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT")
}

/// Every file below `root` but locallibrary's own, by key, with its bytes.
fn files_on(root: &Path) -> IndexMap<String, Vec<u8>> {
    let mut out: IndexMap<String, Vec<u8>> = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"))
        .filter(|k| !k.starts_with(paths::STATE_DIR))
        .map(|k| (k.clone(), std::fs::read(root.join(&k)).unwrap()))
        .collect();
    out.sort_unstable_keys();
    out
}

fn put(root: &Path, key: &str, bytes: &[u8], mtime: i64) {
    let p = paths::to_fs(root, key);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, bytes).unwrap();
    fsutil::set_mtime(&p, mtime).unwrap();
}

/// Writing a drawn library over a drive's: the plan copies exactly the wanted files whose copy on
/// the drive differs (in size or time), the copies land with their sources' bytes and times, and
/// the delete leaves the library files the write doesn't want gone unless they're kept (never the
/// drive's settings). A doomed file something else removed first isn't counted; one turned into
/// a folder stops the delete with an error.
#[hegel::test(test_cases = 30)]
fn a_write_leaves_the_drive_holding_the_library_it_wants(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let (root, src) = (dir.path().join("drive"), dir.path().join("src"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&src).unwrap();
    let keys = ["Contents/a.mp3", "Contents/b.mp3", "Contents/c.mp3", "PIONEER/Artwork/00001/a1.jpg"];
    // What the drive holds now: some of the keys, and its settings.
    let mut on_drive: Vec<&str> = Vec::new();
    for k in keys {
        if tc.draw(gs::booleans()) {
            put(&root, k, k.as_bytes(), 1_700_000_000);
            on_drive.push(k);
        }
    }
    put(&root, "PIONEER/DEVSETTING.DAT", b"settings", 1_700_000_000);
    // What the library wants: some of the keys, each the drive's copy or new content.
    let mut wanted = IndexMap::new();
    for (i, k) in keys.iter().enumerate() {
        if !tc.draw(gs::booleans()) {
            continue;
        }
        let same = on_drive.contains(k) && tc.draw(gs::booleans());
        let bytes = if same { k.as_bytes().to_vec() } else { tc.draw(gs::binary().max_size(40)) };
        let mtime = if same { 1_700_000_000 } else { 1_600_000_000 + i as i64 };
        let path = src.join(format!("{i}"));
        std::fs::write(&path, &bytes).unwrap();
        fsutil::set_mtime(&path, mtime).unwrap();
        wanted.insert(k.to_string(), Source::file(path, bytes.len() as u64, mtime));
    }
    let keep_extra = tc.draw(gs::booleans());
    let databases = Databases {
        pdb: tc.draw(gs::booleans()).then(|| b"pdb".to_vec()),
        ext: tc.draw(gs::booleans()).then(|| b"ext".to_vec()),
        plus: None,
    };
    let target = Device::open(&root).unwrap();
    let ctx = Ctx::silent();
    let tree = crate::tree::scan(target.root(), &crate::tree::ScanOptions::default(), &ctx).unwrap();
    let planned = plan(&target, &tree, &wanted, &[], databases.written(), keep_extra);

    let differs = |k: &str, s: &Source| {
        let p = target.path(k);
        std::fs::read(&p).ok() != std::fs::read(&s.path).ok()
            || !fsutil::mtime_eq(fsutil::mtime_secs(&std::fs::metadata(&p).unwrap()), s.mtime)
    };
    let to_copy: Vec<&String> = wanted.iter().filter(|(k, s)| differs(k, s)).map(|(k, _)| k).collect();
    assert_eq!(planned.copies.iter().map(|c| &c.path).collect::<Vec<_>>(), to_copy);
    assert_eq!(planned.unchanged, wanted.len() - to_copy.len());
    let doomed: Vec<&str> = on_drive.iter().copied().filter(|k| !keep_extra && !wanted.contains_key(*k)).collect();
    assert_eq!(planned.deletes.iter().map(|d| d.path.as_str()).collect::<Vec<_>>(), doomed);

    let (n, bytes) = copy(&target, &planned, &ctx).unwrap();
    assert_eq!((n, bytes), (to_copy.len(), planned.copy_bytes));
    for (k, s) in &wanted {
        let p = target.path(k);
        assert_eq!(std::fs::read(&p).unwrap(), std::fs::read(&s.path).unwrap(), "{k}");
        assert!(fsutil::mtime_eq(fsutil::mtime_secs(&std::fs::metadata(&p).unwrap()), s.mtime), "{k}");
    }

    // Something else gets at a doomed file first: it's gone already, or a folder now.
    let meddled = if doomed.is_empty() { 0 } else { tc.draw(gs::integers::<u8>().max_value(2)) };
    let first = doomed.first().map(|k| target.path(k));
    match (meddled, &first) {
        (1, Some(p)) => std::fs::remove_file(p).unwrap(),
        (2, Some(p)) => {
            std::fs::remove_file(p).unwrap();
            std::fs::create_dir_all(p.join("inside")).unwrap();
        }
        _ => {}
    }
    let deleted = delete(&target, &planned, &ctx);
    if meddled == 2 {
        assert!(matches!(deleted, Err(Error::Io { .. })), "{deleted:?}");
        return;
    }
    let (gone, _) = deleted.unwrap();
    assert_eq!(gone, doomed.len() - usize::from(meddled == 1));
    let after = files_on(&root);
    let mut want: Vec<&str> = wanted.keys().map(String::as_str).collect();
    if keep_extra {
        want.extend(on_drive.iter().filter(|k| !wanted.contains_key(**k)));
    }
    want.push("PIONEER/DEVSETTING.DAT");
    want.sort_unstable();
    assert_eq!(after.keys().map(String::as_str).collect::<Vec<_>>(), want);
}

/// Whichever way a run of copies goes (any size of file, the small ones at once), each comes out
/// once, and an error stops the run: nothing starts after it among the large files, and the first
/// error is the one given back.
#[hegel::test(test_cases = 50)]
fn every_copy_runs_once_until_one_fails(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let sizes: Vec<u64> =
        tc.draw(gs::vecs(gs::sampled_from(vec![0, 1, SMALL_FILE, SMALL_FILE + 1, 1 << 30])).max_size(40));
    let fails_at = tc.draw(gs::optional(gs::integers::<usize>().max_value(40)));
    let items: Vec<(usize, u64)> = sizes.iter().copied().enumerate().collect();
    let tried = std::sync::Mutex::new(Vec::new());
    let (done, stopped) = copy_each(
        &items,
        |i| i.1,
        |&(n, _)| {
            tried.lock().unwrap().push(n);
            match Some(n) == fails_at {
                true => Err(Error::invalid(format!("copy {n}"))),
                false => Ok(n),
            }
        },
    );
    let mut tried = tried.into_inner().unwrap();
    tried.sort_unstable();
    assert!(tried.windows(2).all(|w| w[0] != w[1]), "one copied twice: {tried:?}");
    match fails_at.filter(|n| *n < items.len()) {
        None => {
            assert!(stopped.is_none());
            let mut done = done;
            done.sort_unstable();
            assert_eq!(done, (0..items.len()).collect::<Vec<_>>());
        }
        Some(n) => {
            assert!(matches!(&stopped, Some(Error::Invalid(m)) if *m == format!("copy {n}")), "{stopped:?}");
            assert!(!done.contains(&n));
            let is_small = |i: usize| items[i].1 <= SMALL_FILE;
            if !is_small(n) {
                let last_large = tried.iter().rev().find(|i| !is_small(**i));
                assert_eq!(last_large, Some(&n), "large copies are made in order, and stop at the failure");
            }
            assert!(tried.iter().all(|i| is_small(n) == is_small(*i) || is_small(*i)), "large copies follow the small");
        }
    }
}

/// Downloading ahead reads what it's given and changes nothing, stops once the copy has finished,
/// and passes over a file that's gone.
#[hegel::test(test_cases = 10)]
fn downloading_ahead_leaves_every_file_as_it_was(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let count = tc.draw(gs::integers::<usize>().max_value(6));
    let finished = AtomicBool::new(tc.draw(gs::booleans()));
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> = (0..count).map(|i| dir.path().join(i.to_string())).collect();
    for (i, f) in files.iter().enumerate() {
        if i != 0 {
            std::fs::write(f, i.to_string()).unwrap();
        }
    }
    download_ahead(&files.iter().map(PathBuf::as_path).collect::<Vec<_>>(), &finished);
    for (i, f) in files.iter().enumerate().skip(1) {
        assert_eq!(std::fs::read_to_string(f).unwrap(), i.to_string());
    }
}

/// A failure while another copy is part way through it: that copy finishes and is kept, and the
/// audio that would have followed doesn't start.
#[test]
fn a_copy_under_way_when_another_fails_finishes() {
    let (failed, under_way) = std::sync::mpsc::channel();
    let (failed, under_way) = (std::sync::Mutex::new(failed), std::sync::Mutex::new(under_way));
    let items = [(0, 1), (1, 1), (2, SMALL_FILE + 1)];
    let (done, stopped) = copy_each(
        &items,
        |i| i.1,
        |&(n, _)| match n {
            0 => {
                let wait = std::time::Duration::from_secs(30);
                under_way.lock().unwrap().recv_timeout(wait).expect("the other copy fails");
                Ok(0)
            }
            1 => {
                failed.lock().unwrap().send(()).unwrap();
                Err(Error::invalid("copy 1"))
            }
            n => Ok(n),
        },
    );
    assert_eq!(done, [0]);
    assert!(matches!(&stopped, Some(Error::Invalid(m)) if m == "copy 1"), "{stopped:?}");
}

/// Online-only files come down side by side: here each "download" (a read of a FIFO) waits for a
/// writer, and those only turn up last file first, so reading them in turn would never get past
/// the first.
#[cfg(unix)]
#[test]
fn downloads_overlap_instead_of_waiting_their_turn() {
    use std::fs::File;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> = (0..DOWNLOADS_AT_ONCE).map(|i| dir.path().join(i.to_string())).collect();
    for f in &files {
        let name = std::ffi::CString::new(f.as_os_str().as_bytes()).unwrap();
        // SAFETY: `name` is a C string for the length of the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    }
    let paths: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    let finished = AtomicBool::new(false);
    let give_up = std::time::Instant::now() + std::time::Duration::from_secs(5);
    // Opening a FIFO to write without blocking fails until something is reading it.
    let reached = |f: &PathBuf| {
        while std::time::Instant::now() < give_up {
            if let Ok(mut fifo) = File::options().write(true).custom_flags(libc::O_NONBLOCK).open(f) {
                return std::io::Write::write_all(&mut fifo, b"x").is_ok();
            }
            std::thread::yield_now();
        }
        false
    };
    let overlapped = std::thread::scope(|s| {
        s.spawn(|| download_ahead(&paths, &finished));
        let overlapped = files.iter().rev().all(reached);
        finished.store(true, Ordering::Relaxed);
        // Whatever's still reading, let it go.
        for f in &files {
            let fifo = File::options().read(true).write(true).custom_flags(libc::O_NONBLOCK).open(f).unwrap();
            let _ = std::io::Write::write_all(&mut &fifo, b"x");
        }
        overlapped
    });
    assert!(overlapped, "each waited for the one before it");
}

/// A copy that fails part way (its source gone, not planned, an unsafe path, a folder where its
/// own should go, an analysis file that isn't one, or stopped) reports the error, and the files
/// copied before it are in place with their sources' times, so running it again copies only the
/// rest. The one that failed leaves nothing, and any after it that were under way are whole too.
#[hegel::test(test_cases = 30)]
#[hegel::explicit_test_case(at = 0usize, failure = 0u8, analysis = false)]
#[hegel::explicit_test_case(at = 1usize, failure = 1u8, analysis = true)]
#[hegel::explicit_test_case(at = 2usize, failure = 2u8, analysis = false)]
#[hegel::explicit_test_case(at = 0usize, failure = 3u8, analysis = true)]
#[hegel::explicit_test_case(at = 1usize, failure = 4u8, analysis = false)]
#[hegel::explicit_test_case(at = 2usize, failure = 5u8, analysis = false)]
#[hegel::explicit_test_case(at = 0usize, failure = 6u8, analysis = false)]
fn a_copy_cut_short_keeps_what_it_copied(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let at = tc.draw(gs::integers::<usize>().max_value(2));
    let failure = tc.draw(gs::integers::<u8>().max_value(6));
    let analysis = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let (root, src) = (dir.path().join("drive"), dir.path().join("src"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&src).unwrap();
    let mut planned = Planned::default();
    for i in 0..3 {
        let key = format!("Contents/{i}.mp3");
        let path = src.join(format!("{i}.mp3"));
        std::fs::write(&path, format!("audio {i}")).unwrap();
        planned.copies.push(FileCopy { path: key.clone(), size: 7 });
        planned.sources.insert(key, Source::file(path, 7, 1_600_000_000 + i));
    }
    // The first is an analysis file whose times move, at times.
    if analysis && failure != 3 {
        let key = planned.copies[0].path.clone();
        let s = planned.sources.get_mut(&key).unwrap();
        std::fs::copy(demo_dat(), &s.path).unwrap();
        *s = Source::analysis(s.path.clone(), s.mtime, "/Contents/0.mp3", 14).unwrap();
        planned.copies[0].size = s.size;
    }
    let cancel = crate::progress::CancelToken::new();
    let (key, src_path) = {
        let key = planned.copies[at].path.clone();
        (key.clone(), planned.sources[&key].path.clone())
    };
    match failure {
        0 => std::fs::remove_file(&src_path).unwrap(),
        1 => {
            planned.sources.shift_remove(&key);
        }
        2 => {
            let s = planned.sources.shift_remove(&key).unwrap();
            planned.copies[at].path = "../outside.mp3".into();
            planned.sources.insert("../outside.mp3".into(), s);
        }
        3 => {
            // A file where the copy's folder goes (an analysis file's, at times).
            let mut s = planned.sources.shift_remove(&key).unwrap();
            if analysis {
                std::fs::copy(demo_dat(), &s.path).unwrap();
                s.audio_path = Some("/Contents/x.mp3".into());
            }
            put(&root, "Contents/sub", b"x", 0);
            planned.copies[at].path = "Contents/sub/a.mp3".into();
            planned.sources.insert("Contents/sub/a.mp3".into(), s);
        }
        4 | 5 => {
            // An analysis file that isn't one, or one that's gone.
            let s = planned.sources.get_mut(&key).unwrap();
            match failure {
                4 => std::fs::write(&s.path, b"not an analysis file").unwrap(),
                _ => std::fs::remove_file(&s.path).unwrap(),
            }
            s.audio_path = Some("/Contents/x.mp3".into());
        }
        // Stopped before it starts.
        _ => cancel.cancel(),
    }
    let at = if failure == 6 { 0 } else { at };
    let target = Device::open(&root).unwrap();
    let ctx = Ctx::new(&crate::progress::NoProgress, cancel.clone());
    let err = copy(&target, &planned, &ctx).unwrap_err();
    tc.note(&format!("{err}"));
    match failure {
        4 => assert!(matches!(&err, Error::Parse { path, .. } if *path == src_path), "{err:?}"),
        6 => assert!(matches!(err, Error::Cancelled), "{err}"),
        _ => {}
    }
    for (i, c) in planned.copies.iter().enumerate() {
        let p = target.path(&c.path);
        if i == at || failure == 6 {
            assert!(!p.is_file(), "{} copied", c.path);
            continue;
        }
        if i > at && !p.is_file() {
            continue;
        }
        let s = &planned.sources[&c.path];
        let want = match &s.audio_path {
            Some(audio) => analysis_bytes(&s.path, audio, s.shift_ms).unwrap(),
            None => std::fs::read(&s.path).unwrap(),
        };
        assert_eq!(std::fs::read(&p).unwrap(), want);
        assert_eq!(fsutil::mtime_secs(&std::fs::metadata(&p).unwrap()), s.mtime);
    }
}

/// The databases swapped in are exactly the ones given: each written one holds its bytes, a
/// missing `export.pdb` or OneLibrary database is removed (and OneLibrary's WAL and SHM always
/// are), and a missing `exportExt.pdb` is left as it was.
#[hegel::test(test_cases = 40)]
fn the_databases_swapped_in_are_the_ones_given(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let keys = [
        paths::EXPORT_PDB,
        paths::EXPORT_EXT_PDB,
        paths::LIBRARY_PLUS_DB,
        paths::LIBRARY_PLUS_WAL,
        paths::LIBRARY_PLUS_SHM,
    ];
    let mut before = FxHashMap::default();
    for k in keys {
        if tc.draw(gs::booleans()) {
            put(dir.path(), k, k.as_bytes(), 0);
            before.insert(k, k.as_bytes().to_vec());
        }
    }
    let db = || tc.draw(gs::optional(gs::binary().max_size(16)));
    let databases = Databases { pdb: db(), ext: db(), plus: db() };
    let target = Device::open(dir.path()).unwrap();
    swap_databases(&target, &databases).unwrap();
    let read = |k: &str| std::fs::read(target.path(k)).ok();
    assert_eq!(read(paths::EXPORT_PDB), databases.pdb);
    assert_eq!(
        read(paths::EXPORT_EXT_PDB),
        databases.ext.clone().or_else(|| before.get(paths::EXPORT_EXT_PDB).cloned())
    );
    assert_eq!(read(paths::LIBRARY_PLUS_DB), databases.plus);
    assert_eq!((read(paths::LIBRARY_PLUS_WAL), read(paths::LIBRARY_PLUS_SHM)), (None, None));
    // The new databases are the files the drive keeps besides the library's.
    let written = databases.written().keys();
    assert_eq!(written.contains(&fold(paths::EXPORT_EXT_PDB)), databases.ext.is_some());
    assert_eq!(written.contains(&fold(paths::LIBRARY_PLUS_WAL)), databases.plus.is_some());
}
