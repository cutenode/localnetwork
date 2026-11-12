//! `store browse`'s views, on real folders from drawn snapshots: a view is the snapshot's tree, no
//! write through it reaches a stored file, and closing it leaves nothing behind.

use crate::common;

use indexmap::IndexMap;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use chrono::Utc;
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::Hash;
use locallibrary::device::Device;
use locallibrary::model::LibrarySummary;
use locallibrary::paths::FileClass;
use locallibrary::progress::Ctx;
use locallibrary::retention::{self, PruneOptions};
use locallibrary::store::{Snapshot, Store};
use locallibrary::sync::{self, PullOptions};
use locallibrary::tree::FileEntry;
use locallibrary::view;
use tempfile::TempDir;
use uuid::Uuid;

/// Paths a snapshot may list, and whether a view shows them when listed in this order: the second
/// `one.mp3` is the first on a drive that ignores case, and the rest leave the view or go where
/// locallibrary keeps its own files.
const KEYS: [(&str, bool); 7] = [
    ("Contents/Artist/one.mp3", true),
    ("Contents/Artist/ONE.mp3", false),
    ("Contents/Artist/two.flac", true),
    ("PIONEER/rekordbox/export.pdb", true),
    ("../outside.mp3", false),
    ("Contents/../../outside.mp3", false),
    (".locallibrary/state.json", false),
];

fn fixture() -> (TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    (dir, store)
}

/// A snapshot of `library` listing `files`, kept in the store.
fn snapshot(store: &Store, library: Uuid, files: &[(String, Vec<u8>)]) -> Snapshot {
    let files = files.iter().map(|(path, bytes)| FileEntry {
        path: path.clone(),
        size: bytes.len() as u64,
        mtime: 0,
        class: FileClass::Audio,
        hash: Some(store.import_bytes(bytes).unwrap()),
    });
    let snapshot = Snapshot {
        id: Hash::of(b"ignored"),
        library_id: library,
        parents: Vec::new(),
        created: Utc::now(),
        message: None,
        source: None,
        fingerprint: Hash::of(format!("{library}{files:?}").as_bytes()),
        summary: LibrarySummary::default(),
        lineage: Vec::new(),
        files: files.collect(),
    };
    store.save_snapshot(snapshot).unwrap()
}

/// Every file under `dir`, read through its link.
fn tree(dir: &Path) -> IndexMap<String, Vec<u8>> {
    let files = walkdir::WalkDir::new(dir).into_iter().map(Result::unwrap).filter(|e| !e.file_type().is_dir());
    files
        .map(|e| {
            let key = e.path().strip_prefix(dir).unwrap().to_str().unwrap().to_string();
            (key, fs::read(e.path()).unwrap())
        })
        .collect()
}

