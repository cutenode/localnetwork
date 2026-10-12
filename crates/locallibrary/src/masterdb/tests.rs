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

/// A file that isn't a rekordbox collection is refused saying why: too short, not whole pages,
/// SQLite without rekordbox's tables, or encrypted with another key (then with the way on).
#[hegel::test(test_cases = 15)]
fn files_that_arent_a_collection_say_why(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let kind = tc.draw(gs::integers::<u8>().max_value(3));
    let (bytes, why): (Vec<u8>, &str) = match kind {
        0 => (vec![0; tc.draw(gs::integers::<usize>().max_value(PAGE_SIZE - 1))], "too short for a SQLite database"),
        1 => {
            let pages = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
            (
                vec![0; pages * PAGE_SIZE + tc.draw(gs::integers::<usize>().min_value(1).max_value(PAGE_SIZE - 1))],
                "whole number of 4096-byte pages",
            )
        }
        2 => {
            let other = dir.path().join("other.db");
            Connection::open(&other).unwrap().execute_batch("CREATE TABLE notes (body TEXT)").unwrap();
            (std::fs::read(&other).unwrap(), "no djmdContent table")
        }
        // Few keys: deriving one takes a while, and each is derived once.
        _ => (sample().encrypted(tc.draw(gs::sampled_from(vec!["another key", "rekordbox 8"]))), KEY_ENV),
    };
    let path = write(dir.path(), &bytes);
    let err = open(&path, DEFAULT_KEY).unwrap_err();
    assert!(err.to_string().contains(why), "{err}");
    match kind {
        3 => {
            assert_eq!((err.code(), err.reason()), ("invalid", Some(Reason::WrongKey)));
            assert_eq!(err.detail().fields["env"], KEY_ENV);
        }
        _ => assert_eq!(err.code(), "parse", "{err}"),
    }
}

/// The cloud sample with Cloud Library Sync on `service`, read once per service: building and
/// reading it is most of a case's time, and the draws only pick among three.
fn cloud_collection(service: &'static str) -> Collection {
    static READ: std::sync::Mutex<Vec<(&str, Collection)>> = std::sync::Mutex::new(Vec::new());
    let mut read = READ.lock().unwrap();
    if let Some((_, c)) = read.iter().find(|(s, _)| *s == service) {
        return c.clone();
    }
    let f = cloud_sample();
    f.exec(&format!("UPDATE djmdCloudProperty SET Reserved2 = '{service}'"));
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write(dir.path(), &f.plain()), DEFAULT_KEY).unwrap();
    read.push((service, c.clone()));
    c
}

/// rekordbox's settings say where its cloud folders are (an empty value is none, other values
/// are left alone), and a track Cloud Library Sync keeps in a storage service is found in the
/// first folder that has it, the sync service's own first.
#[hegel::test(test_cases = 12)]
fn cloud_folders_and_the_audio_they_hold(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let (dropbox, drive) = (dir.path().join("Dropbox & Co"), dir.path().join("Google Drive"));
    let given = |tc: &hegel::TestCase, p: &Path| match tc.draw(gs::booleans()) {
        true => p.display().to_string(),
        false => String::new(),
    };
    let (d, g) = (given(&tc, &dropbox), given(&tc, &drive));
    let esc = |s: &str| s.replace('&', "&amp;");
    let settings = dir.path().join("rekordbox3.settings");
    std::fs::write(
        &settings,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PROPERTIES>\n  <VALUE name=\"DropboxToken\" val=\"x\"/>\n  \
             <VALUE name=\"DropboxSharingPath\" val=\"{}\"/>\n  <VALUE name=\"GoogleDriveSharingPath\" val=\"{}\"></VALUE>\n</PROPERTIES>",
            esc(&d),
            esc(&g)
        ),
    )
    .unwrap();
    let folders = CloudFolders::from_settings(&settings).unwrap();
    assert_eq!(folders.dropbox, (!d.is_empty()).then(|| dropbox.clone()));
    assert_eq!(folders.google_drive, (!g.is_empty()).then(|| drive.clone()));

    // The track's file, in some of the folders.
    let service = tc.draw(gs::sampled_from(vec!["Dropbox", "Google Drive", "googledrive"]));
    let c = cloud_collection(service);
    let synced = c.library.track(205356547).unwrap();
    let in_folder = |folder: &Path| folder.join("rekordbox/contents_2354715273/Floorplan/ngo.mp3");
    let mut has = Vec::new();
    for folder in [&dropbox, &drive] {
        if tc.draw(gs::booleans()) {
            std::fs::create_dir_all(in_folder(folder).parent().unwrap()).unwrap();
            std::fs::write(in_folder(folder), b"mp3").unwrap();
            has.push(folder.clone());
        }
    }
    let google_first = service.to_lowercase().contains("google");
    let order = if google_first {
        [&folders.google_drive, &folders.dropbox]
    } else {
        [&folders.dropbox, &folders.google_drive]
    };
    let want = order.into_iter().flatten().find(|f| has.contains(f)).map(|f| in_folder(f));
    assert_eq!(c.audio_file(synced, &folders), want);
}

