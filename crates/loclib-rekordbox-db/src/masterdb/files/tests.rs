use hegel::TestCase;
use hegel::generators as gs;
use rusqlite::params;
use serde_json::json;

use super::*;
use crate::masterdb::{DEFAULT_KEY, PATH_ENV, SETTINGS_ENV, sqlcipher};
use crate::onelibrary::cipher::{SALT_LEN, SQLITE_MAGIC};
use crate::test_util::{Fixture, SECRET, has, library, plain, registry, sample};

/// `bytes` written to `name` in `dir`.
fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn locations_are_where_rekordbox_keeps_its_files() {
    let dir = tempfile::tempdir().unwrap();
    let master = dir.path().join("rekordbox/master.db");
    // SAFETY: nextest runs each test in a process of its own.
    unsafe {
        std::env::set_var(PATH_ENV, &master);
        std::env::remove_var(SETTINGS_ENV);
    }
    let at = locations();
    assert_eq!(at.master_db.as_deref(), Some(master.as_path()));
    assert_eq!(at.data.as_deref(), Some(dir.path().join("rekordbox").as_path()));
    assert_eq!(at.share.as_deref(), Some(dir.path().join("rekordbox/share").as_path()));
    assert!(!at.master_db_exists);
    std::fs::create_dir(dir.path().join("rekordbox")).unwrap();
    std::fs::write(&master, "").unwrap();
    assert!(locations().master_db_exists);
    assert_eq!(at.settings, None, "a test names rekordbox's settings, or reads none");
    unsafe { std::env::set_var(SETTINGS_ENV, dir.path().join("Pioneer")) };
    assert_eq!(locations().settings, Some(dir.path().join("Pioneer/rekordbox6")));
}

/// Anything named for rekordbox is taken for it (a false one only holds a write back), but its agent.
#[test]
fn whatever_is_named_for_rekordbox_but_its_agent_is_rekordbox() {
    for (name, is) in [
        ("rekordbox", true),
        ("rekordbox.exe", true),
        ("REKORDBOX.EXE", true),
        ("rekordboxAgent", false),
        ("rekordbox Helper", true),
        ("sleep", false),
    ] {
        assert_eq!(is_rekordbox(OsStr::new(name)), is, "{name}");
    }
}

/// A program named rekordbox running is rekordbox running.
#[cfg(unix)]
#[test]
fn a_running_rekordbox_is_seen() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("rekordbox");
    std::fs::copy("/bin/sleep", &program).unwrap();
    let mut child = std::process::Command::new(&program).arg("30").spawn().unwrap();
    let status = status();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(status.running);
}

