//! rekordbox's own collection on this computer: `master.db`, read only.
//!
//! rekordbox 6 and 7 keep the collection (tracks, playlists, cues, My Tags, history) in
//! `master.db` in rekordbox's settings folder: `~/Library/Pioneer/rekordbox/` on macOS,
//! `%APPDATA%\Pioneer\rekordbox\` on Windows. Like OneLibrary's `exportLibrary.db` it's SQLite
//! encrypted with SQLCipher 4's defaults (see [`crate::onelibrary::cipher`]), under a passphrase
//! every installation shares. The pyrekordbox project documents the format
//! (`docs/source/formats/db6.md`) and the passphrase (`pyrekordbox/masterdb/database.py` carries
//! it obfuscated, and checks a key given instead starts `402fd`). Should a rekordbox release
//! change it, [`KEY_ENV`] (or a key passed in) overrides [`DEFAULT_KEY`].
//!
//! Nothing here writes to rekordbox's files or opens them with SQLite: the database and its WAL
//! are read into memory and decrypted there (the WAL's committed transactions replayed, as for
//! OneLibrary), and only that plain copy is opened, read only. What needs it as a file (a plain
//! `master.db`, one of 2 GiB or more, an export) goes in a [`crate::scratch::Folder`]. rekordbox
//! may keep running; a read while it saves can miss its latest changes.
//!
//! Tables read, with the column names pyrekordbox gives them: `djmdContent` and its `djmdArtist`,
//! `djmdAlbum`, `djmdGenre`, `djmdLabel` and `djmdKey` lookups, `djmdPlaylist` +
//! `djmdSongPlaylist`, `djmdHistory` + `djmdSongHistory`, `djmdCue`, `djmdMyTag` +
//! `djmdSongMyTag` and `djmdProperty`. Rows rekordbox marks deleted (`rb_local_deleted`) are left
//! out, and a column a version lacks reads as empty. What the values mean:
//!
//! * ids are text (`"123456789"`); they become the model's numeric ids when every one is a
//!   distinct 32-bit number, otherwise tracks and playlists are numbered in order.
//! * `djmdContent.BPM` is hundredths of a beat per minute, `Length` seconds, `Rating` 0-5 stars,
//!   `ColorID` a row of rekordbox's colour table (1 pink ... 8 purple), `FolderPath` the audio
//!   file's full path on this computer (`/Users/me/Music/x.mp3`, `C:/Users/me/Music/x.mp3`), or,
//!   for a track Cloud Library Sync keeps in Dropbox or Google Drive, its path in the service's
//!   `rekordbox` folder (`/contents_2354715273/Artist/x.mp3`, see [`CloudFolders`]),
//!   `StockDate` the date it was added.
//! * `AnalysisDataPath` and `ImagePath` are paths in rekordbox's `share` folder beside
//!   `master.db` (`/PIONEER/USBANLZ/d48/899a4-.../ANLZ0000.DAT`, `/PIONEER/Artwork/...`).
//! * `djmdCloudProperty` names the storage service Cloud Library Sync uses (`Reserved1`
//!   `SyncService`, `Reserved2` `Dropbox`), and `contentFile` lists the analysis and artwork files
//!   it syncs, with their sizes; a file there whose copy on this computer has another size (or
//!   none) hasn't finished syncing.
//! * playlists and My Tags hang off `ParentID` `"root"`; a playlist's `Attribute` is 0 for a
//!   playlist, 1 for a folder and 4 for an intelligent playlist, whose rules (`SmartList`, see
//!   [`smartlist`]) are read but not evaluated here, so it reads as empty.
//! * `djmdCue.Kind` is 0 for a memory cue, 1-3 for hot cues A-C, 4 for a memory loop and 5 on for
//!   hot cues D onwards (checked against the analysis files of 1,594 tracks); `InMsec`/`OutMsec` are
//!   milliseconds, `OutMsec` -1 for a cue that isn't a loop. A hot cue's colour is taken to be
//!   `ColorTableIndex` as a code of rekordbox's hot cue palette, like OneLibrary's
//!   `colorTableIndex` (inferred, not documented).

use indexmap::IndexMap;

use rustc_hash::{FxHashMap, FxHashSet};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use loclib_model::analysis::{Cue, hot_cue_palette};
use loclib_model::model::{Library, LibrarySummary, Playlist, Track, TrackColor, untangle};
use loclib_model::paths;
use loclib_sqlcipher::untrusted::distrust;
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use zeroize::{Zeroize, Zeroizing};

use crate::error::IoContext;
use crate::onelibrary::{cipher, hot_cue_from_kind, wal};
use crate::{Error, Result};
pub use smartlist::SmartList;

/// The SQLCipher passphrase every rekordbox 6 and 7 installation uses for `master.db`, as
/// pyrekordbox publishes it.
pub const DEFAULT_KEY: &str = "402fd482c38817c35ffa8ffb8c7d93143b749e7d315df7a81732a1ff43608497";

/// Environment variable with another passphrase, should rekordbox change it.
pub const KEY_ENV: &str = "LOCALLIBRARY_MASTERDB_KEY";

/// Environment variable with the path of the `master.db` to read instead of rekordbox's.
pub const PATH_ENV: &str = "LOCALLIBRARY_MASTERDB";

/// Where rekordbox keeps `master.db` on this computer: [`PATH_ENV`] when set, else where
/// rekordbox's own settings say once the library was moved (`masterDbDirectory` in
/// `rekordbox3.settings`, else `db-path` in its agent's `options.json`), otherwise
/// `~/Library/Pioneer/rekordbox/master.db` on macOS and `%APPDATA%\Pioneer\rekordbox\master.db`
/// on Windows. `None` elsewhere (rekordbox runs on neither).
pub fn default_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(PATH_ENV).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    moved(settings_path().as_deref(), agent_options_path().as_deref()).or_else(platform_path)
}

#[cfg(target_os = "macos")]
fn platform_path() -> Option<PathBuf> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    Some(home.join("Library/Pioneer/rekordbox/master.db"))
}

#[cfg(windows)]
fn platform_path() -> Option<PathBuf> {
    // %APPDATA% is the roaming application data folder.
    let roaming = directories::BaseDirs::new()?.config_dir().to_path_buf();
    Some(roaming.join("Pioneer").join("rekordbox").join("master.db"))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_path() -> Option<PathBuf> {
    None
}

/// rekordbox's `share` folder beside the `master.db` at `path`, which its `AnalysisDataPath` and
/// `ImagePath` are paths in.
fn share_dir(path: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new(".")).join("share")
}

