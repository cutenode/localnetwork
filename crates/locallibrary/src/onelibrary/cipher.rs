//! OneLibrary's page cipher: SQLCipher 4 ([`loclib_sqlcipher`]) under a passphrase shared by
//! every rekordbox installation (found by the pyrekordbox project). rekordbox's own collection
//! database (`master.db`, see [`crate::masterdb`]) is encrypted the same way under another
//! passphrase ([`Codec::with_key`]).

pub use loclib_sqlcipher::{Codec, PAGE_SIZE, RESERVE, SALT_LEN, SQLITE_MAGIC, check_plain_header};
use loclib_sqlcipher::{Keys, derive_keys};

use crate::{Error, Result};

/// The passphrase every rekordbox installation encrypts `exportLibrary.db` under (`PRAGMA key`).
pub const PASSPHRASE: &str = "r8gddnr4k847830ar6cqzbkk0el6qytmb3trbbx805jm74vez64i5o8fnrqryqls";

mod stored;

/// The codec of an existing encrypted file (its salt is the first 16 bytes).
pub fn for_file(bytes: &[u8]) -> Result<Codec> {
    let salt: [u8; SALT_LEN] = bytes
        .get(..SALT_LEN)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| Error::OneLibrary("file too short".into()))?;
    Ok(with_salt(salt))
}

/// A codec with a fresh random salt, for a new database file.
pub fn random() -> Result<Codec> {
    let mut salt = [0u8; SALT_LEN];
    getrandom::fill(&mut salt).map_err(|e| Error::OneLibrary(format!("no randomness available: {e}")))?;
    Ok(with_salt(salt))
}

fn with_salt(salt: [u8; SALT_LEN]) -> Codec {
    Codec::with_keys(salt, keys(&salt))
}

/// The keys for `salt`, kept from an earlier run where this computer keeps them ([`stored`]), or
/// derived (and kept).
fn keys(salt: &[u8; SALT_LEN]) -> Keys {
    let derive = || derive_keys(PASSPHRASE.as_bytes(), salt);
    let Some(folder) = stored::folder() else { return derive() };
    stored::load(&folder, salt).unwrap_or_else(|| {
        let keys = derive();
        stored::save(&folder, salt, &keys);
        keys
    })
}
