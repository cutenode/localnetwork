//! A folder copy of the store (`offsite::copy`) when one of the store's records or folders, or
//! one of the copy's, can't be read, written or replaced: it says which, and once that's put
//! right the next copy holds the store. One stopped before its first file, or while it re-reads
//! the copy's files, stops there, and the next finishes it.

use crate::common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::offsite::{self, CopyOptions};
use locallibrary::progress::{CancelToken, Ctx, Event, Phase};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Error, Hash};

fn ids(store: &Store) -> BTreeSet<Hash> {
    store.snapshot_ids().unwrap().into_iter().collect()
}

/// A store in `dir` (a resolved path, as a copy names its folder) holding a library pulled
/// twice, a track added in between.
fn pulled_twice(dir: &Path) -> Store {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let a = common::demo_drive(dir, "A");
    let pull = || sync::pull(&store, &mut Device::open_export(&a).unwrap(), &PullOptions::default(), &Ctx::silent());
    pull().unwrap();
    common::export_new_track(&a, "New", "Track");
    pull().unwrap();
    store
}

/// Where the store (or copy) at `root` keeps snapshot `id`'s manifest.
fn manifest(root: &Path, id: &Hash) -> PathBuf {
    root.join("snapshots").join(format!("{}.json", id.to_hex()))
}

/// The stored files in `store`, as found on disk.
fn stored(store: &Store) -> Vec<Hash> {
    walkdir::WalkDir::new(store.root().join("objects"))
        .into_iter()
        .map(Result::unwrap)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let shard = e.path().parent()?.file_name()?.to_string_lossy().into_owned();
            format!("{shard}{}", e.file_name().to_string_lossy()).parse().ok()
        })
        .collect()
}

/// The file an i/o or parse error happened at.
fn at(e: &Error) -> Option<PathBuf> {
    match e.inner() {
        Error::Io { path, .. } | Error::Parse { path, .. } => Some(path.clone()),
        _ => None,
    }
}

/// Bytes no record of the store reads as.
#[hegel::composite]
fn garbage(tc: &TestCase) -> Vec<u8> {
    tc.draw(gs::sampled_from(vec!["", "{", "[1]", "null", "\"text\""])).as_bytes().to_vec()
}

/// Puts `path` back as it `was` (absent when `None`).
fn put_back(path: &Path, was: Option<Vec<u8>>) {
    match was {
        Some(bytes) => fs::write(path, bytes).unwrap(),
        None => fs::remove_file(path).unwrap(),
    }
}

/// A copy, forced, that finishes holding every snapshot of the store.
fn holds_the_store(store: &Store, dest: &Path, opts: CopyOptions) -> offsite::CopyReport {
    let r = offsite::copy(store, dest, &CopyOptions { force: true, ..opts }, &Ctx::silent()).unwrap();
    assert!(r.complete && r.problems.is_empty(), "{r:?}");
    assert_eq!(ids(&Store::open(dest).unwrap()), ids(store));
    r
}