/// A copy of rekordbox's folder taken while its WAL holds drawn changes reads with them, the
/// database plain (SQLite's own WAL) or encrypted (its pages' frames encrypted too), and the copy
/// is left as it was.
#[hegel::test(test_cases = 8)]
fn a_collection_reads_with_the_changes_its_wal_holds(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let f = sample();
    let titles: Vec<(&str, String)> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
        .map(|i| {
            let id = if i % 2 == 0 { "205356547" } else { "84935471" };
            (id, format!("Changed {i} {}", tc.draw(gs::text().alphabet("ab é").max_size(4))))
        })
        .collect();
    let change = |conn: &Connection| {
        for (id, title) in &titles {
            conn.execute("UPDATE djmdContent SET Title = ?2 WHERE ID = ?1", params![id, title]).unwrap();
        }
    };
    let (db, wal) = match tc.draw(gs::booleans()) {
        false => {
            let path = write(dir.path(), &f.plain());
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;").unwrap();
            change(&conn);
            let copied = (std::fs::read(&path).unwrap(), std::fs::read(dir.path().join("master.db-wal")).unwrap());
            drop(conn);
            copied
        }
        true => {
            let codec = Codec::with_passphrase(DEFAULT_KEY.as_bytes(), [9; 16]);
            let before = f.plain();
            change(&f.conn);
            let after = f.plain();
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
            (codec.encrypt(&before).unwrap(), crate::onelibrary::wal::tests::build(PAGE_SIZE, (1, 2), &frames))
        }
    };
    let copy = dir.path().join("copy");
    std::fs::create_dir(&copy).unwrap();
    std::fs::write(copy.join("master.db"), &db).unwrap();
    std::fs::write(copy.join("master.db-wal"), &wal).unwrap();
    let c = open(&copy.join("master.db"), DEFAULT_KEY).unwrap();
    for id in ["205356547", "84935471"] {
        if let Some((_, want)) = titles.iter().rfind(|(i, _)| *i == id) {
            assert_eq!(&c.library.track(id.parse().unwrap()).unwrap().title, want);
        }
    }
    assert_eq!(std::fs::read(copy.join("master.db")).unwrap(), db, "the copy is left alone");
}

/// rekordbox's columns hold what SQLite lets them: a lookup whose ids are numbers and whose names
/// are text, bytes, a whole number or a fraction still names the track, written as the value
/// reads; numbers stored as fractions (or as text with spaces) are cut to whole ones.
#[hegel::test(test_cases = 20)]
fn values_of_any_sqlite_type_read_as_what_they_say(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new();
    // A lookup table with no column types, so SQLite keeps each value as given.
    f.exec("DROP TABLE djmdGenre; CREATE TABLE djmdGenre (ID, Name, rb_local_deleted);");
    let word = tc.draw(gs::text().alphabet("ab Üé&'").min_size(1).max_size(8));
    let bytes = tc.draw(gs::text().alphabet("xy ♪").max_size(6));
    let whole = tc.draw(gs::integers::<i64>());
    let fraction = tc.draw(gs::integers::<i32>().min_value(-99_999).max_value(99_999)) as f64
        + f64::from(tc.draw(gs::integers::<u8>().min_value(1).max_value(99))) / 100.0;
    let names: [(Value, String); 4] = [
        (Value::Text(word.clone()), word),
        (Value::Blob(bytes.clone().into_bytes()), bytes),
        (Value::Integer(whole), whole.to_string()),
        (Value::Real(fraction), fraction.to_string()),
    ];
    // A count rekordbox keeps as text: a fraction is cut, anything else reads as none.
    let plays = tc.draw(gs::integers::<u16>().max_value(5000));
    let (played, want_plays) = match tc.draw(gs::booleans()) {
        true => (format!(" {plays}.{} ", tc.draw(gs::integers::<u8>().max_value(9))), plays),
        false => (tc.draw(gs::sampled_from(vec!["", "many", "1e", "-"])).to_string(), 0),
    };
    let bpm = tc.draw(gs::integers::<u32>().max_value(30_000));
    let size = tc.draw(gs::integers::<u32>());
    let length = tc.draw(gs::integers::<u16>());
    for (i, (name, _)) in names.iter().enumerate() {
        let id = i as i64 + 1;
        f.conn.execute("INSERT INTO djmdGenre (ID, Name) VALUES (?1, ?2)", params![id, name]).unwrap();
        f.track(&id.to_string(), &format!("/Music/{id}.mp3"), "Artist", &format!("Track {id}"));
        f.conn
            .execute(
                "UPDATE djmdContent SET GenreID = ?2, BPM = ?3, FileSize = ?4, Length = ?5, DJPlayCount = ?6 WHERE ID = ?1",
                params![
                    id.to_string(),
                    id.to_string(),
                    f64::from(bpm) + 0.5,
                    f64::from(size) + 0.75,
                    format!(" {length} "),
                    played
                ],
            )
            .unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write(dir.path(), &f.plain()), DEFAULT_KEY).unwrap();
    let got: Vec<&str> = c.library.tracks.iter().map(|t| t.genre.as_str()).collect();
    let want: Vec<&str> = names.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(got, want);
    for t in &c.library.tracks {
        let want = (f64::from(bpm) / 100.0, u64::from(size), u32::from(length), want_plays);
        assert_eq!((t.bpm, t.file_size, t.duration_secs, t.play_count), want, "{played:?}");
    }
}

