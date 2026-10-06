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
//!   file's full path on this computer (`/Users/me/Music/x.mp3`, `C:/Users/me/Music/x.mp3`),
//!   `StockDate` the date it was added.
//! * playlists and My Tags hang off `ParentID` `"root"`; a playlist's `Attribute` is 0 for a
//!   playlist, 1 for a folder and 4 for an intelligent playlist, whose rules (`SmartList`) aren't
//!   evaluated here, so it reads as empty.
//! * `djmdCue.Kind` is 0 for a memory cue, otherwise the hot cue (1 = A); `InMsec`/`OutMsec` are
//!   milliseconds, `OutMsec` -1 for a cue that isn't a loop. A hot cue's colour is taken to be
//!   `ColorTableIndex` as a code of rekordbox's hot cue palette, like OneLibrary's
//!   `colorTableIndex` (inferred, not documented).

use std::collections::{BTreeMap, HashMap, HashSet};
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
    pub cues: BTreeMap<u32, Vec<Cue>>,
    /// My Tags: categories in rekordbox's order, each with its tags and their tracks.
    pub tags: Vec<TagCategory>,
    /// Ids of intelligent playlists, whose rules aren't evaluated.
    pub smart_playlists: Vec<u32>,
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
    let codec = cipher::Codec::with_passphrase(key.as_bytes(), salt);
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
    let mut dst = std::fs::File::create(&out).at(&out)?;
    dst.write_all(&page1).at(&out)?;
    let mut page = vec![0u8; cipher::PAGE_SIZE];
    for pgno in 2..=(len / cipher::PAGE_SIZE as u64) as u32 {
        src.read_exact(&mut page).at(&enc)?;
        dst.write_all(&codec.decrypt_page(pgno, &page).map_err(damaged)?).at(&out)?;
    }
    drop(src);
    std::fs::remove_file(&enc).at(&enc)?;
    if let Some(committed) = wal.as_deref().map(|w| wal::committed(w, cipher::PAGE_SIZE)).transpose()?.flatten() {
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
    fn columns(&self, table: &str) -> Result<HashSet<String>> {
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
    fn names(&self, table: &str, column: &str) -> Result<HashMap<String, String>> {
        Ok(self.select(table, &["ID", column])?.into_iter().map(|r| (text(&r[0]), text(&r[1]))).collect())
    }

    fn collection(&self) -> Result<Collection> {
        let artists = self.names("djmdArtist", "Name")?;
        let genres = self.names("djmdGenre", "Name")?;
        let labels = self.names("djmdLabel", "Name")?;
        let keys = self.names("djmdKey", "ScaleName")?;
        let albums: HashMap<String, (String, String)> = self
            .select("djmdAlbum", &["ID", "Name", "AlbumArtistID"])?
            .into_iter()
            .map(|r| (text(&r[0]), (text(&r[1]), text(&r[2]))))
            .collect();
        let name = |map: &HashMap<String, String>, id: &Value| map.get(&text(id)).cloned().unwrap_or_default();

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
        let mut images: HashMap<String, u32> = HashMap::new();
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

        let mut cues: BTreeMap<u32, Vec<Cue>> = BTreeMap::new();
        for r in self.select("djmdCue", &["ContentID", "InMsec", "OutMsec", "Kind", "ColorTableIndex", "Comment"])? {
            let Some(&track) = track_ids.get(&text(&r[0])) else { continue };
            let start = int(&r[1]).max(0);
            let end = int(&r[2]);
            let kind = int(&r[3]);
            let hot_cue = (1..=16).contains(&kind).then(|| (kind - 1) as u8);
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
        })
    }

    /// A playlist-shaped tree and its entries (`djmdPlaylist` + `djmdSongPlaylist`, or the
    /// history tables), plus the ids of intelligent playlists.
    fn tree(
        &self,
        table: &str,
        entries: &str,
        list_column: &str,
        tracks: &HashMap<String, u32>,
    ) -> Result<(Vec<Playlist>, Vec<u32>)> {
        let rows = self.select(table, &["ID", "Seq", "Name", "Attribute", "ParentID"])?;
        let ids = id_map(rows.iter().map(|r| text(&r[0])));
        let mut songs: HashMap<u32, Vec<(i64, usize, u32)>> = HashMap::new();
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
    fn tags(&self, tracks: &HashMap<String, u32>) -> Result<Vec<TagCategory>> {
        let rows = self.select("djmdMyTag", &["ID", "Seq", "Name", "ParentID"])?;
        let mut links: HashMap<String, Vec<(i64, usize, u32)>> = HashMap::new();
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
                            let mut seen = HashSet::new();
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
fn id_map(ids: impl Iterator<Item = String>) -> HashMap<String, u32> {
    let ids: Vec<String> = ids.collect();
    let parsed: Vec<Option<u32>> = ids.iter().map(|s| s.trim().parse().ok().filter(|n| *n != 0)).collect();
    let distinct: HashSet<u32> = parsed.iter().flatten().copied().collect();
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
        let mut tagged = HashSet::new();
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

    /// My Tags as `Category/Tag` by lower-cased file key, the shape track filters (`tag:`) take.
    pub fn tags_by_file(&self) -> HashMap<String, Vec<String>> {
        let files: HashMap<u32, String> =
            self.library.tracks.iter().map(|t| (t.id, paths::key(&t.file_path).to_lowercase())).collect();
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
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

#[cfg(test)]
#[path = "../tests/support/sqlcipher.rs"]
mod sqlcipher;

#[cfg(test)]
pub(crate) mod tests {
    use rusqlite::params;

    use super::*;
    use crate::model::Playlist;
    use crate::onelibrary::cipher::{Codec, PAGE_SIZE, RESERVE, SQLITE_MAGIC};

    /// Columns every table but `djmdProperty` ends with.
    const STATS: &str = "UUID VARCHAR(255), rb_data_status INTEGER DEFAULT 0, rb_local_data_status INTEGER DEFAULT 0, \
        rb_local_deleted TINYINT(1) DEFAULT 0, rb_local_synced TINYINT(1) DEFAULT 0, usn BIGINT, rb_local_usn BIGINT, \
        created_at TEXT NOT NULL DEFAULT '2024-01-01 10:00:00.000 +00:00', \
        updated_at TEXT NOT NULL DEFAULT '2024-01-01 10:00:00.000 +00:00'";

    /// The tables read here, with the columns pyrekordbox lists for rekordbox 6/7 (less the ones
    /// nothing reads at the end of `djmdContent`).
    const TABLES: &[(&str, &str)] = &[
        ("djmdArtist", "ID VARCHAR(255) PRIMARY KEY, Name VARCHAR(255), SearchStr VARCHAR(255)"),
        (
            "djmdAlbum",
            "ID VARCHAR(255) PRIMARY KEY, Name VARCHAR(255), AlbumArtistID VARCHAR(255), ImagePath VARCHAR(255), \
             Compilation INTEGER, SearchStr VARCHAR(255)",
        ),
        ("djmdGenre", "ID VARCHAR(255) PRIMARY KEY, Name VARCHAR(255)"),
        ("djmdLabel", "ID VARCHAR(255) PRIMARY KEY, Name VARCHAR(255)"),
        ("djmdKey", "ID VARCHAR(255) PRIMARY KEY, ScaleName VARCHAR(255), Seq INTEGER"),
        ("djmdColor", "ID VARCHAR(255) PRIMARY KEY, ColorCode INTEGER, SortKey INTEGER, Commnt VARCHAR(255)"),
        (
            "djmdContent",
            "ID VARCHAR(255) PRIMARY KEY, FolderPath VARCHAR(255), FileNameL VARCHAR(255), FileNameS VARCHAR(255), \
             Title VARCHAR(255), ArtistID VARCHAR(255), AlbumID VARCHAR(255), GenreID VARCHAR(255), BPM INTEGER, \
             Length INTEGER, TrackNo INTEGER, BitRate INTEGER, BitDepth INTEGER, Commnt TEXT, FileType INTEGER, \
             Rating INTEGER, ReleaseYear INTEGER, RemixerID VARCHAR(255), LabelID VARCHAR(255), \
             OrgArtistID VARCHAR(255), KeyID VARCHAR(255), StockDate VARCHAR(255), ColorID VARCHAR(255), \
             DJPlayCount VARCHAR(255), ImagePath VARCHAR(255), MasterDBID VARCHAR(255), MasterSongID VARCHAR(255), \
             AnalysisDataPath VARCHAR(255), SearchStr VARCHAR(255), FileSize INTEGER, DiscNo INTEGER, \
             ComposerID VARCHAR(255), Subtitle VARCHAR(255), SampleRate INTEGER, DisableQuantize INTEGER, \
             Analysed INTEGER, ReleaseDate VARCHAR(255), DateCreated VARCHAR(255), ContentLink INTEGER, \
             Tag VARCHAR(255), ISRC VARCHAR(255)",
        ),
        (
            "djmdCue",
            "ID VARCHAR(255) PRIMARY KEY, ContentID VARCHAR(255), InMsec INTEGER, InFrame INTEGER, \
             InMpegFrame INTEGER, InMpegAbs INTEGER, OutMsec INTEGER, OutFrame INTEGER, OutMpegFrame INTEGER, \
             OutMpegAbs INTEGER, Kind INTEGER, Color INTEGER, ColorTableIndex INTEGER, ActiveLoop INTEGER, \
             Comment VARCHAR(255), BeatLoopSize INTEGER, CueMicrosec INTEGER, InPointSeekInfo VARCHAR(255), \
             OutPointSeekInfo VARCHAR(255), ContentUUID VARCHAR(255)",
        ),
        (
            "djmdPlaylist",
            "ID VARCHAR(255) PRIMARY KEY, Seq INTEGER, Name VARCHAR(255), ImagePath VARCHAR(255), Attribute INTEGER, \
             ParentID VARCHAR(255), SmartList TEXT",
        ),
        (
            "djmdSongPlaylist",
            "ID VARCHAR(255) PRIMARY KEY, PlaylistID VARCHAR(255), ContentID VARCHAR(255), TrackNo INTEGER",
        ),
        (
            "djmdHistory",
            "ID VARCHAR(255) PRIMARY KEY, Seq INTEGER, Name VARCHAR(255), Attribute INTEGER, ParentID VARCHAR(255), \
             DateCreated VARCHAR(255)",
        ),
        (
            "djmdSongHistory",
            "ID VARCHAR(255) PRIMARY KEY, HistoryID VARCHAR(255), ContentID VARCHAR(255), TrackNo INTEGER",
        ),
        (
            "djmdMyTag",
            "ID VARCHAR(255) PRIMARY KEY, Seq INTEGER, Name VARCHAR(255), Attribute INTEGER, ParentID VARCHAR(255)",
        ),
        ("djmdSongMyTag", "ID VARCHAR(255) PRIMARY KEY, MyTagID VARCHAR(255), ContentID VARCHAR(255), TrackNo INTEGER"),
    ];

    /// An empty SQLite database laid out for SQLCipher (4096-byte pages, 80 reserved bytes each),
    /// as rekordbox's are.
    fn empty() -> Vec<u8> {
        let mut page = vec![0u8; PAGE_SIZE];
        page[..16].copy_from_slice(SQLITE_MAGIC);
        page[16..18].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes());
        page[18] = 1;
        page[19] = 1;
        page[20] = RESERVE as u8;
        page[21] = 64;
        page[22] = 32;
        page[23] = 32;
        page[24..28].copy_from_slice(&1u32.to_be_bytes());
        page[28..32].copy_from_slice(&1u32.to_be_bytes());
        page[44..48].copy_from_slice(&4u32.to_be_bytes());
        page[56..60].copy_from_slice(&1u32.to_be_bytes());
        page[92..96].copy_from_slice(&1u32.to_be_bytes());
        page[96..100].copy_from_slice(&3_046_000u32.to_be_bytes());
        page[100] = 0x0d;
        page[105..107].copy_from_slice(&((PAGE_SIZE - RESERVE) as u16).to_be_bytes());
        page
    }

    /// A small rekordbox-shaped `master.db`, built in plain SQLite and encrypted on request.
    pub(crate) struct Fixture {
        _dir: tempfile::TempDir,
        plain: PathBuf,
        pub(crate) conn: Connection,
    }

    impl Fixture {
        pub(crate) fn new() -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let plain = dir.path().join("plain.db");
            std::fs::write(&plain, empty()).unwrap();
            let conn = Connection::open(&plain).unwrap();
            for (name, columns) in TABLES {
                conn.execute_batch(&format!("CREATE TABLE {name} ({columns}, {STATS});")).unwrap();
            }
            conn.execute_batch(
                "CREATE TABLE djmdProperty (DBID VARCHAR(255) PRIMARY KEY, DBVersion VARCHAR(255), \
                 BaseDBDrive VARCHAR(255), CurrentDBDrive VARCHAR(255), DeviceID VARCHAR(255), Reserved1 TEXT, \
                 created_at TEXT NOT NULL DEFAULT '', updated_at TEXT NOT NULL DEFAULT '');
                 INSERT INTO djmdProperty (DBID, DBVersion) VALUES ('1234567890', '7000');",
            )
            .unwrap();
            Fixture { _dir: dir, plain, conn }
        }

        pub(crate) fn exec(&self, sql: &str) {
            self.conn.execute_batch(sql).unwrap();
        }

        /// A track row: `ID`, `FolderPath`, `Title` and the artist's name (an artist row is made).
        pub(crate) fn track(&self, id: &str, path: &str, artist: &str, title: &str) {
            self.conn.execute("INSERT OR IGNORE INTO djmdArtist (ID, Name) VALUES (?1, ?1)", [artist]).unwrap();
            self.conn
                .execute(
                    "INSERT INTO djmdContent (ID, FolderPath, FileNameL, Title, ArtistID, BPM, Length, BitRate, \
                     SampleRate, BitDepth, FileSize, FileType, StockDate) \
                     VALUES (?1, ?2, ?3, ?4, ?5, 12800, 300, 320, 44100, 16, 1000, 1, '2024-03-01')",
                    params![id, path, path.rsplit('/').next().unwrap(), title, artist],
                )
                .unwrap();
        }

        /// The database as plain SQLite.
        pub(crate) fn plain(&self) -> Vec<u8> {
            std::fs::read(&self.plain).unwrap()
        }

        /// The database encrypted as rekordbox does, under `key`.
        pub(crate) fn encrypted(&self, key: &str) -> Vec<u8> {
            Codec::with_passphrase(key.as_bytes(), [9; 16]).encrypt(&self.plain()).unwrap()
        }
    }

    /// Three tracks (and a deleted one), a folder with a playlist, an intelligent playlist, a
    /// history list, cues and My Tags.
    pub(crate) fn sample() -> Fixture {
        let f = Fixture::new();
        f.track("84935471", "/Users/dj/Music/Daft Punk/Discovery/01 One More Time.mp3", "Daft Punk", "One More Time");
        f.track("205356547", "/Users/dj/Music/Downloads/ngo.mp3", "Floorplan", "Never Grow Old");
        f.track("3", "/Users/dj/Music/gone.mp3", "Nobody", "Deleted");
        f.track("4", "/Users/dj/Music/Only Here.mp3", "Somebody", "Only Here");
        f.exec(
            "INSERT INTO djmdAlbum (ID, Name, AlbumArtistID) VALUES ('1', 'Discovery', 'Daft Punk');
             INSERT INTO djmdGenre (ID, Name) VALUES ('1', 'House');
             INSERT INTO djmdLabel (ID, Name) VALUES ('1', 'Virgin');
             INSERT INTO djmdKey (ID, ScaleName, Seq) VALUES ('1', 'Am', 1);
             UPDATE djmdContent SET AlbumID = '1', GenreID = '1', LabelID = '1', KeyID = '1', BPM = 12280, Length = 320,
                 Rating = 5, ColorID = '2', DJPlayCount = '7', ISRC = 'GBDUW0000059', ReleaseYear = 2000,
                 ImagePath = '/PIONEER/Artwork/00001/a1.jpg', Subtitle = 'Radio Edit', Commnt = 'classic',
                 AnalysisDataPath = '/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT'
                 WHERE ID = '84935471';
             UPDATE djmdContent SET Length = 412, StockDate = NULL, DateCreated = '2023-12-24' WHERE ID = '205356547';
             UPDATE djmdContent SET rb_local_deleted = 1 WHERE ID = '3';
             INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID) VALUES
                 ('10', 1, 'Shows', 1, 'root'), ('11', 1, 'Friday', 0, '10'), ('12', 2, 'Recent', 4, 'root');
             INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID, rb_local_deleted) VALUES
                 ('13', 3, 'Binned', 0, 'root', 1);
             INSERT INTO djmdSongPlaylist (ID, PlaylistID, ContentID, TrackNo) VALUES
                 ('a', '11', '84935471', 2), ('b', '11', '205356547', 1), ('c', '11', '3', 3), ('d', '13', '4', 1);
             INSERT INTO djmdHistory (ID, Seq, Name, Attribute, ParentID) VALUES ('20', 1, 'HISTORY 2024-03-01', 0, 'root');
             INSERT INTO djmdSongHistory (ID, HistoryID, ContentID, TrackNo) VALUES ('h', '20', '84935471', 1);
             INSERT INTO djmdCue (ID, ContentID, InMsec, OutMsec, Kind, ColorTableIndex, Comment) VALUES
                 ('c1', '84935471', 1000, -1, 1, 1, 'Drop'),
                 ('c2', '84935471', 500, -1, 0, 0, ''),
                 ('c3', '84935471', 2000, 4000, 2, 0, 'Loop'),
                 ('c4', '3', 100, -1, 1, 0, 'deleted track');
             INSERT INTO djmdMyTag (ID, Seq, Name, Attribute, ParentID) VALUES
                 ('100', 0, 'Genre', 1, 'root'), ('101', 0, 'Deep', 0, '100'),
                 ('200', 1, 'Situation', 1, 'root'), ('201', 0, 'Peak', 0, '200');
             INSERT INTO djmdSongMyTag (ID, MyTagID, ContentID, TrackNo) VALUES
                 ('t1', '101', '84935471', 1), ('t2', '201', '205356547', 1), ('t3', '201', '84935471', 2);",
        );
        f
    }

    fn write(dir: &Path, bytes: &[u8]) -> PathBuf {
        let path = dir.join("master.db");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn reads_tracks_playlists_cues_tags_and_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &sample().encrypted(DEFAULT_KEY));
        let c = open(&path, DEFAULT_KEY).unwrap();
        assert_eq!(c.db_version.as_deref(), Some("7000"));
        let ids: Vec<u32> = c.library.tracks.iter().map(|t| t.id).collect();
        assert_eq!(ids, [4, 84935471, 205356547], "rekordbox's ids, without the deleted track");
        let t = c.library.track(84935471).unwrap();
        assert_eq!(
            (t.artist.as_str(), t.title.as_str(), t.album.as_str(), t.album_artist.as_str()),
            ("Daft Punk", "One More Time", "Discovery", "Daft Punk")
        );
        assert_eq!((t.genre.as_str(), t.label.as_str(), t.key.as_str()), ("House", "Virgin", "Am"));
        assert_eq!((t.bpm, t.duration_secs, t.rating, t.color), (122.8, 320, 5, Some(TrackColor::Red)));
        assert_eq!((t.play_count, t.year, t.isrc.as_str()), (7, 2000, "GBDUW0000059"));
        assert_eq!((t.mix_name.as_str(), t.comment.as_str()), ("Radio Edit", "classic"));
        assert_eq!(t.file_path, "/Users/dj/Music/Daft Punk/Discovery/01 One More Time.mp3");
        assert_eq!(
            (t.date_added.as_str(), t.artwork_path.as_deref()),
            ("2024-03-01", Some("/PIONEER/Artwork/00001/a1.jpg"))
        );
        assert_eq!(c.library.track(205356547).unwrap().date_added, "2023-12-24", "falls back to DateCreated");

        let top: Vec<&Playlist> = c.library.children(None);
        assert_eq!(
            top.iter().map(|p| (p.name.as_str(), p.is_folder)).collect::<Vec<_>>(),
            [("Shows", true), ("Recent", false)]
        );
        let friday = c.library.find_playlist("Shows/Friday").unwrap();
        assert_eq!(friday.track_ids, [205356547, 84935471], "in TrackNo order, without the deleted track");
        assert_eq!(c.smart_playlists, [12]);
        assert_eq!(c.library.history_playlists, 1);
        assert_eq!(c.library.history[0].track_ids, [84935471]);

        let cues = &c.cues[&84935471];
        assert_eq!(cues.len(), 3);
        assert_eq!((cues[0].hot_cue, cues[0].start_ms, cues[0].end_ms), (None, 500, None));
        assert_eq!((cues[1].hot_cue, cues[1].comment.as_str()), (Some(0), "Drop"));
        assert_eq!(cues[1].color, Some(anlz::HOT_CUE_PALETTE[0]));
        assert_eq!((cues[2].hot_cue, cues[2].end_ms), (Some(1), Some(4000)));
        assert!(!c.cues.contains_key(&3));

        assert_eq!(c.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Genre", "Situation"]);
        assert_eq!(c.tags[1].tags[0].track_ids, [205356547, 84935471]);
        let by_file = c.tags_by_file();
        assert_eq!(
            by_file["users/dj/music/daft punk/discovery/01 one more time.mp3"],
            ["Genre/Deep".to_string(), "Situation/Peak".to_string()]
        );

        let info = c.info();
        assert_eq!((info.summary.tracks, info.summary.playlists, info.summary.folders), (3, 2, 1));
        assert_eq!((info.hot_cues, info.memory_cues, info.loops), (2, 1, 1));
        assert_eq!((info.tag_categories, info.tags, info.tagged_tracks), (2, 2, 2));
        assert_eq!(info.smart_playlists, ["Recent"]);
    }

    #[test]
    fn a_wrong_key_is_refused_and_the_right_one_given_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &sample().encrypted("another key"));
        let err = open(&path, DEFAULT_KEY).unwrap_err();
        assert_eq!(err.code(), "invalid", "{err}");
        assert!(err.to_string().contains(KEY_ENV), "{err}");
        assert_eq!(open(&path, "another key").unwrap().library.tracks.len(), 3);
        // Not a database at all.
        let junk = write(dir.path(), &[7u8; 3 * PAGE_SIZE]);
        assert!(open(&junk, DEFAULT_KEY).is_err());
    }

    #[test]
    fn a_damaged_page_is_reported_as_master_dbs() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = sample().encrypted(DEFAULT_KEY);
        bytes[PAGE_SIZE + 100] ^= 1;
        let path = write(dir.path(), &bytes);
        let err = open(&path, DEFAULT_KEY).unwrap_err().to_string();
        assert!(err.starts_with(&format!("could not parse {}: page 2 fails authentication", path.display())), "{err}");
    }

    #[test]
    fn a_plain_database_reads_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &sample().plain());
        assert_eq!(open(&path, "unused").unwrap().library.tracks.len(), 3);
    }

    #[test]
    fn committed_wal_frames_are_replayed_and_the_original_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let f = sample();
        let codec = Codec::with_passphrase(DEFAULT_KEY.as_bytes(), [9; 16]);
        let before = f.plain();
        f.exec("UPDATE djmdContent SET Title = 'One More Time (WAL)' WHERE ID = '84935471'");
        f.track("5", "/Users/dj/Music/New.mp3", "Newcomer", "Added Later");
        let after = f.plain();
        assert!(after.len() >= before.len());
        let pages = (after.len() / PAGE_SIZE) as u32;
        let changed: Vec<u32> = (1..=pages)
            .filter(|&p| {
                let at = (p as usize - 1) * PAGE_SIZE;
                before.get(at..at + PAGE_SIZE) != Some(&after[at..at + PAGE_SIZE])
            })
            .collect();
        let frames: Vec<(u32, u32, Vec<u8>)> = changed
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                let at = (p as usize - 1) * PAGE_SIZE;
                let commit = if i + 1 == changed.len() { pages } else { 0 };
                (p, commit, codec.encrypt_page(p, &after[at..at + PAGE_SIZE]).unwrap())
            })
            .collect();
        let path = write(dir.path(), &codec.encrypt(&before).unwrap());
        let wal = crate::onelibrary::wal::tests::build(PAGE_SIZE, (1, 2), &frames);
        std::fs::write(dir.path().join("master.db-wal"), &wal).unwrap();
        let original = std::fs::read(&path).unwrap();

        let c = open(&path, DEFAULT_KEY).unwrap();
        assert_eq!(c.library.track(84935471).unwrap().title, "One More Time (WAL)");
        assert_eq!(c.library.track(5).unwrap().title, "Added Later");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read(dir.path().join("master.db-wal")).unwrap(), wal);
        assert!(!dir.path().join("master.db-shm").exists(), "never opened with SQLite");
        assert_eq!(c.info().wal_bytes, Some(wal.len() as u64));
    }

    #[test]
    fn master_db_is_copied_again_when_its_wal_restarts_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let (path, to) = (dir.path().join("master.db"), dir.path().join("copy"));
        let wal_path = dir.path().join("master.db-wal");
        let generation = |salts| crate::onelibrary::wal::tests::build(PAGE_SIZE, salts, &[]);
        std::fs::write(&path, "before the checkpoint").unwrap();
        std::fs::write(&wal_path, generation((1, 2))).unwrap();
        // rekordbox checkpoints into master.db and restarts its WAL just after the first copy.
        let copies = std::cell::Cell::new(0);
        let wal = snapshot(&path, &to, |from: &Path, to: &Path| {
            let copied = std::fs::copy(from, to);
            if copies.replace(copies.get() + 1) == 0 {
                std::fs::write(from, "after the checkpoint").unwrap();
                std::fs::write(&wal_path, generation((3, 4))).unwrap();
            }
            copied
        })
        .unwrap();
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "after the checkpoint");
        assert_eq!(wal, Some(generation((3, 4))));
    }

    #[test]
    fn a_drive_is_compared_with_the_collection() {
        use crate::model::tests::track;
        let dir = tempfile::tempdir().unwrap();
        let c = open(&write(dir.path(), &sample().encrypted(DEFAULT_KEY)), DEFAULT_KEY).unwrap();
        // Exported earlier: the same file under /Contents, Floorplan's track under another file
        // name (found by artist, title and length), plus a track the collection no longer has.
        let mut one_more_time =
            track(1, "Daft Punk", "One More Time", "/Contents/Daft Punk/Discovery/01 One More Time.mp3");
        one_more_time.duration_secs = 320;
        let mut never = track(2, "Floorplan", "Never Grow Old", "/Contents/Floorplan/UnknownAlbum/Never Grow Old.mp3");
        never.duration_secs = 412;
        let drive = Library {
            tracks: vec![one_more_time, never, track(3, "Old", "Gone", "/Contents/Old/Gone.mp3")],
            playlists: vec![Playlist {
                id: 1,
                parent_id: None,
                name: "Friday".into(),
                is_folder: false,
                sort_order: 0,
                track_ids: vec![1, 2],
            }],
            ..Default::default()
        };
        let d = c.diff_drive(&drive, "Main", None).unwrap();
        assert_eq!((d.matched_by_file, d.matched_by_details), (1, 1));
        assert!(d.uncertain.is_empty());
        let names = |v: &[TrackRef]| v.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&d.diff.added_tracks), ["Somebody - Only Here"]);
        assert_eq!(names(&d.diff.removed_tracks), ["Old - Gone"]);
        let changed: Vec<(&str, &str)> = d
            .diff
            .changed_tracks
            .iter()
            .flat_map(|c| c.fields.iter().map(move |f| (c.track.name.as_str(), f.field.as_str())))
            .filter(|(_, f)| *f == "rating")
            .collect();
        assert_eq!(changed, [("Daft Punk - One More Time", "rating")]);
        assert_eq!(d.diff.added_playlists, ["Recent", "Shows/", "Shows/Friday"]);
        assert_eq!(d.diff.removed_playlists, ["Friday"]);
        // One playlist only.
        let only = c.diff_drive(&drive, "Main", Some("Shows")).unwrap();
        assert!(only.diff.added_tracks.is_empty());
        assert_eq!(only.diff.added_playlists, ["Shows/", "Shows/Friday"]);
        // One neither has would show as no differences.
        let err = c.diff_drive(&drive, "Main", Some("nope/none")).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "{err}");
    }

    /// SQLCipher itself, given rekordbox's key as `PRAGMA key` (as pyrekordbox opens master.db):
    /// a database it writes reads here, and it reads the fixtures. Skipped without the sqlcipher
    /// CLI (see tests/support/sqlcipher.rs).
    #[test]
    fn sqlcipher_and_this_reader_agree_on_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let theirs = dir.path().join("theirs.db");
        let create = "CREATE TABLE djmdContent (ID VARCHAR(255) PRIMARY KEY, FolderPath VARCHAR(255), \
                      Title VARCHAR(255), BPM INTEGER, rb_local_deleted INTEGER DEFAULT 0); \
                      INSERT INTO djmdContent (ID, FolderPath, Title, BPM) VALUES ('77', '/Users/dj/x.mp3', 'Theirs', 12450);";
        if super::sqlcipher::run(&theirs, DEFAULT_KEY, &[create]).is_none() {
            return;
        }
        let c = open(&theirs, DEFAULT_KEY).unwrap();
        assert_eq!((c.library.tracks[0].id, c.library.tracks[0].title.as_str()), (77, "Theirs"));
        assert_eq!(c.library.tracks[0].bpm, 124.5);

        let ours = write(dir.path(), &sample().encrypted(DEFAULT_KEY));
        let checks = ["PRAGMA cipher_integrity_check;", "SELECT count(*) FROM djmdContent WHERE rb_local_deleted = 0;"];
        assert_eq!(super::sqlcipher::run(&ours, DEFAULT_KEY, &checks).unwrap(), ["ok", "3"]);
    }

    #[test]
    fn the_api_reads_the_collection_it_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &sample().encrypted(DEFAULT_KEY));
        let req = crate::api::CollectionRef { db: Some(path), key: None };
        if std::env::var_os(KEY_ENV).is_none() {
            assert_eq!(crate::api::collection_info(&req).unwrap().summary.tracks, 3);
        }
        let missing = crate::api::CollectionRef { db: Some(dir.path().join("nope.db")), key: None };
        assert_eq!(crate::api::collection_info(&missing).unwrap_err().code(), "not_found");
    }

    #[test]
    fn numeric_ids_are_kept_others_renumbered() {
        let kept = id_map(["30", "7", "123456789"].into_iter().map(String::from));
        assert_eq!((kept["30"], kept["7"], kept["123456789"]), (30, 7, 123456789));
        let renumbered = id_map(["30", "x", "99999999999"].into_iter().map(String::from));
        assert_eq!((renumbered["30"], renumbered["x"], renumbered["99999999999"]), (1, 2, 3));
        // A zero, or two spellings of one number, renumbers too.
        assert_eq!(id_map(["0", "5"].into_iter().map(String::from))["5"], 2);
        assert_eq!(id_map(["05", "5"].into_iter().map(String::from))["5"], 2);
    }

    #[test]
    fn values_read_leniently() {
        assert_eq!(int(&Value::Text(" 12800 ".into())), 12800);
        assert_eq!(int(&Value::Text("3.0".into())), 3);
        assert_eq!(int(&Value::Null), 0);
        assert_eq!(text(&Value::Integer(42)), "42");
        assert_eq!(clamp::<u16>(70_000), u16::MAX);
        assert_eq!(clamp::<u32>(-5), 0);
    }

    #[test]
    fn keys_resolve_in_order() {
        assert_eq!(resolve_key(Some("abc")), "abc");
        // The environment is shared between tests; only check the fallback when it's unset.
        if std::env::var_os(KEY_ENV).is_none() {
            assert_eq!(resolve_key(None), DEFAULT_KEY);
            assert_eq!(resolve_key(Some("")), DEFAULT_KEY);
        }
    }
}
