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
fn kind_4_is_a_memory_loop_and_hot_cue_d_is_5() {
    let f = sample();
    f.exec(
        "INSERT INTO djmdCue (ID, ContentID, InMsec, OutMsec, Kind, ColorTableIndex, Comment) VALUES
                 ('c5', '205356547', 1000, 9000, 4, 0, ''), ('c6', '205356547', 3000, -1, 3, 0, 'C'),
                 ('c7', '205356547', 5000, -1, 5, 0, 'D'), ('c8', '205356547', 7000, -1, 9, 0, 'H');",
    );
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write(dir.path(), &f.encrypted(DEFAULT_KEY)), DEFAULT_KEY).unwrap();
    let cues: Vec<_> = c.cues[&205356547].iter().map(|c| (c.hot_cue, c.start_ms, c.end_ms)).collect();
    assert_eq!(cues, [(None, 1000, Some(9000)), (Some(2), 3000, None), (Some(3), 5000, None), (Some(7), 7000, None)]);
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
fn a_damaged_b_tree_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let f = sample();
    let codec = Codec::with_passphrase(DEFAULT_KEY.as_bytes(), [9; 16]);
    let bytes = crate::onelibrary::tests::with_broken_table(&f.conn, &codec, f.plain(), "djmdCue");
    let path = write(dir.path(), &bytes);
    let err = open(&path, DEFAULT_KEY).unwrap_err().to_string();
    assert!(err.contains("SQLite finds it damaged"), "{err}");
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
    use loclib_model::test_util::track;
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
fn blobs_reals_and_nulls_read_too() {
    assert_eq!(text(&Value::Blob(b"Am".to_vec())), "Am");
    assert_eq!(text(&Value::Real(128.5)), "128.5");
    assert_eq!(text(&Value::Null), "");
    assert_eq!(int(&Value::Real(320.9)), 320);
}

#[test]
fn files_that_arent_a_collection_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let err = |bytes: &[u8]| {
        let path = write(dir.path(), bytes);
        open(&path, DEFAULT_KEY).unwrap_err().to_string()
    };
    assert!(err(&[0; 100]).ends_with("too short for a SQLite database"));
    assert!(err(&[0; PAGE_SIZE + 10]).ends_with("4106 bytes is not a whole number of 4096-byte pages"));

    let other = dir.path().join("other.db");
    Connection::open(&other).unwrap().execute_batch("CREATE TABLE notes (body TEXT)").unwrap();
    let message = err(&std::fs::read(&other).unwrap());
    assert!(message.ends_with("no djmdContent table; not a rekordbox collection database"), "{message}");
}

/// A plain (already decrypted) master.db is read with its WAL, as SQLite would read it.
#[test]
fn a_plain_database_is_read_with_its_wal() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live");
    std::fs::create_dir(&live).unwrap();
    let path = write(&live, &sample().plain());
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;
             UPDATE djmdContent SET Title = 'Never Grow Old (WAL)' WHERE ID = '205356547';",
    )
    .unwrap();
    // Copied while the WAL holds the change, as a backup of rekordbox's folder would be.
    let copy = dir.path().join("copy");
    std::fs::create_dir(&copy).unwrap();
    for name in ["master.db", "master.db-wal"] {
        std::fs::copy(live.join(name), copy.join(name)).unwrap();
    }
    drop(conn);
    let titles: Vec<String> =
        open(&copy.join("master.db"), "unused").unwrap().library.tracks.into_iter().map(|t| t.title).collect();
    assert!(titles.contains(&"Never Grow Old (WAL)".to_string()), "{titles:?}");
}

#[test]
fn cloud_folders_come_from_rekordbox_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rekordbox3.settings");
    std::fs::write(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<PROPERTIES>
  <VALUE name="DropboxSharingPath" val="/Users/dj/Library/CloudStorage/Dropbox &amp; Co"/>
  <VALUE name="GoogleDriveSharingPath" val=""/>
  <VALUE name="DropboxToken" val="not-for-us"/>
</PROPERTIES>"#,
    )
    .unwrap();
    let folders = CloudFolders::from_settings(&path).unwrap();
    assert_eq!(folders.dropbox, Some(PathBuf::from("/Users/dj/Library/CloudStorage/Dropbox & Co")));
    assert_eq!(folders.google_drive, None, "an empty value is no folder");
    assert!(CloudFolders::from_settings(&dir.path().join("missing")).is_err());
}

