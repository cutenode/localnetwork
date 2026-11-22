//! rekordbox's `master.db` as plain SQLite: the collection `masterdb::open` reads from it
//! (tracks, playlist trees and intelligent playlists' rules, history, cues, My Tags, Cloud
//! Library Sync's files) and the counts `collection info` shows. An encrypted `master.db` is
//! decrypted and its WAL replayed by OneLibrary's code, which the `onelibrary` target covers.
//!
//! `open` takes a path and copies the file into a temporary folder before SQLite opens it, so
//! each input is written to a temporary file first. Only inputs starting with SQLite's magic are
//! tried: `open` takes anything else for an encrypted file, and deriving its salt's keys costs
//! 50 ms before the first page fails its HMAC.

#![no_main]

use std::io::Write;

use libfuzzer_sys::fuzz_target;
use locallibrary::masterdb;
use loclib_rekordbox_db::onelibrary::cipher::SQLITE_MAGIC;

fuzz_target!(|data: &[u8]| {
    if !data.starts_with(SQLITE_MAGIC) {
        return;
    }
    let mut file = tempfile::NamedTempFile::new().expect("a temporary file");
    file.write_all(data).expect("the input written");
    let Ok(collection) = masterdb::open(file.path(), masterdb::DEFAULT_KEY) else { return };
    collection.info();
    collection.tags_by_track();
    for list in collection.smart_lists.values() {
        let _ = list.filter();
    }
});
