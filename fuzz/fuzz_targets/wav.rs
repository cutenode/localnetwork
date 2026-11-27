// paths: fuzz/fuzz_targets/wav.rs
// paths: fuzz/seeds/wav/**
// paths: crates/locallibrary/src/{recordings,recordings/wav}.rs
// paths: crates/loclib-interop/src/serato.rs
// coverage: crates/locallibrary/src/recordings/wav.rs
// coverage: crates/loclib-interop/src/serato.rs
//! WAV and AIFF chunk headers: the recordings list's WAV header reader, and the checks the Serato
//! cue tag writer makes on a WAV or AIFF before writing into it, then the write itself, which a
//! file the checks passed must take within the size they planned.

#![no_main]

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use locallibrary::recordings::read_wav_header;
use loclib_interop::serato::{tag_growth, write_markers};

/// The tag writer takes a path: one file per process, rewritten for each input.
static PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = std::env::temp_dir().join(format!("localnetwork-fuzz-wav-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir.join("copy")
});

fuzz_target!(|data: &[u8]| {
    let len = data.len() as u64;
    if let Some((_, audio)) = read_wav_header(Cursor::new(data), len) {
        assert!(audio <= len, "{audio} bytes of audio in a {len} byte file");
    }

    let path = PATH.as_path();
    std::fs::write(path, data).expect("the scratch file");
    // The name says which kind the content has to be; at most one of them matches it.
    for named in ["x.wav", "x.aiff"] {
        let Ok(growth) = tag_growth(path, Path::new(named), b"markers") else { continue };
        if let Err(e) = write_markers(path, Path::new(named), b"markers") {
            panic!("planned to grow by {growth} bytes, then the write failed: {e}");
        }
        let written = std::fs::metadata(path).expect("the scratch file").len();
        assert!(written <= len + growth, "planned {len} + {growth} bytes, wrote {written}");
        break;
    }
});
