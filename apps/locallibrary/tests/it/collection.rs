//! `locallibrary collection` (rekordbox's own collection on this computer, read only, and what a
//! drive lacks against it) and the `collection.*` RPC methods. Reading a `master.db` is tested in
//! the library (`masterdb`); here the collection is built as plain SQLite with the tables and
//! columns rekordbox 7 has (rekordbox encrypts its `master.db`; decrypted, it reads the same).
//! These never look at rekordbox's own.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::support::{Env, Rpc, line, path};

/// `locallibrary` with no `--db`: `LOCALLIBRARY_MASTERDB` names a database that isn't there.
fn without_rekordbox(env: &Env) -> Command {
    let mut c = env.command();
    c.env("LOCALLIBRARY_MASTERDB", env.outside("no-rekordbox/master.db")).env_remove("LOCALLIBRARY_MASTERDB_KEY");
    c
}

#[test]
fn collection_reports_what_it_cannot_read() {
    let env = Env::new();
    let out = without_rekordbox(&env).args(["--json", "collection"]).output().unwrap();
    assert!(!out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["code"], "not_found", "{v}");

    // Not a rekordbox database: refused, not misread.
    let junk = env.outside("master.db");
    std::fs::write(&junk, vec![0x5au8; 8192]).unwrap();
    let out = without_rekordbox(&env).args(["--json", "collection", "--db", path(&junk), "tracks"]).output().unwrap();
    assert!(!out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["code"], "invalid", "{v}");
    assert_eq!(std::fs::read(&junk).unwrap(), vec![0x5au8; 8192], "the database is only read");
}

#[test]
fn rpc_lists_and_answers_the_collection_methods() {
    let env = Env::new();
    let mut rpc = Rpc::start(without_rekordbox(&env));
    let version = rpc.call(1, "version", Value::Null);
    let methods = version["result"]["methods"].as_array().unwrap();
    for m in [
        "collection.info",
        "collection.library",
        "collection.diff",
        "collection.export.plan",
        "collection.export.apply",
    ] {
        assert!(methods.iter().any(|x| x == m), "{m}");
    }
    let info = rpc.call(2, "collection.info", json!({}));
    assert_eq!(info["error"]["data"]["kind"], "not_found", "{info}");
}

/// Four tracks: the demo drive's first (by its file name), two the drive lacks, and one with no artist
/// that only its title matches to a track on the drive. A folder "Shows" with a playlist "Friday", and an
/// intelligent playlist "Recent".
fn master_db(env: &Env) -> PathBuf {
    let db = env.outside("master.db");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            "CREATE TABLE djmdProperty (DBID TEXT, DBVersion TEXT);
             INSERT INTO djmdProperty VALUES ('1234567890', '7000');
             CREATE TABLE djmdArtist (ID TEXT PRIMARY KEY, Name TEXT);
             INSERT INTO djmdArtist VALUES ('1', 'Loopmasters'), ('2', 'Floorplan');
             CREATE TABLE djmdContent (ID TEXT PRIMARY KEY, FolderPath TEXT, FileNameL TEXT, Title TEXT,
                 ArtistID TEXT, BPM INTEGER, Length INTEGER, FileSize INTEGER, rb_local_deleted INTEGER DEFAULT 0);
             INSERT INTO djmdContent (ID, FolderPath, FileNameL, Title, ArtistID, BPM, Length, FileSize) VALUES
                 ('1', '/Users/dj/Music/Demo Track 1.mp3', 'Demo Track 1.mp3', 'Demo Track 1', '1', 12800, 300, 1800),
                 ('2', '/Users/dj/Music/Never Grow Old.mp3', 'Never Grow Old.mp3', 'Never Grow Old', '2', 12400, 412, 9000),
                 ('3', '/Users/dj/Music/Sanctified.mp3', 'Sanctified.mp3', 'Sanctified', '2', 13000, 380, 8000),
                 ('4', '/Users/dj/Music/Bootleg.mp3', 'Bootleg.mp3', 'Demo Track 2', NULL, 12000, 200, 100);
             CREATE TABLE djmdPlaylist (ID TEXT PRIMARY KEY, Seq INTEGER, Name TEXT, Attribute INTEGER, ParentID TEXT);
             INSERT INTO djmdPlaylist VALUES
                 ('10', 1, 'Shows', 1, 'root'), ('11', 1, 'Friday', 0, '10'), ('12', 2, 'Recent', 4, 'root');
             CREATE TABLE djmdSongPlaylist (ID TEXT PRIMARY KEY, PlaylistID TEXT, ContentID TEXT, TrackNo INTEGER);
             INSERT INTO djmdSongPlaylist VALUES ('a', '11', '2', 1), ('b', '11', '1', 2);",
        )
        .unwrap();
    db
}

