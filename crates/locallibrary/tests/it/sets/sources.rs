//! Sets built from what was recorded: the history lists players saved (sessions), added to a set
//! later, and a recording of the set, matched against the drive and kept in the store.

use crate::common;

use std::path::{Path, PathBuf};

use locallibrary::api::{self, EditSet, NewSet, SessionsRequest};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

/// A store named for this thread and Main, a drive in the only folder drives are looked for in,
/// with two history lists.
fn setup(dir: &Path) -> (Store, PathBuf) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    let volumes = dir.join("volumes");
    let main = common::bigger_drive(&volumes, "Main");
    locallibrary::testing::volumes_on_this_thread(Some(vec![volumes]));
    common::add_history(&main, "HISTORY 001", &[1, 2, 3]);
    common::add_history(&main, "HISTORY 002", &[4, 5]);
    (store, main)
}

/// The id of the session of this name.
fn session(store: &Store, name: &str) -> String {
    let got = api::sessions(Some(store), &SessionsRequest::default(), &Ctx::silent()).unwrap();
    got.sessions.into_iter().find(|s| s.name == name).unwrap().id
}

/// A set built from a session holds its tracks; a session added later appends its own, and one
/// that doesn't exist is refused.
#[test]
fn a_set_is_built_from_sessions_and_grows_with_more() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = setup(dir.path());
    let first = session(&store, "HISTORY 001");
    let new = NewSet { sessions: vec![first.clone(), first.clone()], ..Default::default() };
    let set = api::set_new(&store, &new, &Ctx::silent()).unwrap().set;
    assert_eq!((set.sessions.len(), set.tracks.len()), (1, 3), "each session once");

    let add = |sessions: Vec<String>| EditSet { set: "last".into(), add_sessions: sessions, ..Default::default() };
    assert!(api::set_edit(&store, &add(vec!["zzzzzzzz".into()]), &Ctx::silent()).is_err());
    let grown = api::set_edit(&store, &add(vec![session(&store, "HISTORY 002")]), &Ctx::silent()).unwrap().set;
    assert_eq!((grown.sessions.len(), grown.tracks.len()), (2, 5));
}

/// A recording is matched against the drive and its report kept; one that isn't there, isn't
/// audio, or comes with a report too is refused, and so is attaching one along with changes by
/// position.
#[test]
fn a_set_is_built_from_its_recording() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = setup(dir.path());
    let from = |recording: &Path, report: Option<&str>| NewSet {
        recording: Some(recording.to_path_buf()),
        report: report.map(Into::into),
        drive: Some(main.display().to_string()),
        ..Default::default()
    };
    let new = |req: &NewSet| api::set_new(&store, req, &Ctx::silent());

    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, "not audio").unwrap();
    assert!(new(&from(&dir.path().join("gone.wav"), None)).unwrap_err().to_string().contains("gone.wav"));
    assert!(new(&from(&notes, None)).unwrap_err().to_string().contains("isn't a recording mix reads"));
    assert!(new(&from(&notes, Some("abcd"))).unwrap_err().to_string().contains("not both"));

    // Silence plays none of the drive's tracks (their audio here is placeholder bytes anyway).
    let wav = dir.path().join("set.wav");
    std::fs::write(&wav, common::media::wav()).unwrap();
    let set = new(&from(&wav, None)).unwrap().set;
    let kept = set.recording.as_ref().and_then(|r| r.report.clone()).expect("the report is kept");
    assert_eq!(api::mix_reports_list(store.root()).unwrap()[0].id, kept);
    assert!(set.tracks.is_empty());

    let both = EditSet { set: set.id.clone(), report: Some(kept), drop: vec![1], ..Default::default() };
    let err = api::set_edit(&store, &both, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("attach the recording first"), "{err}");
    assert!(Device::open(&main).unwrap().has_export(), "the drive is only read");
}
