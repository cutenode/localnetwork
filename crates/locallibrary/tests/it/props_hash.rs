//! Hashing files: copying a file while hashing it gives the hash of the file and every byte of
//! it, and a read or write that fails partway is an error naming the file it failed on.

use std::io::{self, Write};
use std::path::Path;

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::hash::{copy_hashing, hash_file};
use locallibrary::progress::{Ctx, Phase};
use locallibrary::{Error, Hash};

/// Takes up to `limit` bytes, then fails every write.
struct Full {
    taken: Vec<u8>,
    limit: usize,
}

impl Write for Full {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.taken.len() + buf.len() > self.limit {
            return Err(io::Error::other("no room"));
        }
        self.taken.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn failed_on(e: &Error, path: &Path) -> bool {
    matches!(e, Error::Io { path: p, .. } if p == path)
}

/// Any file, of up to three read buffers, copied while hashing: the copy is the file, the count
/// its length and the hash its hash (and BLAKE3's), whatever phase the bytes are reported under.
/// Into a writer with room for fewer bytes it's an error on the writer's path, after only whole
/// buffers that fit; a directory can't be read for either.
#[hegel::test(test_cases = 20)]
fn prop_copying_while_hashing_copies_the_file_and_its_hash(tc: TestCase) {
    let len = tc.draw(gs::integers::<usize>().max_value(3 << 20));
    let seed = tc.draw(gs::integers::<u8>());
    let phase = tc.draw(gs::sampled_from(vec![Phase::Hash, Phase::Copy, Phase::Verify]));
    let bytes: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(31) ^ seed).collect();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("track.mp3");
    std::fs::write(&src, &bytes).unwrap();
    let ctx = Ctx::silent();
    let hash = hash_file(&src, &ctx, phase).unwrap();
    assert_eq!(hash, Hash::from(blake3::hash(&bytes)));

    let dst = dir.path().join("copy.mp3");
    let mut copy = Vec::new();
    assert_eq!(copy_hashing(&src, &mut copy, &dst, &ctx, phase).unwrap(), (hash, len as u64));
    assert_eq!(copy, bytes);

    let limit = tc.draw(gs::integers::<usize>().max_value(len + 1));
    let mut full = Full { taken: Vec::new(), limit };
    match copy_hashing(&src, &mut full, &dst, &ctx, phase) {
        Ok(copied) => {
            assert!(len <= limit, "{len} bytes copied into room for {limit}");
            assert_eq!(copied, (hash, len as u64));
        }
        Err(e) => {
            assert!(len > limit, "{len} bytes fit in {limit}: {e}");
            assert!(failed_on(&e, &dst), "{e:?}");
            assert_eq!(full.taken[..], bytes[..full.taken.len()]);
        }
    }

    // A directory opens on Unix and fails to read; on Windows it fails to open.
    assert!(failed_on(&hash_file(dir.path(), &ctx, phase).unwrap_err(), dir.path()));
    let e = copy_hashing(dir.path(), &mut Vec::new(), &dst, &ctx, phase).unwrap_err();
    assert!(failed_on(&e, dir.path()), "{e:?}");
}

/// A sparse file can't be made in a folder that isn't there.
#[hegel::test(test_cases = 5)]
fn prop_sparse_files_need_their_folder(tc: TestCase) {
    let len = tc.draw(gs::integers::<u64>().max_value(1 << 34));
    let dir = tempfile::tempdir().unwrap();
    assert!(locallibrary::testing::sparse(&dir.path().join("gone/track.mp3"), len).is_err());
    let made = dir.path().join("track.mp3");
    locallibrary::testing::sparse(&made, len).unwrap();
    assert_eq!(std::fs::metadata(&made).unwrap().len(), len);
}
