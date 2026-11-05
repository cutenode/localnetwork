//! The store when its own files let it down: a record file that's broken fails exactly the reads
//! of it, a folder that's broken fails exactly the listings of it, a folder it can't write refuses
//! the change and leaves the records as they were, a clean-up stopped by a file it can't remove
//! fails and finishes once it can, an import it can't write is said of the store, and a check of
//! the stored files stops when asked and fails on one it can't read.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use chrono::Utc;
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::model::LibrarySummary;
use locallibrary::paths::FileClass;
use locallibrary::progress::{CancelToken, Ctx, NoProgress};
use locallibrary::store::{LibraryRecord, Snapshot, Store};
use locallibrary::tree::FileEntry;
use locallibrary::{Error, Hash, Result};
use uuid::Uuid;

fn set_mode(p: &Path, mode: u32) {
    fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
}

/// Whether this user writes into a folder it took write permission from (root does).
fn writes_anyway(dir: &Path) -> bool {
    let probe = dir.join("probe");
    fs::create_dir_all(&probe).unwrap();
    set_mode(&probe, 0o555);
    let wrote = fs::write(probe.join("x"), "").is_ok();
    set_mode(&probe, 0o755);
    fs::remove_dir_all(&probe).unwrap();
    wrote
}

/// A snapshot of `library` listing `object`, a child of `parents`.
fn snapshot(library: Uuid, parents: Vec<Hash>, object: Hash, fp: &[u8]) -> Snapshot {
    Snapshot {
        id: Hash::of(b"set on save"),
        library_id: library,
        parents,
        created: Utc::now(),
        message: None,
        source: None,
        fingerprint: Hash::of(fp),
        summary: LibrarySummary::default(),
        lineage: Vec::new(),
        files: vec![FileEntry {
            path: format!("Contents/{}.mp3", Hash::of(fp).short()),
            size: 5,
            mtime: 0,
            class: FileClass::Audio,
            hash: Some(object),
        }],
    }
}

/// A store with a library "Main" of two snapshots, the newest its head and labelled "kept", and
/// a drive and settings on record.
fn stocked(dir: &Path) -> (Store, LibraryRecord, Vec<Snapshot>) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let lib = store.create_library("Main").unwrap();
    let mut snaps: Vec<Snapshot> = Vec::new();
    for n in 0..2u8 {
        let object = store.import_bytes(&[b'a', n]).unwrap();
        let parents = snaps.last().map(|s| s.id).into_iter().collect();
        snaps.push(store.save_snapshot(snapshot(lib.id, parents, object, &[n])).unwrap());
    }
    store.set_head(lib.id, snaps[1].id).unwrap();
    store.add_label(&snaps[1].id, "kept").unwrap();
    store.save_settings_profile(&Default::default()).unwrap();
    store.save_player_models(&Default::default()).unwrap();
    store.save_smart_playlists(&Default::default()).unwrap();
    let id = Uuid::new_v4();
    store.update_drives(|d| Ok(d.insert(id, locallibrary::store::DriveRecord::new(id, "A", dir.into())))).unwrap();
    let lib = store.library_by_id(lib.id).unwrap().unwrap();
    (store, lib, snaps)
}

/// A four-hex-digit snapshot id prefix none of `snaps` has.
fn absent_prefix(snaps: &[Snapshot]) -> &'static str {
    ["0000", "ffff"].into_iter().find(|p| snaps.iter().all(|s| !s.id.to_hex().starts_with(p))).unwrap()
}

/// Every file of the store's top folder, with its bytes.
fn records(store: &Store) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = fs::read_dir(store.root())
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), fs::read(e.path()).unwrap()))
        .collect();
    out.sort();
    out
}

const LIBRARIES: &str = "libraries.json";
const LABELS: &str = "labels.json";
const PRUNED: &str = "pruned.json";
const DRIVES: &str = "drives.json";

/// Each read of the store's records, and the files it reads them from.
type Read<'a> = (&'static str, Vec<&'static str>, Box<dyn Fn() -> Result<()> + 'a>);

