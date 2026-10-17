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
//! are copied to a temporary folder and decrypted there (the WAL's committed transactions
//! replayed, as for OneLibrary), and only that plain copy is opened, read only. rekordbox may keep
//! running; a read while it saves can miss its latest changes.
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
//!   playlist, 1 for a folder and 4 for an intelligent playlist, whose rules (`SmartList`) aren't
//!   evaluated here, so it reads as empty.
//! * `djmdCue.Kind` is 0 for a memory cue, 1-3 for hot cues A-C, 4 for a memory loop and 5 on for
//!   hot cues D onwards (checked against the analysis files of 1,594 tracks); `InMsec`/`OutMsec` are
//!   milliseconds, `OutMsec` -1 for a cue that isn't a loop. A hot cue's colour is taken to be
//!   `ColorTableIndex` as a code of rekordbox's hot cue palette, like OneLibrary's
//!   `colorTableIndex` (inferred, not documented).

use indexmap::IndexMap;

use rustc_hash::{FxHashMap, FxHashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};
use schemars::JsonSchema;
use serde::Serialize;

use crate::anlz::{self, Cue};
use crate::diff::{self, PlaylistDiff};
use crate::error::IoContext;
use crate::error::{Action, Reason};
use crate::import::{self, MatchKind, Matcher, Status};
use crate::model::{Library, LibrarySummary, Playlist, Track, TrackColor, TrackRef, untangle};
use crate::onelibrary::{self, cipher, wal};
use crate::paths;
use crate::{Error, Result};

/// The SQLCipher passphrase every rekordbox 6 and 7 installation uses for `master.db`, as
/// pyrekordbox publishes it.
pub const DEFAULT_KEY: &str = "402fd482c38817c35ffa8ffb8c7d93143b749e7d315df7a81732a1ff43608497";

/// Environment variable with another passphrase, should rekordbox change it.
pub const KEY_ENV: &str = "LOCALLIBRARY_MASTERDB_KEY";

/// Environment variable with the path of the `master.db` to read instead of rekordbox's.
pub const PATH_ENV: &str = "LOCALLIBRARY_MASTERDB";

/// Where rekordbox keeps `master.db` on this computer: [`PATH_ENV`] when set, otherwise
/// `~/Library/Pioneer/rekordbox/master.db` on macOS and `%APPDATA%\Pioneer\rekordbox\master.db`
/// on Windows. `None` elsewhere (rekordbox runs on neither).
pub fn default_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(PATH_ENV).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    platform_path()
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

/// The passphrase to use: `key` when given, else [`KEY_ENV`], else [`DEFAULT_KEY`].
pub fn resolve_key(key: Option<&str>) -> String {
    key.map(str::to_string)
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var(KEY_ENV).ok().filter(|k| !k.is_empty()))
        .unwrap_or_else(|| DEFAULT_KEY.to_string())
}

/// rekordbox's collection, read from `master.db`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Collection {
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
    /// Ids of intelligent playlists, whose rules aren't evaluated.
    pub smart_playlists: Vec<u32>,
    /// The storage service Cloud Library Sync keeps audio in (`Dropbox`), if it's on.
    pub sync_service: Option<String>,
    /// Sizes of the files Cloud Library Sync syncs (`contentFile`), by lower-cased path in the
    /// `share` folder (`/pioneer/usbanlz/...`).
    #[serde(skip)]
    #[schemars(skip)]
    pub synced_sizes: FxHashMap<String, u64>,
}

/// The folders Cloud Library Sync keeps audio in on this computer: rekordbox's `rekordbox` folder
/// sits in each. rekordbox records them in its settings (`rekordbox3.settings`), as
/// `DropboxSharingPath` and `GoogleDriveSharingPath`; nothing else in that file is read (it also
/// holds the services' sign-in tokens).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudFolders {
    pub dropbox: Option<PathBuf>,
    pub google_drive: Option<PathBuf>,
}

impl CloudFolders {
    /// The folders rekordbox on this computer recorded; none when it has no settings file.
    pub fn this_computer() -> CloudFolders {
        settings_path().and_then(|p| CloudFolders::from_settings(&p).ok()).unwrap_or_default()
    }

