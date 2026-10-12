//! A copy of the store stopped part way, written to by itself, or that can't be made: it carries
//! on from where it stopped and clears what a pulled plug left, refuses to go over snapshots only
//! the copy has unless forced, and says where it can't go or what it can't read.

use crate::common;

use std::collections::BTreeSet;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::offsite::{self, CopyOptions};
use locallibrary::progress::{CancelToken, Ctx, Event, Phase};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Error, Hash, paths};

fn ids(store: &Store) -> BTreeSet<Hash> {
    store.snapshot_ids().unwrap().into_iter().collect()
}

/// A copy cancelled as its `at`th file starts.
fn stopped(store: &Store, dest: &std::path::Path, at: usize) -> Error {
    let cancel = CancelToken::new();
    let seen = AtomicUsize::new(0);
    let token = cancel.clone();
    let progress = move |e: &Event| {
        if matches!(e, Event::Item { phase: Phase::Copy, .. }) && seen.fetch_add(1, Ordering::SeqCst) + 1 == at {
            token.cancel();
        }
    };
    offsite::copy(store, dest, &CopyOptions::default(), &Ctx::new(&progress, cancel)).unwrap_err()
}

/// A copy stopped at any file carries on from there, clearing what a pulled plug left half
/// written among its files and snapshots, and ends holding the store. Stopped while catching up,
/// the copy may have been pulled into since (with `--store`): copying over the snapshots only it
/// has is refused unless forced. Labels come along.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_stopped_copy_carries_on_from_where_it_stopped(tc: TestCase) {
    for later in [false, true] {
        a_stopped_copy_carries_on_from_where_it_stopped_with(&tc, later);
    }
}