/// A record file that's broken (cut short, or not the JSON it should be) fails every read of it
/// with an error naming it, and nothing else: and it's left as it was.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_broken_record_file_fails_exactly_the_reads_of_it(tc: TestCase) {
    for file in [LIBRARIES, LABELS, PRUNED, DRIVES, "settings.json", "players.json", "smart.json", "store.json"] {
        let junk: &[u8] = tc.draw(gs::sampled_from(vec![&b""[..], b"{", b"not json", b"\"a string\""]));
        broken_record_fails_its_reads(file, junk);
    }
}

/// [`prop_a_broken_record_file_fails_exactly_the_reads_of_it`] for `file` holding `junk`.
fn broken_record_fails_its_reads(file: &'static str, junk: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    let (store, lib, snaps) = stocked(dir.path());
    fs::write(store.root().join(file), junk).unwrap();
    let absent = absent_prefix(&snaps);
    let (s, l) = (&store, &lib);
    // In this order: those that change the records (when they can) come last.
    let reads: Vec<Read<'_>> = vec![
        ("libraries", vec![LIBRARIES], Box::new(|| s.libraries().map(drop))),
        ("find_library", vec![LIBRARIES], Box::new(|| s.find_library("main").map(drop))),
        ("unused_library_name", vec![LIBRARIES], Box::new(|| s.unused_library_name("Main").map(drop))),
        ("library_by_id", vec![LIBRARIES], Box::new(|| s.library_by_id(l.id).map(drop))),
        ("stats", vec![LIBRARIES, DRIVES], Box::new(|| s.stats().map(drop))),
        ("history", vec![LABELS, PRUNED], Box::new(|| s.history(l, None).map(drop))),
        (
            "find_by_fingerprint",
            vec![LABELS, PRUNED],
            Box::new(|| s.find_by_fingerprint(l, &snaps[0].fingerprint).map(drop)),
        ),
        ("snapshot_info", vec![LABELS], Box::new(|| s.snapshot_info(&snaps[0]).map(drop))),
        ("labels_by_snapshot", vec![LABELS], Box::new(|| s.labels_by_snapshot().map(drop))),
        ("find_label", vec![LABELS], Box::new(|| s.find_label("KEPT").map(drop))),
        ("resolve_snapshot", vec![LABELS, PRUNED], Box::new(|| s.resolve_snapshot(absent).map(drop))),
        ("is_ancestor", vec![PRUNED], Box::new(|| s.is_ancestor(&snaps[0].id, &snaps[1].id).map(drop))),
        ("gc", vec![LIBRARIES, LABELS, PRUNED], Box::new(|| s.gc(true).map(drop))),
        ("drives", vec![DRIVES], Box::new(|| s.drives().map(drop))),
        ("forget_drive", vec![DRIVES], Box::new(|| s.forget_drive(Uuid::new_v4()).map(drop))),
        ("settings_profile", vec!["settings.json"], Box::new(|| s.settings_profile().map(drop))),
        ("player_models", vec!["players.json"], Box::new(|| s.player_models().map(drop))),
        ("smart_playlists", vec!["smart.json"], Box::new(|| s.smart_playlists().map(drop))),
        ("id", vec!["store.json"], Box::new(|| s.id().map(drop))),
        ("verify", vec![], Box::new(|| s.verify(&Ctx::silent()).map(drop))),
        ("add_label", vec![LABELS], Box::new(|| s.add_label(&snaps[0].id, "new").map(drop))),
        ("remove_label", vec![LABELS], Box::new(|| s.remove_label("new").map(drop))),
        ("create_library", vec![LIBRARIES], Box::new(|| s.create_library("New").map(drop))),
        ("set_head", vec![LIBRARIES], Box::new(|| s.set_head(l.id, snaps[1].id))),
        ("set_head of no library", vec![LIBRARIES], Box::new(|| s.set_head(Uuid::new_v4(), snaps[1].id))),
        ("rename_library", vec![LIBRARIES], Box::new(|| s.rename_library(l.id, "Main"))),
        ("rename no library", vec![LIBRARIES], Box::new(|| s.rename_library(Uuid::new_v4(), "Other"))),
        ("forget_snapshots", vec![LIBRARIES, LABELS, PRUNED], Box::new(|| s.forget_snapshots(&[]))),
        ("forget no snapshot", vec![LIBRARIES, LABELS, PRUNED], Box::new(|| s.forget_snapshots(&[Hash::of(b"none")]))),
    ];
    let broken = store.root().join(file);
    for (name, files, read) in reads {
        let failed = matches!(read(), Err(Error::Parse { path, .. }) if path == broken);
        assert_eq!(failed, files.contains(&file), "{name} with {file} holding {junk:?}");
    }
    assert_eq!(fs::read(&broken).unwrap(), junk, "a broken file is left as it was");
    // A store whose own file is broken is there, but can't be opened; a folder without one has none.
    assert_eq!(Store::open_existing(store.root()).is_err(), file == "store.json");
    assert!(Store::open_existing(dir.path()).unwrap().is_none());
}

