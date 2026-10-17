//! OneLibrary (`exportLibrary.db`): reading SQLCipher files and their WAL, OneLibrary-only drives,
//! and keeping the database and its WAL together through syncs, edits and restores.

use crate::common::{self, media::png, media::set_artwork};

use rustc_hash::FxHashMap;
use std::fs;

use std::path::Path;

use locallibrary::artwork;
use locallibrary::compare::{self, Relation, Side};
use locallibrary::device::Device;
use locallibrary::merge::{self, MergeOptions};
use locallibrary::onelibrary::{self, OneLibrary};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, SyncOptions};
use locallibrary::xml::{self, XmlOptions};
use locallibrary::{Library, backup, paths};

fn fixture(name: &str) -> Vec<u8> {
    fs::read(common::fixtures().join("onelibrary").join(name)).unwrap()
}

fn titles(lib: &Library) -> Vec<&str> {
    lib.tracks.iter().map(|t| t.title.as_str()).collect()
}

fn read(root: &Path, key: &str) -> Option<Vec<u8>> {
    fs::read(root.join(key)).ok()
}

#[test]
fn reads_sqlcipher_databases_and_their_wal() {
    let (db, wal) = (fixture("wal.db"), fixture("wal.db-wal"));
    let main_only = OneLibrary::from_encrypted(&db, None).unwrap().library().unwrap();
    assert_eq!(titles(&main_only), ["One"]);

    let lib = OneLibrary::from_encrypted(&db, Some(&wal)).unwrap().library().unwrap();
    assert_eq!(titles(&lib), ["One", "Two"], "the second track only exists in the WAL");
    let two = lib.track(2).unwrap();
    assert_eq!((two.artist.as_str(), two.key.as_str(), two.bpm, two.duration_secs), ("Beta", "", 124.0, 240));
    assert_eq!(two.color, Some(locallibrary::model::TrackColor::Orange));
    assert_eq!(two.file_path, "/Contents/Beta/Two.mp3");
    assert_eq!(two.analyze_path, "/PIONEER/USBANLZ/P002/00000002/ANLZ0000.DAT");
    assert_eq!(lib.track(1).unwrap().key, "Am");
    let set = lib.find_playlist("Set").unwrap();
    assert!(!set.is_folder);
    assert_eq!(set.track_ids, [1, 2]);

    // The decrypted database is ordinary SQLite.
    let (_, plain) = onelibrary::decrypt(&db, Some(&wal)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plain.db"), &plain).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("plain.db")).unwrap();
    let check: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(check, "ok");

    // Anything else is refused, not misread.
    assert!(OneLibrary::from_encrypted(b"sqlcipher", None).is_err());
    let mut bad = db.clone();
    bad[5000] ^= 1;
    assert!(OneLibrary::from_encrypted(&bad, None).is_err());
}

#[test]
fn edited_databases_keep_their_salt_and_journal_mode() {
    let db = fixture("wal.db");
    let mut one = OneLibrary::from_encrypted(&db, Some(&fixture("wal.db-wal"))).unwrap();
    let folder = one.create_playlist("Shows", 0, true).unwrap();
    let list = one.create_playlist("Friday", folder, false).unwrap();
    one.add_to_playlist(list, 2).unwrap();
    let bytes = one.to_encrypted().unwrap();
    assert_eq!(&bytes[..16], &db[..16], "same salt");
    let (_, plain) = onelibrary::decrypt(&bytes, None).unwrap();
    assert_eq!(plain[18], 2, "still a WAL-mode database, as rekordbox writes them");
    let lib = OneLibrary::from_encrypted(&bytes, None).unwrap().library().unwrap();
    assert_eq!(titles(&lib), ["One", "Two"]);
    assert_eq!(lib.playlist_path(lib.find_playlist("Friday").unwrap().id), "Shows/Friday");
    assert_eq!(lib.find_playlist("Friday").unwrap().track_ids, [2]);

    // SQLCipher itself must accept the file.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(paths::LIBRARY_PLUS_DB);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, &bytes).unwrap();
    common::sqlcipher_reads(dir.path());
}

#[test]
fn onelibrary_only_drives_are_exports() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let expected = Device::open_export(&root).unwrap().library().unwrap();
    common::onelibrary_only(&root);

    let mut a = Device::open_export(&root).unwrap();
    assert!(a.has_export() && a.has_library_plus() && !a.has_device_library());
    let lib = a.library().unwrap();
    assert_eq!(titles(&lib), titles(&expected));
    let aab = |l: &Library| l.find_playlist("aab").unwrap().track_ids.len();
    assert_eq!(aab(&lib), aab(&expected));
    let info = a.info();
    assert!(info.has_export && info.has_library_plus && !info.has_device_library && info.pdb.is_none());
    assert!(info.exported_at.is_some());
    assert_eq!(info.summary.unwrap().tracks, 12);
    assert!(a.scan_fingerprinted(Some(&lib), &Ctx::silent()).unwrap().fingerprint.is_some());

    // Snapshots of it can be read back and compared.
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let pulled = sync::pull(&store, &mut a, &PullOptions::default(), &Ctx::silent()).unwrap();
    let snap = store.load_snapshot(&pulled.snapshot.unwrap().id).unwrap();
    let side = Side::from_snapshot(&store, &snap, "store").unwrap();
    assert_eq!(side.library.tracks.len(), 12);
    let now = Side::from_device(&a, &Ctx::silent()).unwrap();
    assert_eq!(compare::compare(&side, &now).relation, Relation::Identical);

    // Mirrored to a blank drive, byte for byte.
    fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(common::library_files(&root), common::library_files(b.root()));
    assert_eq!(compare::compare_devices(&a, &b, &Ctx::silent()).unwrap().relation, Relation::Identical);

    // And exported as rekordbox XML.
    let (text, report) = xml::export(&a, &XmlOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(report.tracks, 12);
    assert!(text.contains("aab"));
}

#[test]
fn sync_never_pairs_a_database_with_another_wal() {
    let dir = tempfile::tempdir().unwrap();
    let opts = SyncOptions::default();
    let (db, wal, empty) = (fixture("wal.db"), fixture("wal.db-wal"), fixture("empty.db"));
    let drive = |name: &str, db: &[u8], wal: Option<&[u8]>| {
        let root = common::bigger_drive(dir.path(), name);
        fs::write(root.join(paths::LIBRARY_PLUS_DB), db).unwrap();
        if let Some(wal) = wal {
            fs::write(root.join(paths::LIBRARY_PLUS_WAL), wal).unwrap();
        }
        // What a player or rekordbox leaves behind after opening the database.
        fs::write(root.join(paths::LIBRARY_PLUS_SHM), b"shared memory").unwrap();
        root
    };
    let a_root = drive("A", &db, Some(&wal));
    let mut a = Device::open_export(&a_root).unwrap();

    // The shared-memory index is neither synced nor part of the fingerprint.
    let fp = |d: &Device| d.scan_fingerprinted(None, &Ctx::silent()).unwrap().fingerprint;
    let before = fp(&a);
    fs::write(a_root.join(paths::LIBRARY_PLUS_SHM), b"rewritten by a player").unwrap();
    assert_eq!(fp(&a), before);

    let check = |root: &Path| {
        assert_eq!(read(root, paths::LIBRARY_PLUS_DB).unwrap(), db);
        assert_eq!(read(root, paths::LIBRARY_PLUS_WAL).unwrap(), wal);
        assert!(!root.join(paths::LIBRARY_PLUS_SHM).exists(), "stale -shm removed");
        assert_eq!(titles(&common::plus_library(root)), ["One", "Two"]);
    };
    let copied = |plan: &sync::SyncPlan| -> Vec<String> {
        plan.copies.iter().filter(|c| c.path.contains("exportLibrary")).map(|c| c.path.clone()).collect()
    };

    // A different database with its own WAL.
    let b_root = drive("B", &empty, Some(b"a WAL from another state"));
    let mut b = Device::open_export(&b_root).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut b, &opts, &Ctx::silent()).unwrap();
    check(&b_root);

    // Only the WAL differs.
    let c_root = drive("C", &db, Some(b"older WAL"));
    let mut c = Device::open_export(&c_root).unwrap();
    let (plan, _) = sync::plan_drive_to_drive(&a, &c, &opts, &Ctx::silent()).unwrap();
    assert_eq!(copied(&plan), [paths::LIBRARY_PLUS_WAL]);
    sync::sync_drive_to_drive(&mut a, &mut c, &opts, &Ctx::silent()).unwrap();
    check(&c_root);

    // Only the database differs: the identical WAL is rewritten after it all the same.
    let d_root = drive("D", &empty, Some(&wal));
    let mut d = Device::open_export(&d_root).unwrap();
    let (plan, _) = sync::plan_drive_to_drive(&a, &d, &opts, &Ctx::silent()).unwrap();
    assert_eq!(copied(&plan), [paths::LIBRARY_PLUS_DB, paths::LIBRARY_PLUS_WAL], "database first");
    sync::sync_drive_to_drive(&mut a, &mut d, &opts, &Ctx::silent()).unwrap();
    check(&d_root);

    // A checkpointed source: the target's WAL goes, even when extra files are kept.
    let mut one = OneLibrary::from_encrypted(&db, Some(&wal)).unwrap();
    one.create_playlist("Checkpointed", 0, false).unwrap();
    let checkpointed = one.to_encrypted().unwrap();
    let e_root = drive("E", &checkpointed, None);
    let mut e = Device::open_export(&e_root).unwrap();
    let keep = SyncOptions { delete: false, ..Default::default() };
    let f_root = drive("F", &db, Some(&wal));
    let mut f = Device::open_export(&f_root).unwrap();
    let (plan, _) = sync::plan_drive_to_drive(&e, &f, &keep, &Ctx::silent()).unwrap();
    assert!(plan.deletes.iter().any(|d| d.path == paths::LIBRARY_PLUS_WAL));
    sync::sync_drive_to_drive(&mut e, &mut f, &keep, &Ctx::silent()).unwrap();
    assert!(!f_root.join(paths::LIBRARY_PLUS_WAL).exists());
    assert!(common::plus_library(&f_root).find_playlist("Checkpointed").is_some());
}