fn collection(env: &Env, db: &Path, args: &[&str]) -> String {
    env.ok(&[&["collection", "--db", path(db)], args].concat())
}

#[test]
fn the_collection_says_what_it_holds() {
    let env = Env::new();
    let db = master_db(&env);
    let out = collection(&env, &db, &[]);
    assert!(out.starts_with("rekordbox collection\n") && line(&out, "master.db").contains(path(&db)), "{out}");
    assert!(out.contains("database version 7000") && out.contains("4 tracks by 3 artists"), "{out}");
    assert!(out.contains("2 playlists in 1 folders, 0 history lists"), "{out}");
    assert!(out.contains("1 intelligent playlist(s) aren't evaluated and show empty: Recent"), "{out}");
    assert!(std::fs::read(&db).is_ok_and(|b| b.starts_with(b"SQLite format 3\0")), "only read");

    let out = collection(&env, &db, &["playlists"]);
    assert!(out.contains("Shows/\n  Friday"), "{out}");
    assert!(!env.store.exists(), "reading the collection makes no store");
}

#[test]
fn the_collections_tracks_are_searched_filtered_and_sorted() {
    let env = Env::new();
    let db = master_db(&env);
    // The titles listed, in order.
    let titles = |args: &[&str]| -> Vec<&str> {
        let out = collection(&env, &db, &[&["tracks"], args].concat());
        let all = ["Demo Track 1", "Never Grow Old", "Sanctified"];
        out.lines().filter_map(|l| all.into_iter().find(|t| l.contains(t))).collect()
    };
    assert_eq!(titles(&[]), ["Demo Track 1", "Never Grow Old", "Sanctified"]);
    assert_eq!(titles(&["--search", "floorplan"]), ["Never Grow Old", "Sanctified"]);
    assert_eq!(titles(&["--playlist", "Shows/Friday"]), ["Never Grow Old", "Demo Track 1"], "in the playlist's order");
    assert_eq!(titles(&["--filter", "bpm:>125"]), ["Demo Track 1", "Sanctified"]);
    assert_eq!(titles(&["--sort", "-bpm", "-n", "2"]), ["Sanctified", "Demo Track 1"]);

    let r = env.run(&["collection", "--db", path(&db), "tracks", "--playlist", "Nope"]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("no playlist \"Nope\" in the collection"), "{}", r.err);
}

#[test]
fn a_drive_is_compared_with_the_collection() {
    let env = Env::new();
    let db = master_db(&env);
    let main = env.drive("Main");
    let out = collection(&env, &db, &["diff", path(&main)]);
    assert!(out.contains("+ is in the collection but not on Main, - only on the drive"), "{out}");
    assert!(out.contains("1 track(s) found on the drive by file"), "{out}");
    assert!(out.contains("  + tracks (3):\n    Floorplan - Never Grow Old"), "{out}");
    assert!(out.contains("  - tracks (1):\n    Loopmasters - Demo Track 2"), "{out}");
    assert!(out.contains("  + playlists (3):\n    Recent\n    Shows/\n    Shows/Friday"), "{out}");
    assert!(out.contains("  - playlists (1):\n    Set"), "{out}");
    let loose = line(&out, "only loosely match a track on the drive");
    assert!(loose.starts_with("! 1 collection track(s)") && loose.ends_with("e.g. Demo Track 2"), "{out}");
    assert!(!env.store.exists(), "comparing makes no store");
}

