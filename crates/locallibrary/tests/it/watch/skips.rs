//! What watching leaves alone, and says so: a drive unplugged before it settled, drives that
//! aren't linked to a library in the store (or when there's no store), and a drive it can't
//! read. And a watch told to stop, stops.

use crate::common::{self, path};

use std::path::Path;
use std::time::Duration;

use locallibrary::api::{self, AutoPull, WatchAction, WatchOptions, Watched, Watching};
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::watch::{VolumeWatcher, WatchEvent};

/// What each drive came to, by its name, sorted.
fn outcomes(actions: &[WatchAction]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = actions
        .iter()
        .map(|a| {
            let what = match a {
                WatchAction::Skipped { reason, .. } => reason.clone(),
                WatchAction::Failed { kind, .. } => format!("failed: {kind}"),
                WatchAction::UpToDate { .. } => "up to date".into(),
                other => format!("{other:?}"),
            };
            (a.drive().name.clone(), what)
        })
        .collect();
    out.sort();
    out
}

/// A drive plugged in and unplugged before it settled is never looked at.
#[test]
fn a_drive_unplugged_before_it_settles_is_forgotten() {
    let dir = tempfile::tempdir().unwrap();
    common::demo_drive(dir.path(), "Main");
    let drive = VolumeWatcher::in_folders(vec![dir.path().to_path_buf()]).drives().remove(0);
    let mut auto = AutoPull::new(WatchOptions::default());
    auto.observe(&[WatchEvent::Attached(drive.clone())]);
    assert!(!auto.is_idle());
    auto.observe(&[WatchEvent::Detached(drive)]);
    assert!(auto.is_idle());
}

/// Drives that aren't linked to a library are skipped, saying how to link them; without a store
/// at all, a linked drive is skipped too. A drive whose database can't be read fails alone.
#[test]
fn drives_watching_cant_save_are_skipped_or_reported() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = dir.path().join("volumes");
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let linked = common::demo_drive(&volumes, "Linked");
    common::adopt(None, &linked, "Linked");
    api::pull(&store, &api::PullRequest { drive: path(&linked), ..Default::default() }, &Ctx::silent()).unwrap();
    common::adopt(None, &common::demo_drive(&volumes, "Adopted"), "Adopted");
    common::demo_drive(&volumes, "Bare");
    let broken = common::demo_drive(&volumes, "Broken");
    common::adopt(None, &broken, "Broken");
    api::pull(&store, &api::PullRequest { drive: path(&broken), ..Default::default() }, &Ctx::silent()).unwrap();
    std::fs::write(broken.join(paths::EXPORT_PDB), b"not a database").unwrap();

    let mounted = VolumeWatcher::in_folders(vec![volumes]).drives();
    let look = |store_root: &Path| {
        let mut auto = AutoPull::new(WatchOptions { pull: true, settle: Duration::ZERO, ..Default::default() });
        auto.check_now(&mounted);
        let mut did = Vec::new();
        auto.run(store_root, &mounted, &Ctx::silent(), &mut |a| did.push(a.clone())).unwrap();
        assert!(auto.is_idle());
        outcomes(&did)
    };
    let unlinked = "not linked to a library in the store (pull it once to start one)".to_string();
    let got = look(store.root());
    assert_eq!(got[0], ("Adopted".into(), unlinked.clone()));
    assert_eq!(got[1], ("Bare".into(), unlinked));
    assert_eq!(got[2].0, "Broken");
    assert!(got[2].1.starts_with("failed: "), "{got:?}");
    assert_eq!(got[3], ("Linked".into(), "up to date".into()));
    assert_eq!(got.len(), 4);

    let nowhere = look(&dir.path().join("no store"));
    let linked_without = nowhere.iter().find(|(name, _)| name == "Linked").unwrap();
    assert_eq!(linked_without.1, "there's no store yet (pull a drive once to start one)");
}

/// A watch told to stop looks at what was due first, then stops.
#[test]
fn a_watch_stops_when_told_to() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let volumes = dir.path().join("volumes");
    let main = common::demo_drive(&volumes, "Main");
    common::adopt(None, &main, "Main");
    api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &Ctx::silent()).unwrap();
    let watcher = VolumeWatcher::in_folders(vec![volumes]);
    let mut w = Watching::new(watcher, WatchOptions { settle: Duration::ZERO, ..Default::default() });
    let mut did = Vec::new();
    w.run(store.root(), &Ctx::silent(), &|| true, &mut |x| {
        if let Watched::Did(a) = x {
            did.push(a.clone());
        }
    })
    .unwrap();
    assert_eq!(outcomes(&did).len(), 1);
    assert!(matches!(&did[0], WatchAction::UpToDate { library, .. } if library == "Main"), "{did:?}");
}
