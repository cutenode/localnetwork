use super::*;
use crate::paths::FileClass;

#[test]
fn init_import_snapshot_history_gc() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("s")).unwrap();
    let _lock = store.lock().unwrap();
    assert!(matches!(store.lock(), Err(Error::Locked(_))));

    let src = dir.path().join("a.bin");
    fs::write(&src, b"abc").unwrap();
    let (h, new) = store.import_file(&src, None, &Ctx::silent()).unwrap();
    assert!(new);
    assert_eq!(store.import_file(&src, Some(h), &Ctx::silent()).unwrap(), (h, false));
    let orphan = store.import_bytes(b"orphan").unwrap();

    let lib = store.create_library("Main").unwrap();
    assert!(store.create_library("main").is_err());
    let entry = FileEntry { path: "Contents/a.mp3".into(), size: 3, mtime: 0, class: FileClass::Audio, hash: Some(h) };
    let mk = |parents: Vec<Hash>, msg: &str| Snapshot {
        id: Hash::of(b""),
        library_id: lib.id,
        parents,
        created: Utc::now(),
        message: Some(msg.into()),
        source: None,
        fingerprint: Hash::of(msg.as_bytes()),
        summary: LibrarySummary::default(),
        lineage: vec![],
        files: vec![entry.clone()],
    };
    let s1 = store.save_snapshot(mk(vec![], "one")).unwrap();
    let s2 = store.save_snapshot(mk(vec![s1.id], "two")).unwrap();
    store.set_head(lib.id, s2.id).unwrap();
    let lib = store.find_library("MAIN").unwrap().unwrap();
    let hist = store.history(&lib, None).unwrap();
    assert_eq!(hist.iter().map(|s| s.message.clone().unwrap()).collect::<Vec<_>>(), ["two", "one"]);
    assert!(store.is_ancestor(&s1.id, &s2.id).unwrap());
    assert!(!store.is_ancestor(&s2.id, &s1.id).unwrap());
    assert_eq!(store.resolve_snapshot(&s1.id.short()).unwrap(), s1.id);

    let gc = store.gc(false).unwrap();
    assert_eq!(gc.removed_objects, 1);
    assert!(!store.has_object(&orphan));
    assert!(store.has_object(&h));
    assert!(store.verify(&Ctx::silent()).unwrap().corrupt.is_empty());
}

#[test]
fn a_label_keeps_a_snapshot_of_no_library() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("s")).unwrap();
    let lib = store.create_library("Main").unwrap();
    let h = store.import_bytes(b"db").unwrap();
    let entry =
        FileEntry { path: "PIONEER/x.pdb".into(), size: 2, mtime: 0, class: FileClass::Database, hash: Some(h) };
    let snapshot = |library_id: Uuid, fp: &[u8]| Snapshot {
        id: Hash::of(b""),
        library_id,
        parents: vec![],
        created: Utc::now(),
        message: None,
        source: None,
        fingerprint: Hash::of(fp),
        summary: LibrarySummary::default(),
        lineage: vec![],
        files: vec![entry.clone()],
    };
    // Beside the library's latest (it has none), as a drive back from a set is saved: it
    // stays, whatever the labels.
    let beside = store.save_snapshot(snapshot(lib.id, b"beside")).unwrap();
    let lone = store.save_snapshot(snapshot(Uuid::new_v4(), b"fp")).unwrap();
    store.add_label(&lone.id, "keep me").unwrap();
    assert_eq!(store.gc(false).unwrap().removed_snapshots, 0);
    assert!(store.has_object(&h));
    assert_eq!(store.resolve_snapshot("KEEP ME").unwrap(), lone.id);
    store.remove_label("keep me").unwrap();
    let gc = store.gc(false).unwrap();
    assert_eq!((gc.removed_snapshots, gc.removed_objects), (1, 0), "the beside snapshot uses the file too");
    assert!(store.add_label(&lone.id, "too late").is_err());
    assert!(store.load_snapshot(&beside.id).is_ok());
}