#[test]
fn the_collection_is_exported_to_a_folder_and_pulled() {
    let env = Env::new();
    let db = master_db(&env);
    let music = env.outside("Music");
    std::fs::create_dir_all(&music).unwrap();
    for (id, name) in [("1", "Demo Track 1.mp3"), ("2", "Never Grow Old.mp3")] {
        std::fs::write(music.join(name), name.as_bytes()).unwrap();
        rusqlite::Connection::open(&db)
            .unwrap()
            .execute("UPDATE djmdContent SET FolderPath = ?1 WHERE ID = ?2", (path(&music.join(name)), id))
            .unwrap();
    }
    // The folder isn't there yet: a dry run leaves it that way, an export makes it.
    let folder = env.outside("rekordbox-export");
    let dry = collection(&env, &db, &["export", path(&folder), "--dry-run"]);
    assert!(dry.contains("2 track(s), 1 playlist(s)"), "{dry}");
    assert!(!folder.exists() && !env.store.exists(), "a dry run makes nothing");
    let r = env.run(&["collection", "--db", path(&db), "export", path(&env.outside("gone/rekordbox-export"))]);
    assert!(r.code != 0 && r.err.contains("the folder to export into (is its drive mounted?)"), "{}", r.err);

    let out = collection(&env, &db, &["export", path(&folder), "--yes"]);
    assert!(out.contains("rekordbox's collection -> rekordbox-export: 2 track(s), 1 playlist(s)"), "{out}");
    assert!(line(&out, "aren't exported: their audio isn't on this computer").starts_with("! 2 track(s)"), "{out}");
    assert!(line(&out, "intelligent playlist").contains("Recent"), "{out}");
    assert!(out.contains("exported 2 track(s) to rekordbox-export"), "{out}");
    assert!(folder.join("Contents/Floorplan/UnknownAlbum/Never Grow Old.mp3").is_file());

    let again = collection(&env, &db, &["export", path(&folder), "--dry-run"]);
    assert!(again.contains("copy 0 file(s), 0 B; remove 0 file(s), 0 B"), "{again}");
    let out = env.ok(&["pull", path(&folder), "--yes"]);
    assert!(out.contains("snapshot") && out.contains("2 tracks"), "{out}");
}