#[test]
fn edits_replace_the_wal_and_restores_bring_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::wal_drive(dir.path(), "A");
    fs::write(root.join(paths::LIBRARY_PLUS_SHM), b"shared memory").unwrap();
    let (db, wal) = (read(&root, paths::LIBRARY_PLUS_DB).unwrap(), read(&root, paths::LIBRARY_PLUS_WAL).unwrap());
    let mut d = Device::open_export(&root).unwrap();
    assert_eq!(titles(&d.library().unwrap()), ["One", "Two"]);

    // Track 2 lives only in the WAL; setting its artwork must keep it.
    let r = set_artwork(&mut d, &[2], &png(300, 200, 10), &Ctx::silent()).unwrap();
    assert!(r.databases.iter().any(|f| f == "exportLibrary.db"), "{r:?}");
    assert!(!root.join(paths::LIBRARY_PLUS_WAL).exists() && !root.join(paths::LIBRARY_PLUS_SHM).exists());
    common::sqlcipher_reads(&root);
    let lib = d.library().unwrap();
    assert_eq!(titles(&lib), ["One", "Two"]);
    let key = paths::key(lib.track(2).unwrap().artwork_path.as_ref().unwrap());
    assert_eq!(key, "PIONEER/Artwork/00001/b1.jpg");
    assert_eq!(image::open(d.path(&key)).unwrap().width(), 80);
    assert_eq!(image::open(d.path(&artwork::medium_variant(&key))).unwrap().width(), 240);
    assert!(lib.track(1).unwrap().artwork_id.is_none());

    // The backup holds the database and its WAL; restoring puts both back.
    let saved = backup::list(&d).unwrap().remove(0);
    assert!(saved.files.iter().any(|f| f == paths::LIBRARY_PLUS_WAL));
    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, None, false).unwrap();
    assert_eq!(read(&root, paths::LIBRARY_PLUS_DB).unwrap(), db);
    assert_eq!(read(&root, paths::LIBRARY_PLUS_WAL).unwrap(), wal);
    assert!(d.library().unwrap().track(2).unwrap().artwork_id.is_none());
}

