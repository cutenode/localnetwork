//! OneLibrary (rekordbox's name since 7.2; "Device Library Plus" before): the
//! `PIONEER/rekordbox/exportLibrary.db` database rekordbox 6.8+/7 writes for newer players. The
//! CDJ-3000X, CDJ-1500X, OPUS-QUAD, OMNIS-DUO, XDJ-AZ and XDJ-AN read only this file and ignore
//! `export.pdb`, so every edit locallibrary makes has to land here too.
//!
//! The file is SQLite encrypted with SQLCipher (see [`cipher`]). rekordbox leaves it in WAL mode,
//! often with its latest changes only in `exportLibrary.db-wal`, so reading replays the WAL's
//! committed transactions (see [`wal`]). Audio and analysis files are shared with `export.pdb`;
//! artwork has its own `PIONEER/Artwork/<folder>/b<id>.jpg` (+ `_m`) files.
//!
//! Editing happens on a decrypted working copy in a temporary directory, with plain SQLite. The
//! result is checked (`PRAGMA integrity_check`, an encrypt/decrypt round trip) and written back as
//! one checkpointed file with the drive's own salt, the old WAL removed first (see [`write()`]).

pub mod cipher;
mod cues;
mod edit;
mod tags;
pub mod wal;

pub use cues::{CueRow, CueRowChanges};
pub use edit::Tree;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::config::DbConfig;
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, OptionalExtension, params};

use crate::commit::Swap;
use crate::device::Device;
use crate::error::IoContext;
use crate::model::{Library, Playlist, Track, TrackColor, untangle};
use crate::paths;
use crate::{Error, Result};

use cipher::{Codec, PAGE_SIZE};
pub use tags::MyTagRow;

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::OneLibrary(e.to_string())
    }
}

/// `name` as an SQL identifier: in double quotes, with any it holds doubled (column names come
/// from the drive's own schema).
pub(crate) fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Decrypt `exportLibrary.db` and replay the committed part of its WAL: the database as a player
/// (or rekordbox) opening it would see it, as a plain SQLite file.
pub fn decrypt(db: &[u8], wal: Option<&[u8]>) -> Result<(Codec, Vec<u8>)> {
    let codec = Codec::for_file(db)?;
    let committed = wal.map(|w| wal::committed(w, PAGE_SIZE)).transpose()?.flatten();
    let mut plain = codec.decrypt(db, |pgno| reads_main_page(pgno, committed.as_ref()))?;
    cipher::check_plain_header(&plain)?;
    if let Some(committed) = committed {
        let replayed = wal_pages(&codec, &committed, plain.len() / PAGE_SIZE)?;
        plain.resize(committed.db_pages as usize * PAGE_SIZE, 0);
        for (pgno, page) in replayed {
            let at = (pgno as usize - 1) * PAGE_SIZE;
            plain[at..at + PAGE_SIZE].copy_from_slice(&page);
        }
        cipher::check_plain_header(&plain)?;
    }
    Ok((codec, plain))
}

/// Whether SQLite, opening a database whose WAL holds `committed`, reads page `pgno` from the
/// main file. It doesn't read the lock-byte page, a page a committed frame replaces, or a page
/// past the last commit's size, so those may hold anything (a hole an interrupted checkpoint
/// left, say) and aren't checked. Page 1 always is: its salt keys every other page.
pub(crate) fn reads_main_page(pgno: u32, committed: Option<&wal::Committed<'_>>) -> bool {
    let replaced = committed.is_some_and(|c| c.pages.contains_key(&pgno) || pgno > c.db_pages);
    pgno == 1 || (pgno != cipher::LOCK_PAGE && !replaced)
}

/// The pages of a WAL's `committed` transactions over a main file of `main_pages`, decrypted,
/// that lie within the database size the last commit gives. Every page is decrypted first,
/// which authenticates it against its page number. A database grows only by writing its new
/// pages, so a size past the main file plus the WAL's pages beyond it (and one more: SQLite never
/// writes the lock-byte page at 1 GiB) is corrupt or crafted, and is refused instead of
/// allocated.
pub(crate) fn wal_pages(
    codec: &Codec,
    committed: &wal::Committed<'_>,
    main_pages: usize,
) -> Result<Vec<(u32, Vec<u8>)>> {
    let pages: Vec<(u32, Vec<u8>)> =
        committed.pages.iter().map(|(&n, p)| Ok((n, codec.decrypt_page(n, p)?))).collect::<Result<_>>()?;
    let known = main_pages + pages.iter().filter(|(n, _)| *n as usize > main_pages).count();
    let size = committed.db_pages as usize;
    if size > known + 1 {
        return Err(Error::OneLibrary(format!(
            "the WAL's last commit says the database has {size} pages, but only {known} exist"
        )));
    }
    Ok(pages.into_iter().filter(|(n, _)| *n as usize <= size).collect())
}

/// Identity of the library a database (and WAL) holds: a hash of the decrypted pages after
/// replaying the WAL, without the per-page IV and HMAC. Unchanged by a checkpoint or by
/// re-encryption, which rewrite every byte of the files.
///
/// Two things that don't describe the library are left out, so drives holding the same library
/// hash alike: the per-drive name and background colour players show (`property.deviceName`,
/// `property.backGroundColorType`, see [`crate::player`]), and the header counters SQLite bumps on
/// every write (file change counter and version-valid-for, bytes 24-27 and 92-95).
pub fn content_hash(db: &[u8], wal: Option<&[u8]>) -> Result<crate::hash::Hash> {
    Ok(content_hashes(db, wal)?.0)
}

/// [`content_hash`] and the hash locallibrary computed before the per-drive name was left out
/// (every decrypted page, as is). Lineage recorded before then holds the second kind.
pub fn content_hashes(db: &[u8], wal: Option<&[u8]>) -> Result<(crate::hash::Hash, crate::hash::Hash)> {
    let (_, plain) = decrypt(db, wal)?;
    let property = property_page(&plain).unwrap_or(None);
    let usable = PAGE_SIZE - cipher::RESERVE;
    let mut current = blake3::Hasher::new();
    let mut legacy = blake3::Hasher::new();
    for (i, page) in plain.chunks(PAGE_SIZE).enumerate() {
        let pgno = i as u32 + 1;
        legacy.update(&page[..usable]);
        match &property {
            Some((root, canonical)) if *root == pgno => {
                current.update(b"\0property\0");
                current.update(canonical);
            }
            _ if pgno == 1 => {
                let mut first = page[..usable].to_vec();
                first[24..28].fill(0);
                first[92..96].fill(0);
                current.update(&first);
            }
            _ => {
                current.update(&page[..usable]);
            }
        }
    }
    Ok((current.finalize().into(), legacy.finalize().into()))
}