/// Whether a track's `FolderPath` is its path in a storage service's `rekordbox` folder
/// (`/contents_<id>/...`), where Cloud Library Sync keeps it, rather than on this computer.
fn in_cloud(path: &str) -> bool {
    path.trim_start_matches('/').split('/').next().unwrap_or("").starts_with("contents_")
}

/// The passphrase to use: `key` when given, else [`KEY_ENV`], else [`DEFAULT_KEY`].
pub fn resolve_key(key: Option<&str>) -> String {
    key.map(str::to_string)
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var(KEY_ENV).ok().filter(|k| !k.is_empty()))
        .unwrap_or_else(|| DEFAULT_KEY.to_string())
}

/// rekordbox's collection, read from `master.db`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Collection {
    /// The `master.db` it was read from.
    pub path: PathBuf,
    /// `djmdProperty.DBVersion`, when there is one.
    pub db_version: Option<String>,
    /// Tracks, playlists (intelligent ones empty) and history lists. Track paths are the audio
    /// files' paths on this computer.
    pub library: Library,
    /// Hot cues, memory cues and loops by track id, as the analysis files would list them.
    pub cues: IndexMap<u32, Vec<Cue>>,
    /// My Tags: categories in rekordbox's order, each with its tags and their tracks.
    pub tags: Vec<TagCategory>,
    /// Ids of intelligent playlists, whose rules aren't evaluated here.
    pub smart_playlists: Vec<u32>,
    /// Their rules, by id: those whose `SmartList` could be read.
    #[serde(skip)]
    #[cfg_attr(feature = "schemars", schemars(skip))]
    pub smart_lists: FxHashMap<u32, SmartList>,
    /// The storage service Cloud Library Sync keeps audio in (`Dropbox`), if it's on.
    pub sync_service: Option<String>,
    /// Sizes of the files Cloud Library Sync syncs (`contentFile`), by lower-cased path in the
    /// `share` folder (`/pioneer/usbanlz/...`).
    #[serde(skip)]
    #[cfg_attr(feature = "schemars", schemars(skip))]
    pub synced_sizes: FxHashMap<String, u64>,
}

/// The folders Cloud Library Sync keeps audio in on this computer: rekordbox's `rekordbox` folder
/// sits in each. rekordbox records them in its settings (`rekordbox3.settings`), as
/// `DropboxSharingPath` and `GoogleDriveSharingPath`; nothing else in that file is read (it also
/// holds the services' sign-in tokens).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudFolders {
    /// Dropbox's (`DropboxSharingPath`).
    pub dropbox: Option<PathBuf>,
    /// Google Drive's (`GoogleDriveSharingPath`).
    pub google_drive: Option<PathBuf>,
}

impl CloudFolders {
    /// The folders rekordbox on this computer recorded; none when it has no settings file.
    pub fn this_computer() -> CloudFolders {
        settings_path().and_then(|p| CloudFolders::from_settings(&p).ok()).unwrap_or_default()
    }

    /// The folders a `rekordbox3.settings` file names.
    pub fn from_settings(path: &Path) -> Result<CloudFolders> {
        let mut out = CloudFolders::default();
        for (name, val) in settings_values(path, &["DropboxSharingPath", "GoogleDriveSharingPath"])? {
            let slot = if name == "DropboxSharingPath" { &mut out.dropbox } else { &mut out.google_drive };
            *slot = val.map(PathBuf::from);
        }
        Ok(out)
    }

    /// The folders to look in for `service` (`Dropbox`, `GoogleDrive`): its own first, then the
    /// other, should the name be one this doesn't know.
    fn for_service(&self, service: Option<&str>) -> Vec<&Path> {
        let google = service.is_some_and(|s| s.to_lowercase().contains("google"));
        let order = if google { [&self.google_drive, &self.dropbox] } else { [&self.dropbox, &self.google_drive] };
        order.into_iter().flatten().map(PathBuf::as_path).collect()
    }
}

/// The values of the settings `names` in a `rekordbox3.settings` file (`<VALUE name val/>`), in the
/// file's order, an empty one as none. Nothing else in it is kept: it holds sign-in tokens too.
fn settings_values(path: &Path, names: &[&str]) -> Result<Vec<(String, Option<String>)>> {
    let xml = std::fs::read_to_string(path).at(path)?;
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut out = Vec::new();
    loop {
        let event = reader.read_event().map_err(|e| Error::parse(path, e))?;
        let e = match &event {
            quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => e,
            quick_xml::events::Event::Eof => return Ok(out),
            _ => continue,
        };
        let attr = |name: &str| -> Option<String> {
            let a = e.attributes().flatten().find(|a| a.key.as_ref() == name)?;
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned())
        };
        match attr("name") {
            Some(name) if e.name().as_ref() == "VALUE" && names.contains(&name.as_str()) => {
                out.push((name, attr("val").filter(|v| !v.is_empty())));
            }
            _ => {}
        }
    }
}

/// Where rekordbox's settings say `master.db` is, once its library was moved: the last absolute
/// `masterDbDirectory` in the `rekordbox3.settings` at `settings`, else `db-path` in the agent's
/// `options.json` at `options` (as pyrekordbox finds it).
fn moved(settings: Option<&Path>, options: Option<&Path>) -> Option<PathBuf> {
    let dirs = settings.and_then(|s| settings_values(s, &["masterDbDirectory"]).ok()).unwrap_or_default();
    let dir = dirs.into_iter().rev().filter_map(|(_, v)| v.map(PathBuf::from)).find(|d| d.is_absolute());
    let db_path = || agent_option(options?, "db-path").map(PathBuf::from).filter(|p| p.is_absolute());
    dir.map(|d| d.join("master.db")).or_else(db_path)
}

/// The option `name` in rekordbox's agent's `options.json` at `path` (`{"options": [[name,
/// value], ...]}`), when it's text.
fn agent_option(path: &Path, name: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let option = json["options"].as_array()?.iter().find(|o| o[0] == name)?;
    option[1].as_str().map(str::to_string)
}

/// Pioneer's folder of rekordbox's settings: `~/Library/Application Support/Pioneer/` on macOS,
/// `%APPDATA%\Pioneer\` (roaming) on Windows.
fn pioneer_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    cfg!(any(target_os = "macos", windows)).then(|| base.config_dir().join("Pioneer"))
}

/// rekordbox 6 and 7's settings file, in `rekordbox6/` there.
fn settings_path() -> Option<PathBuf> {
    Some(pioneer_dir()?.join("rekordbox6").join("rekordbox3.settings"))
}