/// The version is the running app's, else the newest installed: by its numbers, not as text.
#[cfg(target_os = "macos")]
#[test]
fn the_version_is_the_running_apps_else_the_newest() {
    let apps = tempfile::tempdir().unwrap();
    let app = |folder: &str, version: &str| {
        let contents = apps.path().join(folder).join("rekordbox.app/Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        let info = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
             <key>CFBundleExecutable</key><string>rekordbox</string>\
             <key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>"
        );
        std::fs::write(contents.join("Info.plist"), info).unwrap();
        contents.join("MacOS/rekordbox")
    };
    let six = app("rekordbox 6", "6.8.5");
    app("rekordbox 7", "7.2.14.0323");
    app("rekordbox 10", "10.0.1");
    app("Other", "99.0");
    let options = write(apps.path(), "options.json", br#"{"options": [["app_ver", "7.2.14"]]}"#);
    assert_eq!(version(None, apps.path(), Some(&options)).as_deref(), Some("10.0.1"));
    assert_eq!(version(Some(&six), apps.path(), Some(&options)).as_deref(), Some("6.8.5"));
    assert_eq!(version(None, &apps.path().join("none"), None), None);
}

/// Without an app to ask (on Windows, or with none installed), the version is the one rekordbox's
/// agent last recorded, when it's text.
#[test]
fn the_version_falls_back_to_the_agents() {
    let dir = tempfile::tempdir().unwrap();
    let none = dir.path().join("no apps");
    let options = |json: &str| write(dir.path(), "options.json", json.as_bytes());
    let recorded = options(r#"{"defaults": [], "options": [["db-path", "/x/master.db"], ["app_ver", "7.2.14"]]}"#);
    assert_eq!(version(None, &none, Some(&recorded)).as_deref(), Some("7.2.14"));
    for odd in [r#"{"options": [["app_ver", 7]]}"#, r#"{"options": []}"#, "not json"] {
        assert_eq!(version(None, &none, Some(&options(odd))), None, "{odd}");
    }
    assert_eq!(version(None, &none, Some(&dir.path().join("missing.json"))), None);
}

/// A library rekordbox's settings moved is found there: the last absolute `masterDbDirectory`,
/// else the agent's `db-path`; with neither, nowhere.
#[test]
fn a_moved_library_is_found_where_rekordbox_says() {
    use crate::masterdb::moved;
    let dir = tempfile::tempdir().unwrap();
    let at = |name: &str| dir.path().join(name);
    let value = |name: &str, val: &Path| format!("<VALUE name=\"{name}\" val=\"{}\"/>", val.display());
    let settings = format!(
        "<?xml version=\"1.0\"?>\n<PROPERTIES>{}{}{}{}</PROPERTIES>",
        value("masterDbDirectory", &at("first")),
        value("masterDbDirectory", &at("moved")),
        value("masterDbDirectory", Path::new("relative")),
        value("DropboxToken", Path::new("secret")),
    );
    let settings = write(dir.path(), "rekordbox3.settings", settings.as_bytes());
    let db_path = format!(r#"{{"options": [["db-path", {}]]}}"#, json!(at("agent/master.db")));
    let options = write(dir.path(), "options.json", db_path.as_bytes());
    assert_eq!(moved(Some(&settings), Some(&options)), Some(at("moved/master.db")));
    assert_eq!(moved(Some(&at("missing")), Some(&options)), Some(at("agent/master.db")));
    assert_eq!(moved(None, Some(&options)), Some(at("agent/master.db")));
    let elsewhere = write(dir.path(), "plain.settings", b"<PROPERTIES><VALUE name=\"lang\" val=\"en\"/></PROPERTIES>");
    assert_eq!(moved(Some(&elsewhere), None), None);
}

#[test]
fn tracks_are_every_row_with_its_files() {
    let dir = tempfile::tempdir().unwrap();
    // An artist without a name, and a track without an artist, have none.
    let f = library();
    f.exec(
        "INSERT INTO djmdArtist (ID, Name) VALUES ('blank', '');
         UPDATE djmdContent SET ArtistID = 'blank' WHERE ID = '4';
         UPDATE djmdContent SET ArtistID = NULL WHERE ID = '3';",
    );
    let path = write(dir.path(), "master.db", &f.encrypted(DEFAULT_KEY));
    let read = tracks(&path, DEFAULT_KEY, 0, 500).unwrap();
    let track = |id: &str, path: &str, artist: Option<&str>, deleted| CollectionTrack {
        id: id.into(),
        uuid: Some(format!("uuid-{id}")),
        path: path.into(),
        deleted,
        artist: artist.map(String::from),
        ..CollectionTrack::default()
    };
    let one_more_time = CollectionTrack {
        analysis_path: Some("/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT".into()),
        image_path: Some("/PIONEER/Artwork/00001/a1.jpg".into()),
        album: Some("Discovery".into()),
        ..track("84935471", "/Users/dj/Music/Daft Punk/Discovery/01 One More Time.mp3", Some("Daft Punk"), false)
    };
    let expected = CollectionTracks {
        db_id: Some("1234567890".into()),
        total: 4,
        tracks: vec![
            one_more_time,
            // Cloud Library Sync's: the file on this computer, not in the service's folder.
            track("205356547", "/Users/dj/Music/Downloads/ngo.mp3", Some("Floorplan"), false),
            track("3", "/Users/dj/Music/gone.mp3", None, true),
            track("4", "/Users/dj/Music/Only Here.mp3", None, false),
        ],
    };
    assert_eq!(read, expected);
    let page = tracks(&path, DEFAULT_KEY, 1, 2).unwrap();
    assert_eq!((page.total, &page.tracks[..]), (4, &expected.tracks[1..3]));
    assert!(tracks(&path, DEFAULT_KEY, 9, 2).unwrap().tracks.is_empty());
    assert!(tracks(&path, DEFAULT_KEY, u64::MAX, u64::MAX).unwrap().tracks.is_empty());
    // A database without OrgFolderPath (rekordbox 6's) reads FolderPath as it is.
    let older = write(dir.path(), "older.db", &sample().encrypted(DEFAULT_KEY));
    let ngo = tracks(&older, DEFAULT_KEY, 1, 1).unwrap().tracks.remove(0);
    assert_eq!((ngo.path.as_str(), ngo.uuid), ("/Users/dj/Music/Downloads/ngo.mp3", None));
}

/// Asked for with credentials, as rekordbox's own backup has them, a copy is the whole
/// collection.
#[test]
fn a_copy_is_the_collection_rebuilt_and_encrypted_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "master.db", &library().encrypted(DEFAULT_KEY));
    let original = std::fs::read(&source).unwrap();
    let into = dir.path().join("copy.db");
    let made = export(&source, DEFAULT_KEY, true, &into).unwrap();
    let copy = std::fs::read(&into).unwrap();
    let expected = CollectionCopy {
        bytes: copy.len() as u64,
        db_id: Some("1234567890".into()),
        db_version: Some("7000".into()),
        credentials_removed: 0,
    };
    assert_eq!(made, expected);
    assert_eq!(copy[..SALT_LEN], original[..SALT_LEN], "under the original's salt");
    assert!(!copy.starts_with(SQLITE_MAGIC));
    assert_eq!(std::fs::read(&source).unwrap(), original, "the original is left alone");
    assert_eq!(tracks(&into, DEFAULT_KEY, 0, 500).unwrap(), tracks(&source, DEFAULT_KEY, 0, 500).unwrap());
    assert_eq!(registry(&into), ["agentCredentials", "localUpdateCount", "SyncAnalysisDataRootPath"]);
    let checks = ["PRAGMA cipher_integrity_check;", "SELECT count(*) FROM djmdContent;"];
    if let Some(out) = sqlcipher::run(&into, DEFAULT_KEY, &checks) {
        assert_eq!(out, ["ok", "4"], "SQLCipher reads it");
    }
}

/// Unless asked for them, the cloud agent's sign-in is nowhere in the copy, not even in a free
/// page, and the rest of the registry is there.
#[test]
fn a_copy_has_no_credentials_unless_asked() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "master.db", &library().encrypted(DEFAULT_KEY));
    let into = dir.path().join("copy.db");
    assert_eq!(export(&source, DEFAULT_KEY, false, &into).unwrap().credentials_removed, 1);
    assert_eq!(registry(&into), ["localUpdateCount", "SyncAnalysisDataRootPath"]);
    assert!(has(&plain(&source), SECRET));
    assert!(!has(&plain(&into), SECRET));
    assert_eq!(tracks(&into, DEFAULT_KEY, 0, 500).unwrap().total, 4);
    // A collection without the registry has nothing to leave out.
    let older = write(dir.path(), "older.db", &sample().encrypted(DEFAULT_KEY));
    let copy = dir.path().join("older copy.db");
    assert_eq!(export(&older, DEFAULT_KEY, false, &copy).unwrap().credentials_removed, 0);
}

