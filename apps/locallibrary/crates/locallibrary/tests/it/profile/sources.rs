//! Where a profile is read from: the drives named (each holding an export), else the mounted
//! drives the store's registry says are the DJ's, and the store's libraries named. With neither
//! a drive nor a store there's nothing to read.

use crate::common::{self, path};

use locallibrary::Error;
use locallibrary::api::{self, ProfileRequest};
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

fn profile(store: Option<&Store>, req: &ProfileRequest) -> locallibrary::Result<api::ProfileView> {
    api::profile(store, &ProfileRequest { no_analysis: true, ..req.clone() }, &Ctx::silent())
}

/// The names of the drives a profile read.
fn drives_read(view: &api::ProfileView) -> Vec<String> {
    view.report.sources.iter().filter(|s| s.kind == "drive").map(|s| s.name.clone()).collect()
}

/// Mounted drives count only when the registry has them, once it has any; with no drive and no
/// store, the profile is refused.
#[test]
fn the_djs_drives_are_the_ones_the_registry_knows() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = dir.path().join("volumes");
    std::fs::create_dir(&volumes).unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![volumes.clone()]));
    let err = profile(None, &ProfileRequest::default()).expect_err("nothing to read");
    assert_eq!(err.reason(), Some(Reason::NoStore), "{err}");

    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let main = common::demo_drive(&volumes, "Main");
    common::demo_drive(&volumes, "Borrowed");
    let mut everything = drives_read(&profile(Some(&store), &ProfileRequest::default()).unwrap());
    everything.sort();
    assert_eq!(everything, ["Borrowed", "Main"], "an empty registry takes every mounted drive");
    common::adopt(Some(&store), &main, "Main");
    assert_eq!(drives_read(&profile(Some(&store), &ProfileRequest::default()).unwrap()), ["Main"]);
}

/// A drive named must hold an export, and a library named must be in the store.
#[test]
fn named_sources_must_be_there() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let blank = dir.path().join("Blank");
    std::fs::create_dir(&blank).unwrap();
    let err = profile(None, &ProfileRequest { drives: vec![path(&blank)], ..Default::default() }).err().unwrap();
    assert!(err.to_string().contains("has no rekordbox export"), "{err}");

    let main = common::demo_drive(dir.path(), "Main");
    let named = |libraries: Vec<String>| ProfileRequest { drives: vec![path(&main)], libraries, ..Default::default() };
    let err = profile(Some(&store), &named(vec!["Nope".into()])).err().unwrap();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("Nope")), "{err}");
    let pull = api::PullRequest { drive: path(&main), ..Default::default() };
    api::pull(&store, &pull, &Ctx::silent()).unwrap();
    let twice = ProfileRequest { drives: vec![path(&main), path(&main)], ..Default::default() };
    assert_eq!(drives_read(&profile(Some(&store), &twice).unwrap()), ["Main"], "read once");
    assert!(profile(Some(&store), &named(vec!["Main".into()])).is_ok());
}
