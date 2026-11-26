// paths: fuzz/fuzz_targets/rekordbox_sync.rs
// paths: fuzz/seeds/rekordbox_sync/**
// paths: crates/locallibrary/src/{device,device/rekordbox_sync}.rs
// coverage: crates/locallibrary/src/device/rekordbox_sync.rs
//! rekordbox's playlist sync files on a drive (`playlists3.sync`, `playlists3Plus.sync`), whose
//! `<Sync>` tag is sliced out by hand.

#![no_main]

use libfuzzer_sys::fuzz_target;
use locallibrary::device::RekordboxSync;

fuzz_target!(|data: &[u8]| {
    // Read from the drive as `Device` reads it.
    let _ = RekordboxSync::parse("onelibrary", &String::from_utf8_lossy(data));
});