    /// The folders a `rekordbox3.settings` file names.
    pub fn from_settings(path: &Path) -> Result<CloudFolders> {
        let xml = std::fs::read_to_string(path).at(path)?;
        let mut reader = quick_xml::Reader::from_str(&xml);
        let mut out = CloudFolders::default();
        loop {
            let event = reader.read_event().map_err(|e| Error::parse(path, e))?;
            let e = match &event {
                quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e) => e,
                quick_xml::events::Event::Eof => break,
                _ => continue,
            };
            if e.name().as_ref() != "VALUE" {
                continue;
            }
            let attr = |name: &str| -> Option<String> {
                let a = e.attributes().flatten().find(|a| a.key.as_ref() == name)?;
                a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned())
            };
            let slot = match attr("name").as_deref() {
                Some("DropboxSharingPath") => &mut out.dropbox,
                Some("GoogleDriveSharingPath") => &mut out.google_drive,
                _ => continue,
            };
            *slot = attr("val").filter(|v| !v.is_empty()).map(PathBuf::from);
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

/// rekordbox 6 and 7's settings file: `~/Library/Application Support/Pioneer/rekordbox6/` on
/// macOS, `%APPDATA%\Pioneer\rekordbox6\` on Windows.
fn settings_path() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    // The application support folder on macOS, %APPDATA% (roaming) on Windows.
    let dir = if cfg!(any(target_os = "macos", windows)) { base.config_dir().to_path_buf() } else { return None };
    Some(dir.join("Pioneer").join("rekordbox6").join("rekordbox3.settings"))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TagCategory {
    pub name: String,
    pub tags: Vec<TagEntry>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TagEntry {
    pub name: String,
    pub track_ids: Vec<u32>,
}

/// What `master.db` holds, in numbers.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CollectionInfo {
    pub path: PathBuf,
    /// Size of `master.db`, and of its WAL when there is one.
    pub bytes: u64,
    pub wal_bytes: Option<u64>,
    pub db_version: Option<String>,
    pub summary: LibrarySummary,
    pub history_lists: usize,
    /// Paths of intelligent playlists (not evaluated, so listed empty).
    pub smart_playlists: Vec<String>,
    pub hot_cues: usize,
    pub memory_cues: usize,
    pub loops: usize,
    pub tag_categories: usize,
    pub tags: usize,
    pub tagged_tracks: usize,
}

/// Read a `master.db` (and its `-wal`, if any). `key` is the SQLCipher passphrase (see
/// [`resolve_key`]); a plain, already decrypted SQLite file is read as it is.
pub fn open(path: &Path, key: &str) -> Result<Collection> {
    let dir = tempfile::tempdir().map_err(|e| Error::Io { path: std::env::temp_dir(), source: e })?;
    let (copy, flags) = match decrypt_copy(path, key, dir.path())? {
        Copy::Decrypted(p) => (p, OpenFlags::SQLITE_OPEN_READ_ONLY),
        // SQLite replays a plain copy's WAL itself, which takes write access to the copy (never
        // to the original).
        Copy::Plain(p) => (p, OpenFlags::SQLITE_OPEN_READ_WRITE),
    };
    let conn = Connection::open_with_flags(&copy, flags | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| Error::parse(path, e))?;
    // Anything that can write rekordbox's folder could have written this, and any path can be
    // given. Only SELECTs run here, so no trigger fires; views aren't refused, as rekordbox may
    // make some.
    let why = onelibrary::distrust(&conn, &[], "rekordbox")
        .and_then(|why| Ok(why.or(onelibrary::damaged(&conn)?)))
        .map_err(|e| Error::parse(path, e))?;
    if let Some(why) = why {
        return Err(Error::parse(path, why));
    }
    let db = Db { conn, path };
    if db.columns("djmdContent")?.is_empty() {
        return Err(Error::parse(path, "no djmdContent table; not a rekordbox collection database"));
    }
    db.collection()
}

/// A working copy of `master.db` in a temporary folder.
enum Copy {
    /// Decrypted, with the WAL's committed pages applied: a plain database on its own.
    Decrypted(PathBuf),
    /// The file was plain SQLite already; copied with its WAL, if any.
    Plain(PathBuf),
}

/// Copy `path` (and its WAL) into `dir` and decrypt it there, replaying the WAL's committed
/// transactions, so SQLite reads the result on its own.
fn decrypt_copy(path: &Path, key: &str, dir: &Path) -> Result<Copy> {
    // Copies first: rekordbox may be writing, and nothing here holds its files open for long.
    let enc = dir.join("master.db.enc");
    let wal = snapshot(path, &enc, |from, to| std::fs::copy(from, to))?;
    let out = dir.join("master.db");
    let mut src = std::fs::File::open(&enc).at(&enc)?;
    let mut first = [0u8; cipher::PAGE_SIZE];
    let len = src.metadata().at(&enc)?.len();
    if len < cipher::PAGE_SIZE as u64 {
        return Err(Error::parse(path, "too short for a SQLite database"));
    }
    src.read_exact(&mut first).at(&enc)?;
    if &first[..16] == cipher::SQLITE_MAGIC {
        // Already plain (decrypted with sqlcipher, say): use the copy as it is.
        drop(src);
        std::fs::rename(&enc, &out).at(&out)?;
        if let Some(w) = wal {
            let copy = PathBuf::from(format!("{}-wal", out.display()));
            std::fs::write(&copy, w).at(&copy)?;
        }
        return Ok(Copy::Plain(out));
    }
    if !len.is_multiple_of(cipher::PAGE_SIZE as u64) {
        return Err(Error::parse(path, format!("{len} bytes is not a whole number of 4096-byte pages")));
    }
    let salt: [u8; cipher::SALT_LEN] = first[..cipher::SALT_LEN].try_into().expect("16 bytes");
    let codec = cipher::Codec::with_key(key.as_bytes(), salt);
    let wrong_key = || {
        Error::invalid(format!(
            "{} can't be decrypted with this key: it isn't a rekordbox 6/7 collection, or rekordbox changed its \
             key (give the right one, or set {KEY_ENV})",
            path.display()
        ))
        .because(Reason::WrongKey)
        .with("env", KEY_ENV)
        .action(Action::retry(serde_json::json!({ "key": null }), "Give the key"))
    };
    // The codec's errors name exportLibrary.db, whose pages it usually reads.
    let damaged = |e: Error| match e {
        Error::OneLibrary(m) => Error::parse(path, m),
        e => e,
    };
    let page1 = codec.decrypt_page(1, &first).map_err(|_| wrong_key())?;
    cipher::check_plain_header(&page1).map_err(|_| wrong_key())?;
    let committed = wal.as_deref().map(|w| wal::committed(w, cipher::PAGE_SIZE)).transpose()?.flatten();
    let mut dst = std::fs::File::create(&out).at(&out)?;
    dst.write_all(&page1).at(&out)?;
    let mut page = vec![0u8; cipher::PAGE_SIZE];
    for pgno in 2..=(len / cipher::PAGE_SIZE as u64) as u32 {
        src.read_exact(&mut page).at(&enc)?;
        if onelibrary::reads_main_page(pgno, committed.as_ref()) {
            dst.write_all(&codec.decrypt_page(pgno, &page).map_err(damaged)?).at(&out)?;
        } else {
            dst.write_all(&[0; cipher::PAGE_SIZE]).at(&out)?;
        }
    }
    drop(src);
    std::fs::remove_file(&enc).at(&enc)?;
    if let Some(committed) = committed {
        let main_pages = (len / cipher::PAGE_SIZE as u64) as usize;
        let replayed = onelibrary::wal_pages(&codec, &committed, main_pages).map_err(damaged)?;
        dst.set_len(u64::from(committed.db_pages) * cipher::PAGE_SIZE as u64).at(&out)?;
        for (pgno, plain) in replayed {
            dst.seek(SeekFrom::Start((u64::from(pgno) - 1) * cipher::PAGE_SIZE as u64)).at(&out)?;
            dst.write_all(&plain).at(&out)?;
        }
    }
    // Header bytes 18-19 say WAL mode (2); the copy, without a WAL, reads with a rollback
    // journal (1).
    dst.seek(SeekFrom::Start(18)).at(&out)?;
    dst.write_all(&[1, 1]).at(&out)?;
    dst.flush().at(&out)?;
    Ok(Copy::Decrypted(out))
}

/// Copy `path` to `to` with `copy` and read its WAL, both as of one moment. rekordbox may
/// checkpoint and then restart its WAL in between: the copy would lack what the old WAL held and
/// the new one doesn't. A restart changes the WAL header's checkpoint sequence and salts, so the
/// header is read before copying too, and the copy made again when it changed.
fn snapshot(path: &Path, to: &Path, copy: impl Fn(&Path, &Path) -> std::io::Result<u64>) -> Result<Option<Vec<u8>>> {
    let wal_path = PathBuf::from(format!("{}-wal", path.display()));
    let read_wal = |limit: u64| -> Result<Option<Vec<u8>>> {
        let file = match std::fs::File::open(&wal_path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::Io { path: wal_path.clone(), source: e }),
        };
        let mut bytes = Vec::new();
        file.take(limit).read_to_end(&mut bytes).at(&wal_path)?;
        Ok((!bytes.is_empty()).then_some(bytes))
    };
    let generation = |wal: &Option<Vec<u8>>| wal.as_ref().and_then(|w| w.get(12..24)).map(<[u8]>::to_vec);
    for _ in 0..3 {
        let before = generation(&read_wal(24)?);
        copy(path, to).at(path)?;
        let wal = read_wal(u64::MAX)?;
        if generation(&wal) == before {
            return Ok(wal);
        }
    }
    Err(Error::Conflict(format!("{} kept changing while it was read (is rekordbox saving?)", path.display())))
}