/// A store folder that's gone, a file, or can't be read (and a lock file that's a folder) fails
/// exactly what lists or takes it, with an error naming it.
#[hegel::test(test_cases = 3)]
fn prop_a_broken_store_folder_fails_exactly_the_listings_of_it(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    // Which of its snapshots' files the unreadable fan-out folder holds.
    let which = tc.draw(gs::integers::<usize>().max_value(1));
    for broken in ["snapshots is a file", "snapshots is gone", "objects is a file", "shard", "lock is a folder"] {
        broken_folder_fails_its_listings(&dir.path().join(broken), broken, which);
    }
}

/// [`prop_a_broken_store_folder_fails_exactly_the_listings_of_it`] for a store in `dir` with
/// `broken`, the `which`th snapshot's fan-out folder the one that can't be read.
fn broken_folder_fails_its_listings(dir: &Path, broken: &str, which: usize) {
    let (store, _, snaps) = stocked(dir);
    let shard = store.object_path(&snaps[which].files[0].hash.unwrap()).parent().unwrap().to_path_buf();
    let (folder, bad) = match broken {
        "lock is a folder" => {
            let lock = store.root().join("lock");
            fs::remove_file(&lock).unwrap();
            fs::create_dir(&lock).unwrap();
            (lock, "lock")
        }
        "shard" if writes_anyway(dir) => return,
        "shard" => {
            set_mode(&shard, 0o000);
            (shard.clone(), "objects")
        }
        _ => {
            let folder = store.root().join(if broken.starts_with("objects") { "objects" } else { "snapshots" });
            fs::remove_dir_all(&folder).unwrap();
            if broken.ends_with("a file") {
                fs::write(&folder, b"").unwrap();
            }
            (folder, if broken.starts_with("objects") { "objects" } else { "snapshots" })
        }
    };
    let absent = absent_prefix(&snaps);
    let listings: [(&str, &[&str], Result<()>); 6] = [
        ("lock", &["lock"], store.lock().map(drop)),
        ("snapshot_ids", &["snapshots"], store.snapshot_ids().map(drop)),
        ("resolve_snapshot", &["snapshots"], store.resolve_snapshot(absent).map(drop)),
        ("verify", &["snapshots"], store.verify(&Ctx::silent()).map(drop)),
        ("stats", &["snapshots", "objects"], store.stats().map(drop)),
        ("gc", &["snapshots", "objects"], store.gc(true).map(drop)),
    ];
    if broken == "shard" {
        set_mode(&shard, 0o755);
    }
    for (name, reads, listed) in listings {
        let failed = matches!(&listed, Err(Error::Io { path, .. }) if *path == folder);
        assert_eq!(failed, reads.contains(&bad), "{name} with {broken}: {listed:?}");
    }
}