fn hash_of(store: &Store, h: &Hash) -> Hash {
    locallibrary::hash::hash_file(&store.object_path(h), &Ctx::silent(), locallibrary::progress::Phase::Verify).unwrap()
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn make_writable(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
}

/// What an older version left: objects anyone with the right could write.
fn as_an_older_version_left(store: &Store, hashes: impl IntoIterator<Item = Hash>) {
    hashes.into_iter().for_each(|h| make_writable(&store.object_path(&h)));
}

/// In-place writes of every kind fail, and so does anything that would change the folder.
fn assert_no_write_gets_through(store: &Store, snapshot: &Snapshot, view: &Path, shown: &IndexMap<String, Vec<u8>>) {
    let open = |link: &Path, how: &mut fs::OpenOptions| how.open(link).unwrap_err().kind();
    let scratch = store.root().with_file_name("scratch");
    fs::write(&scratch, b"edited").unwrap();
    for key in shown.keys() {
        let link = view.join(key);
        let entry = snapshot.files.iter().find(|f| &f.path == key).unwrap();
        let h = entry.hash.unwrap();
        assert_eq!(open(&link, fs::File::options().write(true)), ErrorKind::PermissionDenied, "{key}");
        assert_eq!(open(&link, fs::File::options().append(true)), ErrorKind::PermissionDenied, "{key}");
        assert_eq!(open(&link, fs::File::options().write(true).truncate(true)), ErrorKind::PermissionDenied, "{key}");
        assert_eq!(fs::write(&link, b"edited").unwrap_err().kind(), ErrorKind::PermissionDenied, "{key}");
        // Saving through a temp file and a rename fails too, rather than leaving an edit there.
        let beside = link.with_extension("tmp");
        assert_eq!(fs::write(&beside, b"edited").unwrap_err().kind(), ErrorKind::PermissionDenied, "{key}");
        assert_eq!(fs::rename(&scratch, &link).unwrap_err().kind(), ErrorKind::PermissionDenied, "{key}");
        assert_eq!(fs::remove_file(&link).unwrap_err().kind(), ErrorKind::PermissionDenied, "{key}");
        assert_eq!(hash_of(store, &h), h, "{key}");
    }
}

/// A view shows exactly what the snapshot lists (safe paths, once each), every file a link to an
/// object nothing can write, older objects included, while objects it doesn't show stay as they
/// were. Closing it removes every trace and changes no object.
#[hegel::test(test_cases = 30, suppress_health_check = [HealthCheck::TooSlow])]
fn a_view_is_the_snapshot_and_nothing_writes_through_it(tc: TestCase) {
    let (dir, store) = fixture();
    let keys: Vec<(&str, bool)> = tc.draw(gs::subsequences(KEYS.to_vec()));
    let files: Vec<(String, Vec<u8>)> = keys
        .iter()
        .map(|(key, _)| {
            let bytes: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(2)).max_size(4));
            (key.to_string(), bytes)
        })
        .collect();
    let snap = snapshot(&store, Uuid::new_v4(), &files);
    let unshown = store.import_bytes(b"in no snapshot").unwrap();
    let older: bool = tc.draw(gs::booleans());
    if older {
        as_an_older_version_left(&store, snap.files.iter().filter_map(|f| f.hash).chain([unshown]));
    }
    let to = tc.draw(gs::booleans()).then(|| dir.path().join("chosen"));
    if to.is_some() && tc.draw(gs::booleans()) {
        fs::create_dir(to.as_ref().unwrap()).unwrap();
    }

    let opened = view::open(&store, &snap.id, to.as_deref()).unwrap();

    let path = opened.view.path.clone();
    let one = keys.iter().any(|(key, _)| key.ends_with("one.mp3"));
    let shown = |key: &str, safe: bool| safe || (key.ends_with("ONE.mp3") && !one);
    let want: IndexMap<String, Vec<u8>> =
        keys.iter().zip(&files).filter(|((key, safe), _)| shown(key, *safe)).map(|(_, f)| f.clone()).collect();
    assert_eq!(tree(&path), want);
    assert_eq!(opened.files, want.len());
    assert_eq!(opened.bytes, want.values().map(|b| b.len() as u64).sum::<u64>());
    assert_eq!(opened.skipped.len(), files.len() - want.len(), "{:?}", opened.skipped);
    assert_eq!(opened.missing, Vec::<String>::new());
    assert_eq!(opened.view.snapshot, snap.id);
    match &to {
        Some(to) => assert_eq!(&path, to),
        None => assert!(path.starts_with(std::env::temp_dir()), "{}", path.display()),
    }
    assert_eq!(opened.symlinks, 0, "the view is on the store's volume");
    for key in want.keys() {
        let entry = snap.files.iter().find(|f| &f.path == key).unwrap();
        let object = store.object_path(&entry.hash.unwrap());
        let link = fs::symlink_metadata(path.join(key)).unwrap();
        assert!(link.file_type().is_file(), "{key}");
        assert_eq!(link.ino(), fs::metadata(&object).unwrap().ino(), "{key}");
        assert_eq!(link.mode() & 0o222, 0, "{key}");
    }
    assert_eq!(mode(&store.object_path(&unshown)) & 0o200 != 0, older, "an object the view doesn't show is left alone");
    assert_no_write_gets_through(&store, &snap, &path, &want);
    let listed = view::list(&store).unwrap();
    assert_eq!(listed.iter().map(|v| (&v.view.path, v.symlinks, v.broken)).collect::<Vec<_>>(), [(&path, 0, 0)]);

    let closed = view::close(&store, &path).unwrap();
    assert_eq!((closed.path, closed.left), (path.clone(), 0));
    assert!(fs::symlink_metadata(&path).is_err(), "{}", path.display());
    assert!(view::list(&store).unwrap().is_empty());
    assert_eq!(store.verify(&Ctx::silent()).unwrap().corrupt, Vec::<String>::new());
}

/// New objects are read-only however they came, also one replacing an object a crash cut short,
/// and what's pulled from them onto a drive isn't: only the store's files are.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn objects_are_read_only_from_the_start(tc: TestCase) {
    let (dir, store) = fixture();
    let bytes: Vec<u8> = tc.draw(gs::binary().min_size(2).max_size(16));
    let src = dir.path().join("track.mp3");
    fs::write(&src, &bytes).unwrap();
    let by_file: bool = tc.draw(gs::booleans());
    let import = || match by_file {
        true => store.import_file(&src, None, &Ctx::silent()).unwrap().0,
        false => store.import_bytes(&bytes).unwrap(),
    };
    let h = import();
    let object = store.object_path(&h);
    assert_eq!(mode(&object) & 0o222, 0, "{:o}", mode(&object));
    assert_eq!(fs::write(&object, b"x").unwrap_err().kind(), ErrorKind::PermissionDenied);

    fs::remove_file(&object).unwrap();
    fs::write(&object, &bytes[..1]).unwrap();
    assert_eq!(import(), h);
    assert_eq!(fs::read(&object).unwrap(), bytes);
    assert_eq!(mode(&object) & 0o222, 0, "{:o}", mode(&object));

    let copied = dir.path().join("drive").join("track.mp3");
    locallibrary::fsutil::copy_file(&object, &copied, None, Some(h), false, &Ctx::silent()).unwrap();
    assert_ne!(mode(&copied) & 0o200, 0, "{:o}", mode(&copied));
}