/// The `property` table's page and its rows without the per-drive columns, when the table is a
/// single page (it holds one row).
fn property_page(plain: &[u8]) -> Result<Option<(u32, Vec<u8>)>> {
    let mut copy = plain.to_vec();
    copy[18] = 1;
    copy[19] = 1;
    let dir = tempfile::tempdir().map_err(|e| Error::Io { path: std::env::temp_dir(), source: e })?;
    let path = dir.path().join("hash.db");
    std::fs::write(&path, &copy).at(&path)?;
    let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    // Read before `OneLibrary::open` has looked at the file: only the one table is read here.
    if let Some(why) = distrust(&conn, &[], "rekordbox")?.or(damaged(&conn)?) {
        return Err(Error::OneLibrary(why));
    }
    let root: Option<u32> = conn
        .query_row("SELECT rootpage FROM sqlite_master WHERE type = 'table' AND name = 'property'", [], |r| r.get(0))
        .optional()?;
    let Some(root) = root.filter(|&r| r >= 2) else { return Ok(None) };
    // Only a leaf root holds every row; otherwise hash the pages as they are.
    let at = (root as usize - 1) * PAGE_SIZE;
    if plain.get(at) != Some(&0x0D) {
        return Ok(None);
    }
    let mut stmt = conn.prepare("SELECT * FROM property ORDER BY rowid")?;
    let names: Vec<String> = stmt.column_names().iter().map(|n| n.to_string()).collect();
    let mut canonical = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        for (i, name) in names.iter().enumerate() {
            if crate::player::PER_DRIVE_COLUMNS.iter().any(|c| c.eq_ignore_ascii_case(name)) {
                continue;
            }
            canonical.extend_from_slice(name.as_bytes());
            canonical.push(b'=');
            canonical.extend_from_slice(format!("{:?}", row.get_ref(i)?).as_bytes());
            canonical.push(0);
        }
        canonical.push(b'\n');
    }
    Ok(Some((root, canonical)))
}

/// Make `conn`, open on a database someone else wrote, safe to run SQL on: SQL in its schema may
/// only call functions without side effects, no SQL may corrupt the file on purpose (SQLite's
/// defensive mode), a malformed b-tree cell is an error rather than read out of bounds, and a
/// schema object of one of the `refused` kinds, which `maker` never makes, is refused: a view
/// standing in for a table could loop for ever when read, a trigger would run on every edit made
/// here. `Some` says which one the file has.
pub(crate) fn distrust(conn: &Connection, refused: &[&str], maker: &str) -> rusqlite::Result<Option<String>> {
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    conn.execute_batch("PRAGMA trusted_schema = OFF; PRAGMA cell_size_check = ON")?;
    for kind in refused {
        let name: Option<String> =
            conn.query_row("SELECT name FROM sqlite_master WHERE type = ?1", [kind], |r| r.get(0)).optional()?;
        if let Some(name) = name {
            return Ok(Some(format!("it has a {kind} {name}, which {maker} never makes")));
        }
    }
    Ok(None)
}

/// SQLite's structural check of every b-tree in `conn`'s database, which SQLite advises before
/// reading a file someone else wrote. It reads the whole file, so it's for databases of a few
/// hundred MB at most (rekordbox's), not Audacity projects full of audio. `Some` says what's wrong.
pub(crate) fn damaged(conn: &Connection) -> rusqlite::Result<Option<String>> {
    let check: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    Ok((check != "ok").then(|| format!("SQLite finds it damaged: {check}")))
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
    let bytes = std::fs::read(&db).at(&db)?;
    let wal = paths::to_fs(root, paths::LIBRARY_PLUS_WAL);
    let wal = match std::fs::read(&wal) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Error::Io { path: wal, source: e }),
    };
    Ok((bytes, wal))
}

/// An empty SQLite database (one page: header and an empty schema table) laid out for SQLCipher:
/// 4096-byte pages with [`cipher::RESERVE`] bytes reserved at the end of each for the IV and
/// HMAC. SQLite keeps the reserve for every page it adds.
fn empty_database() -> Vec<u8> {
    let mut page = vec![0u8; PAGE_SIZE];
    page[..16].copy_from_slice(cipher::SQLITE_MAGIC);
    page[16..18].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes());
    page[18] = 1; // write version: rollback journal
    page[19] = 1; // read version
    page[20] = cipher::RESERVE as u8;
    page[21] = 64; // max embedded payload fraction
    page[22] = 32; // min embedded payload fraction
    page[23] = 32; // leaf payload fraction
    page[24..28].copy_from_slice(&1u32.to_be_bytes()); // file change counter
    page[28..32].copy_from_slice(&1u32.to_be_bytes()); // database size in pages
    page[44..48].copy_from_slice(&4u32.to_be_bytes()); // schema format
    page[56..60].copy_from_slice(&1u32.to_be_bytes()); // text encoding: UTF-8
    page[92..96].copy_from_slice(&1u32.to_be_bytes()); // version-valid-for = change counter
    page[96..100].copy_from_slice(&3_046_000u32.to_be_bytes());
    // sqlite_schema: an empty leaf table b-tree whose cell area starts at the end of the usable
    // space.
    page[100] = 0x0d;
    page[105..107].copy_from_slice(&((PAGE_SIZE - cipher::RESERVE) as u16).to_be_bytes());
    page
}

/// rekordbox's file type codes (`content.fileType`).
fn file_type(path: &str) -> i64 {
    match path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("mp3") => 1,
        Some("m4a" | "mp4" | "aac") => 4,
        Some("flac") => 5,
        Some("wav") => 11,
        Some("aif" | "aiff") => 12,
        _ => 0,
    }
}

fn int(v: ValueRef<'_>) -> i64 {
    match v {
        ValueRef::Integer(i) => i,
        ValueRef::Real(f) => f as i64,
        ValueRef::Text(t) => std::str::from_utf8(t).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0),
        _ => 0,
    }
}

fn text(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        _ => String::new(),
    }
}

fn id32(id: i64, what: &str) -> Result<u32> {
    u32::try_from(id).map_err(|_| Error::OneLibrary(format!("{what} id {id} out of range")))
}

/// Lookup tables whose rows are removed once nothing references them, with their id column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Lookup {
    Artist,
    Album,
    Genre,
    Label,
    Key,
    Image,
}

impl Lookup {
    const ALL: [Lookup; 6] = [Lookup::Artist, Lookup::Album, Lookup::Genre, Lookup::Label, Lookup::Key, Lookup::Image];

    fn table(self) -> &'static str {
        match self {
            Lookup::Artist => "artist",
            Lookup::Album => "album",
            Lookup::Genre => "genre",
            Lookup::Label => "label",
            Lookup::Key => "\"key\"",
            Lookup::Image => "image",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Lookup::Artist => "artist_id",
            Lookup::Album => "album_id",
            Lookup::Genre => "genre_id",
            Lookup::Label => "label_id",
            Lookup::Key => "key_id",
            Lookup::Image => "image_id",
        }
    }
}

/// `content` columns that reference other tables; rewritten when a row moves between databases.
const REFERENCE_COLUMNS: [&str; 11] = [
    "content_id",
    "artist_id_artist",
    "artist_id_remixer",
    "artist_id_originalArtist",
    "artist_id_composer",
    "artist_id_lyricist",
    "album_id",
    "genre_id",
    "label_id",
    "key_id",
    "image_id",
];

