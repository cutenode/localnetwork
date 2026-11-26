// paths: fuzz/fuzz_targets/mp4.rs
// paths: crates/locallibrary/tests/fixtures/audio/**
// paths: crates/loclib-audio/src/**
// paths: crates/loclib-model/src/audio.rs
// paths: crates/locallibrary/src/{probe,pdb,error}.rs
// paths: crates/loclib-progress/src/**
//! MP4 and M4A audio: `vet_container`'s atom walk, symphonia opening and decoding the start of
//! whatever it lets through, and the probe's format, health and length checks. symphonia sizes
//! its tables from the counts a file claims, an allocation abort `catch_unwind` can't stop, which
//! libFuzzer's `-malloc_limit_mb` reports as a crash.

#![no_main]

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use locallibrary::Ctx;
use locallibrary::probe::{self, guarded};

/// Everything here takes a path: one file per process, rewritten for each input. Named `.m4a`,
/// so `vet_container` also searches past the start for the `ftyp` atom.
static PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = std::env::temp_dir().join(format!("localnetwork-fuzz-mp4-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir.join("track.m4a")
});

fuzz_target!(|data: &[u8]| {
    let path = PATH.as_path();
    std::fs::write(path, data).expect("the scratch file");

    if loclib_audio::vet_container(path).is_ok() {
        // A second at 11,025 Hz is enough to read the tables and decode a few packets. Panics
        // are caught as they are when locallibrary decodes, quietly.
        let mut left = 11_025usize;
        let _ = guarded(path, || {
            loclib_audio::decode_until(path, 11_025, &Ctx::silent(), &mut |s| {
                left = left.saturating_sub(s.len());
                if left == 0 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
            })
            .map_err(locallibrary::Error::from)
        });
    }

    let _ = probe::format(path);
    // An expected length and size make a file that won't open be measured by its own boxes.
    let _ = guarded(path, || Ok::<_, locallibrary::Error>(probe::health(path, 0.0, 0)));
    let _ = guarded(path, || Ok::<_, locallibrary::Error>(probe::health(path, 300.0, data.len() as u64 * 2)));
});
