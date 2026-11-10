use std::io::{Read, Seek, Write};

use super::*;

fn temp_beside(path: &Path) -> NamedTempFile {
    NamedTempFile::new_in(path.parent().unwrap()).unwrap()
}

/// `tmp` holding `src`'s bytes, however it got them.
fn filled(src: &Path, tmp: NamedTempFile) -> NamedTempFile {
    reflink_into(src, tmp).unwrap_or_else(|mut tmp| {
        tmp.write_all(&std::fs::read(src).unwrap()).unwrap();
        tmp.as_file_mut().rewind().unwrap();
        tmp
    })
}

#[test]
fn a_clone_holds_the_source_and_is_written_apart_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.mp3");
    std::fs::write(&src, b"the audio").unwrap();
    let mut tmp = filled(&src, temp_beside(&src));
    let mut held = Vec::new();
    tmp.as_file_mut().read_to_end(&mut held).unwrap();
    assert_eq!(held, b"the audio");

    tmp.as_file_mut().rewind().unwrap();
    tmp.as_file_mut().write_all(b"TAGGED").unwrap();
    assert_eq!(std::fs::read(tmp.path()).unwrap(), b"TAGGEDdio");
    assert_eq!(std::fs::read(&src).unwrap(), b"the audio", "a write to the copy reached the source");
}

#[test]
fn a_source_that_cant_be_cloned_leaves_the_temp_file_empty_and_usable() {
    let dir = tempfile::tempdir().unwrap();
    let tmp = NamedTempFile::new_in(dir.path()).unwrap();
    let path = tmp.path().to_path_buf();
    let mut back = reflink_into(&dir.path().join("not there"), tmp).expect_err("nothing to clone");
    assert_eq!(back.path(), path);
    back.write_all(b"bytes").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"bytes");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "something was left beside it");
}

/// The clone goes by the temp file's permissions: a read-only source doesn't make a read-only copy.
#[cfg(unix)]
#[test]
fn a_clone_keeps_the_temp_files_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.mp3");
    std::fs::write(&src, b"the audio").unwrap();
    std::fs::set_permissions(&src, PermissionsExt::from_mode(0o400)).unwrap();
    let tmp = temp_beside(&src);
    std::fs::set_permissions(tmp.path(), PermissionsExt::from_mode(0o640)).unwrap();
    match reflink_into(&src, tmp) {
        Ok(cloned) => assert_eq!(std::fs::metadata(cloned.path()).unwrap().permissions().mode() & 0o777, 0o640),
        Err(_) if cfg!(target_os = "macos") => panic!("APFS clones"),
        Err(_) => {}
    }
}

/// On the Mac's APFS the copy shares the source's blocks: both map their first byte to one place
/// on the disk.
#[cfg(target_os = "macos")]
#[test]
fn on_apfs_a_clone_shares_the_sources_blocks() {
    use std::os::fd::AsRawFd;

    #[repr(C, packed(4))]
    struct Log2Phys {
        flags: u32,
        contigbytes: i64,
        devoffset: i64,
    }
    const F_LOG2PHYS_EXT: i32 = 65;
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }
    fn first_block(file: &std::fs::File) -> i64 {
        let mut l = Log2Phys { flags: 0, contigbytes: 1, devoffset: 0 };
        // SAFETY: `l` outlives the call, which fills it in.
        let r = unsafe { fcntl(file.as_raw_fd(), F_LOG2PHYS_EXT, &raw mut l) };
        assert_eq!(r, 0, "{}", std::io::Error::last_os_error());
        l.devoffset
    }

    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.mp3");
    let mut f = std::fs::File::create(&src).unwrap();
    f.write_all(&vec![7; 1 << 20]).unwrap();
    f.sync_all().unwrap();
    let tmp = reflink_into(&src, temp_beside(&src)).unwrap_or_else(|_| panic!("APFS clones"));
    assert_eq!(first_block(tmp.as_file()), first_block(&std::fs::File::open(&src).unwrap()));
}