/// A library's snapshots, oldest first, each on the last, the newest its head.
fn chain(store: &Store, lib: &LibraryRecord, n: u8) -> Vec<Snapshot> {
    let mut out: Vec<Snapshot> = Vec::new();
    for i in 0..n {
        let h = store.import_bytes(&[i]).unwrap();
        let entry =
            FileEntry { path: format!("Contents/{i}.mp3"), size: 1, mtime: 0, class: FileClass::Audio, hash: Some(h) };
        let s = store
            .save_snapshot(Snapshot {
                id: Hash::of(b""),
                library_id: lib.id,
                parents: out.last().map(|s| s.id).into_iter().collect(),
                created: Utc::now(),
                message: None,
                source: None,
                fingerprint: Hash::of(&[i]),
                summary: LibrarySummary::default(),
                lineage: vec![],
                files: vec![entry],
            })
            .unwrap();
        store.set_head(lib.id, s.id).unwrap();
        out.push(s);
    }
    out
}

/// Regression: a head whose manifest was missing kept nothing alive, so gc deleted the
/// library's history and its files.
#[test]
fn gc_keeps_a_library_whose_latest_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("s")).unwrap();
    let lib = store.create_library("Main").unwrap();
    let snaps = chain(&store, &lib, 3);
    fs::remove_file(store.snapshot_path(&snaps[2].id)).unwrap();
    let gc = store.gc(false).unwrap();
    assert_eq!((gc.removed_snapshots, gc.removed_objects), (0, 1), "only the missing snapshot's own file");
    for s in &snaps[..2] {
        assert!(store.load_snapshot(&s.id).is_ok());
        assert!(store.has_object(&s.files[0].hash.unwrap()));
    }
}

/// Regression: an object a crash cut short (renamed into place before it was flushed) was
/// trusted by every later import, as long as a file had its name.
#[test]
fn imports_replace_an_object_cut_short() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("s")).unwrap();
    let src = dir.path().join("a.bin");
    fs::write(&src, b"the whole file").unwrap();
    let (h, _) = store.import_file(&src, None, &Ctx::silent()).unwrap();
    let cut = |store: &Store| fs::write(store.object_path(&h), b"the wh").unwrap();
    let whole = |store: &Store| fs::read(store.object_path(&h)).unwrap() == b"the whole file";

    cut(&store);
    assert_eq!(store.import_file(&src, Some(h), &Ctx::silent()).unwrap(), (h, true), "a cached hash");
    assert!(whole(&store));
    cut(&store);
    assert_eq!(store.import_file(&src, None, &Ctx::silent()).unwrap(), (h, true), "hashed again");
    assert!(whole(&store));
    cut(&store);
    assert_eq!(store.import_bytes(b"the whole file").unwrap(), h);
    assert!(whole(&store));
    assert_eq!(store.import_file(&src, Some(h), &Ctx::silent()).unwrap(), (h, false), "whole: not read");
}

/// Regression: the registry was read and written back with no lock, so two writers at once
/// dropped each other's changes.
#[test]
fn registry_writers_at_once_keep_every_change() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("s")).unwrap();
    let id = Uuid::new_v4();
    store
        .update_drives(|d| {
            d.insert(id, DriveRecord::new(id, "Main", dir.path().to_path_buf()));
            Ok(())
        })
        .unwrap();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..25 {
                    crate::fleet::update(&store, id, |r| r.log.writes += 1).unwrap();
                }
            });
        }
    });
    assert_eq!(store.drives().unwrap()[&id].log.writes, 200);
}

/// History walks read manifests without their file lists: one listing a kind of file this
/// version doesn't know (written by a newer one) still walks.
#[test]
fn history_reads_manifests_without_their_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("s")).unwrap();
    let lib = store.create_library("Main").unwrap();
    let snaps = chain(&store, &lib, 2);
    let path = store.snapshot_path(&snaps[0].id);
    let text = fs::read_to_string(&path).unwrap().replace("\"audio\"", "\"stems\"");
    fs::write(&path, text).unwrap();
    assert!(store.load_snapshot(&snaps[0].id).is_err());
    let lib = store.find_library("Main").unwrap().unwrap();
    let hist = store.history(&lib, None).unwrap();
    assert_eq!(
        hist.iter().map(|s| (s.id, s.files, s.bytes)).collect::<Vec<_>>(),
        [(snaps[1].id, 1, 1), (snaps[0].id, 1, 1)]
    );
    assert_eq!(store.find_by_fingerprint(&lib, &snaps[0].fingerprint).unwrap().map(|s| s.id), Some(snaps[0].id));
}

