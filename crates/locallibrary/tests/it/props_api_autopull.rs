//! Watching a library's drives with `sync_set`, step by step, against a model of which drive
//! holds the newest state: what each drive is told, and which way its group is synced.

use crate::common;
use crate::props_api_drives::{saved_copy, saved_demo};
use crate::props_api_jobs::{bare_export, own_identity};

use std::path::{Path, PathBuf};
use std::time::Duration;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{AutoPull, WatchAction, WatchOptions, Watched, Watching};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::progress::Ctx;
use locallibrary::sync;
use locallibrary::watch::{MountedDrive, VolumeWatcher};

/// What one look at every drive in `volumes` did.
fn watch(store_root: &Path, volumes: &Path, pull: bool, sync_set: bool) -> Vec<WatchAction> {
    let mut w = Watching::new(
        VolumeWatcher::in_folders(vec![volumes.to_path_buf()]),
        WatchOptions { pull, sync_set, settle: Duration::ZERO },
    );
    let mut did = Vec::new();
    w.once(store_root, &Ctx::silent(), &mut |x| {
        if let Watched::Did(a) = x {
            did.push(a.clone());
        }
    })
    .unwrap();
    did
}

/// What `root` was told, as `event: reason` lines (the reason only for a skip).
fn told(did: &[WatchAction], root: &Path) -> Vec<String> {
    did.iter()
        .filter(|a| a.drive().root == root)
        .map(|a| {
            let event = serde_json::to_value(a).unwrap()["event"].as_str().unwrap().to_string();
            match a {
                WatchAction::Skipped { reason, .. } => format!("{event}: {reason}"),
                WatchAction::Failed { kind, .. } => format!("{event}: {kind}"),
                WatchAction::Pulled { beside, .. } | WatchAction::Pending { beside, .. } if *beside => {
                    format!("{event} beside")
                }
                _ => event,
            }
        })
        .collect()
}

/// A rekordbox export (a track, or a history list a player recorded, as drawn).
fn change(tc: &TestCase, root: &Path, what: &str) -> bool {
    let history = tc.draw(gs::booleans());
    match history {
        true => {
            let ids: Vec<u32> = common::library(root).tracks.iter().map(|t| t.id).collect();
            common::add_history(root, &format!("HISTORY {what}"), &ids[..1]);
        }
        false => {
            common::export_new_track(root, "Artist", what);
        }
    }
    history
}

fn same(a: &Path, b: &Path) -> bool {
    common::library(a) == common::library(b)
}

