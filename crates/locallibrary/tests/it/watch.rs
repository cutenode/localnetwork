//! Watching drives come and go, and saving them into the store as they're plugged in.

use crate::common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use locallibrary::api::{self, WatchAction, WatchOptions, Watched, Watching};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::fleet;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::watch::{VolumeWatcher, WatchEvent};

/// A folder of pretend volumes, a store, and drives made outside the folder and moved in to
/// plug them in.
struct Desk {
    dir: tempfile::TempDir,
    volumes: PathBuf,
    store: Store,
}

impl Desk {
    fn new() -> Desk {
        let dir = tempfile::tempdir().unwrap();
        let volumes = dir.path().join("volumes");
        std::fs::create_dir(&volumes).unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        Desk { dir, volumes, store }
    }

    fn store_root(&self) -> &Path {
        self.store.root()
    }

    /// The demo export, unplugged.
    fn drive(&self, name: &str) -> PathBuf {
        common::demo_drive(self.dir.path(), name)
    }

    fn plug(&self, root: &Path) -> PathBuf {
        let to = self.volumes.join(root.file_name().unwrap());
        std::fs::rename(root, &to).unwrap();
        to
    }

    fn unplug(&self, root: &Path) -> PathBuf {
        let to = self.dir.path().join(root.file_name().unwrap());
        std::fs::rename(root, &to).unwrap();
        to
    }

    fn watching(&self, options: WatchOptions) -> Watching {
        let w = VolumeWatcher::in_folders(vec![self.volumes.clone()]);
        Watching::new(w, WatchOptions { settle: Duration::ZERO, ..options })
    }

    fn pulled(&self) -> WatchOptions {
        WatchOptions { pull: true, ..WatchOptions::default() }
    }

    /// Polls once, then looks at what's due; what it did.
    fn step(&self, w: &mut Watching) -> Vec<WatchAction> {
        let events = w.watcher.poll();
        w.auto.observe(&events);
        self.look(w)
    }

    fn look(&self, w: &mut Watching) -> Vec<WatchAction> {
        let mut did = Vec::new();
        w.once(self.store_root(), &Ctx::silent(), &mut |x| {
            if let Watched::Did(a) = x {
                did.push(a.clone());
            }
        })
        .unwrap();
        did
    }

    fn snapshots(&self) -> usize {
        self.store.snapshot_ids().unwrap().len()
    }
}

fn pull(store: &Store, root: &Path) {
    let mut d = Device::open(root).unwrap();
    sync::pull(store, &mut d, &PullOptions { library: Some("Main".into()), ..Default::default() }, &Ctx::silent())
        .unwrap();
}

fn track_ids(root: &Path) -> Vec<u32> {
    Device::open(root).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect()
}