/// Every change to the store's records, in a store folder that can't be written, fails and
/// leaves every record as it was.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_store_folder_that_cant_be_written_refuses_the_change(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    if writes_anyway(dir.path()) {
        return;
    }
    // A drive recorded before takes the registry's lock file: with it, the write fails, without, the lock.
    let unlocked = tc.draw(gs::booleans());
    let name = tc.draw(gs::sampled_from(vec!["New", "Ünï", "main 2"]));
    for change in [
        "create_library",
        "set_head",
        "rename_library",
        "add_label",
        "remove_label",
        "forget_snapshots",
        "record a drive",
        "settings",
        "players",
        "smart",
    ] {
        let (store, lib, snaps) = stocked(&dir.path().join(change));
        if change == "record a drive" && unlocked {
            fs::remove_file(store.root().join("drives.lock")).unwrap();
        }
        let before = records(&store);
        set_mode(store.root(), 0o555);
        let id = Uuid::new_v4();
        let changed = match change {
            "create_library" => store.create_library(name).map(drop),
            "set_head" => store.set_head(lib.id, snaps[0].id),
            "rename_library" => store.rename_library(lib.id, name),
            "add_label" => store.add_label(&snaps[0].id, name).map(drop),
            "remove_label" => store.remove_label("kept").map(drop),
            "forget_snapshots" => store.forget_snapshots(&[snaps[0].id]),
            "record a drive" => store
                .update_drives(|d| Ok(d.insert(id, locallibrary::store::DriveRecord::new(id, name, dir.path().into()))))
                .map(drop),
            "settings" => store.save_settings_profile(&Default::default()),
            "players" => store.save_player_models(&locallibrary::equipment::Remembered::default()),
            _ => store.save_smart_playlists(&Default::default()),
        };
        set_mode(store.root(), 0o755);
        assert!(matches!(&changed, Err(Error::Io { .. })), "{change}: {changed:?}");
        assert_eq!(records(&store), before, "{change} left the records as they were");
    }
}

/// Removing a pruned snapshot's manifest that the folder won't let go of fails after its record is
/// written: the history still walks, and through the manifest that's still there.
#[hegel::test(test_cases = 5)]
fn prop_a_prune_stopped_halfway_keeps_history_whole(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    if writes_anyway(dir.path()) {
        return;
    }
    let (store, lib, snaps) = stocked(dir.path());
    let limit = tc.draw(gs::integers::<usize>().min_value(2).max_value(4));
    set_mode(&store.root().join("snapshots"), 0o555);
    let forgot = store.forget_snapshots(&[snaps[0].id]);
    set_mode(&store.root().join("snapshots"), 0o755);
    assert!(matches!(forgot, Err(Error::Io { .. })), "{forgot:?}");
    assert!(store.pruned().unwrap().contains_key(&snaps[0].id), "the record is written first");
    let walked: Vec<Hash> = store.history(&lib, Some(limit)).unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(walked, [snaps[1].id, snaps[0].id]);
}

/// A clean-up that meets something it can't remove (or a folder it can't list) fails, its plan
/// still says what it would remove, and once the folder lets go the next clean-up finishes it and
/// leaves nothing more to remove, the library's snapshots and files kept.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_clean_up_stopped_by_a_file_finishes_once_it_can_go(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    if writes_anyway(dir.path()) {
        return;
    }
    let mut stray = b"stray".to_vec();
    stray.extend(tc.draw(gs::binary().max_size(32)));
    for stuck in [
        "orphan snapshot",
        "orphan object",
        "cut-off import",
        "unread cache",
        "older index",
        "plugin scratch",
        "pruned record",
        "unlistable folder",
    ] {
        clean_up_stopped_by(&dir.path().join(stuck), stuck, &stray);
    }
}

