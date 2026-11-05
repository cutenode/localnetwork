use super::*;
use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// A share code for a random key and address, with `token`.
fn code(tc: &TestCase, token: String) -> ShareCode {
    let key: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).min_size(32).max_size(32));
    let key = iroh::SecretKey::from_bytes(&key.try_into().expect("32 bytes")).public();
    let ip = std::net::Ipv4Addr::from(tc.draw(gs::integers::<u32>()));
    let port = tc.draw(gs::integers::<u16>());
    let addr = iroh::EndpointAddr::new(key).with_ip_addr(std::net::SocketAddr::from((ip, port)));
    ShareCode { ticket: addr.into(), token }
}

/// A share code reads back from what it prints, around any whitespace, and names the same
/// computer; a fresh one's token is 32 hex digits.
#[hegel::test(test_cases = 200)]
fn share_codes_read_back(tc: TestCase) {
    let token: String = tc.draw(gs::from_regex("[0-9a-fA-F]{32}").fullmatch(true));
    let c = code(&tc, token);
    let printed = c.to_string();
    assert_eq!(format!(" {printed}\n").parse::<ShareCode>().unwrap(), c);
    assert_eq!(printed.parse::<ShareCode>().unwrap().addr(), c.addr());
    let fresh = ShareCode::new(c.addr().clone());
    assert!(fresh.token.len() == 32 && fresh.token.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(fresh.to_string().parse::<ShareCode>().unwrap(), fresh);
}

/// A code with its token cut short, made longer, or with a letter that isn't hex, or with
/// its ticket mangled, isn't read as one; whatever text does read as a code prints back as
/// a code with the same token.
#[hegel::test(test_cases = 300)]
fn mangled_share_codes_are_refused(tc: TestCase) {
    let c = code(&tc, Uuid::new_v4().simple().to_string());
    let printed = c.to_string();
    let (ticket, token) = printed.rsplit_once('-').unwrap();
    let cut = tc.draw(gs::integers::<usize>().min_value(1).max_value(31));
    let junk = tc.draw(gs::sampled_from(vec!['g', 'z', '-', ' ', 'é']));
    let at = tc.draw(gs::integers::<usize>().max_value(31));
    let mut bad_letter: Vec<char> = token.chars().collect();
    bad_letter[at] = junk;
    let bad_letter: String = bad_letter.into_iter().collect();
    for bad in [
        format!("{ticket}-{}", &token[..cut]),
        format!("{ticket}-{token}0"),
        format!("{ticket}-{bad_letter}"),
        format!("{ticket}{token}"),
        format!("-{token}"),
        format!("x{ticket}-{token}"),
    ] {
        assert!(bad.parse::<ShareCode>().is_err(), "{bad:?} was read as a code");
    }
    let padded = format!("{}{printed}{}", " ".repeat(cut % 3), "\n".repeat(at % 2));
    let text: String = tc.draw(hegel::one_of!(
        gs::text().max_size(80),
        gs::just(padded),
        gs::integers::<usize>().max_value(printed.len()).map(move |n| printed[..n].to_string()),
    ));
    if let Ok(read) = text.parse::<ShareCode>() {
        let token = text.trim().rsplit_once('-').unwrap().1;
        assert_eq!(read.token, token);
        assert_eq!(read.to_string().parse::<ShareCode>().unwrap(), read);
    }
}

use crate::hash::Hash;
use crate::progress::{CancelToken, Ctx, Event, Phase, Progress};
use crate::store::{Snapshot, Store};
use protocol::{ALPN, NOT_FOUND, OK, REFUSED, Request};

/// The other computer's store: a library "Home" whose latest snapshot builds on an earlier one,
/// listing three stored files between them.
fn there(dir: &std::path::Path) -> (Store, [Hash; 2]) {
    use crate::paths::FileClass;
    use crate::tree::FileEntry;
    let store = Store::open_or_init(dir.join("there")).unwrap();
    let home = store.create_library("Home").unwrap();
    let file = |i: usize| {
        let bytes = format!("track {i} bytes").into_bytes();
        let hash = store.import_bytes(&bytes).unwrap();
        FileEntry {
            path: format!("Contents/{i}.mp3"),
            size: bytes.len() as u64,
            mtime: 0,
            class: FileClass::Audio,
            hash: Some(hash),
        }
    };
    let made = |k: u8, parents: Vec<Hash>, files: Vec<FileEntry>| {
        let snap = Snapshot {
            id: Hash::of(b""),
            library_id: home.id,
            parents,
            created: chrono::DateTime::from_timestamp(1_790_000_000, 0).unwrap(),
            message: None,
            source: None,
            fingerprint: Hash::of(&[k]),
            summary: crate::model::LibrarySummary::default(),
            lineage: Vec::new(),
            files,
        };
        store.save_snapshot(snap).unwrap().id
    };
    let first = made(0, Vec::new(), vec![file(0), file(1)]);
    let latest = made(1, vec![first], vec![file(0), file(1), file(2)]);
    store.set_head(home.id, latest).unwrap();
    (store, [first, latest])
}

/// How the fake other computer gets an answer wrong.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Fault {
    None,
    /// It can't read its store.
    HelloFails,
    /// Its catalog isn't JSON.
    CatalogGarbage,
    /// The snapshot `.0` (0: the first, 1: the latest) was pruned there since the catalog.
    SnapshotGone(usize),
    /// A snapshot's manifest isn't one.
    SnapshotGarbage,
    /// A stored file is missing there.
    ObjectGone,
    /// A stored file comes back longer than its snapshot says.
    ObjectLonger,
}