/// A collection whose second track Cloud Library Sync keeps in Dropbox, with the analysis
/// file it lists for the first.
fn cloud_sample() -> Fixture {
    let f = sample();
    f.exec(&format!(
        "CREATE TABLE djmdCloudProperty (ID VARCHAR(255) PRIMARY KEY, Reserved1 TEXT, Reserved2 TEXT, {STATS});
             INSERT INTO djmdCloudProperty (ID, Reserved1, Reserved2) VALUES ('1', 'SyncService', 'Dropbox');
             CREATE TABLE contentFile (ID VARCHAR(255) PRIMARY KEY, ContentID VARCHAR(255), Path VARCHAR(255),
                 Hash VARCHAR(255), Size INTEGER, {STATS});
             INSERT INTO contentFile (ID, ContentID, Path, Size) VALUES
                 ('f1', '84935471', '/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT', 4);
             UPDATE djmdContent SET FolderPath = '/contents_2354715273/Floorplan/ngo.mp3' WHERE ID = '205356547';"
    ));
    f
}

#[test]
fn finds_audio_cloud_library_sync_keeps_in_a_storage_service() {
    let dir = tempfile::tempdir().unwrap();
    let rb = dir.path().join("rekordbox");
    std::fs::create_dir(&rb).unwrap();
    let c = open(&write(&rb, &cloud_sample().encrypted(DEFAULT_KEY)), DEFAULT_KEY).unwrap();
    assert_eq!(c.sync_service.as_deref(), Some("Dropbox"));
    let local = c.library.track(84935471).unwrap();
    let synced = c.library.track(205356547).unwrap();
    let (dropbox, drive) = (dir.path().join("Dropbox"), dir.path().join("Google Drive"));
    let audio = dropbox.join("rekordbox/contents_2354715273/Floorplan/ngo.mp3");
    std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
    std::fs::write(&audio, b"mp3").unwrap();

    let both = CloudFolders { dropbox: Some(dropbox.clone()), google_drive: Some(drive.clone()) };
    assert_eq!(c.audio_file(local, &both), Some(PathBuf::from(&local.file_path)), "a full path stays as it is");
    assert_eq!(c.audio_file(synced, &both), Some(audio.clone()));
    let only_drive = CloudFolders { dropbox: None, google_drive: Some(drive) };
    assert_eq!(c.audio_file(synced, &only_drive), None, "not in any folder rekordbox syncs to");
    assert_eq!(c.audio_file(synced, &CloudFolders::default()), None);

    // The analysis file contentFile lists, missing, then a partial copy, then all of it.
    let dat = "/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT";
    assert_eq!(c.share_file(dat), rb.join("share/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT"));
    assert!(c.still_syncing(dat));
    std::fs::create_dir_all(c.share_file(dat).parent().unwrap()).unwrap();
    std::fs::write(c.share_file(dat), b"PM").unwrap();
    assert!(c.still_syncing(dat));
    std::fs::write(c.share_file(dat), b"PMAI").unwrap();
    assert!(!c.still_syncing(dat));
    assert!(!c.still_syncing("/PIONEER/Artwork/00001/a1.jpg"), "a file Cloud Library Sync doesn't list");
}

#[test]
fn entries_rekordbox_left_on_a_folder_are_ignored() {
    let f = sample();
    f.exec("INSERT INTO djmdSongPlaylist (ID, PlaylistID, ContentID, TrackNo) VALUES ('z', '10', '4', 1);");
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write(dir.path(), &f.encrypted(DEFAULT_KEY)), DEFAULT_KEY).unwrap();
    let shows = c.library.find_playlist("Shows").unwrap();
    assert!(shows.is_folder && shows.track_ids.is_empty(), "{:?}", shows.track_ids);
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

/// One drawn row of `djmdContent`, and what the collection should make of it.
#[derive(Debug, Clone)]
struct Row {
    id: String,
    path: String,
    artist: &'static str,
    title: String,
    bpm: i64,
    rating: i64,
    color: i64,
    deleted: bool,
}

