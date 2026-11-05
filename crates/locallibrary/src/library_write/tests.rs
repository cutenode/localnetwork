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

/// A copy that fails part way (its source gone, not planned, an unsafe path, a folder where its
/// own should go, an analysis file that isn't one, or stopped) reports the error, and the files
/// copied before it are in place with their sources' times, so running it again copies only the
/// rest.
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
    for c in &planned.copies[..at] {
        let p = target.path(&c.path);
        let s = &planned.sources[&c.path];
        let want = match &s.audio_path {
            Some(audio) => analysis_bytes(&s.path, audio, s.shift_ms).unwrap(),
            None => std::fs::read(&s.path).unwrap(),
        };
        assert_eq!(std::fs::read(&p).unwrap(), want);
        assert_eq!(fsutil::mtime_secs(&std::fs::metadata(&p).unwrap()), s.mtime);
    }
    for c in &planned.copies[at..] {
        assert!(!target.path(&c.path).is_file(), "{} copied", c.path);
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

/// Of the library files a write doesn't want, only those it writes itself go: audio, analysis,
/// artwork and the databases it doesn't make. Any other file under `PIONEER/`, the drive's
/// settings and what OneLibrary keeps beside its database stay, and the delete leaves them.
#[hegel::test(test_cases = 30)]
fn prop_only_the_files_a_library_write_makes_are_removed(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let ext = tc.draw(gs::booleans());
    // Each file, and whether a write that wants none of them removes it.
    let files = [
        ("Contents/a.mp3", true),
        ("PIONEER/USBANLZ/P001/0000/ANLZ0000.DAT", true),
        ("PIONEER/Artwork/00001/a1.jpg", true),
        (paths::EXPORT_EXT_PDB, !ext),
        ("PIONEER/Other/notes.bin", false),
        ("PIONEER/DEVSETTING.DAT", false),
        (paths::LIBRARY_PLUS_SHM, false),
    ];
    let on_drive: Vec<(&str, bool)> = files.into_iter().filter(|_| tc.draw(gs::booleans())).collect();
    for (k, _) in &on_drive {
        put(dir.path(), k, k.as_bytes(), 1_700_000_000);
    }
    let target = Device::open(dir.path()).unwrap();
    let ctx = Ctx::silent();
    let tree = crate::tree::scan(target.root(), &crate::tree::ScanOptions::default(), &ctx).unwrap();
    let databases = Databases { ext: ext.then(|| b"ext".to_vec()), ..Default::default() };
    let planned = plan(&target, &tree, &IndexMap::new(), &[], databases.written(), false);
    let mut doomed: Vec<&str> = on_drive.iter().filter(|(_, gone)| *gone).map(|(k, _)| *k).collect();
    let mut planned_keys: Vec<&str> = planned.deletes.iter().map(|d| d.path.as_str()).collect();
    doomed.sort_unstable();
    planned_keys.sort_unstable();
    assert_eq!(planned_keys, doomed);
    assert_eq!(delete(&target, &planned, &ctx).unwrap().0, doomed.len());
    for (k, gone) in on_drive {
        assert_eq!(target.path(k).exists(), !gone, "{k}");
    }
}

/// A file copied moments after its source was written is checked by content the next time it's
/// planned: a source rewritten since at the same size and time is copied again (unless the write
/// was told what the source holds), and an unchanged one isn't.
#[hegel::test(test_cases = 20)]
fn prop_a_copy_made_moments_after_its_source_is_checked_by_content(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let (root, src) = (dir.path().join("drive"), dir.path().join("src"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&src).unwrap();
    let now = chrono::Utc::now().timestamp();
    let path = src.join("source");
    let analysis = tc.draw(gs::booleans());
    let told = !analysis && tc.draw(gs::booleans());
    let (key, mut source) = match analysis {
        true => {
            std::fs::copy(demo_dat(), &path).unwrap();
            fsutil::set_mtime(&path, now).unwrap();
            (
                "PIONEER/USBANLZ/P001/0000/ANLZ0000.DAT",
                Source::analysis(path.clone(), now, "/Contents/a.mp3", 0).unwrap(),
            )
        }
        false => {
            std::fs::write(&path, b"audio one").unwrap();
            fsutil::set_mtime(&path, now).unwrap();
            ("Contents/a.mp3", Source::file(path.clone(), 9, now))
        }
    };
    if told {
        source.hash = Some(Hash::of(b"audio one"));
    }
    let wanted = IndexMap::from([(key.to_string(), source)]);
    // Only a drive with an identity keeps a hash cache.
    let mut target = Device::open(&root).unwrap();
    target.adopt(Some("USB")).unwrap();
    let ctx = Ctx::silent();
    let scan = || crate::tree::scan(target.root(), &crate::tree::ScanOptions::default(), &ctx).unwrap();
    let first = plan(&target, &scan(), &wanted, &[], Written::default(), true);
    assert_eq!(first.copies.len(), 1);
    copy(&target, &first, &ctx).unwrap();
    let rewritten = tc.draw(gs::booleans());
    if rewritten {
        let mut bytes = std::fs::read(&path).unwrap();
        let at = tc.draw(gs::integers::<usize>().max_value(bytes.len() - 1));
        bytes[at] ^= 0xff;
        std::fs::write(&path, &bytes).unwrap();
        fsutil::set_mtime(&path, now).unwrap();
    }
    // What the drive would get now, against what it has: an analysis file's own path is written
    // over, so a change there changes nothing.
    let on_drive = std::fs::read(target.path(key)).unwrap();
    let differs = match analysis {
        true => analysis_bytes(&path, "/Contents/a.mp3", 0).ok() != Some(on_drive),
        false => rewritten && !told,
    };
    let again = plan(&target, &scan(), &wanted, &[], Written::default(), true);
    assert_eq!(again.copies.len(), usize::from(differs), "{:?}", again.copies);
    assert_eq!(again.unchanged, usize::from(!differs));
}

/// A database the swap would write or remove that resolves outside the drive (a link out of
/// it), or that can't be staged (its folder is a file), stops the swap before anything changes;
/// an `exportExt.pdb` that isn't written isn't looked at.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn prop_a_database_swap_that_cant_be_staged_changes_nothing(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let keys = [
        paths::EXPORT_PDB,
        paths::EXPORT_EXT_PDB,
        paths::LIBRARY_PLUS_DB,
        paths::LIBRARY_PLUS_WAL,
        paths::LIBRARY_PLUS_SHM,
    ];
    let db = || tc.draw(gs::optional(gs::binary().max_size(8)));
    let databases = Databases { pdb: db(), ext: db(), plus: db() };
    // Each database linked out of the drive in turn, then the folder they go in a file.
    for linked in keys.map(Some).into_iter().chain([None]) {
        let dir = tempfile::tempdir().unwrap();
        let (root, outside) = (dir.path().join("drive"), dir.path().join("outside"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&outside, b"not the drive's").unwrap();
        match linked {
            Some(k) => {
                let p = paths::to_fs(&root, k);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::os::unix::fs::symlink(&outside, &p).unwrap();
            }
            None => put(&root, "PIONEER/rekordbox", b"a file", 0),
        }
        let before = files_on(&root);
        let target = Device::open(&root).unwrap();
        let swapped = swap_databases(&target, &databases);
        let writes = databases.pdb.is_some() || databases.ext.is_some() || databases.plus.is_some();
        match linked {
            Some(k) if k == paths::EXPORT_EXT_PDB && databases.ext.is_none() => {
                swapped.unwrap();
                assert!(std::fs::symlink_metadata(target.path(k)).unwrap().is_symlink());
                continue;
            }
            Some(_) => assert!(matches!(swapped, Err(Error::Invalid(_))), "{linked:?}: {swapped:?}"),
            None if writes => assert!(swapped.is_err()),
            None => continue,
        }
        assert_eq!(files_on(&root), before, "{linked:?}");
        assert_eq!(std::fs::read(&outside).unwrap(), b"not the drive's");
    }
}

/// A delete stops at a path that would leave the drive, having removed what came before it and
/// nothing after.
#[hegel::test(test_cases = 20)]
fn prop_a_delete_stops_at_a_path_off_the_drive(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let keys = ["Contents/a.mp3", "Contents/b.mp3", "Contents/c.mp3"];
    for k in keys {
        put(dir.path(), k, b"x", 0);
    }
    let at = tc.draw(gs::integers::<usize>().max_value(keys.len()));
    let unsafe_key = tc.draw(gs::sampled_from(vec!["../outside.mp3", "Contents/../../outside.mp3", ".locallibrary/x"]));
    let mut planned = Planned {
        deletes: keys.iter().map(|k| FileCopy { path: k.to_string(), size: 1 }).collect(),
        ..Default::default()
    };
    planned.deletes.insert(at, FileCopy { path: unsafe_key.into(), size: 1 });
    let target = Device::open(dir.path()).unwrap();
    assert!(matches!(delete(&target, &planned, &Ctx::silent()), Err(Error::Invalid(_))));
    for (i, k) in keys.iter().enumerate() {
        assert_eq!(target.path(k).exists(), i >= at, "{k}");
    }
}

/// An analysis file copied into a folder that can't be written fails, and leaves nothing there.
#[cfg(unix)]
#[hegel::test(test_cases = 5)]
fn prop_an_analysis_file_copied_into_a_read_only_folder_fails(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (root, src) = (dir.path().join("drive"), dir.path().join("src"));
    std::fs::create_dir_all(&src).unwrap();
    let key = "PIONEER/USBANLZ/P001/0000/ANLZ0000.DAT";
    let folder = paths::to_fs(&root, key).parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&folder).unwrap();
    let path = src.join("ANLZ0000.DAT");
    std::fs::copy(demo_dat(), &path).unwrap();
    let source =
        Source::analysis(path, 0, "/Contents/a.mp3", tc.draw(gs::integers::<i32>().min_value(0).max_value(50)))
            .unwrap();
    let mut planned = Planned::default();
    planned.copies.push(FileCopy { path: key.into(), size: source.size });
    planned.sources.insert(key.into(), source);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let copied = copy(&Device::open(&root).unwrap(), &planned, &Ctx::silent());
    let left = std::fs::read_dir(&folder).unwrap().count();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(copied, Err(Error::Io { .. })), "{copied:?}");
    assert_eq!(left, 0);
}