/// rekordbox's agent's options, in `rekordboxAgent/storage/` there.
fn agent_options_path() -> Option<PathBuf> {
    Some(pioneer_dir()?.join("rekordboxAgent").join("storage").join("options.json"))
}

/// A My Tag category (`Genre`, `Situation`) and its tags.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TagCategory {
    /// What it's called.
    pub name: String,
    /// Its tags, in rekordbox's order.
    pub tags: Vec<TagEntry>,
}

/// A My Tag and the tracks that have it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TagEntry {
    /// What it's called.
    pub name: String,
    /// Its tracks' ids, in rekordbox's order.
    pub track_ids: Vec<u32>,
}

/// What `master.db` holds, in numbers.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CollectionInfo {
    /// The `master.db`.
    pub path: PathBuf,
    /// Size of `master.db`, and of its WAL when there is one.
    pub bytes: u64,
    /// Size of its WAL, when there is one.
    pub wal_bytes: Option<u64>,
    /// `djmdProperty.DBVersion`, when there is one.
    pub db_version: Option<String>,
    /// Tracks, playlists and the rest, counted.
    pub summary: LibrarySummary,
    /// History lists.
    pub history_lists: usize,
    /// Paths of intelligent playlists (not evaluated, so listed empty).
    pub smart_playlists: Vec<String>,
    /// Hot cues.
    pub hot_cues: usize,
    /// Memory cues that aren't loops.
    pub memory_cues: usize,
    /// Loops, hot or memory.
    pub loops: usize,
    /// My Tag categories.
    pub tag_categories: usize,
    /// My Tags.
    pub tags: usize,
    /// Tracks with at least one My Tag.
    pub tagged_tracks: usize,
}

/// Read a `master.db` (and its `-wal`, if any). `key` is the SQLCipher passphrase (see
/// [`resolve_key`]); a plain, already decrypted SQLite file is read as it is.
///
/// The decrypted copy is kept in memory, so nothing here writes it to disk. Two kinds go through a
/// file in a [`crate::scratch::Folder`] instead: a plain `master.db` (already plain on disk), as
/// SQLite replays a plain WAL itself, and a decrypted one of 2 GiB or more, more than SQLite takes
/// into memory.
pub fn open(path: &Path, key: &str) -> Result<Collection> {
    open_db(path, key)?.collection()
}

/// The most SQLite takes into memory at once (`SQLITE_MAX_ALLOCATION_SIZE`, which the bundled
/// SQLite can't raise): a decrypted `master.db` any bigger is read from a file instead.
pub(crate) const IN_MEMORY_LIMIT: usize = 2_147_483_391;

/// [`open`]'s database: in memory, or in a private folder for a plain one or one too big.
pub(crate) fn open_db<'a>(path: &'a Path, key: &str) -> Result<Opened<'a>> {
    open_db_within(path, key, IN_MEMORY_LIMIT)
}

/// [`open_db`], reading a decrypted database of up to `limit` bytes in memory.
fn open_db_within<'a>(path: &'a Path, key: &str, limit: usize) -> Result<Opened<'a>> {
    crate::scratch::sweep();
    // One too big for memory goes to a file a page at a time, never whole into memory. (Its WAL
    // can still grow it past the limit: that one is written from memory below.)
    if std::fs::metadata(path).at(path)?.len() > limit as u64 {
        let folder = crate::scratch::folder()?;
        let (db, _) = open_copy(path, key, &folder, false)?;
        return Ok(Opened { db, _folder: Some(folder) });
    }
    match decrypt(path, key)? {
        Image::Decrypted(mut plain) if plain.len() <= limit => {
            let mut conn = Connection::open_in_memory_with_flags(
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|e| Error::parse(path, e))?;
            let len = plain.len();
            conn.deserialize_read_exact(rusqlite::MAIN_DB, Moving(&mut plain), len, true)
                .map_err(|e| Error::parse(path, e))?;
            drop(plain);
            Ok(Opened { db: checked(conn, path)?, _folder: None })
        }
        image => {
            let folder = crate::scratch::folder()?;
            let db = open_image(path, image, &folder)?;
            Ok(Opened { db, _folder: Some(folder) })
        }
    }
}

/// Reads its bytes out a page at a time and zeroes each once read: SQLite copies a database it
/// deserializes into memory of its own, so only that copy is ever whole.
struct Moving<'a>(&'a mut [u8]);

impl Read for Moving<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.0.len()).min(cipher::PAGE_SIZE);
        let (read, rest) = std::mem::take(&mut self.0).split_at_mut(n);
        buf[..n].copy_from_slice(read);
        read.zeroize();
        self.0 = rest;
        Ok(n)
    }
}

/// A database [`open_db`] opened, with the private folder it's in when it's a file.
pub(crate) struct Opened<'a> {
    pub(crate) db: Db<'a>,
    // Dropped after `db`: the folder goes once SQLite has closed the file.
    _folder: Option<crate::scratch::Folder>,
}

impl<'a> std::ops::Deref for Opened<'a> {
    type Target = Db<'a>;
    fn deref(&self) -> &Db<'a> {
        &self.db
    }
}

impl Opened<'_> {
    fn collection(&self) -> Result<Collection> {
        self.db.collection()
    }
}

/// The salt a file is encrypted with (its first bytes).
type Salt = [u8; cipher::SALT_LEN];

/// [`decrypt_into`] of `path`, into `folder` (a private one, as it holds the whole library
/// decrypted), and opened: read only unless `write` (or a plain copy's WAL is to be replayed,
/// which SQLite does itself), and only once SQLite finds nothing amiss and it has a
/// `djmdContent` table. With the salt it was encrypted with, unless it was plain.
#[cfg_attr(not(any(feature = "files", test)), allow(dead_code, reason = "only exports and tests write a copy yet"))]
pub(crate) fn open_copy<'a>(
    path: &'a Path,
    key: &str,
    folder: &crate::scratch::Folder,
    write: bool,
) -> Result<(Db<'a>, Option<Salt>)> {
    let (copy, salt) = decrypt_into(path, key, folder)?;
    Ok((open_file(path, &copy, write || salt.is_none())?, salt))
}

/// [`open_db`]'s `image` of `path` when it's read from a file, written into `folder` and opened
/// read only (unless it's plain: SQLite replays a plain copy's WAL itself, which takes write
/// access to the copy, never to the original).
fn open_image<'a>(path: &'a Path, image: Image, folder: &crate::scratch::Folder) -> Result<Db<'a>> {
    let (copy, write) = match image {
        Image::Decrypted(plain) => {
            let out = folder.path().join("master.db");
            std::fs::write(&out, &*plain).at(&out)?;
            (out, false)
        }
        Image::Plain(bytes, wal) => (write_plain(folder.path(), &bytes, wal.as_deref())?, true),
    };
    open_file(path, &copy, write)
}