/// Pruning a view's snapshot is warned of and doesn't stop it. Hard links keep their files, whose
/// space stays in use. A symlink (here, to a file lost when the view was made and brought back)
/// breaks, and works again, read-only, if its file comes back. Closing removes all of them.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn pruning_a_views_snapshot_says_so_and_goes_ahead(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    Device::open_export(&root).unwrap().adopt(Some("Main")).unwrap();
    let pull = || {
        sync::pull(&store, &mut Device::open_export(&root).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
        store.load_snapshot(&store.libraries().unwrap()[0].head.unwrap()).unwrap()
    };
    let old = pull();
    common::export_new_track(&root, "New", "Later");
    let latest = pull();
    let kept: Vec<Option<Hash>> = latest.files.iter().map(|f| f.hash).collect();
    let (shared, only_here): (Vec<&FileEntry>, Vec<&FileEntry>) =
        old.files.iter().partition(|f| kept.contains(&f.hash));
    assert!(!shared.is_empty() && !only_here.is_empty());
    let first = store.object_path(&only_here[0].hash.unwrap());
    let bytes = fs::read(&first).unwrap();
    let symlinked: bool = tc.draw(gs::booleans());
    if symlinked {
        fs::remove_file(&first).unwrap();
    }
    let path = view::open(&store, &old.id, None).unwrap().view.path;
    if symlinked {
        store.import_bytes(&bytes).unwrap();
    }
    let linked = path.join(&only_here[0].path);
    assert_eq!(fs::read(&linked).unwrap(), bytes);

    let options = PruneOptions { keep_last: Some(1), ..Default::default() };
    let plan = retention::plan(&store, &options, &Ctx::silent()).unwrap();
    assert_eq!(plan.removed_snapshots, 1);
    let outcome = match symlinked {
        true => "links to files nothing else uses will break",
        false => "the space its files take stays in use until the view is removed",
    };
    assert_eq!(plan.warnings, [format!("the view at {} shows a snapshot this removes: {outcome}", path.display())]);
    retention::apply(&store, &plan).unwrap();

    assert!(fs::symlink_metadata(&first).is_err(), "the store let it go");
    assert_eq!(view::list(&store).unwrap()[0].broken, usize::from(symlinked));
    for f in shared.iter().chain(&only_here[1..]) {
        assert_eq!(fs::metadata(path.join(&f.path)).unwrap().len(), f.size, "{}", f.path);
    }
    match symlinked {
        true => {
            assert_eq!(fs::read(&linked).unwrap_err().kind(), ErrorKind::NotFound);
            if tc.draw(gs::booleans()) {
                store.import_bytes(&bytes).unwrap();
                assert_eq!(fs::read(&linked).unwrap(), bytes);
            }
        }
        false => assert_eq!(fs::read(&linked).unwrap(), bytes),
    }
    if fs::metadata(&linked).is_ok() {
        assert_eq!(fs::write(&linked, b"edited").unwrap_err().kind(), ErrorKind::PermissionDenied);
    }
    assert_eq!(view::close(&store, &path).unwrap().left, 0);
    assert!(fs::symlink_metadata(&path).is_err());
}

/// A file the store has lost is shown as a broken symlink, and said: the rest of the drive is there
/// to see.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_file_the_store_lost_is_said_and_its_link_broken(tc: TestCase) {
    let (_dir, store) = fixture();
    let files: Vec<(String, Vec<u8>)> = (0..3).map(|i| (format!("Contents/{i}.mp3"), vec![i])).collect();
    let snap = snapshot(&store, Uuid::new_v4(), &files);
    let lost: Vec<usize> = tc.draw(gs::subsequences(vec![0, 1, 2]));
    for i in &lost {
        fs::remove_file(store.object_path(&snap.files[*i].hash.unwrap())).unwrap();
    }
    let opened = view::open(&store, &snap.id, None).unwrap();
    let said: Vec<String> = lost.iter().map(|i| format!("Contents/{i}.mp3")).collect();
    assert_eq!(opened.missing, said);
    assert_eq!((opened.files, opened.symlinks), (3, lost.len()));
    assert_eq!(view::list(&store).unwrap()[0].broken, lost.len());
    for i in &lost {
        let target = fs::read_link(opened.view.path.join(format!("Contents/{i}.mp3"))).unwrap();
        assert!(target.is_absolute() && target.starts_with(store.root().join("objects")), "{}", target.display());
    }
    assert_eq!(view::close(&store, &opened.view.path).unwrap().left, 0);
}