/// A drawn `master.db` (tracks, some deleted; folders, playlists and an intelligent one with
/// entries in any order; a history list; cues of every kind; My Tags) reads back as rekordbox
/// holds it: deleted rows left out, ids kept only when they're all distinct numbers, entries in
/// `TrackNo` order, and cues and tags with the tracks they belong to. A drive holding exactly
/// the collection then differs from it in nothing.
#[hegel::test(test_cases = 25, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_drawn_collection_reads_back_as_written(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new();
    let numeric = tc.draw(gs::booleans());
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(6));
    let mut rows = Vec::new();
    for i in 0..n {
        let id = match numeric {
            true => (tc.draw(gs::integers::<u32>().min_value(1).max_value(40)) * 100 + i as u32).to_string(),
            false => format!("t{i}"),
        };
        let artist = tc.draw(gs::sampled_from(vec!["Daft Punk", "Ünï", "Floorplan"]));
        let row = Row {
            id,
            path: format!("/Users/dj/Music/{artist}/Track {i}.mp3"),
            artist,
            title: format!("Track {i}"),
            bpm: tc.draw(gs::integers::<i64>().min_value(0).max_value(20_000)),
            rating: tc.draw(gs::integers::<i64>().min_value(-1).max_value(7)),
            color: tc.draw(gs::integers::<i64>().min_value(0).max_value(9)),
            deleted: tc.draw(gs::weighted_booleans(0.2)),
        };
        f.track(&row.id, &row.path, row.artist, &row.title);
        f.conn
            .execute(
                "UPDATE djmdContent SET BPM = ?2, Rating = ?3, ColorID = ?4, rb_local_deleted = ?5 WHERE ID = ?1",
                params![row.id, row.bpm, row.rating, row.color.to_string(), row.deleted as i64],
            )
            .unwrap();
        rows.push(row);
    }
    let live: Vec<&Row> = rows.iter().filter(|r| !r.deleted).collect();
    let ids_kept = numeric;
    let id_of = |row: &Row| -> u32 {
        match ids_kept {
            true => row.id.parse().unwrap(),
            false => live.iter().position(|r| r.id == row.id).unwrap() as u32 + 1,
        }
    };

    // Playlists: a folder, playlists in it or at the top, one intelligent.
    f.exec("INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID) VALUES ('p0', 0, 'Shows', 1, 'root');");
    // (path, id, entries as TrackNo and row)
    type Drawn<'r> = (String, String, Vec<(i64, &'r Row)>);
    let mut playlists: Vec<Drawn> = Vec::new();
    for p in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let (parent, folder_name) = if tc.draw(gs::booleans()) { ("p0", "Shows/") } else { ("root", "") };
        let id = format!("p{}", p + 1);
        let name = format!("List {p}");
        let smart = tc.draw(gs::weighted_booleans(0.2));
        let deleted = tc.draw(gs::weighted_booleans(0.15));
        f.conn
            .execute(
                "INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID, rb_local_deleted) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, p as i64 + 1, name, if smart { 4 } else { 0 }, parent, deleted as i64],
            )
            .unwrap();
        let mut entries = Vec::new();
        if !smart {
            let picks: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().max_value(n - 1)).max_size(4));
            for (e, k) in picks.into_iter().enumerate() {
                let track_no = tc.draw(gs::integers::<i64>().min_value(1).max_value(5));
                f.conn
                    .execute(
                        "INSERT INTO djmdSongPlaylist (ID, PlaylistID, ContentID, TrackNo) VALUES (?1, ?2, ?3, ?4)",
                        params![format!("{id}e{e}"), id, rows[k].id, track_no],
                    )
                    .unwrap();
                entries.push((track_no, &rows[k]));
            }
        }
        if !deleted {
            playlists.push((format!("{folder_name}{name}"), id, entries));
        }
    }
    // Entries go by TrackNo, then by the order they were stored in; deleted tracks go.
    let want_entries = |entries: &[(i64, &Row)]| -> Vec<u32> {
        let mut e: Vec<(usize, &(i64, &Row))> = entries.iter().enumerate().collect();
        e.sort_by_key(|(i, (no, _))| (*no, *i));
        e.into_iter().filter(|(_, (_, r))| !r.deleted).map(|(_, (_, r))| id_of(r)).collect()
    };

    // Cues on any row, of any kind.
    let mut cues: Vec<(&Row, i64, i64, i64)> = Vec::new();
    for c in 0..tc.draw(gs::integers::<usize>().max_value(5)) {
        let row = &rows[tc.draw(gs::integers::<usize>().max_value(n - 1))];
        let start = tc.draw(gs::integers::<i64>().min_value(0).max_value(9_000));
        let end = if tc.draw(gs::booleans()) { start + tc.draw(gs::integers::<i64>().max_value(4_000)) } else { -1 };
        let kind = tc.draw(gs::integers::<i64>().min_value(0).max_value(10));
        f.conn
            .execute(
                "INSERT INTO djmdCue (ID, ContentID, InMsec, OutMsec, Kind, ColorTableIndex, Comment) \
                     VALUES (?1, ?2, ?3, ?4, ?5, 1, 'cue')",
                params![format!("c{c}"), row.id, start, end, kind],
            )
            .unwrap();
        cues.push((row, start, end, kind));
    }

    // My Tags: one category, a tag on drawn tracks (repeats too).
    f.exec(
        "INSERT INTO djmdMyTag (ID, Seq, Name, Attribute, ParentID) VALUES ('100', 1, 'Mood', 1, 'root'), \
             ('101', 0, 'Deep', 0, '100');",
    );
    let tagged: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().max_value(n - 1)).max_size(4));
    for (i, k) in tagged.iter().enumerate() {
        f.conn
            .execute(
                "INSERT INTO djmdSongMyTag (ID, MyTagID, ContentID, TrackNo) VALUES (?1, '101', ?2, ?3)",
                params![format!("s{i}"), rows[*k].id, i as i64],
            )
            .unwrap();
    }

    let dir = tempfile::tempdir().unwrap();
    let encrypted = tc.draw(gs::booleans());
    let path = write(dir.path(), &if encrypted { f.encrypted(DEFAULT_KEY) } else { f.plain() });
    let c = open(&path, DEFAULT_KEY).unwrap();

    // Tracks.
    let mut want: Vec<(u32, String, String, f64, u8, Option<TrackColor>)> = live
        .iter()
        .map(|r| {
            let color = TrackColor::from_id(r.color as u8);
            (id_of(r), r.path.clone(), r.artist.to_string(), r.bpm as f64 / 100.0, r.rating.clamp(0, 5) as u8, color)
        })
        .collect();
    want.sort_by_key(|w| w.0);
    let got: Vec<_> = c
        .library
        .tracks
        .iter()
        .map(|t| (t.id, t.file_path.clone(), t.artist.clone(), t.bpm, t.rating, t.color))
        .collect();
    assert_eq!(got, want);

    // Playlists.
    let mut got: Vec<(String, Vec<u32>)> = c
        .library
        .playlists
        .iter()
        .filter(|p| !p.is_folder)
        .map(|p| (c.library.playlist_path(p.id), p.track_ids.clone()))
        .collect();
    let mut want: Vec<(String, Vec<u32>)> =
        playlists.iter().map(|(path, _, e)| (path.clone(), want_entries(e))).collect();
    got.sort();
    want.sort();
    assert_eq!(got, want);
    assert_eq!(c.smart_playlists.len(), c.info().smart_playlists.len());

    // Cues: those of live tracks, as hot cues, memory cues and loops.
    let live_cues: Vec<_> = cues.iter().filter(|(r, ..)| !r.deleted).collect();
    assert_eq!(c.cues.values().map(Vec::len).sum::<usize>(), live_cues.len());
    for (r, start, end, kind) in &live_cues {
        let list = &c.cues[&id_of(r)];
        let hot = hot_cue_from_kind(*kind);
        let end = (*end > *start).then_some(*end as u32);
        assert!(list.iter().any(|q| q.hot_cue == hot && q.start_ms == *start as u32 && q.end_ms == end), "{list:?}");
        assert!(hot.is_none_or(|h| hot_cue_from_kind(kind_for_hot_cue(h)) == Some(h)));
    }
    let info = c.info();
    assert_eq!(
        info.hot_cues + info.memory_cues + info.loops,
        live_cues.len() + c.cues.values().flatten().filter(|q| q.hot_cue.is_some() && q.end_ms.is_some()).count()
    );

    // My Tags: each live track once, in the order tagged.
    let mut seen = FxHashSet::default();
    let tag_want: Vec<u32> =
        tagged.iter().map(|k| &rows[*k]).filter(|r| !r.deleted).map(id_of).filter(|id| seen.insert(*id)).collect();
    assert_eq!(c.tags.len(), 1);
    assert_eq!(c.tags[0].tags[0].track_ids, tag_want);
    assert_eq!(info.tagged_tracks, tag_want.len());
    assert_eq!(c.tags_by_file().len(), tag_want.len());

    // A drive with exactly the collection's tracks and playlists differs in nothing.
    let d = c.diff_drive(&c.library, "Main", None).unwrap();
    assert_eq!(d.matched_by_file, c.library.tracks.len());
    let diff = &d.diff;
    assert!(diff.added_tracks.is_empty() && diff.removed_tracks.is_empty() && diff.changed_tracks.is_empty());
    assert!(diff.added_playlists.is_empty() && diff.removed_playlists.is_empty(), "{diff:?}");
    // ...and a drive without one of them lacks exactly that one.
    let mut fewer = c.library.clone();
    if fewer.tracks.is_empty() {
        return;
    }
    let gone = fewer.tracks.remove(0);
    for p in &mut fewer.playlists {
        p.track_ids.retain(|t| *t != gone.id);
    }
    let d = c.diff_drive(&fewer, "Main", None).unwrap();
    assert_eq!(d.diff.added_tracks.iter().map(|t| t.id).collect::<Vec<_>>(), [gone.id]);
}
