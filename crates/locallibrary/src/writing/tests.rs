use crate::device::LineageOp;
use crate::progress::Ctx;
use crate::tidy::{self, TidyKind, TidyOptions};

use super::*;

const TIME: i64 = 1_600_000_000;

/// A drive cut off while writing `new` (noted before a byte was written), holding `there` too.
fn cut_off(dir: &std::path::Path) -> Device {
    let mut d = Device::open(dir).unwrap();
    d.begin(LineageOp::Sync, "sync", None).unwrap();
    d
}

fn put(d: &Device, key: &str, bytes: &[u8], mtime: i64) {
    let p = d.path(key);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, bytes).unwrap();
    fsutil::set_mtime(&p, mtime).unwrap();
}

/// Whatever state a cut-off write left each of its new files in, `drive tidy` removes the ones it
/// didn't finish and leaves the finished ones, the ones that were there before it started and the
/// ones it never got to. The next write to the drive does the same.
#[hegel::test(test_cases = 40)]
fn what_a_cut_off_write_left_goes_and_nothing_else(tc: hegel::TestCase) {
    use hegel::generators as gs;
    // Never started, part written, whole but unstamped, finished.
    let states: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(3)).max_size(6));
    let by_tidy = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let mut d = cut_off(dir.path());
    put(&d, "Contents/old.mp3", b"the DJ's", TIME);
    let key = |i: usize| format!("Contents/{i}.mp3");
    let files = states.iter().enumerate().map(|(i, _)| (key(i), 4, TIME));
    note(&d, files.chain([("Contents/old.mp3".to_string(), 4, TIME), ("../outside.mp3".to_string(), 4, TIME)]))
        .unwrap();
    let now = chrono::Utc::now().timestamp();
    let mut left = Vec::new();
    for (i, state) in states.iter().enumerate() {
        match state {
            0 => {}
            1 => put(&d, &key(i), b"ab", now),
            2 => put(&d, &key(i), b"abcd", now),
            _ => put(&d, &key(i), b"abcd", TIME),
        }
        if matches!(state, 1 | 2) {
            left.push(key(i));
        }
    }
    let mut noted = unfinished(&d);
    noted.sort();
    left.sort();
    assert_eq!(noted, left);

    if by_tidy {
        let plan = tidy::plan(&d, &TidyOptions::default(), &Ctx::silent()).unwrap();
        assert!(plan.items.iter().all(|i| i.kind == TidyKind::TempFiles));
        assert_eq!(plan.items.len(), left.len());
        assert_eq!(tidy::apply(&plan, &d, &Ctx::silent()).unwrap().removed, left.len());
    } else {
        d.begin(LineageOp::Sync, "sync again", None).unwrap();
    }
    for (i, state) in states.iter().enumerate() {
        assert_eq!(d.path(&key(i)).exists(), *state == 3, "{i}: {state}");
    }
    assert_eq!(std::fs::read(d.path("Contents/old.mp3")).unwrap(), b"the DJ's");
}

/// Files are only ever taken for a cut-off write's on a drive marked as cut off, and once a write
/// is done nothing's noted.
#[test]
fn a_finished_write_leaves_nothing_noted() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = cut_off(dir.path());
    note(&d, [("Contents/a.mp3".to_string(), 4, TIME)].into_iter()).unwrap();
    put(&d, "Contents/a.mp3", b"ab", TIME + 500);
    assert_eq!(unfinished(&d), ["Contents/a.mp3"]);
    d.finish(None).unwrap();
    assert_eq!(unfinished(&d), Vec::<String>::new(), "not without the mark");
    forget(&d).unwrap();
    assert!(!file(&d).exists());
    note(&d, std::iter::empty()).unwrap();
    assert!(!file(&d).exists(), "nothing to note, nothing written");
}

/// A source written moments ago gives its copy a time like the write's own, so size and time say
/// nothing: what was written since the write began counts as unfinished, as a plan takes it.
#[test]
fn a_torn_file_with_its_sources_time_is_unfinished() {
    let dir = tempfile::tempdir().unwrap();
    let d = cut_off(dir.path());
    let now = chrono::Utc::now().timestamp();
    note(&d, [("Contents/a.mp3".to_string(), 4, now)].into_iter()).unwrap();
    put(&d, "Contents/a.mp3", b"\0\0\0\0", now);
    assert_eq!(unfinished(&d), ["Contents/a.mp3"]);
}