/// Whichever of the store's records reads as garbage (its id, libraries, pruned snapshots,
/// labels, list of copies, or a snapshot's manifest), or a snapshot that lists a file without its
/// hash, a copy fails saying which, as does listing the copies when it reads that record; put
/// back, the copy goes through.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_copy_names_the_store_record_it_cant_read_and_goes_once_its_put_right(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let base = locallibrary::device::dunce_canonicalize(dir.path());
    let store = pulled_twice(&base);
    let silent = Ctx::silent();
    // The store lists its copies once it has one.
    holds_the_store(&store, &base.join("first"), CopyOptions::default());
    let sid = tc.draw(gs::sampled_from(ids(&store).into_iter().collect::<Vec<_>>()));
    let manifest = manifest(store.root(), &sid);
    let records = ["store.json", "libraries.json", "pruned.json", "labels.json", "copies.json"];
    let listed_by = ["store.json", "libraries.json", "copies.json"];
    for (n, path) in records.iter().map(|r| store.root().join(r)).chain([manifest.clone()]).enumerate() {
        let was = fs::read(&path).ok();
        fs::write(&path, tc.draw(garbage())).unwrap();
        let dest = base.join(format!("copy {n}"));
        let err = offsite::copy(&store, &dest, &CopyOptions::default(), &silent).unwrap_err();
        assert_eq!(at(&err).as_deref(), Some(path.as_path()), "{err}");
        let name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(offsite::copies(&store).is_err(), listed_by.contains(&name), "copies with {name} unreadable");
        put_back(&path, was);
        holds_the_store(&store, &dest, CopyOptions::default());
    }

    let was = fs::read(&manifest).unwrap();
    let mut snap: serde_json::Value = serde_json::from_slice(&was).unwrap();
    let files = snap["files"].as_array_mut().unwrap();
    let i = tc.draw(gs::integers::<usize>().max_value(files.len() - 1));
    files[i].as_object_mut().unwrap().remove("hash");
    let named = files[i]["path"].as_str().unwrap().to_string();
    fs::write(&manifest, serde_json::to_vec(&snap).unwrap()).unwrap();
    let dest = base.join("unhashed");
    let err = offsite::copy(&store, &dest, &CopyOptions::default(), &silent).unwrap_err();
    assert_eq!(err.to_string(), format!("snapshot {}: {named} has no hash", sid.short()));
    assert!(!dest.exists(), "nothing was copied");
    fs::write(&manifest, was).unwrap();
    holds_the_store(&store, &dest, CopyOptions::default());
}