/// The copy of `path` at `copy` opened, for writing when `write`, once [`checked`].
fn open_file<'a>(path: &'a Path, copy: &Path, write: bool) -> Result<Db<'a>> {
    let writes = if write { OpenFlags::SQLITE_OPEN_READ_WRITE } else { OpenFlags::SQLITE_OPEN_READ_ONLY };
    let conn = Connection::open_with_flags(copy, writes | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| Error::parse(path, e))?;
    checked(conn, path)
}

/// `conn`, the copy of `path`, once SQLite finds nothing amiss and it has a `djmdContent` table.
/// Its temporary tables and sorts are kept in memory: SQLite would otherwise spill rows of the
/// decrypted copy to temporary files.
fn checked(conn: Connection, path: &Path) -> Result<Db<'_>> {
    conn.execute_batch("PRAGMA temp_store = MEMORY").map_err(|e| Error::parse(path, e))?;
    // Anything that can write rekordbox's folder could have written this, and any path can be
    // given. Reads run only SELECTs, which fire no trigger. The one write, taking the cloud
    // agent's sign-in out of a copy (`files::export`), refuses a file with a trigger first. Views
    // aren't refused, as rekordbox may make some.
    let why = distrust(&conn, &[], "rekordbox")
        .and_then(|why| Ok(why.or(loclib_sqlcipher::untrusted::damaged(&conn)?)))
        .map_err(|e| Error::parse(path, e))?;
    if let Some(why) = why {
        return Err(Error::parse(path, why));
    }
    let db = Db { conn, path };
    if db.columns("djmdContent")?.is_empty() {
        return Err(Error::parse(path, "no djmdContent table; not a rekordbox collection database"));
    }
    Ok(db)
}

/// A plain `master.db` (`bytes`) written into `dir`, with its WAL beside it when there is one.
fn write_plain(dir: &Path, bytes: &[u8], wal: Option<&[u8]>) -> Result<PathBuf> {
    let out = dir.join("master.db");
    std::fs::write(&out, bytes).at(&out)?;
    if let Some(w) = wal {
        let copy = PathBuf::from(format!("{}-wal", out.display()));
        std::fs::write(&copy, w).at(&copy)?;
    }
    Ok(out)
}

/// `master.db` as read into memory.
enum Image {
    /// Decrypted, with the WAL's committed pages applied: a plain database on its own, zeroed
    /// when dropped.
    Decrypted(Zeroizing<Vec<u8>>),
    /// The file was plain SQLite already, with its WAL, if any.
    Plain(Vec<u8>, Option<Vec<u8>>),
}

/// `path` (and its WAL) read into memory and decrypted there, the WAL's committed transactions
/// replayed, so SQLite reads the result on its own. Nothing is written to disk, and each
/// decrypted buffer is zeroed once done with.
fn decrypt(path: &Path, key: &str) -> Result<Image> {
    // Read whole first: rekordbox may be writing, and nothing here holds its files open for long.
    let read = std::cell::RefCell::new(Vec::new());
    let wal = snapshot(path, || {
        let bytes = std::fs::read(path)?;
        let len = bytes.len() as u64;
        *read.borrow_mut() = bytes;
        Ok(len)
    })?;
    let mut db = Zeroizing::new(read.into_inner());
    if db.len() >= cipher::PAGE_SIZE && &db[..16] == cipher::SQLITE_MAGIC {
        // Already plain (decrypted with sqlcipher, say): use it as it is.
        return Ok(Image::Plain(std::mem::take(&mut *db), wal));
    }
    let len = db.len() as u64;
    decrypt_pages(path, key, len, wal.as_deref(), &mut InPlace(&mut db))?;
    Ok(Image::Decrypted(db))
}

/// `path` (and its WAL) copied into `folder` and decrypted there a page at a time into
/// `master.db`, the WAL's committed transactions replayed: [`decrypt`] without holding the
/// library in memory, for what SQLite needs as a file anyway. The encrypted copy is removed once
/// read. Answers the decrypted file, and the salt the original was encrypted with, unless it
/// was plain (its WAL is then beside it, for SQLite to replay).
#[cfg_attr(not(any(feature = "files", test)), allow(dead_code, reason = "only exports and tests write a copy yet"))]
fn decrypt_into(path: &Path, key: &str, folder: &crate::scratch::Folder) -> Result<(PathBuf, Option<Salt>)> {
    let (copy, out) = (folder.path().join("master.db.enc"), folder.path().join("master.db"));
    let wal = snapshot(path, || std::fs::copy(path, &copy))?;
    let mut from = std::fs::File::open(&copy).at(&copy)?;
    let len = from.metadata().at(&copy)?.len();
    let mut magic = [0u8; 16];
    if len >= cipher::PAGE_SIZE as u64 && from.read_exact(&mut magic).is_ok() && magic[..] == cipher::SQLITE_MAGIC[..] {
        drop(from);
        std::fs::rename(&copy, &out).at(&out)?;
        if let Some(w) = wal {
            let wal_copy = PathBuf::from(format!("{}-wal", out.display()));
            std::fs::write(&wal_copy, w).at(&wal_copy)?;
        }
        return Ok((out, None));
    }
    let to = std::fs::File::create_new(&out).at(&out)?;
    let salt = decrypt_pages(path, key, len, wal.as_deref(), &mut Copied { from, to })?;
    std::fs::remove_file(&copy).at(&copy)?;
    Ok((out, Some(salt)))
}

/// Where [`decrypt_pages`] reads `master.db`'s encrypted pages (numbered from 1) and writes the
/// decrypted ones.
trait Pages {
    /// Encrypted page `pgno` into `page`.
    fn read(&mut self, pgno: u32, page: &mut [u8]) -> std::io::Result<()>;
    /// Decrypted page `pgno`.
    fn write(&mut self, pgno: u32, page: &[u8]) -> std::io::Result<()>;
    /// The decrypted database made `pages` long, cut or grown with zeros, before any page of it
    /// is written: growing it later could leave decrypted pages behind in memory it moved out of.
    fn resize(&mut self, pages: usize) -> std::io::Result<()>;
}

/// The whole file in memory, each page decrypted where it is.
struct InPlace<'a>(&'a mut Vec<u8>);

