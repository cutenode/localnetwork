//! Drive-relative path keys and the rules for which files locallibrary manages.
//!
//! Every file on a drive is identified by a *key* ([`key`], from [`loclib_model::paths`]): its
//! path relative to the drive root, `/`-separated, Unicode NFC-normalised, without a leading slash
//! (`Contents/A/B/x.mp3`). Normalising matters: rekordbox writes NFC names, while macOS may hand
//! back NFD names when listing a directory, and the same track must compare equal either way.

use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use loclib_model::paths::{key, sanitize_component, to_fs};

use loclib_model::paths::is_safe_component;

pub const PIONEER_DIR: &str = "PIONEER";
pub const CONTENTS_DIR: &str = "Contents";
pub const EXPORT_PDB: &str = "PIONEER/rekordbox/export.pdb";
pub const EXPORT_EXT_PDB: &str = "PIONEER/rekordbox/exportExt.pdb";
/// rekordbox 6.8+/7 OneLibrary ("Device Library Plus") database, the only one CDJ-3000X,
/// OPUS-QUAD, XDJ-AZ and other newer players read (SQLCipher, see [`crate::onelibrary`]).
pub const LIBRARY_PLUS_DB: &str = "PIONEER/rekordbox/exportLibrary.db";
/// SQLite's write-ahead log for [`LIBRARY_PLUS_DB`]. rekordbox leaves committed changes in it, so
/// it belongs to the database: the two are only ever copied, backed up and replaced together.
pub const LIBRARY_PLUS_WAL: &str = "PIONEER/rekordbox/exportLibrary.db-wal";
/// SQLite's shared-memory index of the WAL, rebuilt by whoever opens the database next.
pub const LIBRARY_PLUS_SHM: &str = "PIONEER/rekordbox/exportLibrary.db-shm";
/// rekordbox's records of the playlists it syncs to the drive automatically, for `export.pdb`
/// and OneLibrary (XML; see [`crate::device::RekordboxSync`]).
pub const PLAYLIST_SYNC: &str = "PIONEER/rekordbox/playlists3.sync";
pub const PLAYLIST_SYNC_PLUS: &str = "PIONEER/rekordbox/playlists3Plus.sync";
/// locallibrary's own per-drive metadata directory.
pub const STATE_DIR: &str = ".locallibrary";
/// What a file being written is called until it's renamed into place. One left behind is from a
/// write that was cut off (see [`crate::tidy`]).
pub const TEMP_PREFIX: &str = ".locallibrary-tmp-";
/// Folders under `PIONEER/` that players keep their own state in (logs, deleted items, per-model
/// data).
const PLAYER_DIRS: [&str; 4] = ["LOG", "CDJ", "MPJ", "TRASHBOX"];

/// Files that make up the library database itself; written last during a sync. The OneLibrary
/// WAL comes right after its database.
pub const DATABASE_FILES: [&str; 4] = [EXPORT_PDB, EXPORT_EXT_PDB, LIBRARY_PLUS_DB, LIBRARY_PLUS_WAL];

/// Whether `dir` holds a rekordbox export: `export.pdb` (every player), `exportLibrary.db`
/// (OneLibrary, newer players), or both.
pub fn is_export_root(dir: &Path) -> bool {
    to_fs(dir, EXPORT_PDB).is_file() || to_fs(dir, LIBRARY_PLUS_DB).is_file()
}

/// Convert a key back into the absolute device path format used inside `export.pdb`.
pub fn device_path(key: &str) -> String {
    format!("/{key}")
}

/// Whether a key (typically a path read from a drive's database) is a plain relative path that
/// stays below the drive root and outside locallibrary's own directory. A crafted or corrupt
/// `export.pdb` could otherwise name `../../somewhere`.
pub fn is_safe_key(key: &str) -> bool {
    !key.is_empty() && key.split('/').all(is_safe_component) && !key.split('/').next().is_some_and(is_state_dir)
}

/// Whether a path component names locallibrary's own folder. DJ drives are FAT32, exFAT or HFS+,
/// which ignore letter case: `.LOCALLIBRARY` is the same folder (and so is `.locallıbrary`, whose
/// dotless ı Windows upper-cases to I).
pub fn is_state_dir(component: &str) -> bool {
    component.to_uppercase() == STATE_DIR.to_uppercase() || component.to_lowercase() == STATE_DIR
}