/// rekordbox 7's collection as a cue edit finds it ([`writable`]), encrypted and in WAL mode as
/// rekordbox keeps it, in a folder of its own.
fn writable_db(env: &Env) -> PathBuf {
    use loclib_rekordbox_db::masterdb::DEFAULT_KEY;
    use loclib_rekordbox_db::test_util::{codec, writable};

    let db = env.outside("rekordbox/master.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let mut bytes = writable().plain();
    bytes[18..20].copy_from_slice(&[2, 2]);
    std::fs::write(&db, codec(DEFAULT_KEY).encrypt(&bytes).unwrap()).unwrap();
    db
}

/// A run of `locallibrary` on the collection at `db`, in a home of its own: no agent's log there
/// says Cloud Library Sync is off, so a write is refused (or waits, should rekordbox run here).
fn on_collection(env: &Env, db: &Path, args: &[&str]) -> (bool, String, String) {
    std::fs::create_dir_all(env.outside("home")).unwrap();
    let out = env
        .command()
        .env("HOME", env.outside("home"))
        .env("LOCALLIBRARY_MASTERDB", db)
        .env_remove("LOCALLIBRARY_MASTERDB_KEY")
        .args(args)
        .output()
        .unwrap();
    let text = |b: Vec<u8>| String::from_utf8(b).unwrap();
    (out.status.success(), text(out.stdout), text(out.stderr))
}

/// One cue edit of the collection waiting in the store: hot cue C on [`writable`]'s first track.
fn staged(db: &Path) -> serde_json::Value {
    let set = json!({ "hot_cue": "C", "at": { "ms": 3000 } });
    let track = json!({ "id": "84935471", "name": "Daft Punk - One More Time", "after": [] });
    let change = json!({ "kind": "cues", "tracks": [track], "change": { "set": set } });
    json!([{ "id": "s1", "at": "2026-11-26T12:00:00Z", "db": db, "change": change }])
}

#[test]
fn repair_shows_the_lists_it_would_rewrite_and_writes_only_over_a_backup() {
    let env = Env::new();
    let db = writable_db(&env);
    let before = std::fs::read(&db).unwrap();
    let (ok, out, err) = on_collection(&env, &db, &["collection", "repair", "--dry-run"]);
    assert!(ok && out.contains("every track's cue list agrees with its cue rows"), "{out}{err}");

    let (ok, out, err) = on_collection(&env, &db, &["collection", "repair", "--create-missing", "--dry-run"]);
    assert!(ok, "{out}{err}");
    assert!(out.contains("rewrite the cue lists (contentCue) of 1 track(s)"), "{out}");
    assert!(out.contains("84935471  Daft Punk - One More Time"), "{out}");

    let (ok, out, err) = on_collection(&env, &db, &["--yes", "collection", "repair", "--create-missing"]);
    assert!(!ok, "refused with no agent's log to say sync is off, or rekordbox running\n{out}{err}");
    assert_eq!(std::fs::read(&db).unwrap(), before);
}

#[test]
fn apply_and_undo_take_the_edits_waiting_in_the_store() {
    let env = Env::new();
    let db = writable_db(&env);
    let before = std::fs::read(&db).unwrap();
    let (ok, out, err) = on_collection(&env, &db, &["collection", "apply"]);
    assert!(ok && out.contains("no edits of rekordbox's collection are waiting"), "{out}{err}");
    let (ok, out, err) = on_collection(&env, &db, &["collection", "undo"]);
    assert!(ok && out.contains("nothing to undo in rekordbox's collection"), "{out}{err}");

    let folder = env.store.join("collection");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("staged.json"), staged(&db).to_string()).unwrap();
    let (ok, out, err) = on_collection(&env, &db, &["collection", "apply", "--dry-run"]);
    assert!(ok && out.contains("set hot cue C on 1 track(s)"), "{out}{err}");
    let (ok, out, err) = on_collection(&env, &db, &["--yes", "collection", "apply"]);
    assert!(!ok || out.contains("still waiting"), "{out}{err}");
    assert_eq!(std::fs::read(&db).unwrap(), before);

    let (ok, out, err) = on_collection(&env, &db, &["collection", "undo", "--dry-run"]);
    assert!(ok && out.contains("drop set hot cue C on 1 track(s) (waiting, not written yet)"), "{out}{err}");
    let (ok, out, err) = on_collection(&env, &db, &["--yes", "collection", "undo"]);
    assert!(ok && out.contains("dropped it: it was never written"), "{out}{err}");
    assert_eq!(std::fs::read_to_string(folder.join("staged.json")).unwrap().trim(), "[]");

    // A write made earlier is put back as a new one, which is refused here too.
    let written = json!([{
        "id": "w1", "at": "2026-11-26T12:00:00Z", "db": db, "what": "set hot cue C on 1 track(s)",
        "backup": env.outside("backup.db"), "count": [1, 2], "before": [], "after": []
    }]);
    std::fs::write(folder.join("written.json"), written.to_string()).unwrap();
    let (ok, out, err) = on_collection(&env, &db, &["--yes", "collection", "undo"]);
    assert!(!ok || out.contains("still waiting"), "{out}{err}");
    assert_eq!(std::fs::read(&db).unwrap(), before);
}

#[test]
fn a_cue_edit_of_the_collection_is_planned_on_master_db() {
    let env = Env::new();
    let db = writable_db(&env);
    let before = std::fs::read(&db).unwrap();
    let edit = ["cues", "set", "collection", "--track", "84935471", "--hot", "C", "--at", "3"];
    let (ok, out, err) = on_collection(&env, &db, &[&edit[..], &["--dry-run"]].concat());
    assert!(ok && out.contains("master.db, over a backup"), "{out}{err}");
    let (ok, out, err) = on_collection(&env, &db, &[&["--yes"], &edit[..]].concat());
    assert!(!ok || out.contains("waiting"), "{out}{err}");
    assert_eq!(std::fs::read(&db).unwrap(), before);
}
