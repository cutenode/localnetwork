use loclib_interop::File;
use loclib_progress::CancelToken;

use super::*;

fn file(path: &Path, bytes: u64, content: Content) -> File {
    File { path: path.into(), what: "a file".into(), bytes, content }
}

/// Cancelled once the first file is staged, nothing is put in place, and the old database keeps
/// its WAL.
#[test]
fn a_cancel_leaves_what_was_there() {
    let dir = tempfile::tempdir().unwrap();
    let wal = dir.path().join("m.db-wal");
    std::fs::write(&wal, b"stale").unwrap();
    let cancel = CancelToken::new();
    let once_copied = |e: &Event| {
        if let Event::Bytes { .. } = e {
            cancel.cancel();
        }
    };
    let ctx = Ctx::new(&once_copied, cancel.clone());
    let from = dir.path().join("a.mp3");
    std::fs::write(&from, b"audio").unwrap();
    let copy = Content::TaggedCopy { from, markers: None };
    let export = Export {
        files: vec![
            file(&dir.path().join("out/a.mp3"), 5, copy),
            file(&dir.path().join("out/m.db"), 1, Content::Bytes(vec![1])),
        ],
        remove: vec![wal.clone()],
        ..Default::default()
    };
    assert_eq!(write(&export, dir.path(), &ctx).unwrap_err().kind(), Some("cancelled"));
    assert_eq!(std::fs::read(&wal).unwrap(), b"stale");
    let left: Vec<_> = std::fs::read_dir(dir.path().join("out")).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
}

/// What can't be written fails naming the file, before any is put in place when it can: more
/// than there's room for, audio that isn't there, a file that can't be removed or replaced.
#[test]
fn what_cant_be_written_fails_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let at = |name: &str| dir.path().join(name);
    let bytes = |name: &str, bytes| file(&at(name), bytes, Content::Bytes(vec![0; 4]));
    let fails = |export: Export| write(&export, dir.path(), &ctx).unwrap_err();

    let huge = fails(Export { files: vec![bytes("huge", u64::MAX)], ..Default::default() });
    assert_eq!(huge.kind(), Some("no_space"));
    assert!(!at("huge").exists());

    let gone = Content::TaggedCopy { from: at("gone.mp3"), markers: None };
    let e = fails(Export { files: vec![file(&at("copy.mp3"), 4, gone)], ..Default::default() });
    assert_eq!(e.kind(), Some("io"));
    assert!(e.message.starts_with(&at("gone.mp3").display().to_string()), "{}", e.message);

    std::fs::create_dir_all(at("folder/inside")).unwrap();
    let e = fails(Export { files: vec![bytes("new", 4)], remove: vec![at("folder")], ..Default::default() });
    assert!(e.message.starts_with(&at("folder").display().to_string()), "{}", e.message);
    assert!(!at("new").exists(), "nothing is put in place");

    let e = fails(Export { files: vec![bytes("folder", 4)], ..Default::default() });
    assert_eq!(e.kind(), Some("io"));
    assert!(e.message.starts_with(&at("folder").display().to_string()), "{}", e.message);
    assert!(at("folder/inside").is_dir());

    std::fs::write(at("file"), b"").unwrap();
    let e = fails(Export { files: vec![bytes("file/below", 4)], ..Default::default() });
    assert!(e.message.starts_with(&at("file").display().to_string()), "{}", e.message);
}

/// Audio that can't be read or tagged fails naming the drive's file (the copy's is temporary),
/// with nothing put in place.
#[test]
fn audio_that_cant_be_copied_or_tagged_fails_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let at = |name: &str| dir.path().join(name);
    let fails = |f: File| write(&Export { files: vec![f], ..Default::default() }, dir.path(), &ctx).unwrap_err();

    std::fs::write(at("noise.mp3"), b"noise").unwrap();
    let tagged = Content::TaggedCopy { from: at("noise.mp3"), markers: Some(vec![0]) };
    let e = fails(file(&at("out/noise.mp3"), 5, tagged));
    assert_eq!(e.kind(), Some("tags"));
    assert!(e.message.contains(&at("noise.mp3").display().to_string()), "{}", e.message);
    assert_eq!(std::fs::read_dir(at("out")).unwrap().count(), 0);

    // A folder where the audio should be opens, and fails once read.
    std::fs::create_dir(at("album.mp3")).unwrap();
    let folder = Content::TaggedCopy { from: at("album.mp3"), markers: None };
    let e = fails(file(&at("out/album.mp3"), 0, folder));
    assert_eq!(e.kind(), Some("io"));
    assert!(e.message.starts_with(&at("album.mp3").display().to_string()), "{}", e.message);
}

/// A folder that can't be written to fails naming it.
#[cfg(unix)]
#[test]
fn a_folder_that_cant_be_written_to_fails_naming_it() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let export = Export { files: vec![file(&locked.join("m.db"), 1, Content::Bytes(vec![1]))], ..Default::default() };
    let e = write(&export, dir.path(), &Ctx::silent()).unwrap_err();
    assert_eq!(e.kind(), Some("io"));
    assert!(e.message.starts_with(&locked.display().to_string()), "{}", e.message);
}