/// A computer serving `store` the way `serve` would, but with `fault`.
#[derive(Debug, Clone)]
struct Faulty {
    store: Store,
    fault: Fault,
    snapshots: [Hash; 2],
}

impl Faulty {
    fn answer(&self, request: Request) -> (u8, Vec<u8>) {
        let read = |path: std::path::PathBuf| (OK, std::fs::read(path).unwrap());
        match (request, self.fault) {
            (Request::Hello { .. }, Fault::HelloFails) => (NOT_FOUND, b"disk gone".to_vec()),
            (Request::Hello { .. }, Fault::CatalogGarbage) => (OK, b"not a catalog".to_vec()),
            (Request::Hello { .. }, _) => (OK, serde_json::to_vec(&crate::catalog::of(&self.store).unwrap()).unwrap()),
            (Request::Snapshot { id }, Fault::SnapshotGone(k)) if id == self.snapshots[k] => (NOT_FOUND, Vec::new()),
            (Request::Snapshot { .. }, Fault::SnapshotGarbage) => (OK, b"{".to_vec()),
            (Request::Snapshot { id }, _) => read(self.store.snapshot_path(&id)),
            (Request::Object { .. }, Fault::ObjectGone) => (NOT_FOUND, Vec::new()),
            (Request::Object { hash }, Fault::ObjectLonger) => {
                let (status, mut bytes) = read(self.store.object_path(&hash));
                bytes.push(b'!');
                (status, bytes)
            }
            (Request::Object { hash }, _) => read(self.store.object_path(&hash)),
        }
    }
}

impl iroh::protocol::ProtocolHandler for Faulty {
    async fn accept(&self, conn: iroh::endpoint::Connection) -> std::result::Result<(), iroh::protocol::AcceptError> {
        while let Ok((mut send, mut recv)) = conn.accept_bi().await {
            let Ok(request) = protocol::read_request(&mut recv).await else { break };
            let (status, body) = self.answer(request);
            let _ = protocol::answer(&mut send, status, &body).await;
        }
        Ok(())
    }
}

/// [`Faulty`] on this computer's network, on a thread of its own until the sender is dropped.
fn faulty(served: Faulty) -> (ShareCode, std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let (code_tx, code_rx) = std::sync::mpsc::channel();
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let thread = std::thread::spawn(move || {
        runtime().unwrap().block_on(async {
            let endpoint = endpoint(true, vec![ALPN.to_vec()]).await.unwrap();
            code_tx.send(ShareCode::new(endpoint.addr())).unwrap();
            let router = iroh::protocol::Router::builder(endpoint).accept(ALPN, served).spawn();
            let _ = tokio::task::spawn_blocking(move || stopped.recv()).await;
            let _ = router.shutdown().await;
        });
    });
    (code_rx.recv().unwrap(), stop, thread)
}

/// Cancels the fetch at the first stored file it scans or copies in `phase`.
struct CancelAt(Phase, CancelToken);

impl Progress for CancelAt {
    fn on_event(&self, event: &Event) {
        if matches!(event, Event::Item { phase, .. } if *phase == self.0) {
            self.1.cancel();
        }
    }
}

