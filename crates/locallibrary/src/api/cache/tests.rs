use std::sync::mpsc;

use super::*;
use crate::Error;
use crate::mix::index::Cache;
use crate::progress::{CancelToken, Event};

/// A request for the identifier while another builds it waits for that build, and can be
/// cancelled while it waits; nothing builds it twice.
#[test]
fn the_identifier_is_built_once_and_waiting_for_it_can_be_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let cached = CachedLibrary {
        root: dir.path().into(),
        stamp: LibraryStamp::of(dir.path()),
        library: Library::default(),
        tags: Mutex::default(),
        tags_by_track: Mutex::default(),
        identifier: Mutex::default(),
        built: Condvar::new(),
    };
    let fingerprints = Cache::new(dir.path().join("fingerprints"));
    let (started, building) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let (cached, root) = (&cached, dir.path());
    std::thread::scope(|s| {
        let first = s.spawn(|| {
            cached.identifier(root, &Ctx::silent(), move |library| {
                started.send(()).unwrap();
                released.recv().unwrap();
                Identifier::with_library(&device, library.into(), &fingerprints, &Ctx::silent())
            })
        });
        building.recv().unwrap();
        let (done, gave_up) = mpsc::channel();
        s.spawn(move || {
            let cancel = CancelToken::new();
            cancel.cancel();
            let quiet = |_: &Event| {};
            let r = cached.identifier(root, &Ctx::new(&quiet, cancel), |_| panic!("built twice"));
            let _ = done.send(r.err().is_some_and(|e| matches!(e.inner(), Error::Cancelled)));
        });
        // While the first is still building (it's released whatever happens, so nothing
        // waits for ever when this fails).
        let cancelled = gave_up.recv_timeout(std::time::Duration::from_secs(10));
        let second = s.spawn(|| cached.identifier(root, &Ctx::silent(), |_| panic!("built twice")));
        release.send(()).unwrap();
        assert_eq!(cancelled, Ok(true), "the waiting request saw its cancel");
        let (a, b) = (first.join().unwrap().unwrap(), second.join().unwrap().unwrap());
        assert!(Arc::ptr_eq(&a, &b));
    });
}