fn kinds(actions: &[WatchAction]) -> Vec<String> {
    actions.iter().map(|a| serde_json::to_value(a).unwrap()["event"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_drive_plugged_in_with_a_new_history_list_is_pulled() {
    let desk = Desk::new();
    let main = desk.plug(&desk.drive("Main"));
    pull(&desk.store, &main);
    let mut w = desk.watching(desk.pulled());
    let did = desk.look(&mut w);
    assert_eq!(kinds(&did), ["watch.up_to_date"], "{did:?}");

    // At a club, a player records a history list; then the drive comes home.
    let away = desk.unplug(&main);
    assert!(matches!(&w.watcher.poll()[..], [WatchEvent::Detached(_)]));
    common::add_history(&away, "HISTORY 001", &track_ids(&away)[..2]);
    let main = desk.plug(&away);
    let did = desk.step(&mut w);
    let [WatchAction::Pulled { drive, library, history_lists, beside, snapshot }] = &did[..] else { panic!("{did:?}") };
    assert_eq!((drive.root.as_path(), library.as_str()), (dunce_canonicalize(&main).as_path(), "Main"));
    assert_eq!((history_lists.as_slice(), *beside), (&["HISTORY 001".to_string()][..], false));
    let lib = desk.store.find_library("Main").unwrap().unwrap();
    assert_eq!(lib.head, Some(snapshot.id), "the newest snapshot");
    assert_eq!(desk.snapshots(), 2);

    // Its own pull changed only its state: nothing more to do.
    assert!(desk.step(&mut w).is_empty());
}

#[test]
fn without_pull_it_says_what_it_would_save() {
    let desk = Desk::new();
    let main = desk.drive("Main");
    pull(&desk.store, &main);
    common::add_history(&main, "HISTORY 001", &track_ids(&main)[..1]);
    desk.plug(&main);
    let did = desk.look(&mut desk.watching(WatchOptions::default()));
    let [WatchAction::Pending { history_lists, beside: false, .. }] = &did[..] else { panic!("{did:?}") };
    assert_eq!(history_lists, &["HISTORY 001"]);
    assert_eq!(desk.snapshots(), 1);
}

#[test]
fn retired_and_archive_drives_are_left_alone() {
    let desk = Desk::new();
    let (main, old) = (desk.drive("Main"), desk.drive("Old"));
    pull(&desk.store, &main);
    pull(&desk.store, &old);
    let id = |root: &Path| Device::open(root).unwrap().id().unwrap();
    fleet::retire(&desk.store, id(&old), false).unwrap();
    fleet::set_role(&desk.store, id(&main), Some(fleet::Role::Archive)).unwrap();
    let before = desk.snapshots();

    let mut w = desk.watching(desk.pulled());
    for root in [&main, &old] {
        common::add_history(root, "HISTORY 001", &track_ids(root)[..1]);
        desk.plug(root);
    }
    let did = desk.step(&mut w);
    let reasons: Vec<String> = did
        .iter()
        .map(|a| match a {
            WatchAction::Skipped { drive, reason } => format!("{}: {reason}", drive.name),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(reasons, ["Main: an archive drive, kept as it is", "Old: retired"]);
    assert_eq!(desk.snapshots(), before);
}

#[test]
fn a_library_is_never_started() {
    let desk = Desk::new();
    let mut w = desk.watching(desk.pulled());
    // Never pulled: not adopted, then adopted but in no library.
    let friend = desk.plug(&desk.drive("Friend"));
    let did = desk.step(&mut w);
    assert_eq!(kinds(&did), ["watch.skipped"]);
    Device::open(&friend).unwrap().adopt(None).unwrap();
    let mut w = desk.watching(desk.pulled());
    let did = desk.look(&mut w);
    let [WatchAction::Skipped { reason, .. }] = &did[..] else { panic!("{did:?}") };
    assert!(reason.contains("not linked to a library"), "{reason}");
    assert!(desk.store.libraries().unwrap().is_empty());
    assert_eq!(desk.snapshots(), 0);
}

#[test]
fn a_drive_back_from_a_gig_is_saved_beside_a_newer_export() {
    let desk = Desk::new();
    let (main, club) = (desk.drive("Main"), desk.dir.path().join("Club"));
    pull(&desk.store, &main);
    std::fs::create_dir(&club).unwrap();
    api::sync_apply(Some(&desk.store), &sync_req(&main, &club), &Ctx::silent()).unwrap();
    // While Club is out, Main gets a new export, which is pulled.
    common::add_history(&club, "HISTORY 001", &track_ids(&club)[..2]);
    common::export_new_track(&main, "Artist", "Fresh");
    pull(&desk.store, &main);
    let head = desk.store.find_library("Main").unwrap().unwrap().head;

    let mut w = desk.watching(desk.pulled());
    desk.plug(&club);
    let did = desk.step(&mut w);
    let [WatchAction::Pulled { beside: true, history_lists, snapshot, .. }] = &did[..] else { panic!("{did:?}") };
    assert_eq!(history_lists, &["HISTORY 001"]);
    assert_eq!(desk.store.find_library("Main").unwrap().unwrap().head, head, "the newest export stays the newest");
    let archive = locallibrary::plays::archive(&desk.store, None, &Ctx::silent()).unwrap();
    assert!(archive.lists.iter().any(|l| l.list.name == "HISTORY 001" && l.first_seen.snapshot == snapshot.id));

    // Seen again: saved already.
    let mut w = desk.watching(desk.pulled());
    assert_eq!(kinds(&desk.look(&mut w)), ["watch.up_to_date"]);
}

/// Regression: drives due together were looked at in mount-path order, so one back from a set
/// that sorts first was pulled on top of the latest snapshot, and the newer export after it
/// couldn't be saved.
#[test]
fn drives_due_together_are_saved_newest_export_first() {
    let desk = Desk::new();
    let (main, backup) = (desk.drive("Main"), desk.dir.path().join("Backup"));
    pull(&desk.store, &main);
    std::fs::create_dir(&backup).unwrap();
    api::sync_apply(Some(&desk.store), &sync_req(&main, &backup), &Ctx::silent()).unwrap();
    // Backup was played at a set; Main got a new export. Both are newer than the store.
    common::add_history(&backup, "HISTORY 001", &track_ids(&backup)[..2]);
    common::export_new_track(&main, "Artist", "Fresh");
    desk.plug(&backup);
    desk.plug(&main);

    let did = desk.look(&mut desk.watching(desk.pulled()));
    let saved: Vec<(String, bool)> = did
        .iter()
        .map(|a| match a {
            WatchAction::Pulled { drive, beside, .. } => (drive.name.clone(), *beside),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(saved, [("Main".to_string(), false), ("Backup".to_string(), true)]);
    let head = desk.store.find_library("Main").unwrap().unwrap().head.unwrap();
    assert_eq!(desk.store.load_snapshot(&head).unwrap().source.unwrap().drive_name, "Main");
}

/// Regression: drives mounted as watching started were looked at at once, while rekordbox
/// might still be exporting to them.
#[test]
fn drives_mounted_at_the_start_wait_to_settle() {
    let desk = Desk::new();
    let main = desk.drive("Main");
    pull(&desk.store, &main);
    common::add_history(&main, "HISTORY 001", &track_ids(&main)[..1]);
    desk.plug(&main);
    let watcher = || VolumeWatcher::in_folders(vec![desk.volumes.clone()]);

    let mut w = Watching::new(watcher(), WatchOptions { settle: Duration::from_secs(3600), ..desk.pulled() });
    assert!(desk.look(&mut w).is_empty(), "not settled yet");
    assert_eq!(desk.snapshots(), 1);

    // `watch --once` waits for them.
    let mut w = Watching::new(watcher(), WatchOptions { settle: Duration::from_millis(100), ..desk.pulled() });
    w.every = Duration::from_millis(10);
    let mut did = Vec::new();
    w.until_idle(desk.store_root(), &Ctx::silent(), &mut |x| {
        if let Watched::Did(a) = x {
            did.push(a.clone());
        }
    })
    .unwrap();
    assert_eq!(kinds(&did), ["watch.pulled"]);
}

#[test]
fn the_newest_drive_plugged_in_updates_the_rest_of_its_set() {
    let desk = Desk::new();
    let (main, backup) = (desk.drive("Main"), desk.dir.path().join("Backup"));
    pull(&desk.store, &main);
    std::fs::create_dir(&backup).unwrap();
    api::sync_apply(Some(&desk.store), &sync_req(&main, &backup), &Ctx::silent()).unwrap();
    let backup = desk.plug(&backup);
    let mut w = desk.watching(WatchOptions { sync_set: true, ..desk.pulled() });
    assert_eq!(kinds(&desk.look(&mut w)), ["watch.up_to_date"], "in sync: nothing to copy");

    // A new export on Main, which comes in: pulled, then Backup updated from it.
    common::export_new_track(&main, "Artist", "Fresh");
    desk.plug(&main);
    let did = desk.step(&mut w);
    assert_eq!(kinds(&did), ["watch.pulled", "watch.synced"], "{did:?}");
    let WatchAction::Synced { targets, .. } = &did[1] else { unreachable!() };
    assert_eq!(targets.len(), 1);
    assert!(targets[0].error.is_none() && targets[0].target == dunce_canonicalize(&backup));
    assert_eq!(track_ids(&backup).len(), track_ids(&desk.volumes.join("Main")).len());
    // Backup's databases changed: looked at, up to date, and its group already in sync.
    assert_eq!(kinds(&desk.step(&mut w)), ["watch.up_to_date"]);

    // Backup gets an export of its own while Main is behind it: Main isn't the source.
    common::export_new_track(&backup, "Artist", "Later");
    let did = desk.step(&mut w);
    assert_eq!(kinds(&did), ["watch.pulled", "watch.synced"], "Backup is newest: {did:?}");
}

fn sync_req(left: &Path, right: &Path) -> api::SyncRequest {
    api::SyncRequest {
        left: left.display().to_string(),
        right: right.display().to_string(),
        direction: api::Direction::Auto,
        allow_low_confidence: false,
        verify: false,
        keep_extra: false,
        delete_first: false,
        overwrite_archive: false,
        keep_player_changes: false,
    }
}