/// However the other computer gets its answers wrong, the fetch says what went wrong and records
/// nothing: a store it can't read is said in its words, a catalog or manifest that isn't one is
/// refused, a stored file missing there or longer than its snapshot says stops the fetch, and a
/// fetch cancelled while it scans or copies stops. A snapshot pruned there since the catalog is
/// left out: the rest comes over, and a library whose latest went is left alone. Answered right,
/// the library comes over whole.
#[hegel::test(test_cases = 30)]
#[hegel::explicit_test_case(fault = Fault::ObjectLonger, cancel_in = None::<Phase>)]
#[hegel::explicit_test_case(fault = Fault::SnapshotGone(1), cancel_in = Some(Phase::Copy))]
fn prop_a_fetch_from_a_computer_that_answers_wrong_records_nothing(tc: TestCase) {
    let fault = tc.draw(gs::sampled_from(vec![
        Fault::None,
        Fault::HelloFails,
        Fault::CatalogGarbage,
        Fault::SnapshotGone(0),
        Fault::SnapshotGone(1),
        Fault::SnapshotGarbage,
        Fault::ObjectGone,
        Fault::ObjectLonger,
    ]));
    // Now and then cancelled instead, while scanning or copying.
    let cancel_in = tc.draw(gs::optional(gs::sampled_from(vec![Phase::Scan, Phase::Copy])));
    let dir = tempfile::tempdir().unwrap();
    let (store, snapshots) = there(dir.path());
    let (code, stop, thread) = faulty(Faulty { store: store.clone(), fault, snapshots });
    let here = Store::open_or_init(dir.path().join("here")).unwrap();
    let cancel = CancelToken::new();
    let progress = CancelAt(cancel_in.unwrap_or(Phase::Upload), cancel.clone());
    let opts = FetchOptions { local_only: true, ..Default::default() };
    let fetched = fetch(&here, &code, &opts, &Ctx::new(&progress, cancel));
    drop(stop);
    thread.join().unwrap();

    let nothing_recorded = || assert!(here.libraries().unwrap().is_empty() && here.snapshot_ids().unwrap().is_empty());
    let said = |want: &str| {
        let err = fetched.as_ref().unwrap_err().to_string();
        assert!(err.contains(want), "{fault:?} {cancel_in:?}: {err}");
        nothing_recorded();
    };
    match fault {
        Fault::HelloFails => said("the other computer couldn't read its store: disk gone"),
        Fault::CatalogGarbage => said("the other computer's catalog can't be read"),
        Fault::SnapshotGarbage => said("can't be read"),
        // Cancelled at the first snapshot scanned, the fetch stops before the second.
        _ if cancel_in == Some(Phase::Scan) => {
            assert!(matches!(fetched, Err(Error::Cancelled)), "{fault:?}: {fetched:?}");
            nothing_recorded();
        }
        // Every stored file fails, so none is copied to cancel at.
        Fault::ObjectGone => said("is missing a stored file a snapshot lists"),
        Fault::ObjectLonger => said("came back damaged"),
        _ if cancel_in == Some(Phase::Copy) => {
            assert!(matches!(fetched, Err(Error::Cancelled)), "{fault:?}: {fetched:?}");
            nothing_recorded();
        }
        Fault::SnapshotGone(k) => {
            let report = fetched.unwrap();
            assert_eq!(report.snapshots, 1);
            assert_eq!(here.snapshot_ids().unwrap(), vec![snapshots[1 - k]]);
            let outcome = report.libraries[0].outcome;
            assert_eq!(outcome == LibraryOutcome::Unavailable, k == 1, "{outcome:?}");
        }
        Fault::None => {
            let report = fetched.unwrap();
            assert_eq!((report.snapshots, report.files), (2, 3));
            assert_eq!(report.libraries[0].outcome, LibraryOutcome::Added);
            let mut ids = here.snapshot_ids().unwrap();
            ids.sort();
            let mut want = snapshots.to_vec();
            want.sort();
            assert_eq!(ids, want);
        }
    }
}

