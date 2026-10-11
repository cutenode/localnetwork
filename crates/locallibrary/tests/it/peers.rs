//! Fetching snapshots from another store over the network (`peers`), on this computer's own
//! network: no relays.

use std::sync::mpsc;
use std::thread;

use locallibrary::device::Device;
use locallibrary::peers::{self, FetchOptions, LibraryOutcome, ServeEvent, ServeOptions, ShareCode};
use locallibrary::progress::{CancelToken, Ctx, NoProgress};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};

use crate::common;

/// `store` served until the returned guard is dropped, and its share code.
struct Serving {
    cancel: CancelToken,
    thread: Option<thread::JoinHandle<()>>,
    code: ShareCode,
}

impl Drop for Serving {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.thread.take().unwrap().join().unwrap();
    }
}

fn serve(store: &Store) -> Serving {
    let (store, cancel) = (store.clone(), CancelToken::new());
    let (tx, rx) = mpsc::channel();
    let c = cancel.clone();
    let thread = thread::spawn(move || {
        let ctx = Ctx { progress: &NoProgress, cancel: c };
        let opts = ServeOptions { local_only: true };
        peers::serve(&store, &opts, &ctx, &mut |e| {
            if let ServeEvent::Ready { code } = e {
                tx.send(code.clone()).unwrap();
            }
        })
        .unwrap();
    });
    let code = rx.recv().unwrap().parse().unwrap();
    Serving { cancel, thread: Some(thread), code }
}

fn fetch(store: &Store, code: &ShareCode, libraries: &[&str]) -> peers::FetchReport {
    let opts =
        FetchOptions { libraries: libraries.iter().map(|s| s.to_string()).collect(), local_only: true, dry_run: false };
    peers::fetch(store, code, &opts, &Ctx::silent()).unwrap()
}

fn pull(store: &Store, drive: &mut Device) -> locallibrary::hash::Hash {
    sync::pull(store, drive, &PullOptions::default(), &Ctx::silent()).unwrap().snapshot.unwrap().id
}

#[test]
fn a_library_comes_over_and_follows_the_other_computer() {
    let dir = tempfile::tempdir().unwrap();
    let laptop = Store::open_or_init(dir.path().join("laptop")).unwrap();
    let studio = Store::open_or_init(dir.path().join("studio")).unwrap();
    let mut drive = Device::open_export(common::demo_drive(dir.path(), "Home")).unwrap();
    let first = pull(&laptop, &mut drive);
    laptop.add_label(&first, "before NYE").unwrap();
    let serving = serve(&laptop);

    // A dry run says what would come, and copies nothing.
    let opts = FetchOptions { local_only: true, dry_run: true, ..Default::default() };
    let dry = peers::fetch(&studio, &serving.code, &opts, &Ctx::silent()).unwrap();
    assert!(dry.files > 0 && dry.snapshots == 1, "{dry:?}");
    assert!(studio.libraries().unwrap().is_empty());

    let r = fetch(&studio, &serving.code, &[]);
    assert_eq!((r.snapshots, r.files, r.bytes), (dry.snapshots, dry.files, dry.bytes));
    assert_eq!(r.libraries.len(), 1);
    assert_eq!((r.libraries[0].name.as_str(), r.libraries[0].outcome), ("Home", LibraryOutcome::Added));
    let home = studio.find_library("Home").unwrap().unwrap();
    assert_eq!((home.id, home.head), (laptop.find_library("Home").unwrap().unwrap().id, Some(first)));
    assert_eq!(studio.find_label("before NYE").unwrap().unwrap().snapshot, first);
    assert!(studio.verify(&Ctx::silent()).unwrap().missing.is_empty(), "every file the snapshot lists");

    // Nothing new: nothing copied.
    let again = fetch(&studio, &serving.code, &["home"]);
    assert_eq!((again.snapshots, again.files, again.libraries[0].outcome), (0, 0, LibraryOutcome::UpToDate));

    // A new export on the laptop: the studio's library moves to it, and only new files come.
    common::export_new_track(drive.root(), "New", "Track");
    let second = pull(&laptop, &mut drive);
    let r = fetch(&studio, &serving.code, &[]);
    assert_eq!((r.snapshots, r.libraries[0].outcome), (1, LibraryOutcome::Updated));
    assert!(r.present > 0, "files the studio had aren't sent again");
    assert_eq!(studio.find_library("Home").unwrap().unwrap().head, Some(second));
    drop(serving);

    // Both change: the studio keeps its own latest.
    let stick = dir.path().join("Studio stick");
    std::fs::create_dir(&stick).unwrap();
    let mut studio_drive = Device::open(&stick).unwrap();
    let pushed = studio.load_snapshot(&second).unwrap();
    sync::push(&studio, &pushed, &mut studio_drive, &Default::default(), &Ctx::silent()).unwrap();
    common::export_new_track(studio_drive.root(), "Studio", "Only");
    let studio_head = pull(&studio, &mut studio_drive);
    common::export_new_track(drive.root(), "Laptop", "Only");
    pull(&laptop, &mut drive);
    let serving = serve(&laptop);
    let r = fetch(&studio, &serving.code, &[]);
    assert_eq!(r.libraries[0].outcome, LibraryOutcome::Diverged);
    assert_eq!(studio.find_library("Home").unwrap().unwrap().head, Some(studio_head));
}

