//! Traktor's `collection.nml`: the reader, each track's file and the beat grid its grid markers
//! lay down (what a library source takes), and what the interop plugin's import makes of it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use locallibrary_plugin_interop::traktor_read;

/// The startup disk's name, as macOS calls it: a collection's tracks on it are found below `/`.
const STARTUP: &str = "Macintosh HD";

fuzz_target!(|data: &[u8]| {
    let Ok(collection) = loclib_interop::traktor::read(data) else { return };
    for entry in &collection.entries {
        entry.location.key();
        entry.location.path(STARTUP);
        // Grids are sized by the file's tempos and markers.
        let _ = entry.beats();
    }
    traktor_read::import(collection, STARTUP);
});
