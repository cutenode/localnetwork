//! OneLibrary's `exportLibrary.db` and its WAL: the SQLCipher codec, WAL replay, the content hash,
//! and `OneLibrary` with the library read from what decrypts.
//!
//! An input is a database file, followed by its WAL from the first page boundary holding a WAL's
//! magic number (`cat exportLibrary.db exportLibrary.db-wal`). Fuzzed encrypted pages never pass
//! their HMAC, so an input that is plain SQLite instead is encrypted here under one fixed salt
//! before it is read: SQLite and the code reading rows into a library see fuzzed pages too.
//! Reading takes a file on disk, as every `OneLibrary` is a working copy in a temporary folder,
//! which keeps this target to hundreds of runs a second.

#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use locallibrary::onelibrary::cipher::{self, Codec, PAGE_SIZE, SALT_LEN, SQLITE_MAGIC};
use locallibrary::onelibrary::{self, OneLibrary, wal};

/// Deriving a salt's keys takes 256,000 rounds of PBKDF2, so plain inputs all get this one.
static CODEC: LazyLock<Codec> = LazyLock::new(|| Codec::with_salt([0; SALT_LEN]));

fuzz_target!(|data: &[u8]| {
    let _ = cipher::check_plain_header(data);
    if data.starts_with(SQLITE_MAGIC) {
        // Padded to whole pages, so a mutation that changes the length still reaches SQLite.
        let mut plain = data.to_vec();
        plain.resize(plain.len().next_multiple_of(PAGE_SIZE), 0);
        if let Ok(db) = CODEC.encrypt(&plain) {
            read(&db, None);
        }
        return;
    }
    let is_wal = |at: &usize| matches!(data[*at..], [0x37, 0x7f, 0x06, 0x82 | 0x83, ..]);
    let split = (0..data.len()).step_by(PAGE_SIZE).find(is_wal).unwrap_or(data.len());
    let (db, log) = data.split_at(split);
    let log = (!log.is_empty()).then_some(log);
    if let Some(log) = log {
        let _ = wal::committed(log, PAGE_SIZE);
    }
    // `decrypt` refuses anything else only after deriving its salt's keys (50 ms), so it's left out.
    if !db.is_empty() && db.len().is_multiple_of(PAGE_SIZE) {
        read(db, log);
    }
});

fn read(db: &[u8], log: Option<&[u8]>) {
    let _ = onelibrary::content_hash(db, log);
    if let Ok(one) = OneLibrary::from_encrypted(db, log) {
        let _ = one.library();
    }
}