#[test]
fn a_library_of_the_same_name_is_kept_apart_and_a_wrong_code_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let laptop = Store::open_or_init(dir.path().join("laptop")).unwrap();
    let studio = Store::open_or_init(dir.path().join("studio")).unwrap();
    pull(&laptop, &mut Device::open_export(common::demo_drive(dir.path(), "Home")).unwrap());
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    pull(&studio, &mut Device::open_export(common::demo_drive(&other, "Home")).unwrap());
    let serving = serve(&laptop);

    let r = fetch(&studio, &serving.code, &[]);
    assert_eq!(r.libraries[0].outcome, LibraryOutcome::Added);
    assert_eq!((r.libraries[0].name.as_str(), r.libraries[0].was.as_deref()), ("Home 2", Some("Home")));
    assert_eq!(studio.libraries().unwrap().len(), 2);

    let wrong: ShareCode = {
        let code = serving.code.to_string();
        let (ticket, _) = code.rsplit_once('-').unwrap();
        format!("{ticket}-{}", "0".repeat(32)).parse().unwrap()
    };
    let err = peers::fetch(&studio, &wrong, &FetchOptions { local_only: true, ..Default::default() }, &Ctx::silent())
        .unwrap_err();
    assert_eq!(err.code(), "peer");
    assert!(err.to_string().contains("refused the share code"), "{err}");
    let err =
        peers::fetch(&laptop, &serving.code, &FetchOptions { local_only: true, ..Default::default() }, &Ctx::silent())
            .unwrap_err();
    assert!(err.to_string().contains("this store's own"), "{err}");
    let err = fetch_err(&studio, &serving.code, "Nope");
    assert!(err.contains("library \"Nope\" on the other computer (it has Home)"), "{err}");
}

fn fetch_err(store: &Store, code: &ShareCode, library: &str) -> String {
    let opts = FetchOptions { libraries: vec![library.into()], local_only: true, dry_run: false };
    peers::fetch(store, code, &opts, &Ctx::silent()).unwrap_err().to_string()
}

#[test]
fn share_codes_read_back_and_others_are_refused() {
    for bad in ["", "nope", "endpointabc-123", "endpointabc"] {
        assert!(bad.parse::<ShareCode>().is_err(), "{bad}");
    }
}

#[test]
fn a_damaged_file_is_refused_and_nothing_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let laptop = Store::open_or_init(dir.path().join("laptop")).unwrap();
    let studio = Store::open_or_init(dir.path().join("studio")).unwrap();
    let head = pull(&laptop, &mut Device::open_export(common::demo_drive(dir.path(), "Home")).unwrap());
    let file = laptop.load_snapshot(&head).unwrap().files.into_iter().max_by_key(|f| f.size).unwrap();
    let object = laptop.object_path(&file.hash.unwrap());
    let mut bytes = std::fs::read(&object).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&object, bytes).unwrap();
    let serving = serve(&laptop);

    let err =
        peers::fetch(&studio, &serving.code, &FetchOptions { local_only: true, ..Default::default() }, &Ctx::silent())
            .unwrap_err();
    assert!(err.to_string().contains("came back damaged"), "{err}");
    assert!(!studio.has_object(&file.hash.unwrap()));
    assert!(studio.libraries().unwrap().is_empty() && studio.snapshot_ids().unwrap().is_empty());
}

#[test]
fn snapshots_pruned_here_stay_pruned_and_a_missing_latest_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let laptop = Store::open_or_init(dir.path().join("laptop")).unwrap();
    let studio = Store::open_or_init(dir.path().join("studio")).unwrap();
    let mut drive = Device::open_export(common::demo_drive(dir.path(), "Home")).unwrap();
    let first = pull(&laptop, &mut drive);
    common::export_new_track(drive.root(), "New", "Track");
    pull(&laptop, &mut drive);
    let serving = serve(&laptop);
    assert_eq!(fetch(&studio, &serving.code, &[]).snapshots, 2);
    studio.forget_snapshots(&[first]).unwrap();
    let again = fetch(&studio, &serving.code, &[]);
    assert_eq!((again.snapshots, again.files), (0, 0), "the pruned snapshot isn't fetched again");
    assert!(studio.load_info(&first).is_err());
    drop(serving);

    // The laptop's latest snapshot is gone from its store: the library isn't added, or moved, to it.
    common::export_new_track(drive.root(), "Newer", "Track");
    let gone = pull(&laptop, &mut drive);
    std::fs::remove_file(laptop.root().join("snapshots").join(format!("{}.json", gone.to_hex()))).unwrap();
    let serving = serve(&laptop);
    let r = fetch(&studio, &serving.code, &[]);
    assert_eq!(r.libraries[0].outcome, LibraryOutcome::Unavailable);
    assert_ne!(studio.find_library("Home").unwrap().unwrap().head, Some(gone));
}