impl Pages for InPlace<'_> {
    fn read(&mut self, pgno: u32, page: &mut [u8]) -> std::io::Result<()> {
        let at = (pgno as usize - 1) * page.len();
        page.copy_from_slice(&self.0[at..at + page.len()]);
        Ok(())
    }

    fn write(&mut self, pgno: u32, page: &[u8]) -> std::io::Result<()> {
        let at = (pgno as usize - 1) * page.len();
        self.0[at..at + page.len()].copy_from_slice(page);
        Ok(())
    }

    fn resize(&mut self, pages: usize) -> std::io::Result<()> {
        self.0.resize(pages * cipher::PAGE_SIZE, 0);
        Ok(())
    }
}

/// An encrypted copy on disk, decrypted into another file.
struct Copied {
    from: std::fs::File,
    to: std::fs::File,
}

impl Pages for Copied {
    fn read(&mut self, pgno: u32, page: &mut [u8]) -> std::io::Result<()> {
        self.from.seek(std::io::SeekFrom::Start(u64::from(pgno - 1) * page.len() as u64))?;
        self.from.read_exact(page)
    }

    fn write(&mut self, pgno: u32, page: &[u8]) -> std::io::Result<()> {
        self.to.seek(std::io::SeekFrom::Start(u64::from(pgno - 1) * page.len() as u64))?;
        self.to.write_all(page)
    }

    fn resize(&mut self, pages: usize) -> std::io::Result<()> {
        self.to.set_len((pages * cipher::PAGE_SIZE) as u64)
    }
}

/// Decrypts the `len`-byte `master.db` at `path` (encrypted, as read into `pages`) with its WAL's
/// committed pages applied, writing the result into `pages`. Answers the salt it was encrypted
/// with. Each decrypted page is zeroed once written.
fn decrypt_pages(path: &Path, key: &str, len: u64, wal: Option<&[u8]>, pages: &mut impl Pages) -> Result<Salt> {
    let page_size = cipher::PAGE_SIZE;
    if len < page_size as u64 {
        return Err(Error::parse(path, "too short for a SQLite database"));
    }
    if !len.is_multiple_of(page_size as u64) {
        return Err(Error::parse(path, format!("{len} bytes is not a whole number of 4096-byte pages")));
    }
    let io = |e: std::io::Error| Error::Io { path: path.to_path_buf(), source: e };
    let mut page = Zeroizing::new(vec![0u8; page_size]);
    pages.read(1, &mut page).map_err(io)?;
    let salt: Salt = page[..cipher::SALT_LEN].try_into().expect("16 bytes");
    let codec = cipher::Codec::with_key(key.as_bytes(), salt);
    let damaged = |e: loclib_sqlcipher::Error| Error::parse(path, e);
    // A wrong key fails the first page's HMAC.
    let wrong_key = || Error::WrongKey(path.to_path_buf());
    let page1 = Zeroizing::new(codec.decrypt_page(1, &page).map_err(|_| wrong_key())?);
    let committed = wal.map(|w| wal::committed(w, page_size)).transpose()?.flatten();
    let main_pages = (len / page_size as u64) as usize;
    let replayed = match &committed {
        Some(c) => loclib_sqlcipher::wal_pages(&codec, c, main_pages).map_err(damaged)?,
        None => Vec::new(),
    };
    let replayed: Vec<(u32, Zeroizing<Vec<u8>>)> = replayed.into_iter().map(|(n, p)| (n, Zeroizing::new(p))).collect();
    let total = committed.as_ref().map_or(main_pages, |c| c.db_pages as usize);
    pages.resize(total).map_err(io)?;
    let zeros = vec![0u8; page_size];
    for pgno in 2..=main_pages.min(total) as u32 {
        if !loclib_sqlcipher::reads_main_page(pgno, committed.as_ref()) {
            pages.write(pgno, &zeros).map_err(io)?;
            continue;
        }
        pages.read(pgno, &mut page).map_err(io)?;
        let plain = Zeroizing::new(codec.decrypt_page(pgno, &page).map_err(damaged)?);
        pages.write(pgno, &plain).map_err(io)?;
    }
    for (pgno, plain) in replayed.iter().filter(|(n, _)| *n != 1) {
        pages.write(*pgno, plain).map_err(io)?;
    }
    // Page 1 last, as the WAL left it. One that authenticates but isn't laid out for SQLCipher's
    // reserved bytes is no rekordbox collection under that key either. Header bytes 18-19 say WAL
    // mode (2); the copy, without a WAL, reads with a rollback journal (1).
    let mut first = replayed.iter().find(|(n, _)| *n == 1).map_or_else(|| page1.clone(), |(_, p)| p.clone());
    cipher::check_plain_header(&first).map_err(|_| wrong_key())?;
    first[18..20].copy_from_slice(&[1, 1]);
    pages.write(1, &first).map_err(io)?;
    Ok(salt)
}

/// Copy `path` with `copy` and read its WAL, both as of one moment. rekordbox may checkpoint and
/// then restart its WAL in between: the copy would lack what the old WAL held and the new one
/// doesn't. A restart changes the WAL header's checkpoint sequence and salts, so the header is
/// read before copying and again after the WAL is read, and the copy made again when either
/// differs from the header the WAL was read with.
fn snapshot(path: &Path, copy: impl Fn() -> std::io::Result<u64>) -> Result<Option<Vec<u8>>> {
    let wal_path = PathBuf::from(format!("{}-wal", path.display()));
    snapshot_with(path, copy, |limit| read_wal(&wal_path, limit))
}

/// At most `limit` bytes of the WAL at `wal_path`, or `None` when there's none (or it's empty).
fn read_wal(wal_path: &Path, limit: u64) -> Result<Option<Vec<u8>>> {
    let file = match std::fs::File::open(wal_path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io { path: wal_path.to_path_buf(), source: e }),
    };
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).at(wal_path)?;
    Ok((!bytes.is_empty()).then_some(bytes))
}

/// [`snapshot`], reading the WAL with `read_wal` (at most `limit` bytes of it, or `None` when
/// there's none).
fn snapshot_with(
    path: &Path,
    copy: impl Fn() -> std::io::Result<u64>,
    read_wal: impl Fn(u64) -> Result<Option<Vec<u8>>>,
) -> Result<Option<Vec<u8>>> {
    let generation = |wal: &Option<Vec<u8>>| wal.as_ref().and_then(|w| w.get(12..24)).map(<[u8]>::to_vec);
    for _ in 0..3 {
        let before = generation(&read_wal(24)?);
        copy().at(path)?;
        let wal = read_wal(u64::MAX)?;
        // A restart while the WAL was read, after its header was, shows only in a later header:
        // a restart writes the new header before any frame.
        let after = generation(&read_wal(24)?);
        if generation(&wal) == before && after == before {
            return Ok(wal);
        }
    }
    Err(Error::Changing(path.to_path_buf()))
}