/// The decrypted copy, opened read only.
struct Db<'a> {
    conn: Connection,
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
        let cols = self.columns(table)?;
        if cols.is_empty() {
            return Ok(Vec::new());
        }
        let list: Vec<String> = wanted
            .iter()
            .map(|c| if cols.contains(&c.to_lowercase()) { format!("\"{c}\"") } else { "NULL".into() })
            .collect();
        let filter = if cols.contains("rb_local_deleted") { " WHERE IFNULL(rb_local_deleted, 0) = 0" } else { "" };
        let sql = format!("SELECT {} FROM \"{table}\"{filter} ORDER BY rowid", list.join(", "));
        let mut stmt = self.conn.prepare(&sql).map_err(|e| self.sql(e))?;
        let n = wanted.len();
        let rows = stmt
            .query_map([], |r| (0..n).map(|i| r.get::<_, Value>(i)).collect::<rusqlite::Result<Vec<Value>>>())
            .map_err(|e| self.sql(e))?;
        rows.map(|r| r.map_err(|e| self.sql(e))).collect()
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

        let (playlists, smart_playlists) = self.tree("djmdPlaylist", "djmdSongPlaylist", "PlaylistID", &track_ids)?;
        let (history, _) = self.tree("djmdHistory", "djmdSongHistory", "HistoryID", &track_ids)?;
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
                color: hot_cue.and_then(|_| u8::try_from(color_code).ok()).and_then(anlz::hot_cue_palette),
                // Memory cue colours and stored loop lengths aren't read from master.db yet.
                color_id: 0,
                loop_beats: None,
            });
        }
        for list in cues.values_mut() {
            list.sort_by_key(|c| (c.hot_cue.map_or(0, |h| h as u32 + 1), c.start_ms));
        }
        cues.sort_unstable_keys();

        Ok(Collection {
            path: self.path.to_path_buf(),
            db_version: self
                .select("djmdProperty", &["DBVersion"])?
                .first()
                .map(|r| text(&r[0]))
                .filter(|v| !v.is_empty()),
            library,
            cues,
            tags: self.tags(&track_ids)?,
            smart_playlists,
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
    /// history tables), plus the ids of intelligent playlists.
    fn tree(
        &self,
        table: &str,
        entries: &str,
        list_column: &str,
        tracks: &FxHashMap<String, u32>,
    ) -> Result<(Vec<Playlist>, Vec<u32>)> {
        let rows = self.select(table, &["ID", "Seq", "Name", "Attribute", "ParentID"])?;
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
                smart.push(id);
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
        self.path.parent().unwrap_or(Path::new(".")).join("share")
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
        let first = path.trim_start_matches('/').split('/').next().unwrap_or("");
        if !first.starts_with("contents_") {
            return Some(PathBuf::from(path));
        }
        cloud
            .for_service(self.sync_service.as_deref())
            .into_iter()
            .map(|folder| paths::to_fs(&folder.join("rekordbox"), &paths::key(path)))
            .find(|p| p.exists())
    }

    /// My Tags as `Category/Tag` by lower-cased file key, the shape track filters (`tag:`) take.
    pub fn tags_by_file(&self) -> FxHashMap<String, Vec<String>> {
        let files: FxHashMap<u32, String> =
            self.library.tracks.iter().map(|t| (t.id, paths::key(&t.file_path).to_lowercase())).collect();
        let mut out: FxHashMap<String, Vec<String>> = FxHashMap::default();
        for c in &self.tags {
            for tag in &c.tags {
                for id in &tag.track_ids {
                    if let Some(f) = files.get(id) {
                        out.entry(f.clone()).or_default().push(format!("{}/{}", c.name, tag.name));
                    }
                }
            }
        }
        out
    }

    /// What `drive` (a drive's library) lacks against the collection, and what it has that the
    /// collection doesn't: tracks, playlists and their entries, through [`diff::between`] with
    /// the drive on the left. Collection tracks are found on the drive by file name and the
    /// folders above it, then ISRC, then artist, title and length ([`import::Matcher`]); a track
    /// only a looser match finds counts as missing, and is listed in `uncertain`. A `playlist`
    /// neither has is refused.
    pub fn diff_drive(&self, drive: &Library, drive_label: &str, playlist: Option<&str>) -> Result<DriveDiff> {
        diff::check_playlist(drive, &self.library, playlist)?;
        let matcher = Matcher::new(drive);
        let mut translated = self.library.clone();
        let (mut by_file, mut by_details, mut uncertain) = (0, 0, Vec::new());
        for t in &mut translated.tracks {
            let entry = import::Entry {
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: (!t.album.is_empty()).then(|| t.album.clone()),
                mix: (!t.mix_name.is_empty()).then(|| t.mix_name.clone()),
                duration_secs: (t.duration_secs > 0).then_some(t.duration_secs),
                path: (!t.file_path.is_empty()).then(|| t.file_path.clone()),
                isrc: (!t.isrc.is_empty()).then(|| t.isrc.clone()),
                ..Default::default()
            };
            match matcher.find(&entry) {
                Status::Found { track_id, how } if how != MatchKind::Title && how != MatchKind::Fuzzy => {
                    if let Some(d) = drive.track(track_id) {
                        t.file_path = d.file_path.clone();
                        if how == MatchKind::Path { by_file += 1 } else { by_details += 1 }
                    }
                }
                Status::Missing => {}
                _ => uncertain.push(TrackRef::from(&*t)),
            }
        }
        Ok(DriveDiff {
            matched_by_file: by_file,
            matched_by_details: by_details,
            uncertain,
            diff: diff::between(drive, &translated, (drive_label, "rekordbox collection"), playlist),
        })
    }
}

/// [`Collection::diff_drive`]: `diff.added_*` is what the drive lacks, `diff.removed_*` what only
/// the drive has, and changed tracks are details the collection has since changed (or the drive
/// was edited).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveDiff {
    /// Collection tracks found on the drive by their file.
    pub matched_by_file: usize,
    /// Found by ISRC, or artist, title and length.
    pub matched_by_details: usize,
    /// Collection tracks only a loose match (title alone, or similar words) finds on the drive;
    /// counted as missing from it.
    pub uncertain: Vec<TrackRef>,
    pub diff: PlaylistDiff,
}

/// The hot cue (0 = A) a `djmdCue.Kind` stands for, or `None` for a memory cue or loop. rekordbox
/// skips 4, which marks a memory loop, so hot cue D is 5.
pub(crate) fn hot_cue_from_kind(kind: i64) -> Option<u8> {
    match kind {
        1..=3 => Some((kind - 1) as u8),
        5.. => u8::try_from(kind - 2).ok(),
        _ => None,
    }
}

/// The `Kind` hot cue `h` (0 = A) is stored as; the inverse of [`hot_cue_from_kind`].
pub(crate) fn kind_for_hot_cue(h: u8) -> i64 {
    if h < 3 { i64::from(h) + 1 } else { i64::from(h) + 2 }
}

#[cfg(test)]
#[path = "../tests/support/sqlcipher.rs"]
mod sqlcipher;

#[cfg(test)]
pub(crate) mod tests;
