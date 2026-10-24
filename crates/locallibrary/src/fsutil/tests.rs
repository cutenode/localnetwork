use super::*;

#[test]
fn paths_are_made_absolute_lexically() {
    #[cfg(unix)]
    assert_eq!(absolute(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
    assert!(absolute(Path::new("x")).is_absolute());
}

/// The part of a path that exists is resolved (symlinks too, a `..` after one stepping out of its
/// target), the rest appended; `..` below a folder that isn't there yet takes it away.
/// Regressions: `..` after a link went back to the link's folder; export's own copy of this left
/// such a path unresolved, so `/tmp/new/../out` and `/private/tmp/out` didn't compare.
#[test]
fn the_part_that_exists_is_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let real = crate::device::dunce_canonicalize(dir.path());
    assert_eq!(resolved(&dir.path().join("new/../out/x")), real.join("out/x"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real, real.join("link")).unwrap();
        assert_eq!(resolved(&real.join("link/missing/../there")), real.join("there"));
        // `..` out of a link is the folder its target is in, where the OS takes it.
        fs::create_dir_all(real.join("deep/er")).unwrap();
        std::os::unix::fs::symlink(real.join("deep/er"), real.join("up")).unwrap();
        assert_eq!(resolved(&real.join("up/../x")), real.join("deep/x"));
    }
}

/// Regression: a lock was released by closing its file, so it stayed held while a child
/// process another thread was starting still shared the file (until it ran its program), and
/// taking it again right away failed.
#[test]
fn a_lock_is_released_while_a_child_still_shares_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lock");
    let lock = FileLock::try_take(&path).unwrap().unwrap();
    assert!(FileLock::try_take(&path).unwrap().is_none(), "held");
    // What a child being started holds: the same open file.
    let shared = lock.0.try_clone().unwrap();
    drop(lock);
    assert!(FileLock::try_take(&path).unwrap().is_some());
    drop(shared);
}

#[test]
fn files_already_there_are_replaced_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b, c) = (dir.path().join("a.xml"), dir.path().join("b.xml"), dir.path().join("c.xml"));
    for p in [&a, &b] {
        fs::write(p, b"<old/>").unwrap();
    }
    let err = refuse_to_replace(&[&c, &a, &b], false).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::Exists));
    assert!(err.to_string().ends_with("is already there (and 1 more); choose another place, or replace them"), "{err}");
    assert!(refuse_to_replace(&[&a, &b], true).is_ok());
    assert!(write_output(&a, b"<new/>", false).is_err());
    write_output(&c, b"<new/>", false).unwrap();
    assert_eq!(fs::read(&c).unwrap(), b"<new/>");
}

/// A staged copy whose bytes aren't the expected ones is refused, and the file it was to
/// replace stays.
#[test]
fn a_copy_that_doesnt_match_its_hash_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dst) = (dir.path().join("a.mp3"), dir.path().join("Contents/a.mp3"));
    fs::write(&src, b"audio").unwrap();
    let err = stage_copy(&src, &dst, None, Some(Hash::of(b"other audio")), &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::Corrupt { .. }), "{err}");
    assert!(!dst.exists());
    let (tmp, hash) = stage_copy(&src, &dst, Some(1_600_000_000), Some(Hash::of(b"audio")), &Ctx::silent()).unwrap();
    assert_eq!(hash, Hash::of(b"audio"));
    tmp.persist(&dst).unwrap();
    assert_eq!(mtime_secs(&fs::metadata(&dst).unwrap()), 1_600_000_000);
}

#[test]
fn missing_files_read_as_nothing_and_remove_as_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    assert_eq!(read_json_opt::<u32>(&path).unwrap(), None);
    assert!(!file_exists(&path));
    fs::write(&path, b"{").unwrap();
    assert!(file_exists(&path));
    assert!(matches!(read_json_opt::<u32>(&path), Err(Error::Parse { .. })));
    assert!(matches!(read_json_opt::<u32>(dir.path()), Err(Error::Io { .. })), "a folder");
    assert!(remove_if_exists(&path).unwrap());
    assert!(!remove_if_exists(&path).unwrap());
    #[cfg(unix)]
    assert_eq!(absolute(Path::new("/a/./b")), PathBuf::from("/a/b"));
}