/// A collection SQLite finds damaged (a table's b-tree broken, encrypted or not), with a page
/// that fails authentication, or whose table can't be read (a view on a table that isn't there,
/// or any view, which has no row order) is refused as a parse error naming the file, never read
/// as far as it goes.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_collection_that_cant_be_trusted_is_refused(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let codec = Codec::with_passphrase(DEFAULT_KEY.as_bytes(), [9; 16]);
    let tables: Vec<&str> = TABLES.iter().map(|(name, _)| *name).collect();
    // Each kind of damage, once a case.
    for kind in 0..4 {
        let f = sample();
        let (bytes, why) = match kind {
            0 => {
                let table = tc.draw(gs::sampled_from(tables.clone()));
                let root: u32 = f
                    .conn
                    .query_row("SELECT rootpage FROM sqlite_master WHERE name = ?1", [table], |r| r.get(0))
                    .unwrap();
                let mut plain = f.plain();
                let at = (root as usize - 1) * PAGE_SIZE;
                plain[at + 3..at + 5].copy_from_slice(&u16::MAX.to_be_bytes());
                let bytes = if tc.draw(gs::booleans()) { codec.encrypt(&plain).unwrap() } else { plain };
                (bytes, "SQLite finds it damaged".to_string())
            }
            1 => {
                let mut bytes = f.encrypted(DEFAULT_KEY);
                let pages = bytes.len() / PAGE_SIZE;
                let page = tc.draw(gs::integers::<usize>().min_value(2).max_value(pages));
                let at = (page - 1) * PAGE_SIZE + tc.draw(gs::integers::<usize>().max_value(PAGE_SIZE - 1));
                bytes[at] ^= tc.draw(gs::integers::<u8>().min_value(1));
                (bytes, format!("page {page} fails authentication"))
            }
            2 => {
                let table = tc.draw(gs::sampled_from(vec!["djmdArtist", "djmdKey", "djmdCue", "djmdMyTag"]));
                f.exec(&format!("DROP TABLE {table}; CREATE VIEW {table} AS SELECT * FROM gone;"));
                (f.plain(), "no such table: main.gone".to_string())
            }
            // A view, which has no row order to read it in.
            _ => {
                let table = tc.draw(gs::sampled_from(vec!["djmdGenre", "djmdLabel", "djmdKey"]));
                f.exec(&format!("DROP TABLE {table}; CREATE VIEW {table} AS SELECT 1 AS ID;"));
                (f.plain(), "no such column: rowid".to_string())
            }
        };
        let path = write(dir.path(), &bytes);
        let err = open(&path, DEFAULT_KEY).unwrap_err();
        assert_eq!(err.code(), "parse", "{err}");
        let message = err.to_string();
        assert!(message.starts_with(&format!("could not parse {}: ", path.display())), "{message}");
        assert!(message.contains(&why), "{message}");
    }
}

