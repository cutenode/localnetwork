//! A drive's OneLibrary database (`PIONEER/rekordbox/exportLibrary.db` and its WAL), the only one
//! CDJ-3000X, OPUS-QUAD, XDJ-AZ and other newer players read: reading its files from the drive,
//! opening it, and writing an edited one back. Reading and editing the database itself is the
//! loclib-rekordbox-db crate's ([`loclib_rekordbox_db::onelibrary`]).

use std::path::Path;

use crate::commit::Swap;
use crate::device::Device;
use crate::error::IoContext;
use crate::hash::Hash;
use crate::model::Library;
use crate::paths;
use crate::{Error, Result};

pub use loclib_rekordbox_db::onelibrary::OneLibrary;

/// Keep the keys derived for OneLibrary databases in locallibrary's cache folder, readable by this
/// user alone, so later runs skip deriving them. Builds with debug assertions keep none, so tests
/// leave the cache of whoever runs them alone.
pub fn keep_keys() {
    #[cfg(not(debug_assertions))]
    {
        let folder =
            directories::ProjectDirs::from("", "", "locallibrary").map(|d| d.cache_dir().join("onelibrary-keys"));
        loclib_rekordbox_db::onelibrary::cipher::keep_keys_in(folder);
    }
}

/// Identity of the library a database (and WAL) holds, unchanged by a checkpoint, re-encryption or
/// the drive's name (see [`loclib_rekordbox_db::onelibrary::content_hash`]).
pub fn content_hash(db: &[u8], wal: Option<&[u8]>) -> Result<Hash> {
    Ok(loclib_rekordbox_db::onelibrary::content_hash(db, wal)?.into())
}

/// [`content_hash`] and the hash locallibrary computed before the drive's name was left out, which
/// lineage recorded before then holds.
pub fn content_hashes(db: &[u8], wal: Option<&[u8]>) -> Result<(Hash, Hash)> {
    let (current, legacy) = loclib_rekordbox_db::onelibrary::content_hashes(db, wal)?;
    Ok((current.into(), legacy.into()))
}

/// Replace a drive's OneLibrary database with `bytes`, a complete encrypted file.
///
/// The WAL and shared-memory index go before the new file is renamed into place: SQLite would
/// replay a WAL into whatever database sits next to it, and a database without its WAL is still a
/// consistent (older) state. Callers hold the drive lock.
pub fn write(device: &Device, bytes: &[u8]) -> Result<()> {
    let db = Swap::bytes(device.path(paths::LIBRARY_PLUS_DB), bytes)?;
    let wal = [paths::LIBRARY_PLUS_WAL, paths::LIBRARY_PLUS_SHM].map(|key| Swap::Remove(device.path(key)));
    crate::commit::swap(wal.into_iter().chain([db]).collect())
}

/// Read a drive's `exportLibrary.db` and `-wal` (if present).
pub fn read_files(root: &Path) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
    let db = paths::to_fs(root, paths::LIBRARY_PLUS_DB);
    let bytes = crate::fsutil::read(&db).at(&db)?;
    let wal = paths::to_fs(root, paths::LIBRARY_PLUS_WAL);
    let wal = match crate::fsutil::read(&wal) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Error::Io { path: wal, source: e }),
    };
    Ok((bytes, wal))
}

/// Open the OneLibrary database below any directory laid out like a drive (a drive, a backup).
pub fn open_root(root: &Path) -> Result<OneLibrary> {
    let (db, wal) = read_files(root)?;
    Ok(OneLibrary::from_encrypted(&db, wal.as_deref())?)
}

/// The drive's OneLibrary database and its library, if it has one. On a drive that also has
/// `export.pdb`, one that can't be read is reported and left alone rather than failing the edit.
pub(crate) fn open_plus(
    device: &Device,
    has_pdb: bool,
    warnings: &mut Vec<String>,
) -> Result<Option<(Library, OneLibrary)>> {
    if !device.has_library_plus() {
        return Ok(None);
    }
    match device.library_plus().and_then(|one| Ok((one.library()?, one))) {
        Ok(x) => Ok(Some(x)),
        Err(e) if has_pdb => {
            warnings.push(format!(
                "{e}. Only export.pdb will be changed, so CDJ-3000X, OPUS-QUAD, XDJ-AZ and other players that \
                 read OneLibrary won't see this until the drive is exported from rekordbox again"
            ));
            Ok(None)
        }
        Err(e) => Err(e),
    }
}