/// A track row with its references resolved to names, so it can be added to another database.
#[derive(Debug, Clone, Default)]
pub struct ContentRecord {
    /// Every other column of the row, as stored (unknown columns are carried as they are).
    pub values: Vec<(String, Value)>,
    pub artist: Option<String>,
    pub remixer: Option<String>,
    pub original_artist: Option<String>,
    pub composer: Option<String>,
    pub lyricist: Option<String>,
    /// Album name and album artist.
    pub album: Option<(String, Option<String>)>,
    pub genre: Option<String>,
    pub label: Option<String>,
    pub key: Option<String>,
    /// The track's `cue` rows: the source's `cue_id` and every other column but `content_id`.
    pub cues: Vec<(i64, Vec<(String, Value)>)>,
    /// The track's My Tags, as (category, tag) names.
    pub my_tags: Vec<(String, String)>,
}

/// Defaults for columns `export.pdb` has no equivalent for (see [`ContentRecord::from_track`]).
#[derive(Debug, Clone)]
pub struct ContentDefaults {
    pub analysed_bits: Value,
    pub content_link: Value,
}

impl ContentRecord {
    /// A row for a track known only from `export.pdb`. `analysedBits` and `contentLink` describe
    /// the analysis files; they take the values most of the target's tracks have.
    pub fn from_track(t: &Track, analysed: bool, defaults: &ContentDefaults) -> ContentRecord {
        let s = |v: &str| Value::Text(v.to_string());
        let i = |v: i64| Value::Integer(v);
        let opt = |v: &str| (!v.is_empty()).then(|| v.to_string());
        let values = vec![
            ("title", s(&t.title)),
            ("titleForSearch", Value::Null),
            ("subtitle", s(&t.mix_name)),
            ("bpmx100", i((t.bpm * 100.0).round() as i64)),
            ("length", i(t.duration_secs.into())),
            ("trackNo", i(t.track_number.into())),
            ("discNo", i(t.disc_number.into())),
            ("color_id", i(t.color.map_or(0, |c| c.id().into()))),
            ("djComment", s(&t.comment)),
            ("rating", i(t.rating.into())),
            ("releaseYear", i(t.year.into())),
            ("releaseDate", s(&t.release_date)),
            ("dateCreated", s(&t.date_added)),
            ("dateAdded", s(&t.date_added)),
            ("fileSize", i(t.file_size as i64)),
            ("fileType", i(file_type(&t.file_path))),
            ("bitrate", i(t.bitrate.into())),
            ("bitDepth", i(t.sample_depth.into())),
            ("samplingRate", i(t.sample_rate.into())),
            ("isrc", s(&t.isrc)),
            ("djPlayCount", i(t.play_count.into())),
            ("isHotCueAutoLoadOn", i(1)),
            ("isKuvoDeliverStatusOn", i(1)),
            ("kuvoDeliveryComment", s("")),
            ("masterDbId", i(0)),
            ("masterContentId", i(0)),
            ("analysedBits", if analysed { defaults.analysed_bits.clone() } else { i(0) }),
            ("contentLink", defaults.content_link.clone()),
            ("hasModified", i(0)),
            ("cueUpdateCount", i(0)),
            ("analysisDataUpdateCount", i(0)),
            ("informationUpdateCount", i(0)),
        ];
        ContentRecord {
            values: values.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            artist: opt(&t.artist),
            remixer: opt(&t.remixer),
            original_artist: opt(&t.original_artist),
            composer: opt(&t.composer),
            lyricist: None,
            album: opt(&t.album).map(|a| (a, opt(&t.album_artist))),
            genre: opt(&t.genre),
            label: opt(&t.label),
            key: opt(&t.key),
            cues: Vec::new(),
            my_tags: Vec::new(),
        }
    }
}

/// A drive's OneLibrary database, decrypted into a working copy that can be read and edited.
pub struct OneLibrary {
    conn: Connection,
    codec: Codec,
    /// Header bytes 18-19 (file format write/read version: 2 = WAL) as rekordbox left them. The
    /// working copy uses a rollback journal; the original mode is restored on the way out.
    journal: [u8; 2],
    path: PathBuf,
    _dir: tempfile::TempDir,
}

impl std::fmt::Debug for OneLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OneLibrary").field("path", &self.path).finish()
    }
}

impl OneLibrary {
    /// Open a drive's OneLibrary database (and its WAL).
    pub fn open(device: &Device) -> Result<OneLibrary> {
        Self::open_root(device.root())
    }

    /// Open the OneLibrary database below any directory laid out like a drive (a backup, say).
    pub fn open_root(root: &Path) -> Result<OneLibrary> {
        let (db, wal) = read_files(root)?;
        OneLibrary::from_encrypted(&db, wal.as_deref())
    }

