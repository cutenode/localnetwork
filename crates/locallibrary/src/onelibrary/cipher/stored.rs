//! Keys derived for drives' OneLibrary databases, kept between runs. Deriving one takes 256,000
//! rounds of PBKDF2, about 50 ms, which was 60% of the instructions of listing a library of 4,000
//! tracks, in every command. A database's keys depend only on rekordbox's published passphrase
//! and the salt at the start of the file, so whoever can read the drive can derive them anyway; they
//! still go in a folder and files only this user can read, and nowhere else. A file that's been
//! damaged reads as none, and the keys are derived again.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Keys, SALT_LEN};

/// Keys, salt-checked hash: what a file holds.
const FILE_LEN: usize = 32 + 32 + 32;

/// Set by tests: the folder to keep keys in. Builds with debug assertions keep none without it, so
/// tests leave the cache of whoever runs them alone.
#[cfg(debug_assertions)]
const TEST_FOLDER: &str = "LOCALLIBRARY_KEY_CACHE";

/// Where keys are kept, if this computer has somewhere.
#[cfg(debug_assertions)]
pub(super) fn folder() -> Option<PathBuf> {
    std::env::var_os(TEST_FOLDER).map(PathBuf::from)
}

/// Where keys are kept, if this computer has somewhere.
#[cfg(not(debug_assertions))]
pub(super) fn folder() -> Option<PathBuf> {
    Some(directories::ProjectDirs::from("", "", "locallibrary")?.cache_dir().join("onelibrary-keys"))
}

fn file(folder: &Path, salt: &[u8; SALT_LEN]) -> PathBuf {
    let name: String = salt.iter().map(|b| format!("{b:02x}")).collect();
    folder.join(name)
}

/// What tells a file's keys are whole and for `salt`.
fn check(salt: &[u8; SALT_LEN], keys: &Keys) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(salt).update(&keys.cipher).update(&keys.hmac);
    *hash.finalize().as_bytes()
}

/// The keys kept for `salt`, if they're there and whole.
pub(super) fn load(folder: &Path, salt: &[u8; SALT_LEN]) -> Option<Keys> {
    let bytes: [u8; FILE_LEN] = std::fs::read(file(folder, salt)).ok()?.try_into().ok()?;
    let (cipher, rest) = bytes.split_first_chunk::<32>()?;
    let (hmac, stored) = rest.split_first_chunk::<32>()?;
    let keys = Keys { cipher: *cipher, hmac: *hmac };
    (check(salt, &keys) == *stored).then_some(keys)
}

/// Keeps `keys` for `salt`. Keeping them only saves time, so a failure just means deriving them
/// again next run.
pub(super) fn save(folder: &Path, salt: &[u8; SALT_LEN], keys: &Keys) {
    let _ = try_save(folder, salt, keys);
}

fn try_save(folder: &Path, salt: &[u8; SALT_LEN], keys: &Keys) -> std::io::Result<()> {
    make_private(folder)?;
    // Readable by this user alone from the moment it exists, and whole or not there.
    let mut temp = tempfile::NamedTempFile::new_in(folder)?;
    temp.write_all(&keys.cipher)?;
    temp.write_all(&keys.hmac)?;
    temp.write_all(&check(salt, keys))?;
    temp.persist(file(folder, salt))?;
    Ok(())
}

#[cfg(unix)]
fn make_private(folder: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(folder)?;
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))
}

/// A user's local application data is theirs alone already.
#[cfg(not(unix))]
fn make_private(folder: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)
}

#[cfg(test)]
mod tests;