/// A copy is taken again while the WAL restarts between reading its header and copying (a
/// checkpoint), so the copy and the WAL read are of one moment: it's the last one after up to two
/// restarts, and after three the read gives up saying so. A WAL that's there but can't be read
/// is an error, an empty one none at all.
#[hegel::test(test_cases = 20)]
fn a_copy_is_taken_again_until_the_wal_holds_still(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let (path, to) = (dir.path().join("master.db"), dir.path().join("copy"));
    let wal_path = dir.path().join("master.db-wal");
    let generation = |n: u32| crate::onelibrary::wal::tests::build(PAGE_SIZE, (n, n + 1), &[]);
    std::fs::write(&path, "state 0").unwrap();
    let start = tc.draw(gs::integers::<u8>().max_value(2));
    match start {
        0 => {}
        1 => std::fs::write(&wal_path, b"").unwrap(),
        _ => std::fs::write(&wal_path, generation(0)).unwrap(),
    }
    let restarts = tc.draw(gs::integers::<u32>().max_value(4));
    let copies = std::cell::Cell::new(0u32);
    let got = snapshot(&path, &to, |from: &Path, to: &Path| {
        let copied = std::fs::copy(from, to);
        let n = copies.get();
        copies.set(n + 1);
        if n < restarts {
            std::fs::write(from, format!("state {}", n + 1)).unwrap();
            std::fs::write(&wal_path, generation(2 * n + 2)).unwrap();
        }
        copied
    });
    match restarts {
        0..=2 => {
            let wal = got.unwrap();
            assert_eq!(copies.get(), restarts + 1);
            assert_eq!(std::fs::read_to_string(&to).unwrap(), format!("state {restarts}"));
            let want = match (restarts, start) {
                (0, 0 | 1) => None,
                (0, _) => Some(generation(0)),
                (n, _) => Some(generation(2 * n)),
            };
            assert_eq!(wal, want);
        }
        _ => {
            let err = got.unwrap_err();
            assert!(matches!(err, Error::Conflict(_)) && err.to_string().contains("kept changing"), "{err}");
            assert_eq!(copies.get(), 3);
        }
    }
    // A WAL that can't be opened (a link to itself): the read stops there.
    std::fs::remove_file(&wal_path).ok();
    std::os::unix::fs::symlink(&wal_path, &wal_path).unwrap();
    let err = snapshot(&path, &to, |from: &Path, to: &Path| std::fs::copy(from, to)).unwrap_err();
    assert!(matches!(&err, Error::Io { path, .. } if *path == wal_path), "{err}");
}

/// rekordbox's settings file is read for the two folders only: a `VALUE` without a name, or
/// another name, is passed over, and a file that isn't well-formed XML (or isn't there) is
/// refused naming it.
#[hegel::test(test_cases = 20)]
fn settings_name_the_cloud_folders_or_are_refused(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let folder = tc.draw(gs::text().alphabet("ab /é").min_size(1).max_size(10));
    let skipped =
        tc.draw(gs::sampled_from(vec!["<VALUE/>", "<VALUE val=\"/x\"/>", "<VALUE name=\"Other\" val=\"/y\"/>"]));
    let settings = dir.path().join("rekordbox3.settings");
    let xml = format!("<PROPERTIES>{skipped}<VALUE name=\"GoogleDriveSharingPath\" val=\"{folder}\"/></PROPERTIES>");
    std::fs::write(&settings, &xml).unwrap();
    let got = CloudFolders::from_settings(&settings).unwrap();
    assert_eq!(got, CloudFolders { dropbox: None, google_drive: Some(PathBuf::from(&folder)) });

    let broken = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => xml.replace("</PROPERTIES>", "</SETTINGS>"),
        1 => format!("{xml}</PROPERTIES>"),
        _ => format!("{xml}<!-- unclosed"),
    };
    std::fs::write(&settings, &broken).unwrap();
    let err = CloudFolders::from_settings(&settings).unwrap_err();
    assert_eq!(err.code(), "parse", "{broken}: {err}");
    assert!(err.to_string().contains(&settings.display().to_string()), "{err}");
    let missing = dir.path().join("rekordbox6/rekordbox3.settings");
    assert!(matches!(CloudFolders::from_settings(&missing), Err(Error::Io { path, .. }) if path == missing));
}