#[test]
fn a_plain_collection_is_copied_plain() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "master.db", &library().plain());
    let into = dir.path().join("copy.db");
    let made = export(&source, "unused", false, &into).unwrap();
    assert!(std::fs::read(&into).unwrap().starts_with(SQLITE_MAGIC));
    assert_eq!((made.credentials_removed, made.db_id.as_deref()), (1, Some("1234567890")));
    assert!(!has(&plain(&into), SECRET));
}

/// A copy is a new file: one there already is left as it is, and a copy that fails
/// leaves nothing behind.
#[test]
fn a_copy_writes_only_a_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "master.db", &library().encrypted(DEFAULT_KEY));
    let mine = write(dir.path(), "mine", b"mine");
    let e = export(&source, DEFAULT_KEY, false, &mine).unwrap_err();
    assert!(matches!(&e, Error::Io { source, .. } if source.kind() == std::io::ErrorKind::AlreadyExists), "{e}");
    assert_eq!(std::fs::read(&mine).unwrap(), b"mine");
    let into = dir.path().join("copy.db");
    assert!(matches!(export(&source, "another key", false, &into), Err(Error::WrongKey(_))));
    // A registry whose sign-in rows can't be told apart is refused, not copied with them.
    let f = Fixture::new();
    f.exec("CREATE TABLE agentRegistry (name TEXT); INSERT INTO agentRegistry VALUES ('agentCredentials');");
    let odd = write(dir.path(), "odd.db", &f.encrypted(DEFAULT_KEY));
    let e = export(&odd, DEFAULT_KEY, false, &into).unwrap_err().to_string();
    assert!(e.contains("no registry_id"), "{e}");
    assert!(!into.exists());
}

