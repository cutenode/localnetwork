use std::path::PathBuf;

use hegel::TestCase;
use hegel::generators as gs;
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

/// Bytes with a frame sync somewhere in them are MP3 audio after junk, as lofty and players read
/// them, and are tagged.
#[test]
fn audio_after_junk_is_tagged() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("junk.mp3"), dir.path().join("out/junk.mp3"));
    let mut bytes = vec![0; 32];
    bytes[14..16].copy_from_slice(&[0xff, 0xe3]);
    std::fs::write(&from, &bytes).unwrap();
    let tagged = Content::TaggedCopy { from, markers: Some(vec![0]) };
    let export = Export { files: vec![file(&to, 32, tagged)], ..Default::default() };
    write(&export, dir.path(), &Ctx::silent()).unwrap();
    let written = std::fs::read(&to).unwrap();
    assert!(written.starts_with(b"ID3") && written.ends_with(&bytes), "{written:?}");
}

/// A folder that can't be written to fails naming it, without the temp file it tried.
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
    assert!(!e.message.contains(" at path "), "{}", e.message);
}

/// What a drawn file is made from: bytes, or a copy of audio that's there, isn't, is a folder, or
/// can't take the tag asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Made {
    Bytes,
    Copy,
    Missing,
    Folder,
    Untaggable,
}

/// When a drawn write is cancelled: never, before it starts, or once a copy has begun.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Cancel {
    Never,
    AtOnce,
    MidCopy,
}

/// What's in the way of the files: nothing, an old database's WAL, or a folder where the WAL
/// would be.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Stale {
    None,
    File,
    Folder,
}

/// Every file below `dir` and its bytes, sorted.
fn below(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().map(Result::unwrap) {
        match e.file_type().unwrap().is_dir() {
            true => out.extend(below(&e.path())),
            false => out.push((e.path(), std::fs::read(e.path()).unwrap())),
        }
    }
    out.sort();
    out
}

/// For any export's files (bytes, copies of audio that's there or not, or can't be tagged), some
/// replacing files already there or below a file where a folder should be, more than the disk
/// has room for, a stale WAL or a folder in its place, and a cancel at any point: either every
/// file is in place with its content and the WAL is gone, or the write fails naming what failed
/// (or as `cancelled`, or `no_space`) and leaves the folder as it was, with no file half-written.
#[hegel::test(test_cases = 100)]
fn a_write_puts_every_file_in_place_or_none(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (src, out) = (dir.path().join("src"), dir.path().join("out"));
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("plain"), b"a file, not a folder").unwrap();
    let made = tc.draw(
        gs::vecs(gs::sampled_from(vec![
            Made::Bytes,
            Made::Bytes,
            Made::Copy,
            Made::Copy,
            Made::Missing,
            Made::Folder,
            Made::Untaggable,
        ]))
        .max_size(4),
    );
    let mut files = Vec::new();
    // Each file's content once written.
    let mut contents = Vec::new();
    // The first failure, foreseen: its kind, and the path its message names.
    let mut fails: Vec<(&str, PathBuf)> = Vec::new();
    let cancel = tc.draw(gs::sampled_from(vec![Cancel::Never, Cancel::Never, Cancel::AtOnce, Cancel::MidCopy]));
    for (i, &made) in made.iter().enumerate() {
        let blocked = tc.draw(gs::weighted_booleans(0.1));
        let path = match blocked {
            true => out.join("plain").join(format!("f{i}")),
            false => out.join(format!("sub{}", i % 2)).join(format!("f{i}")),
        };
        if !blocked && tc.draw(gs::booleans()) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"old").unwrap();
        }
        // Untaggable audio has no frame sync or chunk id in it: lofty reads a frame sync anywhere
        // in a file's first KiB as MP3 audio after junk, and tags it.
        let top = if made == Made::Untaggable { 0x40 } else { u8::MAX };
        let bytes = tc.draw(gs::vecs(gs::integers::<u8>().max_value(top)).min_size(1).max_size(64));
        let from = src.join(format!("a{i}.mp3"));
        let content = match made {
            Made::Bytes => Content::Bytes(bytes.clone()),
            Made::Missing => Content::TaggedCopy { from: from.clone(), markers: None },
            Made::Folder => {
                std::fs::create_dir(&from).unwrap();
                Content::TaggedCopy { from: from.clone(), markers: None }
            }
            Made::Copy | Made::Untaggable => {
                std::fs::write(&from, &bytes).unwrap();
                let markers = (made == Made::Untaggable).then(|| vec![0]);
                Content::TaggedCopy { from: from.clone(), markers }
            }
        };
        if blocked {
            fails.push(("io", out.join("plain")));
        }
        match made {
            Made::Missing | Made::Folder => fails.push(("io", from.clone())),
            Made::Copy | Made::Untaggable if cancel == Cancel::MidCopy => fails.push(("cancelled", PathBuf::new())),
            Made::Untaggable => fails.push(("tags", from.clone())),
            _ => {}
        }
        files.push(file(&path, bytes.len() as u64, content));
        contents.push((path, bytes));
    }
    if tc.draw(gs::weighted_booleans(0.1))
        && let Some(f) = files.first_mut()
    {
        f.bytes = 1 << 62;
        fails.insert(0, ("no_space", out.clone()));
    }
    if cancel == Cancel::AtOnce && !files.is_empty() {
        fails.insert(usize::from(fails.first().is_some_and(|f| f.0 == "no_space")), ("cancelled", PathBuf::new()));
    }
    let wal = out.join("m.db-wal");
    let stale = tc.draw(gs::sampled_from(vec![Stale::None, Stale::File, Stale::Folder, Stale::Folder]));
    match stale {
        Stale::None => {}
        Stale::File => std::fs::write(&wal, b"stale").unwrap(),
        Stale::Folder => {
            std::fs::create_dir(&wal).unwrap();
            fails.push(("io", wal.clone()));
        }
    }
    let export = Export { files, remove: vec![wal.clone()], ..Default::default() };
    let before = below(&out);

    let token = CancelToken::new();
    let once_copying = |e: &Event| {
        if cancel == Cancel::MidCopy && matches!(e, Event::Bytes { .. }) {
            token.cancel();
        }
    };
    if cancel == Cancel::AtOnce {
        token.cancel();
    }
    let ctx = Ctx::new(&once_copying, token.clone());
    match (write(&export, &out, &ctx), fails.first()) {
        (Ok(()), None) => {
            for (path, bytes) in &contents {
                assert_eq!(&std::fs::read(path).unwrap(), bytes, "{}", path.display());
            }
            assert!(!wal.exists());
        }
        (Err(e), Some((kind, named))) => {
            assert_eq!(e.kind(), Some(*kind), "{e:?}");
            assert!(e.message.contains(&named.display().to_string()), "{e:?} names {}", named.display());
            assert_eq!(below(&out), before, "the folder is as it was");
        }
        (written, foreseen) => panic!("wrote {written:?}, not {foreseen:?}"),
    }
}