/// One case of [`a_stopped_copy_carries_on_from_where_it_stopped`]: `later` when the copy is
/// stopped catching up rather than the first time.
#[hegel::test_helper]
fn a_stopped_copy_carries_on_from_where_it_stopped_with(tc: &TestCase, later: bool) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    let pull = |s: &Store, opts: &PullOptions| {
        sync::pull(s, &mut Device::open_export(&a).unwrap(), opts, &Ctx::silent()).unwrap()
    };
    let first = pull(&store, &PullOptions::default());
    let first_id = first.snapshot.as_ref().unwrap().id;
    let labelled = tc.draw(gs::booleans());
    if labelled {
        store.add_label(&first_id, "before the set").unwrap();
    }
    let dest = dir.path().join("copy");
    if later {
        offsite::copy(&store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
        common::export_new_track(&a, "New", "Track");
        pull(&store, &PullOptions::default());
    }
    // Stopped at any of the files it copies: all of them the first time, at least the new
    // track's audio, analysis and export.pdb when catching up.
    let todo = if later { 3 } else { store.verify(&Ctx::silent()).unwrap().checked };
    let at = tc.draw(gs::integers::<usize>().min_value(1).max_value(todo));
    let err = stopped(&store, &dest, at);
    assert!(matches!(err, Error::Cancelled), "{err}");
    let marker = offsite::marker(&dest).unwrap().unwrap();
    assert!(marker.finished.is_none() && marker.last_complete.is_some() == later);

    // What a pulled plug leaves: temp files among the stored files and snapshots.
    let left: Vec<std::path::PathBuf> = ["objects", "snapshots"]
        .into_iter()
        .filter(|_| tc.draw(gs::booleans()))
        .map(|d| dest.join(d).join(format!("{}stopped", paths::TEMP_PREFIX)))
        .collect();
    for p in &left {
        fs::write(p, b"half written").unwrap();
    }
    // Pulled into while it was behind: the copy has a snapshot the store doesn't.
    let pulled_into = later && tc.draw(gs::booleans());
    if pulled_into {
        let copy = Store::open(&dest).unwrap();
        common::export_new_track(&a, "Only", "On the copy");
        let opts = PullOptions { library: Some(first.library.clone()), force: true, ..Default::default() };
        pull(&copy, &opts);
        let err = offsite::copy(&store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap_err();
        assert_eq!(err.reason(), Some(Reason::CopyAhead), "{err}");
        assert!(err.to_string().contains("the copy has snapshots this store doesn't"), "{err}");
        let retry: Vec<_> = err.detail().actions.iter().map(|a| serde_json::Value::Object(a.params.clone())).collect();
        assert_eq!(retry, [serde_json::json!({ "force": true })]);
    }
    let opts = CopyOptions { force: pulled_into, ..Default::default() };
    let r = offsite::copy(&store, &dest, &opts, &Ctx::silent()).unwrap();
    assert!(r.complete, "{r:?}");
    let copy = Store::open(&dest).unwrap();
    assert!(ids(&copy).is_superset(&ids(&store)));
    let labels: Vec<(Hash, String)> = copy.labels().unwrap().into_iter().map(|l| (l.snapshot, l.name)).collect();
    assert_eq!(labels, if labelled { vec![(first_id, "before the set".to_string())] } else { Vec::new() });
    assert_eq!(copy.libraries().unwrap()[0].head, store.libraries().unwrap()[0].head);
    for p in &left {
        assert!(!p.exists(), "{} is cleared", p.display());
    }
    let v = copy.verify(&Ctx::silent()).unwrap();
    assert!(v.missing.is_empty() && v.corrupt.is_empty(), "{v:?}");
}

/// A copy that can't be made says why, and leaves the way open: under a file (no folder, nor its
/// marker, can go there), or with a record of the store's that can't be read (it says which, and
/// once it can be, the copy carries on). A snapshot one of whose files the store lost is left out.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_copy_that_cant_be_made_says_why(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    sync::pull(&store, &mut Device::open_export(&a).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();

    let file = dir.path().join("notes.txt");
    fs::write(&file, b"x").unwrap();
    let under = file.join(tc.draw(gs::sampled_from(vec!["copy", "NAS"])));
    let named = std::path::Path::new("notes.txt").join(under.file_name().unwrap());
    // Its marker can't be read there; forced past that, its folder can't be made.
    for (force, at) in [(false, named.join("copy.json")), (true, named.clone())] {
        let err =
            offsite::copy(&store, &under, &CopyOptions { force, ..Default::default() }, &Ctx::silent()).unwrap_err();
        assert!(matches!(&err, Error::Io { path, .. } if path.ends_with(&at)), "{err}");
    }

    let record = tc.draw(gs::sampled_from(vec!["settings.json", "smart.json", "players.json"]));
    let at = store.root().join(record);
    let _ = fs::remove_file(&at);
    fs::create_dir(&at).unwrap();
    let dest = dir.path().join("copy");
    let err = offsite::copy(&store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(matches!(&err, Error::Io { path, .. } if path.ends_with(record)), "{err}");
    fs::remove_dir(&at).unwrap();
    let r = offsite::copy(&store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
    assert!(r.complete && ids(&Store::open(&dest).unwrap()) == ids(&store), "{r:?}");

    // The store moves on, and loses a file only its new snapshot has: that snapshot is left out
    // of the copy, saying why, and the copy keeps the library's earlier head.
    let before = store.libraries().unwrap()[0].head;
    common::export_new_track(&a, "New", "Track");
    let pulled =
        sync::pull(&store, &mut Device::open_export(&a).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
    let snap = store.load_snapshot(&pulled.snapshot.unwrap().id).unwrap();
    let old: BTreeSet<Option<Hash>> =
        before.iter().flat_map(|h| store.load_snapshot(h).unwrap().files).map(|f| f.hash).collect();
    let only_new: Vec<_> = snap.files.iter().filter(|f| !old.contains(&f.hash)).collect();
    let lost = only_new[tc.draw(gs::integers::<usize>().max_value(only_new.len() - 1))];
    fs::remove_file(store.object_path(&lost.hash.unwrap())).unwrap();
    let r = offsite::copy(&store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
    assert!(!r.complete);
    // Named by the first file with that content.
    let named = &snap.files.iter().find(|f| f.hash == lost.hash).unwrap().path;
    assert_eq!(r.problems, [format!("snapshot {} left out: {named} is missing from the store", snap.id.short())]);
    let copy = Store::open(&dest).unwrap();
    assert_eq!(copy.libraries().unwrap()[0].head, before);
    assert!(!ids(&copy).contains(&snap.id));
}