/// A drive compared with the collection: tracks at the same file count as found by it, ones moved
/// on the drive are found by ISRC or artist, title and length, and ones only a title finds are
/// listed as uncertain and counted, with the ones the drive lacks, as missing from it. A playlist
/// neither has is refused.
#[hegel::test(test_cases = 15)]
fn a_drive_differs_from_the_collection_in_what_it_lacks(tc: hegel::TestCase) {
    use hegel::Generator;
    use hegel::generators as gs;
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum On {
        Same,
        Moved,
        Gone,
    }
    const TITLES: &[&str] = &["Aerodynamic", "Nightcall", "Voyager", "Harder", "Veridis"];
    const ARTISTS: &[&str] = &["Daft Punk", "Kavinsky", "Justice", "Moby", "Air"];
    let f = Fixture::new();
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(TITLES.len()));
    let mut drawn = Vec::new();
    for i in 0..n {
        // The first track has every detail, the second none, the rest drawn.
        let (artist, isrc, extras) = match i {
            0 => (true, true, true),
            1 => (false, false, false),
            _ => (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans())),
        };
        let on = match i {
            0 | 1 => On::Moved,
            _ => tc.draw(gs::sampled_from(vec![On::Same, On::Moved, On::Gone]).print_as_debug()),
        };
        let id = (i + 1).to_string();
        f.track(&id, &format!("/Users/dj/Music/{}/{}.mp3", ARTISTS[i], TITLES[i]), ARTISTS[i], TITLES[i]);
        if !artist {
            f.conn.execute("UPDATE djmdContent SET ArtistID = NULL WHERE ID = ?1", [&id]).unwrap();
        }
        if isrc {
            f.conn
                .execute("UPDATE djmdContent SET ISRC = ?2 WHERE ID = ?1", params![id, format!("GBDUW00000{i:02}")])
                .unwrap();
        }
        if extras {
            f.exec(&format!("INSERT INTO djmdAlbum (ID, Name) VALUES ('{id}', 'Album {id}')"));
            f.conn
                .execute("UPDATE djmdContent SET AlbumID = ?1, Subtitle = 'Extended Mix' WHERE ID = ?1", [&id])
                .unwrap();
        }
        drawn.push((i as u32 + 1, on, artist || isrc));
    }
    let dir = tempfile::tempdir().unwrap();
    let c = open(&write(dir.path(), &f.plain()), DEFAULT_KEY).unwrap();
    let mut drive = c.library.clone();
    drive.tracks.retain(|t| drawn.iter().any(|(id, on, _)| *id == t.id && *on != On::Gone));
    for t in &mut drive.tracks {
        if drawn.iter().any(|(id, on, _)| *id == t.id && *on == On::Moved) {
            t.file_path = format!("/Contents/Moved/{}.mp3", t.id);
        }
    }
    let d = c.diff_drive(&drive, "Main", None).unwrap();
    let count = |want: &dyn Fn(On, bool) -> bool| drawn.iter().filter(|(_, on, known)| want(*on, *known)).count();
    assert_eq!(d.matched_by_file, count(&|on, _| on == On::Same));
    assert_eq!(d.matched_by_details, count(&|on, known| on == On::Moved && known));
    let uncertain: Vec<u32> = d.uncertain.iter().map(|t| t.id).collect();
    let loose: Vec<u32> =
        drawn.iter().filter(|(_, on, known)| *on == On::Moved && !known).map(|(id, ..)| *id).collect();
    assert_eq!(uncertain, loose);
    let mut missing: Vec<u32> = d.diff.added_tracks.iter().map(|t| t.id).collect();
    missing.sort_unstable();
    let mut lacking: Vec<u32> = drawn
        .iter()
        .filter(|(_, on, known)| *on == On::Gone || (*on == On::Moved && !known))
        .map(|(id, ..)| *id)
        .collect();
    lacking.sort_unstable();
    assert_eq!(missing, lacking);
    assert!(c.diff_drive(&drive, "Main", Some("Nowhere")).is_err());
}

/// A key given is the one used, whatever it says; none (or an empty one) falls back to
/// `LOCALLIBRARY_MASTERDB_KEY`, then the key every rekordbox uses. Without
/// `LOCALLIBRARY_MASTERDB`, the collection read is rekordbox's own, in this user's folders.
#[hegel::test(test_cases = 20)]
fn keys_and_the_collection_default_to_rekordboxs(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let key = tc.draw(gs::text().max_size(70));
    let fallback = std::env::var(KEY_ENV).ok().filter(|k| !k.is_empty()).unwrap_or_else(|| DEFAULT_KEY.to_string());
    let want = if key.is_empty() { fallback.clone() } else { key.clone() };
    assert_eq!(resolve_key(Some(&key)), want);
    assert_eq!(resolve_key(None), fallback);
    #[cfg(target_os = "macos")]
    if std::env::var_os(PATH_ENV).is_none_or(|p| p.is_empty()) {
        let home = directories::BaseDirs::new().map(|b| b.home_dir().join("Library/Pioneer/rekordbox/master.db"));
        assert_eq!(default_path(), home);
    }
}
