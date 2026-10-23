//! Pulls and pushes the store has to turn down or ask about, and syncs between drives that can't
//! be made: a drive back from a set holding an older state of the library (push it, or force the
//! pull), one that went its own way (force it, pull it into another library, or merge), a drive
//! linked to no library while the store holds several, and a drive synced onto itself.

use crate::common;

use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::Error;
use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, PushOptions, SyncOptions};

/// A copy of the drive at `from` as it is now, a drive of its own (no identity or lineage).
fn copy_of(from: &Path, to: &Path) -> Device {
    common::copy_dir(from, to);
    let _ = fs::remove_dir_all(to.join(paths::STATE_DIR));
    Device::open_export(to).unwrap()
}

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_pull_the_store_cant_take_as_it_is_says_how_to_go_on(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let pull = |d: &mut Device, opts: &PullOptions| sync::pull(&store, d, opts, &Ctx::silent());
    let first = pull(&mut Device::open_export(&a).unwrap(), &PullOptions::default()).unwrap();
    let library = first.library.clone();
    // The guest drive: a copy of A as it was pulled.
    let mut guest = copy_of(&a, &dir.path().join("Guest"));
    // A moves on, and is pulled again: the library's head is past the guest's state.
    common::export_new_track(&a, "Later", "On A");
    let again = pull(&mut Device::open_export(&a).unwrap(), &PullOptions::default()).unwrap();
    assert!(!again.up_to_date);
    let unchanged = pull(&mut Device::open_export(&a).unwrap(), &PullOptions::default()).unwrap();
    assert!(unchanged.up_to_date && unchanged.imported_files == 0, "{unchanged:?}");

    // The guest stays as it was, or gets an export of its own.
    let diverged = tc.draw(gs::booleans());
    if diverged {
        common::export_new_track(guest.root(), "Later", "On the guest");
        guest = Device::open_export(guest.root()).unwrap();
    }
    let into = PullOptions { library: Some(library.clone()), ..Default::default() };
    let err = pull(&mut guest, &into).unwrap_err();
    let detail = err.detail();
    let retries: Vec<serde_json::Value> = detail
        .actions
        .iter()
        .filter(|a| a.method.is_none())
        .map(|a| serde_json::Value::Object(a.params.clone()))
        .collect();
    match diverged {
        false => {
            assert_eq!(err.reason(), Some(Reason::OlderThanStore), "{err}");
            assert!(err.to_string().contains(&first.snapshot.as_ref().unwrap().short_id), "names the snapshot: {err}");
            assert!(detail.actions.iter().any(|a| a.method.as_deref() == Some("push.plan")));
            assert_eq!(retries, [serde_json::json!({ "force": true })]);
        }
        true => {
            assert_eq!(err.reason(), Some(Reason::DivergedFromStore), "{err}");
            assert_eq!(retries, [serde_json::json!({ "force": true }), serde_json::json!({ "library": null })]);
        }
    }
    assert_eq!(detail.fields["library"], library);
    // Forced, it's the newest state.
    let forced = pull(&mut guest, &PullOptions { force: true, ..into }).unwrap();
    let lib = store.find_library(&library).unwrap().unwrap();
    assert_eq!(lib.head.map(|h| h.short()), forced.snapshot.map(|s| s.short_id));
}

