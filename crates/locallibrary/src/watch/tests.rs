use super::*;

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn roots(events: &[WatchEvent]) -> Vec<(&'static str, String)> {
    events
        .iter()
        .map(|e| match e {
            WatchEvent::StoreChanged { changed, .. } => (e.kind(), changed.join(",")),
            _ => (e.kind(), e.drive().unwrap().name.clone()),
        })
        .collect()
}

#[test]
fn reports_drives_attached_changed_and_detached() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = dir.path().join("volumes");
    std::fs::create_dir_all(volumes.join("Old")).unwrap();
    let mut w = VolumeWatcher::in_folders(vec![volumes.clone()]);
    assert_eq!(w.drives().len(), 1, "what's mounted at the start is known");
    assert!(w.poll().is_empty(), "and not reported");

    // Plugged in: a drive with an export.
    let a = volumes.join("A");
    write(&a.join(paths::EXPORT_PDB), b"one");
    let events = w.poll();
    assert_eq!(roots(&events), [("drive.attached", "A".to_string())]);
    let WatchEvent::Attached(d) = &events[0] else { unreachable!() };
    assert!(d.has_export && d.id.is_none());
    assert_eq!(d.root, crate::device::dunce_canonicalize(&a));
    assert!(w.poll().is_empty(), "nothing changed since");

    // Its database rewritten, then its identity given.
    write(&a.join(paths::EXPORT_PDB), b"two, longer");
    assert_eq!(
        w.poll(),
        [WatchEvent::Changed { drive: d.clone(), changed: vec![DriveChange::Databases] }],
        "a rewritten database"
    );
    let id = Uuid::new_v4();
    let state = serde_json::json!({ "format": 1, "id": id, "name": "Main", "created": "2026-10-05T00:00:00Z" });
    write(&a.join(".locallibrary/state.json"), state.to_string().as_bytes());
    let events = w.poll();
    let WatchEvent::Changed { drive, changed } = &events[0] else { panic!("{events:?}") };
    assert_eq!((drive.id, drive.name.as_str(), changed.as_slice()), (Some(id), "Main", &[DriveChange::State][..]));

    // Unplugged: the drive as it was last seen.
    std::fs::remove_dir_all(&a).unwrap();
    std::fs::remove_dir_all(volumes.join("Old")).unwrap();
    assert_eq!(roots(&w.poll()), [("drive.detached", "Main".to_string()), ("drive.detached", "Old".to_string())]);
    assert!(w.drives().is_empty());
}

/// Folders are looked in the way `$LOCALLIBRARY_VOLUMES` is: a folder given by two names
/// holds each drive once.
#[test]
fn a_folder_given_twice_holds_each_drive_once() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = dir.path().join("volumes");
    write(&volumes.join("A").join(paths::EXPORT_PDB), b"one");
    let w = VolumeWatcher::in_folders(vec![volumes.clone(), volumes.join("../volumes")]);
    assert_eq!(w.drives().len(), 1);
}

#[test]
fn reports_the_store_changing() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    std::fs::create_dir_all(store.join("snapshots")).unwrap();
    let mut w = VolumeWatcher::in_folders(vec![]).with_store(&store);
    assert!(w.poll().is_empty());
    write(&store.join("libraries.json"), b"[]");
    write(&store.join("drives.json"), b"{}");
    assert_eq!(roots(&w.poll()), [("store.changed", "libraries,drives".to_string())]);
    // A new snapshot: the folder's listing changes.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(&store.join("snapshots/abc.json"), b"{}");
    assert_eq!(roots(&w.poll()), [("store.changed", "snapshots".to_string())]);
    write(&store.join(crate::sets::SETS_FILE), b"{}");
    assert_eq!(roots(&w.poll()), [("store.changed", "sets".to_string())]);
    assert!(w.poll().is_empty());
}

#[test]
fn stamps_tell_rewritten_databases_apart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(!LibraryStamp::of(root).has_export());
    write(&root.join(paths::LIBRARY_PLUS_DB), b"x");
    let before = LibraryStamp::of(root);
    assert!(before.has_export());
    assert_eq!(LibraryStamp::of(root), before);
    // Replaced by a file of the same size, as a sync renames a new one into place.
    write(&root.join("new"), b"y");
    std::fs::rename(root.join("new"), root.join(paths::LIBRARY_PLUS_DB)).unwrap();
    assert_ne!(LibraryStamp::of(root), before);
}

/// Regression: a watcher stamping a drive's database while another thread renamed a new one over
/// it failed the rename on Windows ("Access is denied"), so an edit failed now and then while a
/// GUI or `watch` polled.
#[test]
fn stamps_taken_while_another_thread_writes_never_fail_the_write() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(paths::EXPORT_PDB);
    write(&db, b"0");
    let done = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            while !done.load(std::sync::atomic::Ordering::SeqCst) {
                FileStamp::of(&db);
            }
        });
        for i in 0..2000 {
            // Without an fsync each: the renames are what's tested.
            crate::fsutil::write_deferred(&db, i.to_string().as_bytes()).unwrap();
        }
        done.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    assert_eq!(std::fs::read(&db).unwrap(), b"1999");
}

#[test]
fn events_serialize_flat() {
    let d = MountedDrive { root: "/Volumes/A".into(), id: None, name: "A".into(), has_export: true };
    let e = WatchEvent::Changed { drive: d.clone(), changed: vec![DriveChange::Databases] };
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        serde_json::json!({ "event": "drive.changed", "root": "/Volumes/A", "id": null, "name": "A", "has_export": true, "changed": ["databases"] })
    );
    assert_eq!(serde_json::to_value(WatchEvent::Attached(d)).unwrap()["event"], "drive.attached");
}