/// What a managed file is, which decides how it is synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileClass {
    /// `PIONEER/rekordbox/*`: export.pdb, exportExt.pdb, exportLibrary.db and its WAL.
    Database,
    /// `PIONEER/USBANLZ/**`: beat grids, cues, waveforms.
    Analysis,
    /// `PIONEER/Artwork/**`: cover art thumbnails.
    Artwork,
    /// `Contents/**` and any other file referenced by the database.
    Audio,
    /// `PIONEER/*SETTING*.DAT`: player/mixer "My Settings". Managed by the settings profile,
    /// never by library sync, so per-drive deviations survive.
    Settings,
    /// `PIONEER/DeviceLibBackup/**`: rekordbox's per-device identity. Never copied between
    /// drives, otherwise rekordbox would mistake one drive for another.
    DeviceIdentity,
    /// `exportLibrary.db-shm`: SQLite's shared-memory index, rebuilt from the WAL when the
    /// database is opened. Never copied or fingerprinted (opening the database rewrites it);
    /// deleted whenever the database or its WAL is replaced.
    Transient,
    /// `PIONEER/extracted/**`: per-drive data rekordbox writes when it first sets a drive up
    /// (`gcred.dat`, which looks like an account credential). Never copied between drives or into
    /// the store, never deleted.
    Private,
    /// What players keep for themselves on a drive (`PIONEER/LOG`, `CDJ`, `MPJ` and `TRASHBOX`,
    /// and the track filter `PIONEER/rekordbox/RBFLTR.DAT`) and rekordbox's records of the
    /// playlists it syncs to the drive (`playlists3*.sync`). It belongs to the drive, like
    /// rekordbox's device identity: never copied, deleted or counted in the library's fingerprint.
    DriveState,
    /// Anything else under `PIONEER/` (e.g. `djprofile.nxs`).
    Other,
}

impl FileClass {
    /// Whether library sync copies this class between drives / snapshots.
    pub fn is_library(self) -> bool {
        !matches!(
            self,
            FileClass::Settings
                | FileClass::DeviceIdentity
                | FileClass::Transient
                | FileClass::Private
                | FileClass::DriveState
        )
    }
}

const IGNORED_NAMES: &[&str] = &[
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    ".Spotlight-V100",
    ".Trashes",
    ".fseventsd",
    ".TemporaryItems",
    "System Volume Information",
    "$RECYCLE.BIN",
];

/// Whether a single path component is OS clutter (or an interrupted write's temp file) that must
/// never be synced.
pub fn is_ignored_name(name: &str) -> bool {
    name.starts_with("._") || name.starts_with(TEMP_PREFIX) || IGNORED_NAMES.contains(&name)
}

/// Classify a key. `None` means locallibrary does not manage the file at all (it lives outside
/// `PIONEER/` and `Contents/`, or is OS clutter) and it is never touched.
pub fn classify(key: &str) -> Option<FileClass> {
    if key.split('/').any(is_ignored_name) || key.split('/').next().is_some_and(is_state_dir) {
        return None;
    }
    let (top, rest) = key.split_once('/')?;
    match top {
        CONTENTS_DIR => Some(FileClass::Audio),
        PIONEER_DIR => Some(classify_pioneer(rest)),
        _ => None,
    }
}

fn classify_pioneer(rest: &str) -> FileClass {
    let (dir, name) = rest.split_once('/').unwrap_or((rest, ""));
    if rest.contains('/') {
        match dir {
            "rekordbox" if rest.eq_ignore_ascii_case(&LIBRARY_PLUS_SHM[PIONEER_DIR.len() + 1..]) => {
                FileClass::Transient
            }
            "rekordbox" if is_drive_state(name) => FileClass::DriveState,
            "rekordbox" => FileClass::Database,
            d if PLAYER_DIRS.iter().any(|p| d.eq_ignore_ascii_case(p)) => FileClass::DriveState,
            "USBANLZ" => FileClass::Analysis,
            "Artwork" => FileClass::Artwork,
            "DeviceLibBackup" => FileClass::DeviceIdentity,
            "extracted" => FileClass::Private,
            _ => FileClass::Other,
        }
    } else if is_settings_file(rest) {
        FileClass::Settings
    } else {
        FileClass::Other
    }
}