/// A pull cut off by a crash leaves what it was writing behind: `gc` counts it with the files
/// nothing uses, and removes it.
#[test]
fn gc_removes_what_cut_off_imports_left() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("s")).unwrap();
    let _lock = store.lock().unwrap();
    let stray = store.import_bytes(b"abc").unwrap();
    let shard = store.object_path(&stray).parent().unwrap().to_path_buf();
    let left = [
        store.root().join("objects/tmp/import-x1y2"),
        shard.join(".locallibrary-tmp-ab"),
        store.root().join("snapshots/.locallibrary-tmp-cd"),
    ];
    for p in &left {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"half").unwrap();
    }
    // The stray object and the three leftovers.
    let plan = store.gc(true).unwrap();
    assert_eq!((plan.removed_objects, plan.freed_bytes), (4, 15));
    store.gc(false).unwrap();
    assert!(left.iter().all(|p| !p.exists()));
    assert_eq!(store.gc(true).unwrap().removed_objects, 0);
}

/// A store whose folders went read-only says so of the store, whichever file or folder in it was
/// being made (a new object's fan-out folder, a snapshot's temp file); a drive's file that can't
/// be read is still said of that file.
#[cfg(unix)]
#[test]
fn a_store_that_cant_be_written_says_so_of_the_store() {
    use std::os::unix::fs::PermissionsExt;
    let set_mode = |p: &Path, mode| fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("s")).unwrap();
    let src = dir.path().join("a.bin");
    fs::write(&src, b"abc").unwrap();
    let (h, _) = store.import_file(&src, None, &Ctx::silent()).unwrap();
    let lib = store.create_library("Main").unwrap();
    let fresh = (0u8..).map(|i| vec![i]).find(|b| !store.object_path(&Hash::of(b)).parent().unwrap().exists()).unwrap();
    let folders = ["objects", "snapshots"].map(|f| store.root().join(f));
    folders.iter().for_each(|f| set_mode(f, 0o555));
    // Root writes anyway.
    if fs::write(store.root().join("objects/probe"), "").is_ok() {
        return;
    }
    // Not the temp file it tried either, whose name is new each time.
    let said = |e: Error| match e {
        Error::Io { path, source } if !source.to_string().contains("at path") => (path, source.kind()),
        e => panic!("{e}"),
    };
    let denied = (store.root().to_path_buf(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(said(store.import_bytes(&fresh).unwrap_err()), denied);
    fs::write(&src, &fresh).unwrap();
    assert_eq!(said(store.import_file(&src, None, &Ctx::silent()).unwrap_err()), denied);
    let entry = FileEntry { path: "Contents/a.mp3".into(), size: 3, mtime: 0, class: FileClass::Audio, hash: Some(h) };
    let snapshot = Snapshot {
        id: Hash::of(b""),
        library_id: lib.id,
        parents: vec![],
        created: Utc::now(),
        message: None,
        source: None,
        fingerprint: Hash::of(b"one"),
        summary: LibrarySummary::default(),
        lineage: vec![],
        files: vec![entry],
    };
    assert_eq!(said(store.save_snapshot(snapshot).unwrap_err()), denied);
    set_mode(&src, 0o000);
    let unreadable = said(store.import_file(&src, None, &Ctx::silent()).unwrap_err());
    set_mode(&src, 0o644);
    folders.iter().for_each(|f| set_mode(f, 0o755));
    assert_eq!(unreadable, (src, std::io::ErrorKind::PermissionDenied));
}

#[test]
fn refuses_to_init_over_unrelated_directory() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("something.txt"), "hi").unwrap();
    assert!(Store::open_or_init(dir.path()).is_err());
}

/// What older versions wrote into the store's folder before making the store doesn't stop it
/// being made there.
#[test]
fn inits_over_what_locallibrary_left_there() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("cache/fingerprints/3a")).unwrap();
    fs::create_dir_all(dir.path().join("reports")).unwrap();
    fs::write(dir.path().join("profile-document.json"), "{}").unwrap();
    Store::open_or_init(dir.path()).unwrap();
    assert!(dir.path().join("store.json").is_file());
}