/// A drawn collection's track rows, some deleted: ids, paths and whether each is deleted.
#[hegel::test_helper]
fn drawn(tc: &TestCase) -> (Fixture, Vec<(String, String, bool)>) {
    let f = Fixture::new();
    f.exec("ALTER TABLE djmdContent ADD COLUMN OrgFolderPath VARCHAR(255);");
    let n = tc.draw(gs::integers::<usize>().max_value(24));
    let mut rows = Vec::new();
    for i in 0..n {
        let name = tc.draw(gs::from_regex("[A-Za-z0-9 ÜïØ'&]{1,10}").fullmatch(true));
        let path = format!("/Users/dj/Music/{name}/{i}.mp3");
        let deleted = tc.draw(gs::weighted_booleans(0.2));
        f.track(&i.to_string(), &path, "Artist", "Title");
        f.conn
            .execute("UPDATE djmdContent SET rb_local_deleted = ?1 WHERE ID = ?2", params![deleted, i.to_string()])
            .unwrap();
        rows.push((i.to_string(), path, deleted));
    }
    (f, rows)
}

/// The rows of `read`, as [`drawn`] has them.
fn rows(read: &CollectionTracks) -> Vec<(String, String, bool)> {
    read.tracks.iter().map(|t| (t.id.clone(), t.path.clone(), t.deleted)).collect()
}

/// Whatever the page size, paging through the tracks reads every row once, in order, each page
/// saying how many there are in all.
#[hegel::test(test_cases = 30)]
fn prop_tracks_pages_read_every_row_once(tc: TestCase) {
    let (f, expected) = drawn(&tc);
    let size = tc.draw(gs::integers::<u64>().min_value(1).max_value(10));
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "master.db", &f.plain());
    let mut read = CollectionTracks::default();
    loop {
        let mut page = tracks(&path, DEFAULT_KEY, read.tracks.len() as u64, size).unwrap();
        assert_eq!(page.total, expected.len());
        assert!(page.tracks.len() as u64 <= size);
        if page.tracks.is_empty() {
            break;
        }
        read.tracks.append(&mut page.tracks);
    }
    assert_eq!(rows(&read), expected);
}

/// A copy, asked for credentials or not, holds every track row as the collection does, and
/// every registry row but the sign-in ones, which it keeps only when asked.
#[hegel::test(test_cases = 10)]
fn prop_a_copy_round_trips_the_rows(tc: TestCase) {
    let (f, expected) = drawn(&tc);
    f.exec(&format!(
        "CREATE TABLE agentRegistry (registry_id VARCHAR(255) PRIMARY KEY, str_1 VARCHAR(255));
         INSERT INTO agentRegistry VALUES ('agentCredentials', '{SECRET}'), ('localUpdateCount', '1');"
    ));
    let with = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let source = write(dir.path(), "master.db", &f.encrypted(DEFAULT_KEY));
    let into = dir.path().join("copy.db");
    let made = export(&source, DEFAULT_KEY, with, &into).unwrap();
    assert_eq!((made.bytes, made.credentials_removed), (std::fs::metadata(&into).unwrap().len(), u64::from(!with)));
    assert_eq!(rows(&tracks(&into, DEFAULT_KEY, 0, 500).unwrap()), expected);
    let kept: &[&str] = if with { &["agentCredentials", "localUpdateCount"] } else { &["localUpdateCount"] };
    assert_eq!(registry(&into), kept);
    assert_eq!(has(&plain(&into), SECRET), with);
}