/// Flushing a copied file works on every platform (on Windows it once failed with "Access is
/// denied"), leaves its contents and modification time alone, and skips files that are gone.
#[test]
fn flush_then_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dst) = (dir.path().join("a.mp3"), dir.path().join("Contents/b.mp3"));
    fs::write(&src, b"audio").unwrap();
    copy_file(&src, &dst, None, None, false, &Ctx::silent()).unwrap();
    flush(&[dst.clone(), dir.path().join("gone.mp3")]).unwrap();
    set_mtime(&dst, 1_600_000_000).unwrap();
    flush(std::slice::from_ref(&dst)).unwrap();
    assert_eq!(fs::read(&dst).unwrap(), b"audio");
    assert_eq!(mtime_secs(&fs::metadata(&dst).unwrap()), 1_600_000_000);
}

/// Regression: files written through a temp file got tempfile's 0600, so an export (or a file
/// it replaced) was readable by its owner only.
#[cfg(unix)]
#[test]
fn written_files_get_the_usual_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    // What a new file gets here (the umask is the process's).
    let plain = dir.path().join("plain.txt");
    fs::write(&plain, b"x").unwrap();
    let (new, copied) = (dir.path().join("new.m3u8"), dir.path().join("Contents/a.mp3"));
    write_atomic(&new, b"#EXTM3U").unwrap();
    copy_file(&plain, &copied, None, None, true, &Ctx::silent()).unwrap();
    assert_eq!((mode(&new), mode(&copied)), (mode(&plain), mode(&plain)));
    fs::set_permissions(&new, fs::Permissions::from_mode(0o640)).unwrap();
    write_atomic(&new, b"#EXTM3U again").unwrap();
    assert_eq!(mode(&new), 0o640, "a file replaced keeps its permissions");
}

/// Files in `dir` that a writer's temp files left.
fn temp_files(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with(paths::TEMP_PREFIX))
                .collect()
        })
        .unwrap_or_default()
}