/// Over a copy of the store: one whose libraries or own record read as garbage is refused saying
/// which, forced or not; one stopped and pulled into since is refused unless forced; a record the store
/// lacks that's a folder in the copy, or (mirroring) a snapshot or stored file the store lacks
/// that's a folder there, can't be removed (a dry run says what would be), nor a record it replaces or a half-written file it
/// clears, and the copy says so; nor does it go while another holds the store or the copy. Put right, the copy goes, and
/// mirroring removes what the store lacks. Into an empty folder, a copy goes as into a new one.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_copy_names_what_it_cant_read_or_remove_in_the_copy_and_goes_once_its_put_right(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let base = locallibrary::device::dunce_canonicalize(dir.path());
    let store = pulled_twice(&base);
    let silent = Ctx::silent();
    let forced = CopyOptions { force: true, ..Default::default() };
    let mirror = CopyOptions { mirror: true, ..Default::default() };
    let copy = |name: &str| {
        let dest = base.join(name);
        holds_the_store(&store, &dest, CopyOptions::default());
        dest
    };

    // Its libraries, and its own record.
    for (n, record) in ["libraries.json", "copy.json"].into_iter().enumerate() {
        let dest = copy(&format!("record {n}"));
        let path = dest.join(record);
        let was = fs::read(&path).ok();
        fs::write(&path, tc.draw(garbage())).unwrap();
        for opts in [&CopyOptions::default(), &forced] {
            let err = offsite::copy(&store, &dest, opts, &silent).unwrap_err();
            assert_eq!(at(&err).as_deref(), Some(path.as_path()), "{err}");
        }
        // A copy whose own record can't be read isn't warned of as one.
        assert_eq!(offsite::written_copy_warning(&dest).is_none(), record == "copy.json");
        put_back(&path, was);
        holds_the_store(&store, &dest, CopyOptions::default());
    }

    // A record of the store's it lacks, a folder in the copy.
    let lacking: Vec<&str> = ["smart.json", "players.json", "settings.json", "pruned.json"]
        .into_iter()
        .filter(|r| !store.root().join(r).exists())
        .collect();
    let record = tc.draw(gs::sampled_from(lacking));
    let dest = copy("lacking");
    let folder = dest.join(record);
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("inside"), b"x").unwrap();
    let unforced = offsite::copy(&store, &dest, &CopyOptions::default(), &silent).unwrap_err();
    assert_eq!(unforced.reason(), Some(Reason::CopyAhead), "what it holds can't be told: {unforced}");
    let err = offsite::copy(&store, &dest, &forced, &silent).unwrap_err();
    assert_eq!(at(&err).as_deref(), Some(folder.as_path()), "{err}");
    fs::remove_dir_all(&folder).unwrap();
    holds_the_store(&store, &dest, CopyOptions::default());
    assert!(!folder.exists());

    // Mirroring, a snapshot and a stored file the store lacks, folders in the copy.
    let dest = copy("mirrored");
    let any = ids(&store).into_iter().next().unwrap();
    let stray = Hash::of(tc.draw(gs::text().max_size(8)).as_bytes());
    let snapshot = dest.join("snapshots").join(format!("{}.json", stray.to_hex()));
    let hex = Hash::of(b"a stored file the store lacks").to_hex();
    let object = dest.join("objects").join(&hex[..2]).join(&hex[2..]);
    let as_folder = |p: &Path| {
        fs::create_dir_all(p).unwrap();
        fs::write(p.join("inside"), b"x").unwrap();
    };
    as_folder(&snapshot);
    let err = offsite::copy(&store, &dest, &CopyOptions { mirror: true, ..forced.clone() }, &silent).unwrap_err();
    assert_eq!(at(&err).as_deref(), Some(snapshot.as_path()), "{err}");
    fs::remove_dir_all(&snapshot).unwrap();
    fs::copy(manifest(&dest, &any), &snapshot).unwrap();
    as_folder(&object);
    let err = offsite::copy(&store, &dest, &CopyOptions { mirror: true, ..forced.clone() }, &silent).unwrap_err();
    assert_eq!(at(&err).as_deref(), Some(object.as_path()), "{err}");
    fs::remove_dir_all(&object).unwrap();
    fs::write(&object, b"stray").unwrap();
    // The copy that failed on the stored file had removed the snapshot already.
    fs::copy(manifest(&dest, &any), &snapshot).unwrap();
    let dry = CopyOptions { dry_run: true, force: true, ..mirror.clone() };
    let planned = offsite::copy(&store, &dest, &dry, &silent).unwrap();
    assert_eq!((planned.removed_snapshots, planned.removed_objects), (1, 1), "{planned:?}");
    let r = holds_the_store(&store, &dest, mirror);
    assert_eq!((r.removed_snapshots, r.removed_objects), (1, 1), "{r:?}");
    assert!(!snapshot.exists() && !object.exists());

    // Stopped (unfinished) and pulled into since: its libraries' latest is one the store lacks.
    let dest = copy("pulled into");
    let mut libraries: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(dest.join("libraries.json")).unwrap()).unwrap();
    let ahead = Hash::of(tc.draw(gs::text().max_size(8)).as_bytes());
    libraries[0]["head"] = ahead.to_hex().into();
    fs::write(dest.join("libraries.json"), serde_json::to_vec(&libraries).unwrap()).unwrap();
    let mut record: serde_json::Value = serde_json::from_slice(&fs::read(dest.join("copy.json")).unwrap()).unwrap();
    record["finished"] = serde_json::Value::Null;
    fs::write(dest.join("copy.json"), serde_json::to_vec(&record).unwrap()).unwrap();
    let err = offsite::copy(&store, &dest, &CopyOptions::default(), &silent).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::CopyAhead), "{err}");
    assert!(err.to_string().contains(&ahead.short()), "{err}");
    holds_the_store(&store, &dest, CopyOptions::default());

    // Records it replaces, folders in the copy; and a half-written file left there that's a
    // folder of things.
    let dest = copy("in the way");
    let leftover = dest.join("objects").join(format!("{}stopped", locallibrary::paths::TEMP_PREFIX));
    for path in ["drives.json", "labels.json"].map(|r| dest.join(r)).into_iter().chain([leftover]) {
        let was = fs::read(&path).ok();
        let _ = fs::remove_file(&path);
        as_folder(&path);
        let err = offsite::copy(&store, &dest, &forced, &silent).unwrap_err();
        assert!(at(&err).is_some_and(|p| p.starts_with(&path)), "{err}");
        fs::remove_dir_all(&path).unwrap();
        if let Some(bytes) = was {
            fs::write(&path, bytes).unwrap();
        }
        holds_the_store(&store, &dest, CopyOptions::default());
    }

    // Held by another: the store, or the copy.
    let held = store.lock().unwrap();
    assert!(matches!(offsite::copy(&store, &dest, &forced, &silent), Err(Error::Locked(_))));
    drop(held);
    let copy = Store::open(&dest).unwrap();
    let held = copy.lock().unwrap();
    let err = offsite::copy(&store, &dest, &forced, &silent).unwrap_err();
    assert!(matches!(&err, Error::Locked(p) if p.starts_with(&dest)), "{err}");
    drop(held);

    // An empty folder.
    let empty = base.join("empty");
    fs::create_dir(&empty).unwrap();
    holds_the_store(&store, &empty, CopyOptions::default());
}