/// The decrypted copy, opened read only.
pub(crate) struct Db<'a> {
    pub(crate) conn: Connection,
    path: &'a Path,
}

impl Db<'_> {
    fn sql(&self, e: rusqlite::Error) -> Error {
        Error::parse(self.path, e)
    }

    /// A table's columns, lower-cased; empty when there's no such table.
    fn columns(&self, table: &str) -> Result<FxHashSet<String>> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info(\"{table}\")")).map_err(|e| self.sql(e))?;
        let names = stmt.query_map([], |r| r.get::<_, String>(1)).map_err(|e| self.sql(e))?;
        names.map(|n| n.map(|n| n.to_lowercase()).map_err(|e| self.sql(e))).collect()
    }

    /// `wanted` columns of every row rekordbox hasn't deleted, in storage order; columns the table
    /// lacks come back NULL, and a missing table has no rows.
    fn select(&self, table: &str, wanted: &[&str]) -> Result<Vec<Vec<Value>>> {
        self.rows(table, wanted, false, "")
    }

    /// [`Db::select`], with the rows rekordbox deleted too when `deleted`, and only those `then`
    /// (an SQL `LIMIT` clause) leaves.
    fn rows(&self, table: &str, wanted: &[&str], deleted: bool, then: &str) -> Result<Vec<Vec<Value>>> {
        let cols = self.columns(table)?;
        if cols.is_empty() {
            return Ok(Vec::new());
        }
        let list: Vec<String> = wanted
            .iter()
            .map(|c| if cols.contains(&c.to_lowercase()) { format!("\"{c}\"") } else { "NULL".into() })
            .collect();
        let filter = match !deleted && cols.contains("rb_local_deleted") {
            true => " WHERE IFNULL(rb_local_deleted, 0) = 0",
            false => "",
        };
        let sql = format!("SELECT {} FROM \"{table}\"{filter} ORDER BY rowid{then}", list.join(", "));
        let mut stmt = self.conn.prepare(&sql).map_err(|e| self.sql(e))?;
        let n = wanted.len();
        let rows = stmt
            .query_map([], |r| (0..n).map(|i| r.get::<_, Value>(i)).collect::<rusqlite::Result<Vec<Value>>>())
            .map_err(|e| self.sql(e))?;
        rows.map(|r| r.map_err(|e| self.sql(e))).collect()
    }

    /// `djmdProperty`'s `column` (`DBID`, `DBVersion`), when it's there and isn't empty.
    fn property(&self, column: &str) -> Result<Option<String>> {
        Ok(self.select("djmdProperty", &[column])?.first().map(|r| text(&r[0])).filter(|v| !v.is_empty()))
    }

    /// `ID` -> `column` of a lookup table.
    fn names(&self, table: &str, column: &str) -> Result<FxHashMap<String, String>> {
        Ok(self.select(table, &["ID", column])?.into_iter().map(|r| (text(&r[0]), text(&r[1]))).collect())
    }

    fn collection(&self) -> Result<Collection> {
        let artists = self.names("djmdArtist", "Name")?;
        let genres = self.names("djmdGenre", "Name")?;
        let labels = self.names("djmdLabel", "Name")?;
        let keys = self.names("djmdKey", "ScaleName")?;
        let albums: FxHashMap<String, (String, String)> = self
            .select("djmdAlbum", &["ID", "Name", "AlbumArtistID"])?
            .into_iter()
            .map(|r| (text(&r[0]), (text(&r[1]), text(&r[2]))))
            .collect();
        let name = |map: &FxHashMap<String, String>, id: &Value| map.get(&text(id)).cloned().unwrap_or_default();

        const CONTENT: &[&str] = &[
            "ID",
            "FolderPath",
            "Title",
            "ArtistID",
            "AlbumID",
            "GenreID",
            "BPM",
            "Length",
            "TrackNo",
            "BitRate",
            "BitDepth",
            "Commnt",
            "Rating",
            "ReleaseYear",
            "RemixerID",
            "LabelID",
            "OrgArtistID",
            "KeyID",
            "StockDate",
            "ColorID",
            "DJPlayCount",
            "ImagePath",
            "AnalysisDataPath",
            "FileSize",
            "DiscNo",
            "ComposerID",
            "Subtitle",
            "SampleRate",
            "ReleaseDate",
            "DateCreated",
            "ISRC",
            "created_at",
        ];
        let rows = self.select("djmdContent", CONTENT)?;
        let track_ids = id_map(rows.iter().map(|r| text(&r[0])));
        let mut images: FxHashMap<String, u32> = FxHashMap::default();
        let mut tracks = Vec::with_capacity(rows.len());
        for r in &rows {
            let v = |c: &str| &r[CONTENT.iter().position(|x| *x == c).expect("known column")];
            let album = albums.get(&text(v("AlbumID")));
            let image = text(v("ImagePath"));
            let artwork_id = (!image.is_empty()).then(|| {
                let n = images.len() as u32 + 1;
                *images.entry(image.clone()).or_insert(n)
            });
            let date_added = [text(v("StockDate")), text(v("DateCreated")), text(v("created_at"))]
                .into_iter()
                .find(|d| !d.is_empty())
                .map(|d| d.get(..10).unwrap_or(&d).to_string())
                .unwrap_or_default();
            tracks.push(Track {
                id: track_ids[&text(v("ID"))],
                title: text(v("Title")),
                artist: name(&artists, v("ArtistID")),
                album: album.map(|a| a.0.clone()).unwrap_or_default(),
                album_artist: album.and_then(|a| artists.get(&a.1)).cloned().unwrap_or_default(),
                genre: name(&genres, v("GenreID")),
                label: name(&labels, v("LabelID")),
                key: name(&keys, v("KeyID")),
                remixer: name(&artists, v("RemixerID")),
                original_artist: name(&artists, v("OrgArtistID")),
                composer: name(&artists, v("ComposerID")),
                comment: text(v("Commnt")),
                mix_name: text(v("Subtitle")),
                isrc: text(v("ISRC")),
                bpm: int(v("BPM")) as f64 / 100.0,
                duration_secs: clamp(int(v("Length"))),
                year: clamp(int(v("ReleaseYear"))),
                track_number: clamp(int(v("TrackNo"))),
                disc_number: clamp(int(v("DiscNo"))),
                rating: int(v("Rating")).clamp(0, 5) as u8,
                color: TrackColor::from_id(int(v("ColorID")).clamp(0, 255) as u8),
                play_count: clamp(int(v("DJPlayCount"))),
                bitrate: clamp(int(v("BitRate"))),
                sample_rate: clamp(int(v("SampleRate"))),
                sample_depth: clamp(int(v("BitDepth"))),
                file_size: int(v("FileSize")).max(0) as u64,
                file_path: text(v("FolderPath")),
                analyze_path: text(v("AnalysisDataPath")),
                date_added,
                release_date: text(v("ReleaseDate")),
                artwork_id,
                artwork_path: (!image.is_empty()).then_some(image),
            });
        }
        tracks.sort_by_key(|t| t.id);

        let (playlists, smart) = self.tree("djmdPlaylist", "djmdSongPlaylist", "PlaylistID", &track_ids)?;
        let (history, _) = self.tree("djmdHistory", "djmdSongHistory", "HistoryID", &track_ids)?;
        let tag_paths = self.tag_paths()?;
        let smart_lists = smart
            .iter()
            .filter_map(|(id, xml)| Some((*id, SmartList::parse(xml, |tag| tag_paths.get(tag).cloned())?)))
            .collect();
        let smart_playlists = smart.into_iter().map(|(id, _)| id).collect();
        let library =
            Library { tracks, playlists, history_playlists: history.iter().filter(|h| !h.is_folder).count(), history };

        let mut cues: IndexMap<u32, Vec<Cue>> = IndexMap::new();
        for r in self.select("djmdCue", &["ContentID", "InMsec", "OutMsec", "Kind", "ColorTableIndex", "Comment"])? {
            let Some(&track) = track_ids.get(&text(&r[0])) else { continue };
            let start = int(&r[1]).max(0);
            let end = int(&r[2]);
            let kind = int(&r[3]);
            let hot_cue = hot_cue_from_kind(kind);
            let color_code = int(&r[4]);
            cues.entry(track).or_default().push(Cue {
                hot_cue,
                start_ms: clamp(start),
                end_ms: (end > start).then(|| clamp(end)),
                comment: text(&r[5]),
                color: hot_cue.and_then(|_| u8::try_from(color_code).ok()).and_then(hot_cue_palette),
                // Memory cue colours, stored loop lengths and active loops aren't read from
                // master.db yet.
                color_id: 0,
                loop_beats: None,
                active_loop: false,
            });
        }
        for list in cues.values_mut() {
            list.sort_by_key(|c| (c.hot_cue.map_or(0, |h| h as u32 + 1), c.start_ms));
        }
        cues.sort_unstable_keys();

        Ok(Collection {
            path: self.path.to_path_buf(),
            db_version: self.property("DBVersion")?,
            library,
            cues,
            tags: self.tags(&track_ids)?,
            smart_playlists,
            smart_lists,
            sync_service: self
                .select("djmdCloudProperty", &["Reserved1", "Reserved2"])?
                .into_iter()
                .find(|r| text(&r[0]) == "SyncService")
                .map(|r| text(&r[1]))
                .filter(|v| !v.is_empty()),
            synced_sizes: self
                .select("contentFile", &["Path", "Size"])?
                .into_iter()
                .map(|r| (text(&r[0]).to_lowercase(), int(&r[1]).max(0) as u64))
                .collect(),
        })
    }

    /// A playlist-shaped tree and its entries (`djmdPlaylist` + `djmdSongPlaylist`, or the
    /// history tables), plus the intelligent playlists (see [`Smart`]).
    fn tree(
        &self,
        table: &str,
        entries: &str,
        list_column: &str,
        tracks: &FxHashMap<String, u32>,
    ) -> Result<(Vec<Playlist>, Smart)> {
        let rows = self.select(table, &["ID", "Seq", "Name", "Attribute", "ParentID", "SmartList"])?;
        let ids = id_map(rows.iter().map(|r| text(&r[0])));
        let mut songs: FxHashMap<u32, Vec<(i64, usize, u32)>> = FxHashMap::default();
        for (n, r) in self.select(entries, &[list_column, "ContentID", "TrackNo"])?.into_iter().enumerate() {
            if let (Some(&list), Some(&track)) = (ids.get(&text(&r[0])), tracks.get(&text(&r[1]))) {
                songs.entry(list).or_default().push((int(&r[2]), n, track));
            }
        }
        let mut smart = Vec::new();
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let id = ids[&text(&r[0])];
            let attribute = int(&r[3]);
            if attribute == 4 {
                smart.push((id, text(&r[5])));
            }
            let mut entries = songs.remove(&id).unwrap_or_default();
            // rekordbox leaves entries on some folders, which it doesn't show; a folder holds none.
            if attribute == 1 {
                entries.clear();
            }
            entries.sort_unstable();
            out.push(Playlist {
                id,
                parent_id: ids.get(&text(&r[4])).copied(),
                name: text(&r[2]),
                is_folder: attribute == 1,
                sort_order: clamp(int(&r[1])),
                track_ids: entries.into_iter().map(|(_, _, t)| t).collect(),
            });
        }
        Ok((untangle(out), smart))
    }

    /// Each My Tag's `Category/Tag` path by its `djmdMyTag` id, as intelligent playlists name them.
    fn tag_paths(&self) -> Result<FxHashMap<String, String>> {
        let rows = self.select("djmdMyTag", &["ID", "Name", "ParentID"])?;
        let categories: FxHashMap<String, String> =
            rows.iter().filter(|r| is_root(&text(&r[2]))).map(|r| (text(&r[0]), text(&r[1]))).collect();
        Ok(rows
            .iter()
            .filter_map(|r| Some((text(&r[0]), format!("{}/{}", categories.get(&text(&r[2]))?, text(&r[1])))))
            .collect())
    }

    /// My Tags: categories hang off `"root"`, tags off their category.
    fn tags(&self, tracks: &FxHashMap<String, u32>) -> Result<Vec<TagCategory>> {
        let rows = self.select("djmdMyTag", &["ID", "Seq", "Name", "ParentID"])?;
        let mut links: FxHashMap<String, Vec<(i64, usize, u32)>> = FxHashMap::default();
        for (n, r) in self.select("djmdSongMyTag", &["MyTagID", "ContentID", "TrackNo"])?.into_iter().enumerate() {
            if let Some(&track) = tracks.get(&text(&r[1])) {
                links.entry(text(&r[0])).or_default().push((int(&r[2]), n, track));
            }
        }
        let mut categories: Vec<(i64, String, String)> =
            rows.iter().filter(|r| is_root(&text(&r[3]))).map(|r| (int(&r[1]), text(&r[0]), text(&r[2]))).collect();
        categories.sort();
        Ok(categories
            .into_iter()
            .map(|(_, id, name)| {
                let mut tags: Vec<(i64, String, String)> =
                    rows.iter().filter(|r| text(&r[3]) == id).map(|r| (int(&r[1]), text(&r[0]), text(&r[2]))).collect();
                tags.sort();
                TagCategory {
                    name,
                    tags: tags
                        .into_iter()
                        .map(|(_, tag, name)| {
                            let mut l = links.remove(&tag).unwrap_or_default();
                            l.sort_unstable();
                            let mut seen = FxHashSet::default();
                            TagEntry {
                                name,
                                track_ids: l.into_iter().map(|x| x.2).filter(|t| seen.insert(*t)).collect(),
                            }
                        })
                        .collect(),
                }
            })
            .collect())
    }
}

