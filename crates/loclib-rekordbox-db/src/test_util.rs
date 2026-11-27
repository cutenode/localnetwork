//! Each section's items are for tests of that part: OneLibrary's, master.db's (a collection built in
//! plain SQLite, and what a backup of it reads) and intelligent playlists' (drawn tracks and
//! conditions).

use std::path::{Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use loclib_model::model::{Track, TrackColor};
use loclib_sqlcipher::{Codec, PAGE_SIZE, RESERVE, SALT_LEN, SQLITE_MAGIC};
use rusqlite::{Connection, params};

use crate::masterdb::DEFAULT_KEY;
use crate::masterdb::smartlist::Condition;

// -- OneLibrary -----------------------------------------------------------------------------

/// A database whose b-tree for `table` claims more cells than a page holds, re-encrypted.
pub fn with_broken_table(conn: &Connection, codec: &Codec, mut plain: Vec<u8>, table: &str) -> Vec<u8> {
    let root: u32 =
        conn.query_row("SELECT rootpage FROM sqlite_master WHERE name = ?1", [table], |r| r.get(0)).unwrap();
    let at = (root as usize - 1) * PAGE_SIZE;
    plain[at + 3..at + 5].copy_from_slice(&u16::MAX.to_be_bytes());
    codec.encrypt(&plain).unwrap()
}

// -- master.db ------------------------------------------------------------------------------

/// Columns every table but `djmdProperty` ends with.
pub const STATS: &str = "UUID VARCHAR(255), rb_data_status INTEGER DEFAULT 0, rb_local_data_status INTEGER DEFAULT 0, \
        rb_local_deleted TINYINT(1) DEFAULT 0, rb_local_synced TINYINT(1) DEFAULT 0, usn BIGINT, rb_local_usn BIGINT, \
        created_at TEXT NOT NULL DEFAULT '2024-01-01 10:00:00.000 +00:00', \
        updated_at TEXT NOT NULL DEFAULT '2024-01-01 10:00:00.000 +00:00'";

/// The tables read here, with the columns pyrekordbox lists for rekordbox 6/7 (less the ones
/// nothing reads at the end of `djmdContent`).
pub const TABLES: &[(&str, &str)] = &[
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
    ("djmdSongHistory", "ID VARCHAR(255) PRIMARY KEY, HistoryID VARCHAR(255), ContentID VARCHAR(255), TrackNo INTEGER"),
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

/// A connection for building a test's database. Each statement commits on its own, and on Windows
/// a commit's journal file and flushes take milliseconds, which hundreds of property test cases turn
/// into minutes. A temporary file doesn't need either.
pub fn scratch_db(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA journal_mode = MEMORY; PRAGMA synchronous = OFF;").unwrap();
    conn
}

/// A small rekordbox-shaped `master.db`, built in plain SQLite and encrypted on request.
pub struct Fixture {
    _dir: tempfile::TempDir,
    plain: PathBuf,
    /// The plain database, open.
    pub conn: Connection,
}

#[allow(clippy::new_without_default, reason = "each one makes its own temporary database")]
impl Fixture {
    /// An empty collection: rekordbox's tables, and a `djmdProperty` row.
    pub fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain.db");
        std::fs::write(&plain, empty()).unwrap();
        let conn = scratch_db(&plain);
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

    /// Run `sql` on the plain database.
    pub fn exec(&self, sql: &str) {
        self.conn.execute_batch(sql).unwrap();
    }

    /// A track row: `ID`, `FolderPath`, `Title` and the artist's name (an artist row is made).
    pub fn track(&self, id: &str, path: &str, artist: &str, title: &str) {
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
    pub fn plain(&self) -> Vec<u8> {
        std::fs::read(&self.plain).unwrap()
    }

    /// The database encrypted as rekordbox does, under `key`.
    pub fn encrypted(&self, key: &str) -> Vec<u8> {
        codec(key).encrypt(&self.plain()).unwrap()
    }
}

/// A codec under `key` with a fresh salt, as a new file gets one: a file's salt is read back from
/// it, so no test depends on which.
pub fn codec(key: &str) -> Codec {
    let mut salt = [0; SALT_LEN];
    getrandom::fill(&mut salt).unwrap();
    Codec::with_key(key.as_bytes(), salt)
}

/// Three tracks (and a deleted one), a folder with a playlist, an intelligent playlist, a
/// history list, cues and My Tags.
pub fn sample() -> Fixture {
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

/// The cloud agent's secret in [`library`], which a copy without credentials has nowhere.
pub const SECRET: &str = "SECRET-TOKEN-0123456789";

/// The sample collection with what a backup reads besides: `UUID`s, `OrgFolderPath` (for a track
/// Cloud Library Sync keeps, and one it doesn't), and `agentRegistry` with the cloud agent's
/// sign-in among its rows.
pub fn library() -> Fixture {
    let f = sample();
    f.exec(&format!(
        "ALTER TABLE djmdContent ADD COLUMN OrgFolderPath VARCHAR(255);
         UPDATE djmdContent SET UUID = 'uuid-' || ID;
         UPDATE djmdContent SET FolderPath = '/contents_1234567890/floorplan/ngo.mp3',
             OrgFolderPath = '/Users/dj/Music/Downloads/ngo.mp3' WHERE ID = '205356547';
         UPDATE djmdContent SET OrgFolderPath = '/Users/dj/Music/Elsewhere.mp3' WHERE ID = '4';
         CREATE TABLE agentRegistry (registry_id VARCHAR(255) PRIMARY KEY, id_1 VARCHAR(255), int_1 BIGINT,
             str_1 VARCHAR(255), text_1 TEXT, created_at TEXT, updated_at TEXT);
         INSERT INTO agentRegistry (registry_id, id_1, int_1, str_1, text_1) VALUES
             ('agentCredentials', 'abcdefabcdefabcdefabcdefabcdef12', NULL, '{SECRET}', 'another secret'),
             ('localUpdateCount', NULL, 74383, NULL, NULL),
             ('SyncAnalysisDataRootPath', NULL, NULL, '/Users/dj/Library/Pioneer/rekordbox/share', NULL);"
    ));
    f
}

/// The update count [`writable`]'s `agentRegistry` starts at.
pub const UPDATE_COUNT: i64 = 74383;

/// [`library`] with what a write of cues reads and writes besides, as rekordbox 7 has it:
/// `contentCue` (empty: no track's list is written yet), `cloudAgentRegistry` (empty: Cloud
/// Library Sync is off), `djmdContent.CueUpdated`, the cues' `ContentUUID`s, and a few rows
/// stamped below [`UPDATE_COUNT`].
pub fn writable() -> Fixture {
    let f = library();
    f.exec(&format!(
        "ALTER TABLE djmdContent ADD COLUMN CueUpdated VARCHAR(255);
         CREATE TABLE contentCue (ID VARCHAR(255) PRIMARY KEY, ContentID VARCHAR(255), Cues TEXT,
             rb_cue_count INTEGER, {STATS});
         CREATE TABLE cloudAgentRegistry (ID VARCHAR(255) PRIMARY KEY, int_1 BIGINT, {STATS});
         UPDATE djmdCue SET ContentUUID = 'uuid-' || ContentID;
         UPDATE djmdContent SET rb_local_usn = 74000 + rowid, CueUpdated = '1';"
    ));
    f
}

/// The database at `path`, decrypted (a plain one as it is): every byte of every page.
pub fn plain(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    if bytes.starts_with(SQLITE_MAGIC) {
        return bytes;
    }
    let salt: [u8; SALT_LEN] = bytes[..SALT_LEN].try_into().unwrap();
    Codec::with_key(DEFAULT_KEY.as_bytes(), salt).decrypt(&bytes, |_| true).unwrap()
}

/// The `registry_id`s of the database at `path`'s `agentRegistry`, in order.
pub fn registry(path: &Path) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = crate::masterdb::open_copy(path, DEFAULT_KEY, dir.path(), false).unwrap();
    let mut stmt = db.conn.prepare("SELECT registry_id FROM agentRegistry ORDER BY rowid").unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

/// Whether `needle` is anywhere in `haystack`.
pub fn has(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle.as_bytes())
}

// -- intelligent playlists ------------------------------------------------------------------

/// A track drawn from small pools, so conditions often hit, at `/Music/{i}.mp3`.
#[hegel::test_helper]
pub fn draw_track(tc: &TestCase, i: u32) -> Track {
    let text = || gs::sampled_from(vec!["", "Deep House", "deep  house", "Techno", "Ünï", "dub techno", "8A", "Am"]);
    let date = || gs::sampled_from(vec!["", "2024-03-01", "2023-12-24", "2024-05-31", "2024-13-01", "2024-03"]);
    Track {
        id: i,
        file_path: format!("/Music/{}{i}.mp3", tc.draw(text())),
        title: tc.draw(text()).into(),
        artist: tc.draw(text()).into(),
        album: tc.draw(text()).into(),
        album_artist: tc.draw(text()).into(),
        genre: tc.draw(text()).into(),
        label: tc.draw(text()).into(),
        key: tc.draw(text()).into(),
        comment: tc.draw(text()).into(),
        remixer: tc.draw(text()).into(),
        mix_name: tc.draw(text()).into(),
        composer: tc.draw(text()).into(),
        original_artist: tc.draw(text()).into(),
        bpm: tc.draw(gs::sampled_from(vec![0.0, 122.8, 128.0, 128.5, 130.0])),
        rating: tc.draw(gs::integers::<u8>().max_value(5)),
        play_count: tc.draw(gs::integers::<u16>().max_value(3)),
        year: tc.draw(gs::sampled_from(vec![0u16, 1999, 2000, 2024])),
        duration_secs: tc.draw(gs::sampled_from(vec![0u32, 300, 320])),
        date_added: tc.draw(date()).into(),
        release_date: tc.draw(date()).into(),
        color: TrackColor::from_id(tc.draw(gs::integers::<u8>().max_value(8))),
        ..Default::default()
    }
}

/// The properties of each kind of condition, its operators and values to draw from (some that
/// can't be read or said among them), and last any property with any operator and value.
#[allow(clippy::type_complexity)]
pub const KINDS: &[(&[&str], &[u8], &[&str])] = &[
    (
        &[
            "artist",
            "album",
            "albumArtist",
            "originalArtist",
            "producer",
            "genre",
            "key",
            "label",
            "mixName",
            "remixedBy",
            "name",
            "comments",
            "fileName",
        ],
        &[1, 2, 8, 9, 10, 11],
        &["", "deep", "Deep House", "techno", "ü", "8A", "a|b", "\"q\"", "x"],
    ),
    (
        &["bpm", "rating", "counter", "year", "duration"],
        &[1, 2, 3, 4, 5],
        &["128", "12850", "122.8", "3", "-1", "0", "2000", "300", "x"],
    ),
    (
        &["stockDate", "dateReleased", "dateCreated"],
        &[1, 2, 3, 4, 5, 6, 7],
        &["2024-03-01", "2024-05-31", "2024", "30", "x"],
    ),
    (&["grouping"], &[1, 2, 3], &["0", "2", "9", "x"]),
    (&["myTag"], &[8, 9, 1], &["Genre/Deep", "Situation/Peak", "genre/deep", "Deep", ""]),
    (
        &["artist", "genre", "bpm", "stockDate", "grouping", "myTag", "dateCreated", "unknown"],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        &["", "deep", "128", "2", "2024-03-01", "Genre/Deep", "a|b", "x"],
    ),
];

/// A condition of the `kind`th of [`KINDS`], on `property` when it's given.
#[hegel::test_helper]
pub fn draw_condition(tc: &TestCase, kind: usize, property: Option<&str>) -> Condition {
    let (properties, operators, values) = KINDS[kind];
    let property = property.unwrap_or_else(|| tc.draw(gs::sampled_from(properties.to_vec())));
    let operator = tc.draw(gs::sampled_from(operators.to_vec()));
    let unit = tc.draw(gs::sampled_from(vec!["day", "week", "month", "year", ""]));
    Condition::new(
        property,
        operator,
        unit,
        tc.draw(gs::sampled_from(values.to_vec())),
        tc.draw(gs::sampled_from(values.to_vec())),
    )
}