#[test]
fn artwork_goes_into_both_databases() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let by_path: FxHashMap<String, u32> =
        common::plus_library(&root).tracks.iter().map(|t| (t.file_path.clone(), t.id)).collect();
    let ids: Vec<u32> = lib.tracks.iter().take(2).map(|t| t.id).collect();

    let database = |f: &String| f == "export.pdb" || f == "exportLibrary.db";
    let r = set_artwork(&mut d, &ids, &png(300, 200, 20), &Ctx::silent()).unwrap();
    assert_eq!(r.databases.iter().filter(|f| database(f)).count(), 2, "one new row in each database");
    common::sqlcipher_reads(&root);
    let pdb = d.library().unwrap();
    let plus = common::plus_library(&root);
    for id in &ids {
        let path = &lib.track(*id).unwrap().file_path;
        let a = pdb.track(*id).unwrap().artwork_path.clone().unwrap();
        let b = plus.track(by_path[path]).unwrap().artwork_path.clone().unwrap();
        assert!(a.contains("/a") && b.contains("/b"), "{a} {b}");
        assert_eq!(fs::read(d.path(&paths::key(&a))).unwrap(), fs::read(d.path(&paths::key(&b))).unwrap());
    }

    // Again on exactly those tracks: both images are replaced in place.
    let r = set_artwork(&mut d, &ids, &png(300, 200, 200), &Ctx::silent()).unwrap();
    assert!(!r.databases.iter().any(database), "{r:?}");
    assert_eq!(common::plus_library(&root).tracks.iter().filter(|t| t.artwork_id.is_some()).count(), 2);
}