/// [`prop_a_clean_up_stopped_by_a_file_finishes_once_it_can_go`] for what's `stuck`, in a store in
/// `dir`, `stray` the bytes of what nothing uses.
fn clean_up_stopped_by(dir: &Path, stuck: &str, stray: &[u8]) {
    let (store, lib, snaps) = stocked(dir);
    let root = store.root().to_path_buf();
    // What `gc` would remove, and the folder that won't let go of it.
    let (gone, locked, mode) = match stuck {
        "orphan snapshot" => {
            let object = snaps[0].files[0].hash.unwrap();
            let s = store.save_snapshot(snapshot(Uuid::new_v4(), vec![], object, b"orphan")).unwrap();
            (root.join(format!("snapshots/{}.json", s.id.to_hex())), root.join("snapshots"), 0o555)
        }
        "orphan object" => {
            let h = store.import_bytes(stray).unwrap();
            (store.object_path(&h), store.object_path(&h).parent().unwrap().to_path_buf(), 0o555)
        }
        "cut-off import" => {
            fs::create_dir_all(root.join("objects/tmp")).unwrap();
            fs::write(root.join("objects/tmp/import-cut"), stray).unwrap();
            (root.join("objects/tmp/import-cut"), root.join("objects/tmp"), 0o555)
        }
        "unread cache" => {
            fs::create_dir_all(root.join("cache/analysis/ab")).unwrap();
            fs::write(root.join("cache/analysis/ab/abcdef.json"), b"{}").unwrap();
            (root.join("cache/analysis"), root.join("cache"), 0o555)
        }
        "older index" => {
            // A key, then `LLIX` and the versions of prints and of the index, older than any.
            let mut head = vec![0u8; 32];
            head.extend(b"LLIX\0\0\0\0");
            fs::create_dir_all(root.join("cache/fingerprints/index")).unwrap();
            fs::write(root.join("cache/fingerprints/index/old.idx"), head).unwrap();
            (root.join("cache/fingerprints/index/old.idx"), root.join("cache/fingerprints/index"), 0o555)
        }
        "plugin scratch" => {
            let scratch = root.join("plugins/echo/scratch");
            fs::create_dir_all(scratch.join("job")).unwrap();
            fs::write(scratch.join("job/out.bin"), b"left").unwrap();
            fs::write(scratch.join("job.lock"), b"").unwrap();
            (scratch.join("job"), scratch, 0o555)
        }
        "pruned record" => {
            // A snapshot of the library beside its latest, pruned: nothing walks to its record.
            let object = snaps[0].files[0].hash.unwrap();
            let s = store.save_snapshot(snapshot(lib.id, vec![], object, b"beside")).unwrap();
            store.forget_snapshots(&[s.id]).unwrap();
            assert!(store.pruned().unwrap().contains_key(&s.id));
            (root.join(PRUNED), root.clone(), 0o555)
        }
        _ => {
            fs::create_dir_all(root.join("snapshots/stray")).unwrap();
            (root.join("snapshots/stray"), root.join("snapshots/stray"), 0o000)
        }
    };
    let pruned_before = store.pruned().unwrap().len();
    set_mode(&locked, mode);
    let plan = store.gc(true);
    let cleaned = store.gc(false);
    set_mode(&locked, 0o755);
    if stuck == "unlistable folder" {
        // What's in it can't be known, so neither the plan nor the clean-up gets anywhere.
        assert!(matches!(&plan, Err(Error::Io { path, .. }) if *path == locked), "{plan:?}");
    } else {
        let plan = plan.unwrap();
        assert!(plan.dry_run);
        let counted = plan.removed_snapshots + plan.removed_objects;
        assert_eq!(counted > 0, stuck != "pruned record", "{stuck}: {plan:?}");
    }
    assert!(matches!(&cleaned, Err(Error::Io { .. })), "{stuck}: {cleaned:?}");
    let _lock = store.lock().unwrap();
    store.gc(false).unwrap();
    let left = store.gc(true).unwrap();
    assert_eq!((left.removed_snapshots, left.removed_objects), (0, 0), "{stuck}: {left:?}");
    match stuck {
        "pruned record" => assert_eq!(store.pruned().unwrap().len(), pruned_before - 1),
        "unlistable folder" => assert!(gone.is_dir(), "a folder in snapshots is no leftover"),
        _ => assert!(!gone.exists(), "{stuck}: {} is still there", gone.display()),
    }
    assert!(!root.join("plugins/echo/scratch/job.lock").exists());
    for s in &snaps {
        assert!(store.load_snapshot(&s.id).is_ok() && store.has_object(&s.files[0].hash.unwrap()));
    }
}

/// An import that a store folder refuses (the objects folder, its staging folder, a fan-out folder)
/// fails said of the store; one whose file is gone or can't be read fails said of that file.
/// Nothing is stored either way.
#[hegel::test(test_cases = 8)]
fn prop_an_import_the_store_cant_write_is_said_of_the_store(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    if writes_anyway(dir.path()) {
        return;
    }
    let bytes = tc.draw(gs::binary().min_size(1).max_size(64));
    for way in ["objects", "staging", "no fan-out", "fan-out", "gone", "unreadable"] {
        import_fails(&dir.path().join(way), way, &bytes);
    }
}