/// A collection a table of which a backup reads can't be read (a view of a table that's gone) has
/// no tracks to list and no copy to make, and leaves none behind; nor has one under another key.
#[test]
fn a_collection_that_cant_be_read_is_neither_listed_nor_copied() {
    let dir = tempfile::tempdir().unwrap();
    let into = dir.path().join("copy.db");
    for table in ["djmdContent", "djmdArtist", "djmdAlbum", "djmdProperty"] {
        let f = library();
        f.exec(&format!("DROP TABLE {table}; CREATE VIEW {table} AS SELECT * FROM gone;"));
        let path = write(dir.path(), "master.db", &f.encrypted(DEFAULT_KEY));
        let e = tracks(&path, DEFAULT_KEY, 0, 500).unwrap_err();
        assert!(e.to_string().contains("no such table: main.gone"), "{table}: {e}");
        if matches!(table, "djmdContent" | "djmdProperty") {
            let e = export(&path, DEFAULT_KEY, false, &into).unwrap_err();
            assert!(e.to_string().contains("no such table: main.gone"), "{table}: {e}");
            assert!(!into.exists(), "{table}");
        }
    }
    let path = write(dir.path(), "master.db", &library().encrypted(DEFAULT_KEY));
    assert!(matches!(tracks(&path, "another key", 0, 1), Err(Error::WrongKey(_))));
}

/// A collection with a trigger is refused a copy without the sign-in: taking the rows out would
/// fire it, and this one keeps them in another table. Nothing is left behind. Asked for with
/// the sign-in, nothing fires, so it's copied.
#[test]
fn a_trigger_cant_keep_the_sign_in_in_a_copy() {
    let dir = tempfile::tempdir().unwrap();
    let f = library();
    f.exec(
        "CREATE TABLE kept (v TEXT);
         CREATE TRIGGER keep BEFORE DELETE ON agentRegistry BEGIN INSERT INTO kept VALUES (old.str_1); END;",
    );
    let source = write(dir.path(), "master.db", &f.encrypted(DEFAULT_KEY));
    let into = dir.path().join("copy.db");
    let e = export(&source, DEFAULT_KEY, false, &into).unwrap_err();
    assert!(matches!(e, Error::Parse { .. }), "{e}");
    let why = "it has a trigger keep, which rekordbox never makes";
    let instead = "copy it with the credentials kept (with_credentials)";
    assert!(e.to_string().ends_with(&format!("{why}; {instead}")), "{e}");
    assert!(!into.exists());
    assert_eq!(export(&source, DEFAULT_KEY, true, &into).unwrap().credentials_removed, 0);
}

/// A registry the sign-in rows can't be taken out of (a view of another table) is refused, not
/// copied with them, and no copy is left behind.
#[test]
fn a_registry_whose_sign_in_cant_be_left_out_isnt_copied() {
    let dir = tempfile::tempdir().unwrap();
    let f = sample();
    f.exec(&format!(
        "CREATE TABLE registry (registry_id VARCHAR(255), str_1 VARCHAR(255));
         INSERT INTO registry VALUES ('agentCredentials', '{SECRET}');
         CREATE VIEW agentRegistry AS SELECT * FROM registry;"
    ));
    let source = write(dir.path(), "master.db", &f.encrypted(DEFAULT_KEY));
    let into = dir.path().join("copy.db");
    let e = export(&source, DEFAULT_KEY, false, &into).unwrap_err();
    assert!(matches!(e, Error::Parse { .. }), "{e}");
    assert!(e.to_string().contains("cannot modify agentRegistry because it is a view"), "{e}");
    assert!(!into.exists());
}