    pub fn from_encrypted(db: &[u8], wal: Option<&[u8]>) -> Result<OneLibrary> {
        let (codec, plain) = decrypt(db, wal)?;
        let one = OneLibrary::from_plain(codec, plain)?;
        let has_content: bool = one.conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'content'",
            [],
            |r| Ok(r.get::<_, i64>(0)? > 0),
        )?;
        if !has_content {
            return Err(Error::OneLibrary("no content table; not a rekordbox OneLibrary database".into()));
        }
        Ok(one)
    }

    /// A new, empty OneLibrary database: rekordbox 7's schema and default rows (colours, browse
    /// menus, sort columns), a fresh salt, and WAL mode like rekordbox's own files.
    pub fn create() -> Result<OneLibrary> {
        let mut one = OneLibrary::from_plain(Codec::random()?, empty_database())?;
        one.journal = [2, 2];
        one.conn.execute_batch(include_str!("schema.sql"))?;
        one.conn.execute(
            "UPDATE property SET createdDate = ?1, numberOfContents = 0",
            [chrono::Utc::now().format("%Y-%m-%d").to_string()],
        )?;
        Ok(one)
    }

    /// A working copy of a plain database image (header bytes 18-19 still as stored).
    fn from_plain(codec: Codec, mut plain: Vec<u8>) -> Result<OneLibrary> {
        let journal = [plain[18], plain[19]];
        plain[18] = 1;
        plain[19] = 1;
        let dir = tempfile::tempdir().map_err(|e| Error::Io { path: std::env::temp_dir(), source: e })?;
        let path = dir.path().join("exportLibrary.db");
        std::fs::write(&path, &plain).at(&path)?;
        let conn = Connection::open(&path)?;
        // The schema comes from the drive. Anyone can write this file (its key is public), and
        // rekordbox makes no views or triggers.
        if let Some(why) = distrust(&conn, &["view", "trigger"], "rekordbox")?.or(damaged(&conn)?) {
            return Err(Error::OneLibrary(why));
        }
        // A scratch copy: no need for durability, and thousands of single-row edits stay fast.
        conn.execute_batch("PRAGMA journal_mode = MEMORY; PRAGMA synchronous = OFF;")?;
        Ok(OneLibrary { conn, codec, journal, path, _dir: dir })
    }

    /// Plain SQLite access to the working copy (tests and diagnostics).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// The edited database, encrypted with the original file's salt, after checking that SQLite
    /// finds it intact and that it decrypts back to exactly what was edited.
    pub fn to_encrypted(&self) -> Result<Vec<u8>> {
        let check: String = self.conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(Error::OneLibrary(format!("integrity check failed: {check}")));
        }
        let mut plain = std::fs::read(&self.path).at(&self.path)?;
        cipher::check_plain_header(&plain)?;
        plain[18..20].copy_from_slice(&self.journal);
        let bytes = self.codec.encrypt(&plain)?;
        let back = self.codec.decrypt(&bytes, |_| true)?;
        let usable = PAGE_SIZE - cipher::RESERVE;
        if back.len() != plain.len()
            || back.chunks(PAGE_SIZE).zip(plain.chunks(PAGE_SIZE)).any(|(a, b)| a[..usable] != b[..usable])
        {
            return Err(Error::OneLibrary("encrypted database failed verification".into()));
        }
        Ok(bytes)
    }

    // -- reading ----------------------------------------------------------------------------

    pub fn library(&self) -> Result<Library> {
        let mut stmt = self.conn.prepare(
            "SELECT c.content_id, c.title, a.name, al.name, aa.name, g.name, l.name, k.name, r.name, oa.name,
                    cp.name, c.djComment, c.subtitle, c.isrc, c.bpmx100, c.length, c.releaseYear, c.trackNo,
                    c.discNo, c.rating, c.color_id, c.djPlayCount, c.bitrate, c.samplingRate, c.bitDepth,
                    c.fileSize, c.path, c.analysisDataFilePath, c.dateAdded, c.releaseDate, i.image_id, i.path
             FROM content c
             LEFT JOIN artist a ON a.artist_id = c.artist_id_artist
             LEFT JOIN album al ON al.album_id = c.album_id
             LEFT JOIN artist aa ON aa.artist_id = al.artist_id
             LEFT JOIN genre g ON g.genre_id = c.genre_id
             LEFT JOIN label l ON l.label_id = c.label_id
             LEFT JOIN \"key\" k ON k.key_id = c.key_id
             LEFT JOIN artist r ON r.artist_id = c.artist_id_remixer
             LEFT JOIN artist oa ON oa.artist_id = c.artist_id_originalArtist
             LEFT JOIN artist cp ON cp.artist_id = c.artist_id_composer
             LEFT JOIN image i ON i.image_id = c.image_id
             ORDER BY c.content_id",
        )?;
        let mut tracks = Vec::new();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let v = |i: usize| row.get_ref(i).unwrap_or(ValueRef::Null);
            let image_id = int(v(30));
            tracks.push(Track {
                id: id32(int(v(0)), "content")?,
                title: text(v(1)),
                artist: text(v(2)),
                album: text(v(3)),
                album_artist: text(v(4)),
                genre: text(v(5)),
                label: text(v(6)),
                key: text(v(7)),
                remixer: text(v(8)),
                original_artist: text(v(9)),
                composer: text(v(10)),
                comment: text(v(11)),
                mix_name: text(v(12)),
                isrc: text(v(13)),
                bpm: int(v(14)) as f64 / 100.0,
                duration_secs: int(v(15)).clamp(0, u32::MAX.into()) as u32,
                year: int(v(16)).clamp(0, u16::MAX.into()) as u16,
                track_number: int(v(17)).clamp(0, u32::MAX.into()) as u32,
                disc_number: int(v(18)).clamp(0, u16::MAX.into()) as u16,
                rating: int(v(19)).clamp(0, 5) as u8,
                color: TrackColor::from_id(int(v(20)).clamp(0, 255) as u8),
                play_count: int(v(21)).clamp(0, u16::MAX.into()) as u16,
                bitrate: int(v(22)).clamp(0, u32::MAX.into()) as u32,
                sample_rate: int(v(23)).clamp(0, u32::MAX.into()) as u32,
                sample_depth: int(v(24)).clamp(0, u16::MAX.into()) as u16,
                file_size: int(v(25)).max(0) as u64,
                file_path: text(v(26)),
                analyze_path: text(v(27)),
                date_added: text(v(28)),
                release_date: text(v(29)),
                artwork_id: (image_id > 0).then(|| id32(image_id, "image")).transpose()?,
                artwork_path: (image_id > 0).then(|| text(v(31))),
            });
        }

        let playlists = self.tree("playlist", self.entries("playlist")?)?;
        let history = self.tree("history", self.entries("history")?)?;
        Ok(Library { tracks, history_playlists: history.iter().filter(|h| !h.is_folder).count(), playlists, history })
    }

    /// `<table>_content` rows by list, in order.
    fn entries(&self, table: &str) -> Result<HashMap<u32, Vec<u32>>> {
        let mut entries: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {table}_id, content_id FROM {table}_content ORDER BY {table}_id, sequenceNo, rowid"
        ))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            entries.entry(id32(int(row.get_ref(0)?), table)?).or_default().push(id32(int(row.get_ref(1)?), "content")?);
        }
        Ok(entries)
    }

    /// A playlist-shaped tree (`playlist`, `history`): attribute 1 is a folder.
    fn tree(&self, table: &str, mut entries: HashMap<u32, Vec<u32>>) -> Result<Vec<Playlist>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {table}_id, {table}_id_parent, name, attribute, sequenceNo FROM {table}"))?;
        let mut out = Vec::new();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id = id32(int(row.get_ref(0)?), table)?;
            let parent = int(row.get_ref(1)?);
            out.push(Playlist {
                id,
                parent_id: (parent > 0).then(|| id32(parent, table)).transpose()?,
                name: text(row.get_ref(2)?),
                is_folder: int(row.get_ref(3)?) == 1,
                sort_order: int(row.get_ref(4)?).clamp(0, u32::MAX.into()) as u32,
                track_ids: entries.remove(&id).unwrap_or_default(),
            });
        }
        Ok(untangle(out))
    }

    /// Paths of images that rows other than tracks (albums, playlists, hot cue bank lists) use.
    pub fn images_used_outside_tracks(&self) -> Result<Vec<(u32, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT image_id, path FROM image WHERE image_id IN (
                 SELECT image_id FROM album UNION SELECT image_id FROM playlist
                 UNION SELECT image_id FROM hotCueBankList)",
        )?;
        let rows = stmt.query_map([], |r| Ok((int(r.get_ref(0)?), text(r.get_ref(1)?))))?;
        rows.map(|r| {
            let (id, path) = r?;
            Ok((id32(id, "image")?, path))
        })
        .collect()
    }

    fn next_id(&self, table: &str, column: &str) -> Result<i64> {
        Ok(self.conn.query_row(&format!("SELECT coalesce(max({column}), 0) + 1 FROM {table}"), [], |r| r.get(0))?)
    }

    fn update_count(&self) -> Result<()> {
        self.conn
            .execute("UPDATE property SET numberOfContents = (SELECT count(*) FROM content)", [])
            .map(|_| ())
            .map_err(Into::into)
    }

    // -- removing tracks --------------------------------------------------------------------

    /// Lookup rows (artists, albums, genres, labels, keys, images) only the `removed` tracks use.
    fn unused_lookups(&self, removed: &HashSet<u32>) -> Result<Vec<(Lookup, Vec<i64>)>> {
        let mut gone: HashMap<Lookup, HashSet<i64>> = HashMap::new();
        let mut kept: HashMap<Lookup, HashSet<i64>> = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT content_id, artist_id_artist, artist_id_remixer, artist_id_originalArtist, artist_id_composer,
                    artist_id_lyricist, album_id, genre_id, label_id, key_id, image_id FROM content",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let is_gone = u32::try_from(int(row.get_ref(0)?)).is_ok_and(|id| removed.contains(&id));
            let side = if is_gone { &mut gone } else { &mut kept };
            let refs = [
                (Lookup::Artist, 1),
                (Lookup::Artist, 2),
                (Lookup::Artist, 3),
                (Lookup::Artist, 4),
                (Lookup::Artist, 5),
                (Lookup::Album, 6),
                (Lookup::Genre, 7),
                (Lookup::Label, 8),
                (Lookup::Key, 9),
                (Lookup::Image, 10),
            ];
            for (table, col) in refs {
                side.entry(table).or_default().insert(int(row.get_ref(col)?));
            }
        }
        let unused = |t: Lookup, gone: &HashMap<Lookup, HashSet<i64>>, kept: &HashMap<Lookup, HashSet<i64>>| {
            let used = kept.get(&t);
            let mut ids: Vec<i64> = gone
                .get(&t)
                .into_iter()
                .flatten()
                .copied()
                .filter(|id| *id > 0 && !used.is_some_and(|u| u.contains(id)))
                .collect();
            ids.sort_unstable();
            ids
        };
        let albums = unused(Lookup::Album, &gone, &kept);
        // Albums reference an artist and an image: one that stays keeps them, one that goes may
        // take them along. Playlists and hot cue bank lists can use images too.
        let mut stmt = self.conn.prepare("SELECT album_id, artist_id, image_id FROM album")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let side = if albums.contains(&int(row.get_ref(0)?)) { &mut gone } else { &mut kept };
            side.entry(Lookup::Artist).or_default().insert(int(row.get_ref(1)?));
            side.entry(Lookup::Image).or_default().insert(int(row.get_ref(2)?));
        }
        let mut stmt = self.conn.prepare("SELECT image_id FROM playlist UNION SELECT image_id FROM hotCueBankList")?;
        for id in stmt.query_map([], |r| Ok(int(r.get_ref(0)?)))? {
            kept.entry(Lookup::Image).or_default().insert(id?);
        }
        Ok(Lookup::ALL
            .into_iter()
            .map(|t| (t, if t == Lookup::Album { albums.clone() } else { unused(t, &gone, &kept) }))
            .filter(|(_, ids)| !ids.is_empty())
            .collect())
    }

    /// How many lookup rows removing `ids` would leave unused (and remove).
    pub fn unused_lookup_rows(&self, ids: &[u32]) -> Result<usize> {
        Ok(self.unused_lookups(&ids.iter().copied().collect())?.iter().map(|(_, ids)| ids.len()).sum())
    }

    /// Remove tracks with their cues, playlist, history, My Tag and recommendation entries, and the
    /// lookup rows nothing else uses. Playlists and histories are renumbered without gaps.
    pub fn remove_tracks(&mut self, ids: &[u32]) -> Result<()> {
        self.check_no_unknown_references()?;
        let removed: HashSet<u32> = ids.iter().copied().collect();
        let lookups = self.unused_lookups(&removed)?;
        let tx = self.conn.transaction()?;
        let mut touched_lists: Vec<(&str, &str, i64)> = Vec::new();
        for (table, list) in [("playlist_content", "playlist_id"), ("history_content", "history_id")] {
            let mut stmt = tx.prepare(&format!("SELECT DISTINCT {list} FROM {table} WHERE content_id = ?1"))?;
            for id in ids {
                for l in stmt.query_map([id], |r| r.get::<_, i64>(0))? {
                    touched_lists.push((table, list, l?));
                }
            }
        }
        for id in ids {
            tx.execute(
                "DELETE FROM hotCueBankList_cue WHERE cue_id IN (SELECT cue_id FROM cue WHERE content_id = ?1)",
                [id],
            )?;
            for sql in [
                "DELETE FROM cue WHERE content_id = ?1",
                "DELETE FROM playlist_content WHERE content_id = ?1",
                "DELETE FROM history_content WHERE content_id = ?1",
                "DELETE FROM myTag_content WHERE content_id = ?1",
                "DELETE FROM recommendedLike WHERE content_id_1 = ?1 OR content_id_2 = ?1",
                "DELETE FROM content WHERE content_id = ?1",
            ] {
                tx.execute(sql, [id])?;
            }
        }
        touched_lists.sort_unstable();
        touched_lists.dedup();
        for (table, list, id) in touched_lists {
            let rowids: Vec<i64> = tx
                .prepare(&format!("SELECT rowid FROM {table} WHERE {list} = ?1 ORDER BY sequenceNo, rowid"))?
                .query_map([id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let mut update = tx.prepare(&format!("UPDATE {table} SET sequenceNo = ?1 WHERE rowid = ?2"))?;
            for (n, rowid) in rowids.iter().enumerate() {
                update.execute(params![n as i64 + 1, rowid])?;
            }
        }
        for (table, rows) in &lookups {
            let mut stmt = tx.prepare(&format!("DELETE FROM {} WHERE {} = ?1", table.table(), table.id()))?;
            for id in rows {
                stmt.execute([id])?;
            }
        }
        tx.commit()?;
        self.update_count()
    }

    // -- artwork ----------------------------------------------------------------------------

    /// Add an image row for `device_path` (`/PIONEER/Artwork/.../b<id>.jpg`).
    pub fn add_image(&mut self, device_path: &str) -> Result<u32> {
        let id = self.next_id("image", "image_id")?;
        self.conn.execute("INSERT INTO image (image_id, path) VALUES (?1, ?2)", params![id, device_path])?;
        id32(id, "image")
    }

    /// The id the next [`OneLibrary::add_image`] will use.
    pub fn next_image_id(&self) -> Result<u32> {
        id32(self.next_id("image", "image_id")?, "image")
    }

    pub fn set_track_image(&mut self, ids: &[u32], image: u32) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE content SET image_id = ?1 WHERE content_id = ?2")?;
            for id in ids {
                if stmt.execute(params![image, id])? != 1 {
                    return Err(Error::NotFound(format!("track #{id} in exportLibrary.db")));
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    // -- adding tracks and playlists --------------------------------------------------------

    /// The value of `column` most tracks have (e.g. for `analysedBits`), if any track has one.
    fn typical(&self, column: &str) -> Result<Option<Value>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {column} FROM content WHERE {column} IS NOT NULL GROUP BY {column} ORDER BY count(*) DESC LIMIT 1"
                ),
                [],
                |r| r.get::<_, Value>(0),
            )
            .optional()?)
    }

    /// Defaults for rows built from `export.pdb` tracks, taken from this database's own tracks.
    pub fn content_defaults(&self) -> Result<ContentDefaults> {
        // What rekordbox 7 writes for an analysed track, should the database have no tracks yet.
        Ok(ContentDefaults {
            analysed_bits: self.typical("analysedBits")?.unwrap_or(Value::Integer(105)),
            content_link: self.typical("contentLink")?.unwrap_or(Value::Integer(788_224)),
        })
    }

    /// A track row with its references resolved, ready for [`OneLibrary::add_track`] elsewhere.
    pub fn export_track(&self, id: u32) -> Result<Option<ContentRecord>> {
        let mut stmt = self.conn.prepare("SELECT * FROM content WHERE content_id = ?1")?;
        let names: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();
        let Some(values) = stmt
            .query_row([id], |r| (0..names.len()).map(|i| r.get::<_, Value>(i)).collect::<rusqlite::Result<Vec<_>>>())
            .optional()?
        else {
            return Ok(None);
        };
        let row: HashMap<&str, &Value> = names.iter().map(String::as_str).zip(values.iter()).collect();
        let reference = |col: &str| match row.get(col) {
            Some(Value::Integer(i)) if *i > 0 => Some(*i),
            _ => None,
        };
        let name = |table: Lookup, col: &str| -> Result<Option<String>> {
            let Some(id) = reference(col) else { return Ok(None) };
            Ok(self
                .conn
                .query_row(&format!("SELECT name FROM {} WHERE {} = ?1", table.table(), table.id()), [id], |r| {
                    Ok(text(r.get_ref(0)?))
                })
                .optional()?
                .filter(|n| !n.is_empty()))
        };
        let album = match reference("album_id") {
            None => None,
            Some(album_id) => self
                .conn
                .query_row(
                    "SELECT al.name, a.name FROM album al LEFT JOIN artist a ON a.artist_id = al.artist_id
                     WHERE al.album_id = ?1",
                    [album_id],
                    |r| {
                        let artist = text(r.get_ref(1)?);
                        Ok((text(r.get_ref(0)?), (!artist.is_empty()).then_some(artist)))
                    },
                )
                .optional()?
                .filter(|(n, _)| !n.is_empty()),
        };
        let mut cues = Vec::new();
        let mut cue_stmt = self.conn.prepare("SELECT * FROM cue WHERE content_id = ?1 ORDER BY cue_id")?;
        let cue_names: Vec<String> = cue_stmt.column_names().into_iter().map(String::from).collect();
        let mut rows = cue_stmt.query([id])?;
        while let Some(r) = rows.next()? {
            let mut cue = Vec::new();
            let mut cue_id = 0;
            for (i, n) in cue_names.iter().enumerate() {
                match n.as_str() {
                    "cue_id" => cue_id = int(r.get_ref(i)?),
                    "content_id" => {}
                    _ => cue.push((n.clone(), r.get::<_, Value>(i)?)),
                }
            }
            cues.push((cue_id, cue));
        }
        let my_tags = self
            .conn
            .prepare(
                "SELECT c.name, t.name FROM myTag_content mc
                 JOIN myTag t ON t.myTag_id = mc.myTag_id JOIN myTag c ON c.myTag_id = t.myTag_id_parent
                 WHERE mc.content_id = ?1 ORDER BY c.sequenceNo, t.sequenceNo",
            )?
            .query_map([id], |r| Ok((text(r.get_ref(0)?), text(r.get_ref(1)?))))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Some(ContentRecord {
            values: names
                .iter()
                .zip(values.iter())
                .filter(|(n, _)| !REFERENCE_COLUMNS.contains(&n.as_str()))
                .map(|(n, v)| (n.clone(), v.clone()))
                .collect(),
            artist: name(Lookup::Artist, "artist_id_artist")?,
            remixer: name(Lookup::Artist, "artist_id_remixer")?,
            original_artist: name(Lookup::Artist, "artist_id_originalArtist")?,
            composer: name(Lookup::Artist, "artist_id_composer")?,
            lyricist: name(Lookup::Artist, "artist_id_lyricist")?,
            album,
            genre: name(Lookup::Genre, "genre_id")?,
            label: name(Lookup::Label, "label_id")?,
            key: name(Lookup::Key, "key_id")?,
            cues,
            my_tags,
        }))
    }

    /// The id of the row named `name`, created if missing.
    fn named(&self, table: Lookup, name: &str) -> Result<i64> {
        let found = self
            .conn
            .query_row(&format!("SELECT {} FROM {} WHERE name = ?1", table.id(), table.table()), [name], |r| r.get(0))
            .optional()?;
        if let Some(id) = found {
            return Ok(id);
        }
        let id = self.next_id(table.table(), table.id())?;
        let sql = match table {
            Lookup::Artist => "INSERT INTO artist (artist_id, name, nameForSearch) VALUES (?1, ?2, NULL)",
            Lookup::Genre => "INSERT INTO genre (genre_id, name) VALUES (?1, ?2)",
            Lookup::Label => "INSERT INTO label (label_id, name) VALUES (?1, ?2)",
            Lookup::Key => "INSERT INTO \"key\" (key_id, name) VALUES (?1, ?2)",
            Lookup::Album | Lookup::Image => unreachable!("albums and images are not looked up by name alone"),
        };
        self.conn.execute(sql, params![id, name])?;
        Ok(id)
    }

    fn album(&self, name: &str, artist: Option<i64>) -> Result<i64> {
        let found = self
            .conn
            .query_row("SELECT album_id FROM album WHERE name = ?1 AND artist_id IS ?2", params![name, artist], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(id) = found {
            return Ok(id);
        }
        let id = self.next_id("album", "album_id")?;
        self.conn.execute(
            "INSERT INTO album (album_id, name, artist_id, image_id, isComplation, nameForSearch)
             VALUES (?1, ?2, ?3, NULL, 0, NULL)",
            params![id, name, artist],
        )?;
        Ok(id)
    }

    /// Refuse to delete tracks or cues while a table rekordbox 7 doesn't have (`schema.sql`) has
    /// a column naming them: a newer rekordbox's, whose rows would be left pointing at nothing.
    pub(super) fn check_no_unknown_references(&self) -> Result<()> {
        let known: HashSet<&str> = include_str!("schema.sql")
            .lines()
            .filter_map(|l| l.strip_prefix("CREATE TABLE ")?.split('(').next())
            .collect();
        let mut stmt = self.conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
        let tables: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for table in tables.iter().filter(|t| !known.contains(t.as_str()) && !t.starts_with("sqlite_")) {
            let columns = self.columns(&ident(table))?;
            if let Some(c) = columns.iter().find(|c| ["content_id", "cue_id"].iter().any(|k| c.contains(k))) {
                return Err(Error::OneLibrary(format!(
                    "its table {table} (from a newer rekordbox?) refers to tracks or cues by {c}, and \
                     removing them would leave its rows dangling; remove them in rekordbox instead"
                )));
            }
        }
        Ok(())
    }

    fn columns(&self, table: &str) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let names = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<_>>()?;
        Ok(names)
    }

    /// Add a track. `path` and `analyze_path` are device paths (`/Contents/...`); columns this
    /// database doesn't have are dropped.
    pub fn add_track(
        &mut self,
        rec: &ContentRecord,
        path: &str,
        analyze_path: &str,
        image: Option<u32>,
    ) -> Result<u32> {
        Ok(self.add_track_with_cues(rec, path, analyze_path, image)?.0)
    }

    /// [`OneLibrary::add_track`], also returning the new id of each of `rec`'s cues (keyed by
    /// the source's `cue_id`), for hot cue bank lists that point at them.
    pub fn add_track_with_cues(
        &mut self,
        rec: &ContentRecord,
        path: &str,
        analyze_path: &str,
        image: Option<u32>,
    ) -> Result<(u32, HashMap<i64, i64>)> {
        let artist = |n: &Option<String>| n.as_deref().map(|n| self.named(Lookup::Artist, n)).transpose();
        let file_name = path.rsplit('/').next().unwrap_or(path).to_string();
        let album_artist = rec.album.as_ref().and_then(|(_, a)| a.clone());
        let album = match &rec.album {
            Some((name, _)) => Some(self.album(name, artist(&album_artist)?)?),
            None => None,
        };
        let named = |t: Lookup, n: &Option<String>| n.as_deref().map(|n| self.named(t, n)).transpose();
        let id = self.next_id("content", "content_id")?;
        let mut values: Vec<(String, Value)> = vec![
            ("content_id".into(), Value::Integer(id)),
            ("artist_id_artist".into(), artist(&rec.artist)?.into()),
            ("artist_id_remixer".into(), artist(&rec.remixer)?.into()),
            ("artist_id_originalArtist".into(), artist(&rec.original_artist)?.into()),
            ("artist_id_composer".into(), artist(&rec.composer)?.into()),
            // rekordbox writes 0, not NULL, for "no lyricist" and "no key".
            ("artist_id_lyricist".into(), Value::Integer(artist(&rec.lyricist)?.unwrap_or(0))),
            ("album_id".into(), album.into()),
            ("genre_id".into(), named(Lookup::Genre, &rec.genre)?.into()),
            ("label_id".into(), named(Lookup::Label, &rec.label)?.into()),
            ("key_id".into(), Value::Integer(named(Lookup::Key, &rec.key)?.unwrap_or(0))),
            ("image_id".into(), image.map(i64::from).into()),
            ("path".into(), Value::Text(path.to_string())),
            ("fileName".into(), Value::Text(file_name)),
            ("analysisDataFilePath".into(), Value::Text(analyze_path.to_string())),
        ];
        let set: HashSet<String> = values.iter().map(|(k, _)| k.clone()).collect();
        values.extend(rec.values.iter().filter(|(k, _)| !set.contains(k)).cloned());
        let columns = self.columns("content")?;
        values.retain(|(k, _)| columns.contains(k));
        let sql = format!(
            "INSERT INTO content ({}) VALUES ({})",
            values.iter().map(|(k, _)| ident(k)).collect::<Vec<_>>().join(", "),
            vec!["?"; values.len()].join(", ")
        );
        self.conn.execute(&sql, rusqlite::params_from_iter(values.iter().map(|(_, v)| v)))?;

        let cue_columns = self.columns("cue")?;
        let mut cue_ids = HashMap::new();
        for (source_id, cue) in &rec.cues {
            let cue_id = self.next_id("cue", "cue_id")?;
            cue_ids.insert(*source_id, cue_id);
            let mut row =
                vec![("cue_id".to_string(), Value::Integer(cue_id)), ("content_id".into(), Value::Integer(id))];
            row.extend(cue.iter().filter(|(k, _)| cue_columns.contains(k)).cloned());
            let sql = format!(
                "INSERT INTO cue ({}) VALUES ({})",
                row.iter().map(|(k, _)| ident(k)).collect::<Vec<_>>().join(", "),
                vec!["?"; row.len()].join(", ")
            );
            self.conn.execute(&sql, rusqlite::params_from_iter(row.iter().map(|(_, v)| v)))?;
        }
        for (category, tag) in &rec.my_tags {
            let category = match self.find_node("myTag", category, 0)? {
                Some(c) => c,
                None => self.create_node("myTag", category, 0, true)?.into(),
            };
            let parent = id32(category, "myTag")?;
            let tag = match self.find_node("myTag", tag, parent)? {
                Some(t) => t,
                None => self.create_node("myTag", tag, parent, false)?.into(),
            };
            self.conn.execute("INSERT INTO myTag_content (myTag_id, content_id) VALUES (?1, ?2)", params![tag, id])?;
        }
        self.update_count()?;
        Ok((id32(id, "content")?, cue_ids))
    }

    // -- playlist-shaped trees: playlist, history, hotCueBankList, myTag ---------------------

    /// Add a node (attribute 1 = folder) to a playlist-shaped table as the last child of
    /// `parent` (0 for the top level).
    fn create_node(&self, table: &str, name: &str, parent: u32, is_folder: bool) -> Result<u32> {
        let id = self.next_id(table, &format!("{table}_id"))?;
        let seq: i64 = self.conn.query_row(
            &format!("SELECT coalesce(max(sequenceNo) + 1, 0) FROM {table} WHERE {table}_id_parent = ?1"),
            [parent],
            |r| r.get(0),
        )?;
        self.conn.execute(
            &format!(
                "INSERT INTO {table} ({table}_id, sequenceNo, name, attribute, {table}_id_parent) VALUES (?1, ?2, ?3, ?4, ?5)"
            ),
            params![id, seq, name, i64::from(is_folder), parent],
        )?;
        id32(id, table)
    }

    fn find_node(&self, table: &str, name: &str, parent: u32) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {table}_id FROM {table} WHERE name = ?1 AND {table}_id_parent = ?2"),
                params![name, parent],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Append `item` to list `list` of `<table>` in `<table>_<entries>` (sequence numbers from 1).
    fn append_entry(&self, table: &str, entries: &str, item_column: &str, list: u32, item: i64) -> Result<()> {
        self.conn.execute(
            &format!(
                "INSERT INTO {table}_{entries} ({table}_id, {item_column}, sequenceNo)
                 VALUES (?1, ?2, (SELECT coalesce(max(sequenceNo), 0) + 1 FROM {table}_{entries} WHERE {table}_id = ?1))"
            ),
            params![list, item],
        )?;
        Ok(())
    }

    /// Create a playlist (or folder) as the last child of `parent` (0 for the top level).
    pub fn create_playlist(&mut self, name: &str, parent: u32, is_folder: bool) -> Result<u32> {
        self.create_node("playlist", name, parent, is_folder)
    }

    /// Append a track to a playlist.
    pub fn add_to_playlist(&mut self, playlist: u32, track: u32) -> Result<()> {
        self.append_entry("playlist", "content", "content_id", playlist, track.into())
    }

    /// Create a history playlist (or folder) as the last child of `parent` (0 for the top level).
    pub fn create_history(&mut self, name: &str, parent: u32, is_folder: bool) -> Result<u32> {
        self.create_node("history", name, parent, is_folder)
    }

    /// Append a track to a history playlist.
    pub fn add_to_history(&mut self, history: u32, track: u32) -> Result<()> {
        self.append_entry("history", "content", "content_id", history, track.into())
    }

    /// Hot cue bank lists (and folders), shaped like playlists; `track_ids` are `cue_id`s.
    pub fn hot_cue_bank_lists(&self) -> Result<Vec<Playlist>> {
        let mut entries: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT hotCueBankList_id, cue_id FROM hotCueBankList_cue ORDER BY hotCueBankList_id, sequenceNo, rowid",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            entries
                .entry(id32(int(row.get_ref(0)?), "hotCueBankList")?)
                .or_default()
                .push(id32(int(row.get_ref(1)?), "cue")?);
        }
        self.tree("hotCueBankList", entries)
    }

    /// Create a hot cue bank list (or folder) as the last child of `parent`.
    pub fn create_hot_cue_bank_list(&mut self, name: &str, parent: u32, is_folder: bool) -> Result<u32> {
        self.create_node("hotCueBankList", name, parent, is_folder)
    }

    /// Append a cue to a hot cue bank list.
    pub fn add_to_hot_cue_bank_list(&mut self, list: u32, cue: i64) -> Result<()> {
        self.append_entry("hotCueBankList", "cue", "cue_id", list, cue)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// rekordbox's columns are loosely typed: numbers can come back as text or reals, and text
    /// as numbers.
    #[test]
    fn values_read_as_the_type_a_column_should_have() {
        assert_eq!(int(ValueRef::Real(2.9)), 2);
        assert_eq!(int(ValueRef::Text(b" 42 ")), 42);
        assert_eq!(int(ValueRef::Text(b"forty")), 0);
        assert_eq!(int(ValueRef::Null), 0);
        assert_eq!(text(ValueRef::Integer(5)), "5");
        assert_eq!(text(ValueRef::Real(1.5)), "1.5");
        assert_eq!(text(ValueRef::Null), "");
    }

    #[test]
    fn a_database_without_a_content_table_isnt_onelibrarys() {
        let one = OneLibrary::create().unwrap();
        one.conn.execute_batch("DROP TABLE content").unwrap();
        let err = OneLibrary::from_encrypted(&one.to_encrypted().unwrap(), None).unwrap_err();
        assert_eq!(
            err.to_string(),
            Error::OneLibrary("no content table; not a rekordbox OneLibrary database".into()).to_string()
        );
        assert!(format!("{one:?}").starts_with("OneLibrary { path: "));
    }

    #[test]
    fn creates_empty_databases_sqlite_and_the_cipher_accept() {
        let mut one = OneLibrary::create().unwrap();
        let check: String = one.conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
        assert_eq!(check, "ok");
        let menus: i64 = one.conn.query_row("SELECT count(*) FROM menuItem", [], |r| r.get(0)).unwrap();
        assert_eq!(menus, 27);
        let p = one.create_playlist("Set", 0, false).unwrap();
        let defaults = one.content_defaults().unwrap();
        let rec =
            ContentRecord::from_track(&crate::model::tests::track(1, "A", "B", "/Contents/A/B.mp3"), true, &defaults);
        let t = one.add_track(&rec, "/Contents/A/B.mp3", "", None).unwrap();
        one.add_to_playlist(p, t).unwrap();
        let bytes = one.to_encrypted().unwrap();
        let (_, plain) = decrypt(&bytes, None).unwrap();
        assert_eq!((plain[18], plain[19], plain[20]), (2, 2, cipher::RESERVE as u8), "WAL mode, room for the cipher");
        let lib = OneLibrary::from_encrypted(&bytes, None).unwrap().library().unwrap();
        assert_eq!(lib.tracks.len(), 1);
        assert_eq!(lib.find_playlist("Set").unwrap().track_ids, [t]);
        // Two new databases never share a salt.
        assert_ne!(bytes[..16], OneLibrary::create().unwrap().to_encrypted().unwrap()[..16]);
    }

    /// A database whose b-tree for `table` claims more cells than a page holds, re-encrypted.
    pub(crate) fn with_broken_table(conn: &Connection, codec: &Codec, mut plain: Vec<u8>, table: &str) -> Vec<u8> {
        let root: u32 =
            conn.query_row("SELECT rootpage FROM sqlite_master WHERE name = ?1", [table], |r| r.get(0)).unwrap();
        let at = (root as usize - 1) * PAGE_SIZE;
        plain[at + 3..at + 5].copy_from_slice(&u16::MAX.to_be_bytes());
        codec.encrypt(&plain).unwrap()
    }

    #[test]
    fn a_damaged_b_tree_is_refused_before_anything_reads_it() {
        let one = OneLibrary::create().unwrap();
        let (codec, plain) = decrypt(&one.to_encrypted().unwrap(), None).unwrap();
        let bytes = with_broken_table(&one.conn, &codec, plain, "menuItem");
        let err = OneLibrary::from_encrypted(&bytes, None).unwrap_err().to_string();
        assert!(err.contains("SQLite finds it damaged"), "{err}");
    }

    /// SQLite reads a page from the WAL when a committed frame holds it, so the main file's copy
    /// may be anything (zeros an interrupted checkpoint left, say). Anywhere else, zeros are damage.
    #[test]
    fn only_pages_sqlite_reads_must_authenticate() {
        let one = OneLibrary::create().unwrap();
        let bytes = one.to_encrypted().unwrap();
        let (codec, plain) = decrypt(&bytes, None).unwrap();
        let pages = (plain.len() / PAGE_SIZE) as u32;
        let frame = codec.encrypt_page(2, &plain[PAGE_SIZE..2 * PAGE_SIZE]).unwrap();
        let wal = wal::tests::build(PAGE_SIZE, (1, 2), &[(2, pages, frame)]);
        let mut holed = bytes.clone();
        holed[PAGE_SIZE..2 * PAGE_SIZE].fill(0);
        // Equal but for the reserve, which keeps each page's own IV and HMAC.
        let usable = |db: &[u8]| {
            db.chunks(PAGE_SIZE).flat_map(|p| p[..PAGE_SIZE - cipher::RESERVE].to_vec()).collect::<Vec<_>>()
        };
        assert_eq!(usable(&decrypt(&holed, Some(&wal)).unwrap().1), usable(&plain));
        let err = decrypt(&holed, None).map(|_| ()).unwrap_err().to_string();
        assert!(err.contains("page 2 fails authentication"), "{err}");

        let committed = wal::committed(&wal, PAGE_SIZE).unwrap().unwrap();
        assert!(!reads_main_page(2, Some(&committed)));
        assert!(!reads_main_page(pages + 1, Some(&committed)), "past the last commit");
        assert!(reads_main_page(3, Some(&committed)));
        assert!(!reads_main_page(cipher::LOCK_PAGE, None));
        assert!(reads_main_page(1, None));
    }

    #[test]
    fn file_types() {
        assert_eq!(file_type("/Contents/a/b.MP3"), 1);
        assert_eq!(file_type("/Contents/a/b.m4a"), 4);
        assert_eq!(file_type("/Contents/a/b.aiff"), 12);
        assert_eq!(file_type("/Contents/a/b"), 0);
    }
}