/// [`prop_an_import_the_store_cant_write_is_said_of_the_store`] for `bytes` imported the `way` it
/// fails, into a store in `dir`.
fn import_fails(dir: &Path, way: &str, bytes: &[u8]) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let src = dir.join("a.bin");
    fs::write(&src, bytes).unwrap();
    let h = Hash::of(bytes);
    let objects = store.root().join("objects");
    let (refused, locked) = match way {
        // No staging folder yet, and none can be made.
        "objects" => {
            let _ = fs::remove_dir_all(objects.join("tmp"));
            (true, objects.clone())
        }
        "staging" => {
            fs::create_dir_all(objects.join("tmp")).unwrap();
            (true, objects.join("tmp"))
        }
        // Staging, but no fan-out folder can be made.
        "no fan-out" => {
            fs::create_dir_all(objects.join("tmp")).unwrap();
            (true, objects.clone())
        }
        "fan-out" => {
            let shard = store.object_path(&h).parent().unwrap().to_path_buf();
            fs::create_dir_all(&shard).unwrap();
            (true, shard)
        }
        // The file to import went before it was read, or can't be read: said of the file.
        "gone" => {
            fs::remove_file(&src).unwrap();
            (false, objects.clone())
        }
        _ => (false, src.clone()),
    };
    match (refused, way) {
        (true, _) => set_mode(&locked, 0o555),
        (_, "unreadable") => set_mode(&src, 0o000),
        _ => {}
    }
    let imported = store.import_file(&src, (way == "gone").then_some(h), &Ctx::silent());
    // Bytes are written straight to their fan-out folder, past the staging one.
    let bytes_refused = matches!(way, "objects" | "no fan-out" | "fan-out").then(|| store.import_bytes(bytes));
    set_mode(&locked, if way == "unreadable" { 0o644 } else { 0o755 });
    if let Some(e) = bytes_refused {
        assert!(matches!(&e, Err(Error::Io { path, .. }) if path == store.root()), "{e:?}");
    }
    let said = match imported {
        Err(Error::Io { path, .. }) => path,
        other => panic!("{other:?}"),
    };
    assert_eq!(said, if refused { store.root().to_path_buf() } else { src.clone() });
    assert!(!store.has_object(&h));
}

/// A check of the stored files checks each hashed file the manifests list, stops when asked, and
/// fails on a stored file it can't read.
#[hegel::test(test_cases = 8)]
fn prop_a_check_of_stored_files_stops_when_asked_and_fails_on_one_it_cant_read(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    if writes_anyway(dir.path()) {
        return;
    }
    let (store, _, snaps) = stocked(dir.path());
    // A manifest that lists a file without its hash: there's nothing to check it against.
    let unhashed = tc.draw(gs::booleans());
    if unhashed {
        let p = store.root().join(format!("snapshots/{}.json", snaps[0].id.to_hex()));
        let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        json["files"][0].as_object_mut().unwrap().remove("hash");
        fs::write(&p, serde_json::to_vec(&json).unwrap()).unwrap();
    }
    let report = store.verify(&Ctx::silent()).unwrap();
    assert_eq!((report.checked, report.corrupt.len(), report.missing.len()), (2 - usize::from(unhashed), 0, 0));
    let token = CancelToken::new();
    token.cancel();
    let stopped = store.verify(&Ctx::new(&NoProgress, token));
    assert!(matches!(stopped, Err(Error::Cancelled)), "{stopped:?}");
    let object = store.object_path(&snaps[1].files[0].hash.unwrap());
    set_mode(&object, 0o000);
    let unread = store.verify(&Ctx::silent());
    set_mode(&object, 0o644);
    assert!(matches!(&unread, Err(Error::Io { path, .. }) if *path == object), "{unread:?}");
}

