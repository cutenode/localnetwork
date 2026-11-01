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
        library: Arc::default(),
        combined: Mutex::default(),
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

/// However many requests for the identifier come while the first builds it, some of them
/// cancelled, and whether that build succeeds or fails: a cancelled one gives up, the rest wait
/// and end up with one identifier, built once after a success, or once more by one of them
/// after a failure.
#[hegel::test(test_cases = 10)]
fn requests_waiting_for_the_identifier_share_one_build(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let cached = CachedLibrary {
        root: dir.path().into(),
        stamp: LibraryStamp::of(dir.path()),
        library: Arc::default(),
        combined: Mutex::default(),
        tags: Mutex::default(),
        tags_by_track: Mutex::default(),
        identifier: Mutex::default(),
        built: Condvar::new(),
    };
    let fingerprints = Cache::new(dir.path().join("fingerprints"));
    let first_fails = tc.draw(gs::booleans());
    let cancelled: Vec<bool> = tc.draw(gs::vecs(gs::booleans()).min_size(1).max_size(4));
    let builds = AtomicUsize::new(0);
    let build = &|library: Library| {
        builds.fetch_add(1, Ordering::SeqCst);
        Identifier::with_library(&device, library.into(), &fingerprints, &Ctx::silent())
    };
    let (started, building) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let (cached, root) = (&cached, dir.path());
    std::thread::scope(|s| {
        let first = s.spawn(move || {
            cached.identifier(root, &Ctx::silent(), |library| {
                started.send(()).unwrap();
                released.recv().unwrap();
                match first_fails {
                    true => Err(Error::invalid("the build failed")),
                    false => build(library),
                }
            })
        });
        building.recv().unwrap();
        let waiting: Vec<_> = cancelled
            .iter()
            .map(|&cancel| {
                s.spawn(move || {
                    let token = CancelToken::new();
                    if cancel {
                        token.cancel();
                    }
                    let quiet = |_: &Event| {};
                    (cancel, cached.identifier(root, &Ctx::new(&quiet, token), build))
                })
            })
            .collect();
        // Long enough for them to be waiting (the property holds however late they come).
        std::thread::sleep(std::time::Duration::from_millis(50));
        release.send(()).unwrap();
        let first = first.join().unwrap();
        assert_eq!(first.is_err(), first_fails);
        let mut got: Vec<Arc<Identifier>> = first.into_iter().collect();
        for w in waiting {
            let (cancel, r) = w.join().unwrap();
            match r {
                Ok(id) => got.push(id),
                Err(e) => assert!(cancel && matches!(e.inner(), Error::Cancelled), "{e}"),
            }
        }
        assert!(got.windows(2).all(|w| Arc::ptr_eq(&w[0], &w[1])), "more than one identifier");
        let built = builds.load(Ordering::SeqCst);
        assert_eq!(built, usize::from(!got.is_empty()), "built {built} times");
    });
}