#[test]
fn merge_adds_tracks_and_playlists_to_onelibrary() {
    let dir = tempfile::tempdir().unwrap();
    // Target: export.pdb and OneLibrary, as rekordbox 7 exports.
    let dst_root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&dst_root);
    let mut dst = Device::open_export(&dst_root).unwrap();
    // Source: a OneLibrary-only drive whose rows carry values export.pdb has no column for.
    let src_root = common::demo_drive(dir.path(), "Guest");
    common::onelibrary_only(&src_root);
    let one = OneLibrary::open_root(&src_root).unwrap();
    one.connection().execute("UPDATE content SET masterDbId = 777, contentLink = 4242", []).unwrap();
    one.connection()
        .execute("INSERT INTO cue (cue_id, content_id, kind, inUsec, outUsec) VALUES (1, 1, 0, 1500000, -1)", [])
        .unwrap();
    fs::write(src_root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let src = Device::open_export(&src_root).unwrap();

    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(plan.new_tracks.len(), 2);
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    let report = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(report.tracks_added, 2);

    common::sqlcipher_reads(&dst_root);
    let pdb = dst.library().unwrap();
    let plus = common::plus_library(&dst_root);
    assert_eq!((pdb.tracks.len(), plus.tracks.len()), (14, 14));
    let mut pdb_paths: Vec<&str> = pdb.tracks.iter().map(|t| t.file_path.as_str()).collect();
    let mut plus_paths: Vec<&str> = plus.tracks.iter().map(|t| t.file_path.as_str()).collect();
    pdb_paths.sort_unstable();
    plus_paths.sort_unstable();
    assert_eq!(pdb_paths, plus_paths);
    for lib in [&pdb, &plus] {
        assert!(lib.find_playlist("Guest").unwrap().is_folder);
        assert_eq!(lib.find_playlist("aab").unwrap().track_ids.len(), 12);
        for t in lib.tracks.iter().filter(|t| t.artist == "Loopmasters") {
            assert!(dst.path(&paths::key(&t.analyze_path)).is_file());
        }
    }
    // Copied whole from the source's OneLibrary row, cues included.
    let target = OneLibrary::open(&dst).unwrap();
    let conn = target.connection();
    let copied: i64 = conn
        .query_row("SELECT count(*) FROM content WHERE masterDbId = 777 AND contentLink = 4242", [], |r| r.get(0))
        .unwrap();
    assert_eq!(copied, 2);
    let cue: (i64, i64) = conn
        .query_row(
            "SELECT c.inUsec, c.content_id FROM cue c JOIN content t ON t.content_id = c.content_id WHERE t.masterDbId = 777",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(cue.0, 1_500_000);
    assert!(cue.1 > 12, "points at the new row");
    let count: i64 = conn.query_row("SELECT numberOfContents FROM property", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 14);

    // Undo restores both databases.
    backup::restore(&mut dst, report.backup.as_deref(), true).unwrap();
    assert_eq!(common::plus_library(&dst_root).tracks.len(), 12);
    assert_eq!(dst.library().unwrap().tracks.len(), 12);
}

#[test]
fn merge_into_a_onelibrary_only_drive() {
    let dir = tempfile::tempdir().unwrap();
    let dst_root = common::bigger_drive(dir.path(), "Main");
    common::onelibrary_only(&dst_root);
    let mut dst = Device::open_export(&dst_root).unwrap();
    // A source with only export.pdb: OneLibrary rows are built from it.
    let src = Device::open_export(common::demo_drive(dir.path(), "Guest")).unwrap();

    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let report = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!((report.tracks_added, report.playlists_created), (2, 1));
    common::sqlcipher_reads(&dst_root);
    let lib = dst.library().unwrap();
    assert_eq!(lib.tracks.len(), 14);
    assert!(lib.find_playlist("Guest").unwrap().is_folder);
    let one = OneLibrary::open(&dst).unwrap();
    let rows: Vec<(String, i64, i64)> = one
        .connection()
        .prepare("SELECT fileName, fileType, analysedBits FROM content WHERE content_id > 12")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    for (name, file_type, bits) in rows {
        assert!(!name.is_empty() && !name.contains('/'));
        assert!(file_type > 0, "{name}");
        assert_eq!(bits, 105, "like the target's other analysed tracks");
    }
    assert!(!dst.has_device_library(), "no export.pdb appears");
}

#[test]
fn checkpoints_and_reencryption_keep_the_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::wal_drive(dir.path(), "A");
    let b = common::wal_drive(dir.path(), "B");
    // B holds the same library after a checkpoint: one re-encrypted file, no WAL (as a player
    // closing the database may leave it).
    let one = OneLibrary::open_root(&b).unwrap();
    fs::write(b.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    fs::remove_file(b.join(paths::LIBRARY_PLUS_WAL)).unwrap();
    assert_ne!(read(&a, paths::LIBRARY_PLUS_DB), read(&b, paths::LIBRARY_PLUS_DB));
    let fp = |root: &Path| Device::open(root).unwrap().scan_fingerprinted(None, &Ctx::silent()).unwrap().fingerprint;
    assert_eq!(fp(&a), fp(&b));
    let (da, db) = (Device::open_export(&a).unwrap(), Device::open_export(&b).unwrap());
    assert_eq!(compare::compare_devices(&da, &db, &Ctx::silent()).unwrap().relation, Relation::Identical);

    // A real change does change it.
    let mut one = OneLibrary::open_root(&b).unwrap();
    one.create_playlist("New", 0, false).unwrap();
    fs::write(b.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    assert_ne!(fp(&a), fp(&b));
}

#[test]
fn private_rekordbox_files_stay_on_their_drive() {
    let dir = tempfile::tempdir().unwrap();
    let key = "PIONEER/extracted/gcred.dat";
    let a_root = common::bigger_drive(dir.path(), "A");
    fs::create_dir_all(a_root.join("PIONEER/extracted")).unwrap();
    fs::write(a_root.join(key), b"token of A").unwrap();
    let mut a = Device::open_export(&a_root).unwrap();

    // A drive with its own: kept. A blank drive: doesn't get A's.
    let b_root = common::bigger_drive(dir.path(), "B");
    fs::create_dir_all(b_root.join("PIONEER/extracted")).unwrap();
    fs::write(b_root.join(key), b"token of B").unwrap();
    common::export_new_track(&a_root, "New", "Track");
    let mut b = Device::open_export(&b_root).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(read(&b_root, key).unwrap(), b"token of B");
    fs::create_dir(dir.path().join("C")).unwrap();
    let mut c = Device::open(dir.path().join("C")).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut c, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert!(!c.path(key).exists());

    // Nor does it go into the store.
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let pulled = sync::pull(&store, &mut a, &PullOptions::default(), &Ctx::silent()).unwrap();
    let snap = store.load_snapshot(&pulled.snapshot.unwrap().id).unwrap();
    assert!(snap.files.iter().all(|f| !f.path.contains("extracted")));
}

#[test]
fn rekordbox_playlist_sync_is_reported_and_warned_about() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    fs::write(
        root.join(paths::PLAYLIST_SYNC_PLUS),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<Sync DBID=\"-1940252023\" AutomaticSync=\"1\" AllPlaylists=\"0\">\r\n\
         <Playlists>\r\n<NODE Id=\"0\" ParentId=\"0\" Attribute=\"1\"/>\r\n<NODE Id=\"30D40\" ParentId=\"0\" Attribute=\"0\"/>\r\n\
         </Playlists>\r\n</Sync>\r\n",
    )
    .unwrap();
    let dst = Device::open_export(&root).unwrap();
    let sync = &dst.info().rekordbox_sync;
    assert_eq!(sync.len(), 1);
    assert!(sync[0].automatic && sync[0].library == "onelibrary");
    assert_eq!((sync[0].collection, sync[0].playlists), (Some(2_354_715_273), 2));

    let src = Device::open_export(common::demo_drive(dir.path(), "Guest")).unwrap();
    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("syncs 2 playlist(s)")), "{:?}", plan.warnings);
}