/// Intelligent playlists by id, each with its `SmartList` column.
type Smart = Vec<(u32, String)>;

fn is_root(parent: &str) -> bool {
    parent.is_empty() || parent.eq_ignore_ascii_case("root")
}

/// rekordbox's text ids as the model's numbers: the ids themselves when every one is a distinct,
/// non-zero 32-bit number, otherwise 1, 2, 3... in order.
fn id_map(ids: impl Iterator<Item = String>) -> FxHashMap<String, u32> {
    let ids: Vec<String> = ids.collect();
    let parsed: Vec<Option<u32>> = ids.iter().map(|s| s.trim().parse().ok().filter(|n| *n != 0)).collect();
    let distinct: FxHashSet<u32> = parsed.iter().flatten().copied().collect();
    if parsed.iter().all(Option::is_some) && distinct.len() == ids.len() {
        ids.into_iter().zip(parsed.into_iter().flatten()).collect()
    } else {
        ids.into_iter().zip(1u32..).collect()
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::Text(t) => t.clone(),
        Value::Integer(i) => i.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        Value::Null => String::new(),
    }
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Integer(i) => *i,
        Value::Real(f) => *f as i64,
        Value::Text(t) => {
            t.trim().parse::<i64>().ok().or_else(|| t.trim().parse::<f64>().ok().map(|f| f as i64)).unwrap_or(0)
        }
        _ => 0,
    }
}

