//! Keys derived for drives' OneLibrary databases, kept between runs. Deriving one takes 256,000
//! rounds of PBKDF2, about 50 ms, which was 60% of the instructions of listing a library of 4,000
//! tracks, in every command. A database's keys depend only on rekordbox's published passphrase
//! and the salt at the start of the file, so whoever can read the drive can derive them anyway; they
//! are still kept only where the caller says ([`super::keep_keys_in`]), in a folder and files only
//! this user can read. A file that's been damaged reads as none, and the keys are derived again.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use super::{Keys, SALT_LEN};

/// Keys, salt-checked hash: what a file holds.
const FILE_LEN: usize = 32 + 32 + 32;

/// The folder keys are kept in: none until the caller names one.
static FOLDER: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Where keys are kept, if the caller named somewhere.
pub(super) fn folder() -> Option<PathBuf> {
    FOLDER.read().unwrap_or_else(PoisonError::into_inner).clone()
}

pub(super) fn set_folder(folder: Option<PathBuf>) {
    *FOLDER.write().unwrap_or_else(PoisonError::into_inner) = folder;
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