/// Which snapshot a push writes to a drive: its own library's newest; for a drive linked to no
/// library, the only library's, or with several, the one named (else it asks which); none in an
/// empty store, by a name the store doesn't have, from a library without snapshots, or for a
/// drive linked to a library the store doesn't have.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_push_takes_the_library_it_can(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let blank = Device::open(common::blank(dir.path(), "Blank")).unwrap();
    let resolve = |d: &Device, library: Option<&str>| {
        sync::resolve_push_snapshot(
            &store,
            d,
            &PushOptions { library: library.map(String::from), ..Default::default() },
        )
    };
    assert!(resolve(&blank, None).unwrap_err().to_string().contains("no libraries yet"));
    let mut names = Vec::new();
    let mut drives = Vec::new();
    for n in 0..2 {
        let root = common::bigger_drive(dir.path(), &format!("Drive {n}"));
        if n == 1 {
            common::export_new_track(&root, "Other", "Library");
        }
        let opts = PullOptions { library: Some(format!("Library {n}")), ..Default::default() };
        let mut d = Device::open_export(&root).unwrap();
        names.push(sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap().library);
        drives.push(root);
        if n == 0 {
            let only = resolve(&blank, None).unwrap();
            assert_eq!(Some(only.id), store.find_library(&names[0]).unwrap().unwrap().head, "the only library");
        }
    }
    let head = |name: &str| store.find_library(name).unwrap().unwrap().head;
    // A drive pulled into a library pushes that one's newest.
    let linked = tc.draw(gs::integers::<usize>().max_value(drives.len() - 1));
    let own = resolve(&Device::open_export(&drives[linked]).unwrap(), None).unwrap();
    assert_eq!(Some(own.id), head(&names[linked]));
    for named in [None, Some("Library 0"), Some("library 1"), Some("Nope")] {
        match (resolve(&blank, named), named) {
            (Ok(s), Some(name)) => assert_eq!(Some(s.id), head(name)),
            (Err(e), Some(name)) => assert!(name == "Nope" && e.to_string().contains(name), "{e}"),
            (Err(e), None) => {
                assert_eq!(e.reason(), Some(Reason::ChooseLibrary), "{e}");
                assert_eq!(e.detail().fields["libraries"], serde_json::json!(names));
            }
            (Ok(s), None) => panic!("{s:?} picked from two libraries"),
        }
    }
    // A library nothing was pulled into has nothing to push, and a store without the library a
    // drive is linked to doesn't pick another for it.
    let empty = store.create_library(&format!("Empty {}", tc.draw(gs::integers::<u8>()))).unwrap();
    let e = resolve(&blank, Some(&empty.name)).unwrap_err();
    assert_eq!(e.to_string(), Error::NotFound(format!("library {:?} has no snapshots", empty.name)).to_string());
    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    other.create_library(&names[linked]).unwrap();
    let d = Device::open_export(&drives[linked]).unwrap();
    let id = d.state().and_then(|s| s.library_id).unwrap();
    let e = sync::resolve_push_snapshot(&other, &d, &PushOptions::default()).unwrap_err();
    assert_eq!(e.to_string(), Error::NotFound(format!("library {id}")).to_string());
}

/// A sync that can't be made is refused before anything is read or written, whatever its
/// options: a drive onto itself, from a folder with no export, or with a confirmed plan missing
/// for a target.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn syncs_that_cant_be_made_are_refused(tc: TestCase) {
    for variant in 0..5 {
        syncs_that_cant_be_made_are_refused_with(&tc, variant);
    }
}

/// One case of [`syncs_that_cant_be_made_are_refused`], `variant` choosing what's drawn.
#[hegel::test_helper]
fn syncs_that_cant_be_made_are_refused_with(tc: &TestCase, variant: u8) {
    let dir = tempfile::tempdir().unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let b = common::blank(dir.path(), "B");
    let before = common::drive_files(&a);
    // Whatever the options say.
    let opts = SyncOptions {
        delete: tc.draw(gs::booleans()),
        verify: tc.draw(gs::booleans()),
        delete_first: tc.draw(gs::booleans()),
    };
    let silent = Ctx::silent();
    let (mut src, mut dst) = (Device::open(&a).unwrap(), Device::open(&b).unwrap());
    let (err, why): (locallibrary::Result<()>, &str) = match variant {
        0 => (
            sync::sync_drive_to_drive(&mut src, &mut Device::open(&a).unwrap(), &opts, &silent).map(drop),
            "same drive",
        ),
        1 => (sync::plan_drive_to_drive(&dst, &src, &opts, &silent).map(drop), "no export"),
        2 => (
            sync::sync_drive_to_many(&mut dst, &mut [Device::open(&a).unwrap()], &opts, &silent).map(drop),
            "no export",
        ),
        3 => (
            sync::sync_drive_to_many(&mut src, &mut [Device::open(&a).unwrap()], &opts, &silent).map(drop),
            "both the source and a target",
        ),
        _ => (
            sync::sync_drive_to_many_as_planned(&mut src, std::slice::from_mut(&mut dst), &opts, &[], &silent)
                .map(drop),
            "one confirmed plan per target",
        ),
    };
    let err = err.unwrap_err();
    match why {
        "no export" => assert!(matches!(err, locallibrary::Error::NotAnExport(_)), "{err}"),
        _ => assert!(err.to_string().contains(why), "{err}"),
    }
    assert!(common::changed_files(&before, &common::drive_files(&a)).is_empty());
    assert!(common::drive_files(&b).is_empty());
}