#[test]
fn merge_brings_history_hot_cue_bank_lists_and_my_tags() {
    let dir = tempfile::tempdir().unwrap();
    // Source: both databases, a club's history, and (in OneLibrary) a cue in a hot cue bank list and
    // a My Tag on track 1.
    let src_root = common::demo_drive(dir.path(), "Guest");
    common::add_history(&src_root, "HISTORY 2026-10-04", &[2, 1]);
    common::add_onelibrary(&src_root);
    let one = OneLibrary::open_root(&src_root).unwrap();
    one.connection()
        .execute_batch(
            "INSERT INTO cue (cue_id, content_id, kind, inUsec) VALUES (5, 1, 0, 2000000);
             INSERT INTO hotCueBankList VALUES (1, 0, 'Bank', NULL, 0, 0);
             INSERT INTO hotCueBankList_cue VALUES (1, 5, 1);
             INSERT INTO myTag VALUES (1, 0, 'Mood', 1, 0), (2, 0, 'Dark', 0, 1);
             INSERT INTO myTag_content VALUES (2, 1);",
        )
        .unwrap();
    fs::write(src_root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let src = Device::open_export(&src_root).unwrap();
    let dst_root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&dst_root);
    let mut dst = Device::open_export(&dst_root).unwrap();

    let opts = MergeOptions { history: true, ..Default::default() };
    let plan = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    assert_eq!(plan.history, ["HISTORY 2026-10-04"]);
    let report = merge::apply(&plan, &src, &mut dst, &opts, &Ctx::silent()).unwrap();
    assert_eq!((report.history_playlists_added, report.hot_cue_bank_lists_added), (1, 1));

    let titles_in = |lib: &Library| -> Vec<String> {
        let h = lib.history.iter().find(|h| h.name == "HISTORY 2026-10-04").unwrap();
        h.track_ids.iter().map(|id| lib.track(*id).unwrap().title.clone()).collect()
    };
    common::sqlcipher_reads(&dst_root);
    let pdb = dst.library().unwrap();
    let plus = common::plus_library(&dst_root);
    assert_eq!(titles_in(&pdb), ["Demo Track 2", "Demo Track 1"]);
    assert_eq!(titles_in(&plus), titles_in(&pdb));

    let one = OneLibrary::open(&dst).unwrap();
    let conn = one.connection();
    let new_id = plus.tracks.iter().find(|t| t.title == "Demo Track 1").unwrap().id;
    let (cue, content): (i64, i64) = conn
        .query_row(
            "SELECT c.cue_id, c.content_id FROM hotCueBankList l JOIN hotCueBankList_cue lc
               ON lc.hotCueBankList_id = l.hotCueBankList_id JOIN cue c ON c.cue_id = lc.cue_id WHERE l.name = 'Bank'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(content, i64::from(new_id), "the bank list points at the copied cue");
    assert!(cue > 0);
    let folder: String = conn
        .query_row(
            "SELECT p.name FROM hotCueBankList l JOIN hotCueBankList p ON p.hotCueBankList_id = l.hotCueBankList_id_parent
             WHERE l.name = 'Bank'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(folder, "Guest");
    let tag: (String, String) = conn
        .query_row(
            "SELECT c.name, t.name FROM myTag_content mc JOIN myTag t ON t.myTag_id = mc.myTag_id
             JOIN myTag c ON c.myTag_id = t.myTag_id_parent WHERE mc.content_id = ?1",
            [new_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(tag, ("Mood".to_string(), "Dark".to_string()));

    // Merging again brings nothing new, history included.
    let again = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    assert!(again.history.is_empty() && again.new_tracks.is_empty(), "{:?}", again.history);
}

/// exportLibrary.db's key is public, so a drive's can hold anything SQLite does. A view or a
/// trigger (rekordbox makes neither) is refused, a column whose name holds a quote is still
/// quoted right, and tracks aren't removed while a table rekordbox 7 lacks points at them.
#[test]
fn a_drives_schema_is_not_trusted() {
    let lib =
        locallibrary::pdb::read_library(&common::fixtures().join("rekordcrate/demo_tracks").join(paths::EXPORT_PDB))
            .unwrap();
    let one = common::onelibrary_for(&lib);
    let trusted: i64 = one.connection().query_row("PRAGMA trusted_schema", [], |r| r.get(0)).unwrap();
    assert_eq!(trusted, 0);
    for sql in [
        "CREATE TRIGGER keep AFTER DELETE ON content BEGIN DELETE FROM playlist; END",
        "CREATE VIEW loop AS WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) SELECT i FROM n",
    ] {
        let crafted = common::onelibrary_for(&lib);
        crafted.connection().execute_batch(sql).unwrap();
        let err = OneLibrary::from_encrypted(&crafted.to_encrypted().unwrap(), None).unwrap_err().to_string();
        assert!(err.contains("which rekordbox never makes"), "{err}");
    }

    let mut one = one;
    let id = one.library().unwrap().tracks[0].id;
    one.connection()
        .execute_batch("ALTER TABLE content ADD COLUMN \"odd\"\"name\" integer; UPDATE content SET \"odd\"\"name\" = 7")
        .unwrap();
    let rec = one.export_track(id).unwrap().unwrap();
    let copy = one.add_track(&rec, "/Contents/copy.mp3", "", None).unwrap();
    let odd: i64 = one
        .connection()
        .query_row("SELECT \"odd\"\"name\" FROM content WHERE content_id = ?1", [copy], |r| r.get(0))
        .unwrap();
    assert_eq!(odd, 7);

    one.connection()
        .execute_batch("CREATE TABLE futureCue (futureCue_id integer primary key, content_id integer)")
        .unwrap();
    let err = one.remove_tracks(&[copy]).unwrap_err().to_string();
    assert!(err.contains("futureCue") && err.contains("content_id"), "{err}");
    one.connection().execute_batch("DROP TABLE futureCue; CREATE TABLE futureMenu (name varchar)").unwrap();
    one.remove_tracks(&[copy]).unwrap();
}
