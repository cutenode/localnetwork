use super::*;

#[test]
fn paths_are_made_absolute_lexically() {
    #[cfg(unix)]
    assert_eq!(absolute(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
    assert!(absolute(Path::new("x")).is_absolute());
}

/// The part of a path that exists is resolved (symlinks too), the rest appended; `..` below a
/// folder that isn't there yet is folded first. Regression: export's own copy of this left
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