/// The track filter and rekordbox's playlist sync records, in `PIONEER/rekordbox/`.
fn is_drive_state(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "rbfltr.dat" || (lower.starts_with("playlists3") && lower.ends_with(".sync"))
}

fn is_settings_file(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.ends_with(".DAT") && upper.contains("SETTING")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_normalised() {
        // "é" as e + combining acute (NFD) must match the precomposed form (NFC).
        assert_eq!(key("/Contents/Cafe\u{301}/x.mp3"), "Contents/Caf\u{e9}/x.mp3");
        assert_eq!(key("PIONEER\\rekordbox\\export.pdb"), EXPORT_PDB);
        assert_eq!(device_path("Contents/a.mp3"), "/Contents/a.mp3");
    }

    #[test]
    fn classification() {
        assert_eq!(classify(EXPORT_PDB), Some(FileClass::Database));
        assert_eq!(classify(LIBRARY_PLUS_DB), Some(FileClass::Database));
        assert_eq!(classify(LIBRARY_PLUS_WAL), Some(FileClass::Database));
        assert_eq!(classify(LIBRARY_PLUS_SHM), Some(FileClass::Transient));
        assert_eq!(classify("PIONEER/rekordbox/EXPORTLIBRARY.DB-SHM"), Some(FileClass::Transient));
        assert!(!FileClass::Transient.is_library());
        assert_eq!(classify("PIONEER/extracted/gcred.dat"), Some(FileClass::Private));
        assert!(!FileClass::Private.is_library());
        assert_eq!(classify("PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT"), Some(FileClass::Analysis));
        assert_eq!(classify("PIONEER/Artwork/00001/a1.jpg"), Some(FileClass::Artwork));
        assert_eq!(classify("PIONEER/MYSETTING.DAT"), Some(FileClass::Settings));
        assert_eq!(classify("PIONEER/DJMMYSETTING.DAT"), Some(FileClass::Settings));
        assert_eq!(classify("PIONEER/DeviceLibBackup/rbDevLibBaInfo_1.json"), Some(FileClass::DeviceIdentity));
        assert_eq!(classify("PIONEER/djprofile.nxs"), Some(FileClass::Other));
        assert_eq!(classify("Contents/A/B/x.mp3"), Some(FileClass::Audio));
        assert_eq!(classify("Contents/A/._x.mp3"), None);
        assert_eq!(classify("Contents/.DS_Store"), None);
        assert_eq!(classify("Contents/A/.locallibrary-tmp-x1y2"), None);
        assert_eq!(classify(".locallibrary/state.json"), None);
        assert_eq!(classify(".LOCALLIBRARY/state.json"), None);
        assert_eq!(classify("Music/x.mp3"), None);
        assert_eq!(classify("README.txt"), None);
    }

    #[test]
    fn what_players_and_rekordbox_keep_for_a_drive_stays_with_it() {
        for key in [
            "PIONEER/LOG/0001.LOG",
            "PIONEER/TRASHBOX/x.dat",
            "PIONEER/cdj/settings.dat",
            "PIONEER/MPJ/a/b.dat",
            "PIONEER/rekordbox/RBFLTR.DAT",
            PLAYLIST_SYNC,
            PLAYLIST_SYNC_PLUS,
        ] {
            assert_eq!(classify(key), Some(FileClass::DriveState), "{key}");
        }
        assert!(!FileClass::DriveState.is_library());
        assert_eq!(classify("PIONEER/rekordbox/other.dat"), Some(FileClass::Database));
        assert_eq!(classify("PIONEER/LOGS/x"), Some(FileClass::Other));
    }

    #[test]
    fn unsafe_keys() {
        for bad in [
            "",
            "../x",
            "Contents/../../x",
            "a//b",
            "./x",
            "C:/x",
            "a\\b",
            ".locallibrary/state.json",
            ".LOCALLIBRARY/state.json",
            ".LocalLibrary/index.json",
            ".locall\u{131}brary/state.json",
        ] {
            assert!(!is_safe_key(bad), "{bad:?}");
        }
        assert!(is_safe_key("Contents/A/x.mp3"));
        assert!(is_safe_key("Music/x.mp3"));
    }
}