/// Every way of writing a file: where it can write, the file ends up holding exactly the bytes
/// (a staged one only once persisted, a copy stamped with its time and checked against its hash);
/// where it can't (the place is a folder, its folder is a file, it has no folder, the source is
/// gone, the copy isn't what was expected), it fails, and what was there stays as it was with no
/// temp file left behind.
#[hegel::test(test_cases = 50)]
#[hegel::explicit_test_case(writer = 0u8, obstacle = 0u8, durable = false, mtime = None::<i64>)]
#[hegel::explicit_test_case(writer = 5u8, obstacle = 4u8, durable = true, mtime = Some(1_600_000_000i64))]
#[hegel::explicit_test_case(writer = 6u8, obstacle = 1u8, durable = false, mtime = Some(1_600_000_000i64))]
#[hegel::explicit_test_case(writer = 7u8, obstacle = 2u8, durable = false, mtime = None::<i64>)]
#[hegel::explicit_test_case(writer = 1u8, obstacle = 3u8, durable = false, mtime = None::<i64>)]
// Each writer once where its folder is missing, or a folder stands where it would persist.
#[hegel::explicit_test_case(writer = 2u8, obstacle = 3u8, durable = false, mtime = None::<i64>)]
#[hegel::explicit_test_case(writer = 4u8, obstacle = 3u8, durable = false, mtime = None::<i64>)]
#[hegel::explicit_test_case(writer = 5u8, obstacle = 3u8, durable = false, mtime = None::<i64>)]
#[hegel::explicit_test_case(writer = 6u8, obstacle = 3u8, durable = false, mtime = None::<i64>)]
#[hegel::explicit_test_case(writer = 0u8, obstacle = 1u8, durable = false, mtime = None::<i64>)]
#[hegel::explicit_test_case(writer = 1u8, obstacle = 1u8, durable = false, mtime = None::<i64>)]
#[hegel::explicit_test_case(writer = 2u8, obstacle = 1u8, durable = false, mtime = None::<i64>)]
#[hegel::explicit_test_case(writer = 5u8, obstacle = 1u8, durable = true, mtime = None::<i64>)]
#[hegel::explicit_test_case(writer = 6u8, obstacle = 5u8, durable = false, mtime = None::<i64>)]
fn a_write_lands_whole_or_not_at_all(tc: hegel::TestCase) {
    use hegel::generators as gs;
    // write_atomic, write_private, write_deferred, write_output, stage_bytes, copy_file,
    // stage_copy, write_json_atomic.
    let writer = tc.draw(gs::integers::<u8>().max_value(7));
    // None, the place is a folder, its folder is a file, no folder at all, the source is gone
    // (copies), the copy isn't the expected one (copies).
    let obstacle = tc.draw(gs::integers::<u8>().max_value(5));
    let durable = tc.draw(gs::booleans());
    let mtime = tc.draw(gs::optional(gs::integers::<i64>().min_value(1_000_000_000).max_value(1_900_000_000)));
    let copies = matches!(writer, 5 | 6);
    let bytes: &[u8] = if writer == 7 { b"[\n  1,\n  2\n]\n" } else { b"the new bytes" };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    fs::write(&src, bytes).unwrap();
    let folder = dir.path().join("out");
    fs::create_dir(&folder).unwrap();
    let mut dst = folder.join("file.bin");
    match obstacle {
        1 => fs::create_dir(&dst).unwrap(),
        2 => {
            fs::write(folder.join("plain"), b"a file").unwrap();
            dst = folder.join("plain/file.bin");
        }
        3 => dst = PathBuf::from("/"),
        4 if copies => fs::remove_file(&src).unwrap(),
        _ => {}
    }
    let wrong = obstacle == 5 && copies;
    let expected = match (copies, wrong) {
        (true, true) => Some(Hash::of(b"other bytes")),
        (true, false) => Some(Hash::of(bytes)),
        _ => None,
    };
    let ctx = Ctx::silent();
    let result: Result<Option<tempfile::NamedTempFile>> = match writer {
        0 => write_atomic(&dst, bytes).map(|()| None),
        1 => write_private(&dst, bytes).map(|()| None),
        2 => write_deferred(&dst, bytes).map(|()| None),
        3 => write_output(&dst, bytes, true).map(|()| None),
        4 => stage_bytes(&dst, bytes).map(Some),
        5 => copy_file(&src, &dst, mtime, expected, durable, &ctx).map(|h| {
            assert_eq!(h, Hash::of(bytes));
            None
        }),
        6 => stage_copy(&src, &dst, mtime, expected, &ctx).map(|(t, h)| {
            assert_eq!(h, Hash::of(bytes));
            Some(t)
        }),
        _ => write_json_atomic(&dst, &[1, 2]).map(|()| None),
    };
    // Staging writes only next to the place: a folder there stops the persisting instead.
    let staged_only = matches!(writer, 4 | 6);
    let fails = matches!(obstacle, 2 | 3) || (obstacle == 1 && !staged_only) || (copies && obstacle >= 4);
    match result {
        Ok(staged) => {
            assert!(!fails, "writer {writer}, obstacle {obstacle}");
            if let Some(tmp) = staged {
                assert!(!dst.is_file(), "not until persisted");
                if obstacle == 1 {
                    assert!(tmp.persist(&dst).is_err());
                    assert!(dst.is_dir() && temp_files(&folder).is_empty());
                    return;
                }
                tmp.persist(&dst).unwrap();
            }
            assert_eq!(fs::read(&dst).unwrap(), bytes);
            if let (true, Some(secs)) = (copies, mtime) {
                assert_eq!(mtime_secs(&fs::metadata(&dst).unwrap()), secs);
            }
        }
        Err(e) => {
            assert!(fails, "writer {writer}, obstacle {obstacle}: {e}");
            match obstacle {
                1 => assert!(dst.is_dir() && fs::read_dir(&dst).unwrap().next().is_none()),
                2 => assert_eq!(fs::read(folder.join("plain")).unwrap(), b"a file"),
                3 => assert!(matches!(e, Error::Invalid(ref m) if m.ends_with("has no parent")), "{e}"),
                _ => assert!(!dst.exists()),
            }
            if wrong {
                assert!(matches!(e, Error::Corrupt { .. }), "{e}");
            }
        }
    }
    assert!(temp_files(&folder).is_empty(), "{:?}", temp_files(&folder));
}