/// `store` served on this computer's network on a thread of its own until `cancel` is, with the
/// share code it printed.
fn served(store: &Store, cancel: &CancelToken) -> (ShareCode, std::thread::JoinHandle<Result<ServeReport>>) {
    let (store, stop) = (store.clone(), cancel.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let ctx = Ctx::new(&crate::progress::NoProgress, stop);
        serve(&store, &ServeOptions { local_only: true }, &ctx, &mut |e| {
            if let ServeEvent::Ready { code } = e {
                tx.send(code.clone()).unwrap();
            }
        })
    });
    (rx.recv().unwrap().parse().unwrap(), thread)
}

/// What one stream of a connection to `code` gets back for `request` (sent as these bytes): its
/// status and body, or `None` when it isn't answered.
async fn asked(conn: &iroh::endpoint::Connection, request: &[u8]) -> Option<(u8, Vec<u8>)> {
    let (mut send, mut recv) = conn.open_bi().await.ok()?;
    send.write_all(request).await.ok()?;
    send.finish().ok()?;
    let mut body = recv.read_to_end(1 << 20).await.ok()?;
    (!body.is_empty()).then(|| (body.remove(0), body))
}

/// Serving answers only what the protocol asks, in order: after the hello with its token, a
/// second hello is refused, a request that isn't one (or is too long to be one) goes unanswered,
/// and a snapshot or file it doesn't have is not found; the connection stays usable throughout,
/// whoever else connected and hung up without asking anything.
/// A store whose catalog can't be read is said so to the computer that connects, in words a
/// fetch shows, and one that can't be read before serving isn't served.
#[hegel::test(test_cases = 4)]
#[hegel::explicit_test_case(broken = false, strays = vec![0u8, 1, 2, 3], hung_up = true)]
fn prop_serving_answers_only_what_the_protocol_asks(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (store, snapshots) = there(dir.path());
    let broken = tc.draw(gs::booleans());
    let cancel = CancelToken::new();
    let (code, thread) = served(&store, &cancel);
    if broken {
        std::fs::remove_file(store.root().join("store.json")).unwrap();
    }
    let strays: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(3)).max_size(4));
    let hung_up = tc.draw(gs::booleans());
    let token = code.token.clone();
    let answers = runtime().unwrap().block_on(async {
        let endpoint = endpoint(true, Vec::new()).await.unwrap();
        if hung_up {
            let stranger = endpoint.connect(code.addr().clone(), ALPN).await.unwrap();
            stranger.close(0u32.into(), b"bye");
        }
        let conn = endpoint.connect(code.addr().clone(), ALPN).await.unwrap();
        let hello = serde_json::to_vec(&Request::Hello { token }).unwrap();
        let mut answers = vec![asked(&conn, &hello).await];
        for stray in &strays {
            let request = match stray {
                0 => hello.clone(),
                1 => b"not a request".to_vec(),
                2 => vec![b' '; 5000],
                _ => serde_json::to_vec(&Request::Object { hash: Hash::of(b"nowhere") }).unwrap(),
            };
            answers.push(asked(&conn, &request).await);
        }
        let latest = serde_json::to_vec(&Request::Snapshot { id: snapshots[1] }).unwrap();
        answers.push(asked(&conn, &latest).await);
        conn.close(0u32.into(), b"done");
        endpoint.close().await;
        answers
    });
    cancel.cancel();
    thread.join().unwrap().unwrap();

    let (status, body) = answers[0].clone().unwrap();
    if broken {
        assert_eq!(status, NOT_FOUND);
        assert!(String::from_utf8_lossy(&body).contains("store.json"), "{}", String::from_utf8_lossy(&body));
        let err = serve(&store, &ServeOptions { local_only: true }, &Ctx::silent(), &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("store.json"), "{err}");
    } else {
        assert_eq!(status, OK);
        let catalog: crate::catalog::Catalog = serde_json::from_slice(&body).unwrap();
        assert_eq!(catalog.store, store.id().unwrap());
    }
    for (stray, answer) in strays.iter().zip(&answers[1..]) {
        match stray {
            0 => assert_eq!(answer.as_ref().map(|a| a.0), Some(REFUSED)),
            1 | 2 => assert_eq!(answer, &None),
            _ => assert_eq!(answer.as_ref().map(|a| a.0), Some(NOT_FOUND)),
        }
    }
    let (status, body) = answers.last().unwrap().clone().unwrap();
    assert_eq!((status, body), (OK, std::fs::read(store.snapshot_path(&snapshots[1])).unwrap()));
}