/// A folder holding anything is refused and left as it was, as is a path that can't be a folder, a
/// snapshot that isn't there makes nothing, and a failure part way removes what it made.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_view_that_cant_be_made_leaves_nothing(tc: TestCase) {
    let (dir, store) = fixture();
    let files = [("Contents/a.mp3".to_string(), b"a".to_vec())];
    let snap = snapshot(&store, Uuid::new_v4(), &files);

    let taken = dir.path().join("taken");
    fs::create_dir(&taken).unwrap();
    fs::write(taken.join("mine.txt"), b"mine").unwrap();
    let e = view::open(&store, &snap.id, Some(&taken)).unwrap_err();
    assert_eq!(e.to_string(), format!("{} isn't empty; choose another folder", taken.display()));
    assert_eq!(fs::read(taken.join("mine.txt")).unwrap(), b"mine");

    let a_file = taken.join("mine.txt");
    view::open(&store, &snap.id, Some(&a_file)).unwrap_err();
    assert_eq!(fs::read(&a_file).unwrap(), b"mine");

    let unknown = Hash::of(b"no such snapshot");
    let to = dir.path().join("for-nothing");
    assert!(view::open(&store, &unknown, Some(&to)).unwrap_err().to_string().contains("snapshot"));
    assert!(!to.exists());

    // A file and a folder of one name: the links before it are made by then.
    let folder: bool = tc.draw(gs::booleans());
    let mut clash: Vec<(String, Vec<u8>)> = vec![("Contents/x".to_string(), b"file".to_vec())];
    clash.insert(usize::from(folder), ("Contents/x/y.mp3".to_string(), b"inside".to_vec()));
    let clash = snapshot(&store, Uuid::new_v4(), &clash);
    let to = dir.path().join("clash");
    view::open(&store, &clash.id, Some(&to)).unwrap_err();
    assert!(!to.exists());
    assert!(view::list(&store).unwrap().is_empty());
}

/// Only a recorded view can be closed: this deletes. What someone put in a view is theirs, and
/// stays, as does the stored file a hard link shared; one deleted by hand drops off the list.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn closing_deletes_only_a_views_own_links(tc: TestCase) {
    let (dir, store) = fixture();
    let files = [("Contents/Artist/a.mp3".to_string(), b"a".to_vec())];
    let snap = snapshot(&store, Uuid::new_v4(), &files);

    let elsewhere: PathBuf = dir.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let e = view::close(&store, &elsewhere).unwrap_err();
    assert_eq!(e.to_string(), format!("not found: a view at {}", elsewhere.display()));
    assert!(elsewhere.is_dir());

    let to = dir.path().join("view");
    let path = view::open(&store, &snap.id, Some(&to)).unwrap().view.path;
    assert_eq!(mode(&path), 0o500);
    if tc.draw(gs::booleans()) {
        for e in walkdir::WalkDir::new(&path).into_iter().map(Result::unwrap).filter(|e| e.file_type().is_dir()) {
            make_folder_writable(e.path());
        }
        fs::remove_dir_all(&path).unwrap();
        assert!(view::list(&store).unwrap().is_empty());
        let again = view::open(&store, &snap.id, None).unwrap().view.path;
        assert_eq!(view::list(&store).unwrap().len(), 1, "the one deleted by hand is gone from the list");
        assert_eq!(view::close(&store, &again).unwrap().left, 0);
        // A folder made where it was isn't that view, and closing deletes.
        fs::create_dir(&path).unwrap();
        assert!(view::list(&store).unwrap().is_empty());
        assert!(view::close(&store, &path).is_err());
        assert!(path.is_dir());
        return;
    }
    let folder = path.join("Contents/Artist");
    make_folder_writable(&folder);
    fs::write(folder.join("mine.txt"), b"mine").unwrap();
    let closed = view::close(&store, &path).unwrap();
    assert_eq!(closed.left, 1);
    assert_eq!(fs::read(folder.join("mine.txt")).unwrap(), b"mine");
    assert!(fs::symlink_metadata(folder.join("a.mp3")).is_err());
    assert!(view::list(&store).unwrap().is_empty(), "it's no longer a view");
    assert_eq!(fs::read(store.object_path(&snap.files[0].hash.unwrap())).unwrap(), b"a");
}

fn make_folder_writable(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