/// A drive record JSON can't hold (a mount path that isn't UTF-8) is refused, as is a change
/// that fails, and the registry stays as it was.
#[hegel::test(test_cases = 8)]
fn prop_a_drive_record_json_cant_hold_isnt_recorded(tc: TestCase) {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let (store, _, _) = stocked(dir.path());
    let mut name = tc.draw(gs::binary().max_size(8));
    name.insert(tc.draw(gs::integers::<usize>().max_value(name.len())), 0xff);
    let mount = Path::new("/Volumes").join(OsStr::from_bytes(&name));
    let before = store.drives().unwrap();
    let id = Uuid::new_v4();
    let recorded =
        store.update_drives(|d| Ok(d.insert(id, locallibrary::store::DriveRecord::new(id, "Odd", mount.clone()))));
    assert!(matches!(&recorded, Err(Error::Invalid(_))), "{recorded:?}");
    // Nor is a change that fails halfway.
    let failed = store.update_drives(|d| -> Result<()> {
        d.clear();
        Err(Error::NotFound("drive".into()))
    });
    assert!(matches!(&failed, Err(Error::NotFound(_))), "{failed:?}");
    assert_eq!(store.drives().unwrap().keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
}

/// The player's file waits in a folder beside the store's, named after it; a root has none.
#[hegel::test(test_cases = 20)]
fn prop_the_players_fallback_sits_beside_the_store(tc: TestCase) {
    let name = tc.draw(gs::sampled_from(vec!["locallibrary", "store", "Ünï data"]));
    let root = Path::new("/Users/dj/Library").join(name);
    let beside = locallibrary::store::player_fallback(&root).unwrap();
    assert_eq!(beside, Path::new("/Users/dj/Library").join(format!("{name}-player")));
    assert_eq!(locallibrary::store::player_fallback(Path::new("/")), None);
}

/// A store is made only in a folder that can be written (or made), and one that's there already is
/// opened whatever its folder allows: a refused one leaves the folder as it was.
#[hegel::test(test_cases = 20)]
fn prop_a_store_is_made_only_where_it_can_be_written(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    if writes_anyway(dir.path()) {
        return;
    }
    let parent = dir.path().join("data");
    let root = parent.join("store");
    let held = tc.draw(gs::sampled_from(vec!["nothing", "ours", "a store", "no folder"]));
    let id = match held {
        "no folder" => {
            fs::create_dir(&parent).unwrap();
            None
        }
        "a store" => Some(Store::open_or_init(&root).unwrap().id().unwrap()),
        _ => {
            fs::create_dir_all(&root).unwrap();
            if held == "ours" {
                fs::write(root.join("profile-document.json"), b"{}").unwrap();
            }
            None
        }
    };
    let locked = if held == "no folder" { &parent } else { &root };
    let before: Vec<_> = fs::read_dir(locked).unwrap().flatten().map(|e| e.file_name()).collect();
    set_mode(locked, 0o555);
    let opened = Store::open_or_init(&root);
    set_mode(locked, 0o755);
    match id {
        Some(id) => assert_eq!(opened.unwrap().id().unwrap(), id),
        None => {
            assert!(matches!(&opened, Err(Error::Io { .. })), "{held}: {:?}", opened.as_ref().err());
            let after: Vec<_> = fs::read_dir(locked).unwrap().flatten().map(|e| e.file_name()).collect();
            assert_eq!(after, before);
        }
    }
}

/// A snapshot of no library stays while a snapshot that stays descends from it, its files with it;
/// once that one is pruned, nothing reaches it, and it goes with its files.
#[hegel::test(test_cases = 6)]
fn prop_a_snapshot_of_no_library_stays_while_one_that_stays_descends_from_it(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (store, lib, _) = stocked(dir.path());
    let n = tc.draw(gs::integers::<u8>().min_value(1).max_value(3));
    let objects: Vec<Hash> = (0..n).map(|i| store.import_bytes(&[b'o', i]).unwrap()).collect();
    // A chain of snapshots of no library, the newest the parent of one of the library's.
    let mut parents = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        parents = vec![store.save_snapshot(snapshot(Uuid::new_v4(), parents, *o, &[b'o', i as u8])).unwrap().id];
    }
    // Two of them, at times: the walk meets their parent twice.
    let mut children = vec![store.save_snapshot(snapshot(lib.id, parents.clone(), objects[0], b"child")).unwrap().id];
    if tc.draw(gs::booleans()) {
        children.push(store.save_snapshot(snapshot(lib.id, parents, objects[0], b"twin")).unwrap().id);
    }
    let pruned = tc.draw(gs::booleans());
    if pruned {
        store.forget_snapshots(&children).unwrap();
    }
    let gc = store.gc(false).unwrap();
    let removed = if pruned { usize::from(n) } else { 0 };
    assert_eq!((gc.removed_snapshots, gc.removed_objects), (removed, removed), "{n} of no library");
    assert_eq!(objects.iter().all(|o| store.has_object(o)), !pruned);
}