/// Main and Backup, clones of the library Main, watched as they change in turn, beside drives
/// that can't be saved. Without a store, adopted drives wait for one. A drive linked to a library
/// the store lacks isn't linked; one holding another library, or behind the library's newest
/// snapshot, is left alone, saying so; one that can't be read fails; one of a library with no
/// snapshot yet is its first. With `sync_set`, a drive found up to date or pulled copies itself
/// onto the rest of its group when it's the newest, and says why not otherwise: another is newer,
/// or the drives diverged (then the other is left for a merge, or saved beside the newest
/// snapshot when only players changed it).
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_watched_group_is_synced_from_its_newest_drive(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved_copy(dir.path());
    let volumes = main.parent().unwrap().to_path_buf();
    let in_volumes = |name: &str| dunce_canonicalize(&volumes).join(name);
    let backup = in_volumes("Backup");
    saved_demo().1.write(&backup);
    own_identity(&store, &mut Device::open(&backup).unwrap(), "Backup");
    let main = dunce_canonicalize(&main);
    let lib = store.find_library("Main").unwrap().unwrap();

    // Drives that can't be saved, each in its own way.
    let link = |root: &Path, name: &str, library: uuid::Uuid| {
        let mut d = Device::open(root).unwrap();
        d.adopt(Some(name)).unwrap();
        d.state_mut().unwrap().library_id = Some(library);
        d.save_state().unwrap();
        sync::register_drive(&store, &d, None).unwrap();
    };
    let broken = in_volumes("Broken");
    bare_export(&broken);
    link(&broken, "Broken", lib.id);
    std::fs::write(broken.join(locallibrary::paths::EXPORT_PDB), b"not a database").unwrap();
    let stray = in_volumes("Stray");
    bare_export(&stray);
    link(&stray, "Stray", uuid::Uuid::new_v4());
    let other = in_volumes("Other");
    common::bigger_drive(&volumes, "Other");
    link(&other, "Other", lib.id);
    let radio = store.create_library("Radio").unwrap();
    let fresh = in_volumes("Fresh");
    bare_export(&fresh);
    link(&fresh, "Fresh", radio.id);

    // No store yet: adopted drives wait for one.
    let did = watch(&dir.path().join("no store"), &volumes, true, false);
    for root in [&main, &backup, &stray] {
        assert_eq!(told(&did, root), ["watch.skipped: there's no store yet (pull a drive once to start one)"]);
    }
    // A drive with no export, though the watcher saw one, is passed over.
    let blank = common::blank(&volumes, "Blank");
    let mut auto = AutoPull::new(WatchOptions { settle: Duration::ZERO, ..Default::default() });
    let claimed = MountedDrive { root: dunce_canonicalize(&blank), id: None, name: "Blank".into(), has_export: true };
    auto.check_now(std::slice::from_ref(&claimed));
    let mut said = Vec::new();
    auto.run(store.root(), &[claimed], &Ctx::silent(), &mut |a| said.push(a.clone())).unwrap();
    assert!(said.is_empty() && auto.is_idle(), "{said:?}");
    std::fs::remove_dir(blank).unwrap();

    // Main moves on, and is saved; Backup is now behind.
    change(&tc, &main, "at home");
    let did = watch(store.root(), &volumes, true, false);
    assert_eq!(told(&did, &main), ["watch.pulled"]);
    assert_eq!(told(&did, &backup), ["watch.skipped: behind \"Main\"'s newest snapshot: nothing new to save"]);
    assert_eq!(told(&did, &broken).len(), 1);
    assert!(told(&did, &broken)[0].starts_with("watch.failed: "), "{:?}", told(&did, &broken));
    assert_eq!(told(&did, &stray), ["watch.skipped: not linked to a library in the store (pull it once to start one)"]);
    assert_eq!(told(&did, &other), ["watch.skipped: holds a different library from \"Main\""]);
    assert_eq!(told(&did, &fresh), ["watch.pulled"]);
    assert!(store.find_library("Radio").unwrap().unwrap().head.is_some(), "its first snapshot");
    for root in [&broken, &stray, &other, &fresh] {
        std::fs::remove_dir_all(root).unwrap();
    }

    // With its group: Main, up to date, copies itself onto Backup; then neither has anything to do.
    let did = watch(store.root(), &volumes, true, true);
    assert_eq!(told(&did, &main), ["watch.up_to_date", "watch.synced"]);
    let synced: Vec<&WatchAction> = did.iter().filter(|a| matches!(a, WatchAction::Synced { .. })).collect();
    let [WatchAction::Synced { targets, .. }] = &synced[..] else { panic!("{did:?}") };
    assert_eq!(targets.iter().map(|t| &t.target).collect::<Vec<&PathBuf>>(), [&backup]);
    assert!(same(&main, &backup));
    let did = watch(store.root(), &volumes, true, true);
    assert_eq!(
        (told(&did, &main), told(&did, &backup)),
        (vec!["watch.up_to_date".to_string()], vec!["watch.up_to_date".to_string()])
    );

    // Backup moves on, unsaved: Main isn't copied over it.
    let backup_played = change(&tc, &backup, "at the club");
    let did = watch(store.root(), &volumes, false, true);
    assert_eq!(told(&did, &backup), ["watch.pending"]);
    assert_eq!(told(&did, &main), ["watch.up_to_date", "watch.skipped: not syncing its group: Backup is newer"]);

    // Main moves on too. The two are saved exports first, then drives only players changed, by
    // path between equals: the first is pulled, and as the two diverged its group isn't synced;
    // the second, changed only by players since the state it shared with the library, is saved
    // beside the newest snapshot, and is otherwise left alone.
    let main_played = change(&tc, &main, "at home again");
    let (first, second, second_played) = match u8::from(backup_played) <= u8::from(main_played) {
        true => (&backup, &main, main_played),
        false => (&main, &backup, backup_played),
    };
    let did = watch(store.root(), &volumes, true, true);
    let firsts = told(&did, first);
    assert_eq!(firsts[0], "watch.pulled", "{did:?}");
    assert!(firsts[1].starts_with("watch.skipped: not syncing its group: "), "{firsts:?}");
    match second_played {
        true => assert_eq!(told(&did, second), ["watch.pulled beside"]),
        false => assert_eq!(
            told(&did, second),
            ["watch.skipped: both it and \"Main\" changed since they last matched: merge, or sync it by hand"]
        ),
    }
    assert!(!same(&main, &backup), "neither copied over the other");
}

/// Watching Main while rekordbox exports to it (a track, or a history list a player recorded,
/// as drawn): the poll sees its databases change, and the next look saves it; the pull rewrites
/// its state, which the next poll sees, and nothing more after that.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_drive_changed_while_watched_is_seen_and_saved(tc: TestCase) {
    use locallibrary::watch::{DriveChange, WatchEvent};
    use std::cell::Cell;
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved_copy(dir.path());
    let volumes = main.parent().unwrap().to_path_buf();
    let main = dunce_canonicalize(&main);
    let mut w = Watching::new(
        VolumeWatcher::in_folders(vec![volumes]),
        WatchOptions { pull: true, sync_set: false, settle: Duration::ZERO },
    );
    w.every = Duration::from_millis(1);
    let mut first = Vec::new();
    w.once(store.root(), &Ctx::silent(), &mut |x| {
        if let Watched::Did(a) = x {
            first.push(a.clone());
        }
    })
    .unwrap();
    assert_eq!(told(&first, &main), ["watch.up_to_date"]);

    change(&tc, &main, "while watched");
    let pulled = Cell::new(false);
    let mut seen: Vec<WatchEvent> = Vec::new();
    w.run(store.root(), &Ctx::silent(), &|| pulled.get(), &mut |x| match x {
        Watched::Seen(e) => seen.push(e.clone()),
        Watched::Did(a) => pulled.set(pulled.get() || matches!(a, WatchAction::Pulled { .. })),
    })
    .unwrap();
    assert!(pulled.get());
    let databases = |e: &WatchEvent| matches!(e, WatchEvent::Changed { drive, changed } if drive.root == main && changed.contains(&DriveChange::Databases));
    assert!(seen.iter().any(databases), "{seen:?}");
    let after = w.watcher.poll();
    let state =
        |e: &WatchEvent| matches!(e, WatchEvent::Changed { changed, .. } if changed.contains(&DriveChange::State));
    assert!(after.iter().any(state), "the pull rewrote its state: {after:?}");
    assert_eq!(w.watcher.poll(), [], "nothing since");
}