/// A copy stopped before it starts copies no file, whether into a new folder or re-reading an
/// up-to-date copy's files, and so does one stopped while it re-reads them; the next copy
/// finishes, holding the store. A copy, unlike the store, warns of changes written to it.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_copy_stopped_before_its_first_file_or_while_rereading_finishes_next_time(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let base = locallibrary::device::dunce_canonicalize(dir.path());
    let store = pulled_twice(&base);
    let verify = CopyOptions { verify: true, ..Default::default() };
    let cancelled = CancelToken::new();
    cancelled.cancel();
    let ignore = |_: &Event| {};
    let before = Ctx::new(&ignore, cancelled);

    let dest = base.join("new");
    let opts = if tc.draw(gs::booleans()) { verify.clone() } else { CopyOptions::default() };
    let err = offsite::copy(&store, &dest, &opts, &before).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    let copy = Store::open(&dest).unwrap();
    assert!(stored(&copy).is_empty(), "no stored file was copied");
    assert!(offsite::marker(&dest).unwrap().unwrap().finished.is_none());
    holds_the_store(&store, &dest, CopyOptions::default());

    // Once it's a copy, a change written to it is warned of; not one to the store.
    let warning = offsite::written_copy_warning(&dest).unwrap();
    assert!(warning.contains(&format!("a copy of the store at {}", store.root().display())), "{warning}");
    assert_eq!(offsite::written_copy_warning(store.root()), None);

    let err = offsite::copy(&store, &dest, &verify, &before).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    let token = CancelToken::new();
    let stop = token.clone();
    let rereading = move |e: &Event| {
        if matches!(e, Event::Bytes { phase: Phase::Verify, .. }) {
            stop.cancel();
        }
    };
    let err = offsite::copy(&store, &dest, &verify, &Ctx::new(&rereading, token)).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    let r = holds_the_store(&store, &dest, verify);
    assert_eq!((r.objects_copied, r.objects_repaired), (0, 0), "{r:?}");
}