/// A number into a narrower unsigned field, clamped.
fn clamp<T: TryFrom<i64> + Bounded>(n: i64) -> T {
    T::try_from(n.max(0)).unwrap_or(T::MAX)
}

trait Bounded {
    const MAX: Self;
}

impl Bounded for u16 {
    const MAX: Self = u16::MAX;
}

impl Bounded for u32 {
    const MAX: Self = u32::MAX;
}

impl Collection {
    /// Counts for `collection info`.
    pub fn info(&self) -> CollectionInfo {
        let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
        let all = || self.cues.values().flatten();
        let mut tagged = FxHashSet::default();
        for t in self.tags.iter().flat_map(|c| &c.tags) {
            tagged.extend(t.track_ids.iter().copied());
        }
        CollectionInfo {
            path: self.path.clone(),
            bytes: size(&self.path).unwrap_or(0),
            wal_bytes: size(&PathBuf::from(format!("{}-wal", self.path.display()))),
            db_version: self.db_version.clone(),
            summary: self.library.summary(),
            history_lists: self.library.history_playlists,
            smart_playlists: self.smart_playlists.iter().map(|id| self.library.playlist_path(*id)).collect(),
            hot_cues: all().filter(|c| c.hot_cue.is_some()).count(),
            memory_cues: all().filter(|c| c.hot_cue.is_none() && c.end_ms.is_none()).count(),
            loops: all().filter(|c| c.end_ms.is_some()).count(),
            tag_categories: self.tags.len(),
            tags: self.tags.iter().map(|c| c.tags.len()).sum(),
            tagged_tracks: tagged.len(),
        }
    }

    /// rekordbox's `share` folder beside `master.db`, which `AnalysisDataPath` and `ImagePath`
    /// are paths in.
    pub fn share_dir(&self) -> PathBuf {
        share_dir(&self.path)
    }

    /// Where a file the collection names in its `share` folder is on this computer.
    pub fn share_file(&self, path: &str) -> PathBuf {
        paths::to_fs(&self.share_dir(), &paths::key(path))
    }

    /// Whether Cloud Library Sync is still bringing `path` (in the `share` folder) to this
    /// computer: it lists the file with a size the copy here doesn't have yet.
    pub fn still_syncing(&self, path: &str) -> bool {
        let Some(&size) = self.synced_sizes.get(&path.to_lowercase()) else { return false };
        std::fs::metadata(self.share_file(path)).map_or(true, |m| m.len() != size)
    }

    /// Where `track`'s audio file is on this computer: its path as it is, or, for a track Cloud
    /// Library Sync keeps in a storage service (`/contents_<id>/...`), that path in the service's
    /// `rekordbox` folder, the first of `cloud`'s that has it. `None` when it's in none of them.
    pub fn audio_file(&self, track: &Track, cloud: &CloudFolders) -> Option<PathBuf> {
        let path = &track.file_path;
        if !in_cloud(path) {
            return Some(PathBuf::from(path));
        }
        cloud
            .for_service(self.sync_service.as_deref())
            .into_iter()
            .map(|folder| paths::to_fs(&folder.join("rekordbox"), &paths::key(path)))
            .find(|p| p.exists())
    }

    /// The library with every file it names as a full path on this computer, so it reads like a
    /// drive's library whose drive is `/`: each track's audio where [`Collection::audio_file`]
    /// finds it (a cloud track that isn't here keeps its path, and reads as missing), and its
    /// analysis and artwork in the `share` folder. The player opens the collection this way.
    pub fn library_from_root(&self, cloud: &CloudFolders) -> Library {
        let mut lib = self.library.clone();
        let share = |p: &str| self.share_file(p).to_string_lossy().into_owned();
        for t in &mut lib.tracks {
            if let Some(audio) = self.audio_file(t, cloud) {
                t.file_path = audio.to_string_lossy().into_owned();
            }
            if !t.analyze_path.is_empty() {
                t.analyze_path = share(&t.analyze_path);
            }
            if let Some(art) = t.artwork_path.as_mut() {
                *art = share(art);
            }
        }
        lib
    }
}

#[cfg(feature = "files")]
#[cfg_attr(docsrs, doc(cfg(feature = "files")))]
pub mod files;
pub mod smartlist;

#[cfg(test)]
use loclib_testkit::sqlcipher;

#[cfg(test)]
mod tests;