/// JSON written is read back as it was; a value JSON can't hold isn't written; a missing file
/// reads as nothing only where that's allowed; and something that isn't a JSON file is an error
/// naming it.
#[hegel::test(test_cases = 40)]
#[hegel::explicit_test_case(entries = vec![(3i64, "x".to_string())], keyed = true)]
#[hegel::explicit_test_case(entries = Vec::<(i64, String)>::new(), keyed = false)]
fn json_reads_back_what_was_written(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use indexmap::IndexMap;
    let entries: Vec<(i64, String)> =
        tc.draw(gs::vecs(gs::tuples!(gs::integers::<i64>(), gs::text().max_size(6))).max_size(4));
    // Keyed by a pair, which JSON objects can't be.
    let keyed = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let by_name: IndexMap<String, i64> = entries.iter().map(|(n, s)| (s.clone(), *n)).collect();
    write_json_atomic(&path, &by_name).unwrap();
    assert_eq!(read_json::<IndexMap<String, i64>>(&path).unwrap(), by_name);
    assert_eq!(read_json_opt::<IndexMap<String, i64>>(&path).unwrap(), Some(by_name.clone()));
    if keyed {
        let by_pair: IndexMap<(i64, i64), i64> = entries.iter().map(|(n, _)| ((*n, *n), *n)).collect();
        let result = write_json_atomic(&dir.path().join("pairs.json"), &by_pair);
        // An empty map is `{}`, whatever its keys.
        assert_eq!(result.is_err(), !by_pair.is_empty());
        assert!(matches!(result, Ok(()) | Err(Error::Invalid(_))));
    }
    let missing = dir.path().join("missing.json");
    assert_eq!(read_json_opt::<IndexMap<String, i64>>(&missing).unwrap(), None);
    assert!(matches!(read_json::<IndexMap<String, i64>>(&missing), Err(Error::Io { path, .. }) if path == missing));
    assert!(
        matches!(read_json_opt::<IndexMap<String, i64>>(dir.path()), Err(Error::Io { path, .. }) if path == dir.path())
    );
    fs::write(&path, b"{\"a\":").unwrap();
    assert!(matches!(read_json::<IndexMap<String, i64>>(&path), Err(Error::Parse { .. })));
}

