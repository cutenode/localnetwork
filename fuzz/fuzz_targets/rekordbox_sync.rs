//! rekordbox's playlist sync files on a drive (`playlists3.sync`, `playlists3Plus.sync`), whose
//! `<Sync>` tag is sliced out by hand.

#![no_main]

use libfuzzer_sys::fuzz_target;
use locallibrary::device::RekordboxSync;

fuzz_target!(|data: &[u8]| {
    // Read from the drive as `Device` reads it.
    let _ = RekordboxSync::parse("onelibrary", &String::from_utf8_lossy(data));
});