/// Where a folder of the store's, or of the copy's, can't be read or written (its permissions
/// say so), a copy fails saying where: the store's snapshots, a folder in the copy's place that
/// isn't a copy, an empty one it can't make the copy's folders in, a copy's stored files,
/// snapshots (or a folder among them) or own folder, or (re-reading them) one of its stored files. Its permissions put
/// back, the copy goes through.
#[cfg(unix)]
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_copy_names_the_folder_it_cant_read_or_write_and_goes_once_its_put_right(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path, m: u32| fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let base = locallibrary::device::dunce_canonicalize(dir.path());
    let store = pulled_twice(&base);
    let silent = Ctx::silent();
    let forced = CopyOptions { force: true, ..Default::default() };
    let verify = CopyOptions { verify: true, ..Default::default() };
    let copy = |name: &str| {
        let dest = base.join(name);
        holds_the_store(&store, &dest, CopyOptions::default());
        dest
    };
    let refused = |dest: &Path, opts: &CopyOptions| offsite::copy(&store, dest, opts, &silent).unwrap_err();

    // The store's snapshots.
    let snapshots = store.root().join("snapshots");
    mode(&snapshots, 0o000);
    let err = refused(&base.join("unread store"), &CopyOptions::default());
    mode(&snapshots, 0o755);
    assert_eq!(at(&err).as_deref(), Some(snapshots.as_path()), "{err}");
    holds_the_store(&store, &base.join("unread store"), CopyOptions::default());

    // A folder that isn't a copy, unreadable; an empty one, read-only.
    for (name, m, at_) in [("unreadable", 0o000, ""), ("read-only", 0o555, "objects")] {
        let dest = base.join(name);
        fs::create_dir(&dest).unwrap();
        mode(&dest, m);
        let err = refused(&dest, &CopyOptions::default());
        mode(&dest, 0o755);
        assert_eq!(at(&err), Some(dest.join(at_)), "{err}");
        holds_the_store(&store, &dest, CopyOptions::default());
    }

    // A copy's stored files, snapshots, and own folder.
    let dest = copy("objects");
    mode(&dest.join("objects"), 0o000);
    let err = refused(&dest, &CopyOptions::default());
    mode(&dest.join("objects"), 0o755);
    assert_eq!(at(&err), Some(dest.join("objects")), "{err}");
    holds_the_store(&store, &dest, CopyOptions::default());

    let dest = copy("snapshots");
    mode(&dest.join("snapshots"), 0o000);
    let unforced = refused(&dest, &CopyOptions::default());
    let err = refused(&dest, &forced);
    mode(&dest.join("snapshots"), 0o755);
    assert_eq!(unforced.reason(), Some(Reason::CopyAhead), "its snapshots can't be told: {unforced}");
    assert_eq!(at(&err), Some(dest.join("snapshots")), "{err}");
    holds_the_store(&store, &dest, CopyOptions::default());

    let dest = copy("own folder");
    mode(&dest, 0o555);
    let err = refused(&dest, &CopyOptions::default());
    mode(&dest, 0o755);
    assert!(at(&err).is_some_and(|p| p.starts_with(&dest)), "{err}");
    holds_the_store(&store, &dest, CopyOptions::default());

    // A folder among its snapshots it can't look in for what a stopped copy left.
    let dest = copy("unlooked");
    let folder = dest.join("snapshots").join("folder");
    fs::create_dir(&folder).unwrap();
    mode(&folder, 0o000);
    let err = refused(&dest, &CopyOptions::default());
    mode(&folder, 0o755);
    assert_eq!(at(&err).as_deref(), Some(folder.as_path()), "{err}");
    holds_the_store(&store, &dest, CopyOptions::default());

    // A snapshot the copy lost, its snapshots folder read-only.
    let dest = copy("lost");
    let sid = tc.draw(gs::sampled_from(ids(&store).into_iter().collect::<Vec<_>>()));
    fs::remove_file(manifest(&dest, &sid)).unwrap();
    mode(&dest.join("snapshots"), 0o555);
    let err = refused(&dest, &forced);
    mode(&dest.join("snapshots"), 0o755);
    assert!(at(&err).is_some_and(|p| p.starts_with(dest.join("snapshots"))), "{err}");
    holds_the_store(&store, &dest, CopyOptions::default());

    // Re-reading a stored file of the copy's it can't read.
    let dest = copy("reread");
    let h = tc.draw(gs::sampled_from(stored(&Store::open(&dest).unwrap())));
    let object = Store::open(&dest).unwrap().object_path(&h);
    mode(&object, 0o000);
    let err = refused(&dest, &verify);
    mode(&object, 0o644);
    assert_eq!(at(&err).as_deref(), Some(object.as_path()), "{err}");
    holds_the_store(&store, &dest, verify);
}