/// Flushing makes every file that's there durable and skips the ones that are gone, but a file it
/// may not open stops it, naming the file; a file exists for reading exactly when it opens.
#[cfg(unix)]
#[hegel::test(test_cases = 30)]
#[hegel::explicit_test_case(kinds = vec![0u8, 1, 2])]
#[hegel::explicit_test_case(kinds = vec![1u8, 0])]
fn flushing_skips_the_gone_and_stops_at_the_unreadable(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::os::unix::fs::PermissionsExt;
    // There, gone, unreadable.
    let kinds: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(2)).max_size(5));
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> = kinds.iter().enumerate().map(|(n, _)| dir.path().join(format!("{n}.mp3"))).collect();
    let mut denied = Vec::new();
    for (p, kind) in files.iter().zip(&kinds) {
        if *kind != 1 {
            fs::write(p, b"audio").unwrap();
        }
        if *kind == 2 {
            fs::set_permissions(p, fs::Permissions::from_mode(0o000)).unwrap();
            // Root opens anything.
            denied.push(File::open(p).is_err());
        } else {
            denied.push(false);
        }
    }
    let result = flush(&files);
    let opens: Vec<bool> = files.iter().map(|p| file_exists(p)).collect();
    for (p, kind) in files.iter().zip(&kinds) {
        if *kind == 2 {
            fs::set_permissions(p, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }
    let first_denied = denied.iter().position(|d| *d);
    match result {
        Ok(()) => assert!(first_denied.is_none()),
        Err(Error::Io { path, .. }) => assert_eq!(Some(path), first_denied.map(|i| files[i].clone())),
        Err(e) => panic!("{e}"),
    }
    for ((o, kind), d) in opens.iter().zip(&kinds).zip(&denied) {
        assert_eq!(*o, *kind != 1 && !d);
    }
    // Making the renames into a folder durable asks nothing of one that isn't there.
    sync_dir(dir.path());
    sync_dir(&dir.path().join("gone"));
    for (p, kind) in files.iter().zip(&kinds) {
        assert_eq!(p.exists(), *kind != 1);
    }
}

/// A drive's lock: one holder at a time (whether it waited for it or tried), a reader never makes
/// the file, and a lock in a folder that isn't there can't be had.
#[hegel::test(test_cases = 40)]
#[hegel::explicit_test_case(steps = vec![0u8, 1, 2, 3, 1, 4])]
fn a_lock_has_one_holder_at_a_time(tc: hegel::TestCase) {
    use hegel::generators as gs;
    // Wait, try, try as a reader, release, check the file.
    let steps: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(4)).max_size(8));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lock");
    let mut held: Option<FileLock> = None;
    let mut made = false;
    for step in steps {
        match step {
            0 if held.is_none() => {
                held = Some(FileLock::wait(&path).unwrap());
                made = true;
            }
            0 | 1 => {
                let got = FileLock::try_take(&path).unwrap();
                made = true;
                assert_eq!(got.is_some(), held.is_none());
                if held.is_none() {
                    held = got;
                }
            }
            2 => match FileLock::try_take_existing(&path) {
                Ok(got) => {
                    assert!(made);
                    assert_eq!(got.is_some(), held.is_none());
                }
                Err(e) => assert!(!made && e.kind() == std::io::ErrorKind::NotFound, "{e}"),
            },
            3 => held = None,
            _ => assert_eq!(path.exists(), made),
        }
    }
    let nowhere = dir.path().join("gone/lock");
    assert!(matches!(FileLock::wait(&nowhere), Err(Error::Io { path, .. }) if path == nowhere));
    assert!(matches!(FileLock::try_take(&nowhere), Err(Error::Io { path, .. }) if path == nowhere));
    assert!(!dir.path().join("gone").exists());
}

/// A rename waits for a look [`unreplaced`] is taking (on Windows the look's handles would fail
/// it), and is made once the look is done.
#[test]
fn a_rename_waits_for_a_look_to_finish() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("export.pdb");
    fs::write(&path, b"old").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|s| {
        unreplaced(|| {
            s.spawn(|| tx.send(write_atomic(&path, b"new")).unwrap());
            assert!(rx.recv_timeout(std::time::Duration::from_millis(100)).is_err(), "renamed during the look");
            assert_eq!(fs::read(&path).unwrap(), b"old");
        });
        rx.recv().unwrap().unwrap();
    });
    assert_eq!(fs::read(&path).unwrap(), b"new");
}

/// One thread reads a JSON file while another writes it over and over: every read finds the file
/// whole, and every write lands. Regression: on Windows a rename over the file while a read held
/// it open failed ("Access is denied"), as a plugin's settings changed while its service ran.
#[test]
fn a_file_read_while_its_rewritten_is_read_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    write_json_atomic(&path, &0u32).unwrap();
    let writing = std::sync::atomic::AtomicBool::new(true);
    std::thread::scope(|s| {
        s.spawn(|| {
            let mut last = 0;
            while writing.load(std::sync::atomic::Ordering::SeqCst) {
                let read: u32 = read_json(&path).unwrap();
                assert!(read >= last, "read {read} after {last}");
                last = read;
            }
        });
        for n in 1..=50u32 {
            write_json_atomic(&path, &n).unwrap();
        }
        writing.store(false, std::sync::atomic::Ordering::SeqCst);
    });
    assert_eq!(read_json::<u32>(&path).unwrap(), 50);
}