/// The published note and its directory must be durable before any direct write can start.
/// Even an existing state directory may just have been made by lock/begin on this run.
#[hegel::test(test_cases = 8)]
#[hegel::explicit_test_case(existing = false, fail_at = 0usize)]
#[hegel::explicit_test_case(existing = true, fail_at = 1usize)]
#[hegel::explicit_test_case(existing = false, fail_at = 2usize)]
fn noting_files_persists_both_directory_entries_or_returns_the_error(tc: hegel::TestCase) {
    use crate::Error;
    use hegel::generators as gs;
    let existing = tc.draw(gs::booleans());
    let fail_at = tc.draw(gs::integers::<usize>().max_value(2));
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    if existing {
        std::fs::create_dir(d.state_dir()).unwrap();
    }
    let mut synced = Vec::new();
    let result = note_with_sync(&d, [("Contents/a.mp3".to_string(), 4, TIME)].into_iter(), |path| {
        let note: Note = fsutil::read_json(&file(&d)).unwrap();
        let f = &note.files[0];
        assert_eq!((f.key.as_str(), f.size, f.mtime), ("Contents/a.mp3", 4, TIME));
        assert!(!d.path("Contents/a.mp3").exists());
        synced.push(path.to_path_buf());
        if synced.len() - 1 == fail_at {
            return Err(Error::Io { path: path.to_path_buf(), source: std::io::ErrorKind::PermissionDenied.into() });
        }
        Ok(())
    });
    let expected = [d.state_dir(), d.root().to_path_buf()];
    assert_eq!(synced, expected[..(fail_at + 1).min(2)]);
    match fail_at {
        0 | 1 => assert!(matches!(result, Err(Error::Io { path, source })
            if path == expected[fail_at] && source.kind() == std::io::ErrorKind::PermissionDenied)),
        _ => result.unwrap(),
    }
}

/// A note whose removal never reached the disk outlives the write that made it. A later write,
/// cut off in turn, keeps the file it finished replacing, whatever the stale note says about it.
#[hegel::test(test_cases = 4)]
#[hegel::explicit_test_case(by_tidy = false)]
#[hegel::explicit_test_case(by_tidy = true)]
fn a_stale_note_leaves_a_later_writes_files(tc: hegel::TestCase) {
    let by_tidy = tc.draw(hegel::generators::booleans());
    let dir = tempfile::tempdir().unwrap();
    let mut d = cut_off(dir.path());
    let key = "Contents/a.mp3";
    note(&d, [(key.to_string(), 4, TIME)].into_iter()).unwrap();
    put(&d, key, b"old!", TIME);
    // The write finished, but the note's removal was lost (not `forget`).
    d.finish(None).unwrap();
    d.begin(LineageOp::Sync, "replace it", None).unwrap();
    put(&d, key, b"new audio", TIME + 300);
    // Cut off here: the next run finds the drive marked, and the first write's note.
    let mut d = Device::open(dir.path()).unwrap();
    if by_tidy {
        let plan = tidy::plan(&d, &TidyOptions::default(), &Ctx::silent()).unwrap();
        assert!(plan.items.is_empty(), "{:?}", plan.items);
    } else {
        d.begin(LineageOp::Sync, "sync again", None).unwrap();
    }
    assert_eq!(std::fs::read(d.path(key)).unwrap(), b"new audio");
}

/// The real copy path must propagate a directory-open error from the durability barrier, even
/// when it can publish the note and create the audio. Only fixtures have their permissions changed.
#[cfg(unix)]
#[hegel::test(test_cases = 4)]
#[hegel::explicit_test_case(parent = false)]
#[hegel::explicit_test_case(parent = true)]
fn a_note_directory_sync_error_prevents_the_first_copy(tc: hegel::TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let parent = tc.draw(hegel::generators::booleans());
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drive");
    let src = dir.path().join("source");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(&src, b"audio").unwrap();
    let d = Device::open(&root).unwrap();
    std::fs::create_dir(d.state_dir()).unwrap();
    std::fs::create_dir(root.join("Contents")).unwrap();
    let mut planned = crate::library_write::Planned::default();
    planned.copies.push(crate::library_write::FileCopy { path: "Contents/a.mp3".into(), size: 5 });
    planned.sources.insert("Contents/a.mp3".into(), crate::library_write::Source::file(src, 5, TIME));
    let blocked = if parent { d.root().to_path_buf() } else { d.state_dir() };
    let permissions = std::fs::metadata(&blocked).unwrap().permissions();
    // Write and traversal still work, but opening the directory to sync it does not.
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o300)).unwrap();
    let denied = std::fs::File::open(&blocked).is_err();
    let result = crate::library_write::copy(&d, &planned, &Ctx::silent());
    std::fs::set_permissions(&blocked, permissions).unwrap();
    if denied {
        assert!(matches!(result, Err(crate::Error::Io { path, source })
            if path == blocked && source.kind() == std::io::ErrorKind::PermissionDenied));
        assert!(file(&d).is_file(), "the atomic note write itself succeeded");
        assert!(!d.path("Contents/a.mp3").exists(), "no direct write may start after the barrier failed");
    } else {
        // An elevated test process may still open the directory.
        assert_eq!(result.unwrap(), (1, 5));
    }
}

/// An export leaves nothing noted, and one cut off part way leaves its record until it's run again.
#[test]
fn nothing_is_left_noted_after_a_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (root, src) = (dir.path().join("drive"), dir.path().join("a.mp3"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&src, b"abcd").unwrap();
    let target = Device::open(&root).unwrap();
    let mut planned = crate::library_write::Planned::default();
    planned.copies.push(crate::library_write::FileCopy { path: "Contents/a.mp3".into(), size: 4 });
    planned.sources.insert("Contents/a.mp3".into(), crate::library_write::Source::file(src, 4, TIME));
    crate::library_write::copy(&target, &planned, &Ctx::silent()).unwrap();
    assert!(!file(&target).exists());
}
