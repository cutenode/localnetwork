//! Properties of fetching snapshots from another store over the network (`peers`), on this
//! computer's own network: no relays.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use indexmap::IndexMap;
use rustc_hash::FxHashSet;
use uuid::Uuid;

use locallibrary::device::Device;
use locallibrary::hash::Hash;
use locallibrary::peers::{
    self, FetchOptions, FetchReport, LibraryOutcome, ServeEvent, ServeOptions, ServeReport, ShareCode,
};
use locallibrary::progress::{CancelToken, Ctx, NoProgress};
use locallibrary::store::{LibraryRecord, Store};
use locallibrary::sync::{self, PullOptions};

use crate::common::{self, Template};

/// The other computer's store, with two libraries ("Home": two snapshots, the first labelled;
/// "Club": one). Setting it up is the slow part, so it's made once and written out for each case.
fn laptop() -> &'static Template {
    static LAPTOP: OnceLock<Template> = OnceLock::new();
    LAPTOP.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("laptop")).unwrap();
        let pull = |d: &mut Device, library: &str| {
            let opts = PullOptions { library: Some(library.into()), ..Default::default() };
            sync::pull(&store, d, &opts, &Ctx::silent()).unwrap().snapshot.unwrap().id
        };
        let mut home = Device::open_export(common::demo_drive(dir.path(), "Home")).unwrap();
        let first = pull(&mut home, "Home");
        store.add_label(&first, "before NYE").unwrap();
        common::export_new_track(home.root(), "New", "Track");
        pull(&mut home, "Home");
        let club = dir.path().join("club");
        std::fs::create_dir(&club).unwrap();
        pull(&mut Device::open_export(common::demo_drive(&club, "Club")).unwrap(), "Club");
        Template::of(store.root())
    })
}

/// [`laptop`]'s store, in `dir`.
fn laptop_in(dir: &Path) -> Store {
    Store::open(laptop().write(&dir.join("laptop"))).unwrap()
}

fn fetch(store: &Store, code: &ShareCode, libraries: &[String], dry_run: bool) -> locallibrary::Result<FetchReport> {
    let opts = FetchOptions { libraries: libraries.to_vec(), local_only: true, dry_run };
    peers::fetch(store, code, &opts, &Ctx::silent())
}

/// The laptop's snapshots, by library.
fn snapshots_by_library(store: &Store) -> IndexMap<Uuid, FxHashSet<Hash>> {
    let mut out: IndexMap<Uuid, FxHashSet<Hash>> = IndexMap::new();
    for id in store.snapshot_ids().unwrap() {
        out.entry(store.load_info(&id).unwrap().library_id).or_default().insert(id);
    }
    out
}

fn snapshot_set(store: &Store) -> FxHashSet<Hash> {
    store.snapshot_ids().unwrap().into_iter().collect()
}

/// Libraries named by name (any case) or id, at random.
fn names(tc: &TestCase, libs: &[&LibraryRecord]) -> Vec<String> {
    libs.iter().map(|l| if tc.draw(gs::booleans()) { l.id.to_string() } else { l.name.to_uppercase() }).collect()
}

/// What became of "Home" here after it was fetched before: nothing, moved back to its first
/// snapshot (behind the laptop), or a snapshot of this store's own on top of the laptop's
/// latest (ahead) or of its first (both changed).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Here {
    AsFetched,
    Behind,
    Ahead,
    Diverged,
}

/// A snapshot of `studio`'s own on top of `parent`, as an edit here would make, and the library
/// moved to it.
fn edit_here(studio: &Store, library: Uuid, parent: Hash) -> Hash {
    let base = studio.load_snapshot(&parent).unwrap();
    let mine = locallibrary::store::Snapshot {
        parents: vec![parent],
        message: Some("edited here".into()),
        created: chrono::Utc::now(),
        ..base
    };
    let id = studio.save_snapshot(mine).unwrap().id;
    studio.set_head(library, id).unwrap();
    id
}

/// Whatever this store fetched before, and whatever became of it here since, a dry run says what
/// a fetch of the libraries asked for (all, when none are named) would copy, and copies nothing;
/// the fetch then copies exactly that: the snapshots of those libraries this store lacks, every
/// file they list, and the label on a snapshot it now has. A library new here is added at the
/// other computer's latest; one behind it moves there; one as new, or newer, stays; one changed
/// on both keeps its own latest. Fetching again copies nothing.
#[hegel::test(test_cases = 3)]
fn a_fetch_copies_what_the_other_store_has_and_this_one_lacks(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let laptop = laptop_in(dir.path());
    let served = serve(&laptop);
    let libs = laptop.libraries().unwrap();
    let snaps = snapshots_by_library(&laptop);
    let studio = Store::open_or_init(dir.path().join("studio")).unwrap();
    let pick = |tc: &TestCase| -> Vec<&LibraryRecord> {
        let mut picked: Vec<&LibraryRecord> = tc
            .draw(gs::vecs(gs::sampled_from((0..libs.len()).collect::<Vec<_>>())).max_size(2))
            .into_iter()
            .map(|i| &libs[i])
            .collect();
        picked.dedup_by_key(|l| l.id);
        picked
    };
    let home = libs.iter().find(|l| l.name == "Home").unwrap();
    let latest = home.head.unwrap();
    let first = *snaps[&home.id].iter().find(|h| **h != latest).unwrap();
    let here =
        tc.draw(gs::sampled_from(vec![Here::AsFetched, Here::Behind, Here::Ahead, Here::Diverged]).print_as_debug());
    let mut earlier = pick(&tc);
    // Changed here since: fetched before, then.
    if here != Here::AsFetched && !earlier.iter().any(|l| l.id == home.id) {
        earlier.push(home);
    }
    if !earlier.is_empty() {
        fetch(&studio, &served.code, &names(&tc, &earlier), false).unwrap();
    }
    let own = match here {
        Here::AsFetched => None,
        Here::Behind => {
            studio.set_head(home.id, first).unwrap();
            None
        }
        Here::Ahead => Some(edit_here(&studio, home.id, latest)),
        Here::Diverged => Some(edit_here(&studio, home.id, first)),
    };
    let chosen = pick(&tc);
    let asked = names(&tc, &chosen);
    let chosen: Vec<&LibraryRecord> = if chosen.is_empty() { libs.iter().collect() } else { chosen };

    let (had, had_libraries) = (snapshot_set(&studio), studio.libraries().unwrap());
    let want: FxHashSet<Hash> =
        chosen.iter().flat_map(|l| snaps[&l.id].iter().copied()).filter(|h| !had.contains(h)).collect();
    let dry = fetch(&studio, &served.code, &asked, true).unwrap();
    assert_eq!(dry.snapshots, want.len());
    assert!(dry.dry_run && (want.is_empty() || dry.files + dry.present > 0), "{dry:?}");
    assert_eq!(snapshot_set(&studio), had, "a dry run copies nothing");
    assert_eq!(studio.libraries().unwrap().len(), had_libraries.len());

    let r = fetch(&studio, &served.code, &asked, false).unwrap();
    assert_eq!((r.snapshots, r.files, r.bytes, r.present), (dry.snapshots, dry.files, dry.bytes, dry.present));
    let outcome = |r: &FetchReport| r.libraries.iter().map(|l| (l.id, l.outcome)).collect::<Vec<_>>();
    assert_eq!(outcome(&r), outcome(&dry));
    let expected = |lib: &LibraryRecord| match (had_libraries.iter().any(|h| h.id == lib.id), lib.id == home.id, here) {
        (false, _, _) => LibraryOutcome::Added,
        (true, true, Here::Behind) => LibraryOutcome::Updated,
        (true, true, Here::Diverged) => LibraryOutcome::Diverged,
        (true, _, _) => LibraryOutcome::UpToDate,
    };
    for (lib, got) in chosen.iter().zip(&r.libraries) {
        assert_eq!((got.id, got.outcome, got.head), (lib.id, expected(lib), lib.head), "{here:?}");
        let mine = studio.library_by_id(lib.id).unwrap().unwrap();
        let head = if lib.id == home.id { own.or(lib.head) } else { lib.head };
        assert_eq!((mine.name.as_str(), mine.head), (lib.name.as_str(), head), "{here:?}");
    }
    let now = snapshot_set(&studio);
    assert_eq!(now, had.union(&want).copied().collect::<FxHashSet<_>>());
    assert!(studio.verify(&Ctx::silent()).unwrap().missing.is_empty(), "every file the snapshots list");
    let label = laptop.find_label("before NYE").unwrap().unwrap();
    assert_eq!(studio.find_label("before NYE").unwrap().is_some(), now.contains(&label.snapshot));

    let again = fetch(&studio, &served.code, &asked, false).unwrap();
    assert_eq!((again.snapshots, again.files, again.bytes), (0, 0, 0));
    for l in &again.libraries {
        let stays =
            if l.id == home.id && here == Here::Diverged { LibraryOutcome::Diverged } else { LibraryOutcome::UpToDate };
        assert_eq!(l.outcome, stays, "{again:?}");
    }
    served.stop(0);
}

/// `store` served on the local network on a thread of its own, saying what happens.
struct Served {
    code: ShareCode,
    cancel: CancelToken,
    events: Arc<Mutex<Vec<ServeEvent>>>,
    thread: thread::JoinHandle<ServeReport>,
}

impl Served {
    fn count(&self, kind: &str) -> usize {
        let events = self.events.lock().unwrap();
        events.iter().filter(|e| serde_json::to_value(e).unwrap()["event"] == kind).count()
    }

    /// Stops serving once it has said `connected` and `done` that often (the other end's last
    /// hang-up is told a moment after the fetch returns), and what it reported.
    fn stop(self, connected: usize) -> (Vec<ServeEvent>, ServeReport) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while (self.count("connected") < connected || self.count("done") < connected)
            && std::time::Instant::now() < deadline
        {
            thread::sleep(std::time::Duration::from_millis(10));
        }
        self.cancel.cancel();
        let report = self.thread.join().unwrap();
        (self.events.lock().unwrap().clone(), report)
    }
}

fn serve(store: &Store) -> Served {
    let (store, cancel) = (store.clone(), CancelToken::new());
    let events: Arc<Mutex<Vec<ServeEvent>>> = Arc::default();
    let (tx, rx) = mpsc::channel();
    let (stop, seen) = (cancel.clone(), events.clone());
    let thread = thread::spawn(move || {
        let ctx = Ctx { progress: &NoProgress, cancel: stop };
        peers::serve(&store, &ServeOptions { local_only: true }, &ctx, &mut |e| {
            if let ServeEvent::Ready { code } = e {
                tx.send(code.clone()).unwrap();
            }
            seen.lock().unwrap().push(e.clone());
        })
        .unwrap()
    });
    Served { code: rx.recv().unwrap().parse().unwrap(), cancel, events, thread }
}

/// Only the code `serve` printed reads the store: any other token is refused, a store's own
/// code is refused, a library the other computer doesn't have is named in the refusal, and a
/// stored file that comes back other than its snapshot says (damaged, or missing over there)
/// stops the fetch. Each leaves this store as it was: no library, no snapshot, not the bad file.
/// Serving says who it turned away and who connected, and once stopped, reports what it sent.
#[hegel::test(test_cases = 2)]
fn what_isnt_right_is_refused_and_records_nothing(tc: TestCase) {
    // The laptop's store, copied, with one of its stored files damaged or gone.
    let dir = tempfile::tempdir().unwrap();
    let broken = laptop_in(dir.path());
    let mut files: Vec<(Hash, u64)> = Vec::new();
    for id in broken.snapshot_ids().unwrap() {
        files.extend(broken.load_snapshot(&id).unwrap().files.iter().filter_map(|f| Some((f.hash?, f.size))));
    }
    files.sort();
    files.dedup();
    let (hash, size) = tc.draw(gs::sampled_from(files).print_as_debug());
    let object = broken.object_path(&hash);
    let missing = size == 0 || tc.draw(gs::booleans());
    match missing {
        true => std::fs::remove_file(&object).unwrap(),
        false => {
            let mut bytes = std::fs::read(&object).unwrap();
            let at = tc.draw(gs::integers::<usize>().max_value(bytes.len() - 1));
            bytes[at] ^= 0xff;
            std::fs::write(&object, bytes).unwrap();
        }
    }
    let served = serve(&broken);
    let code = served.code.clone();
    let studio = Store::open_or_init(dir.path().join("studio")).unwrap();
    let nothing_here = || {
        assert!(studio.libraries().unwrap().is_empty() && studio.snapshot_ids().unwrap().is_empty());
        assert!(!studio.has_object(&hash));
    };

    let printed = code.to_string();
    let (ticket, token) = printed.rsplit_once('-').unwrap();
    let other: String = tc.draw(gs::from_regex("[0-9a-f]{32}").fullmatch(true));
    tc.assume(other != token);
    let wrong: ShareCode = format!("{ticket}-{other}").parse().unwrap();
    let opts = FetchOptions { local_only: true, ..Default::default() };
    let err = peers::fetch(&studio, &wrong, &opts, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("refused the share code"), "{err}");
    nothing_here();
    let err = peers::fetch(&broken, &code, &opts, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("this store's own"), "{err}");
    let name: String = tc.draw(gs::sampled_from(vec!["Nope", "Hom", "Club 2"]).map(String::from));
    let named = FetchOptions { libraries: vec![name.clone()], ..opts.clone() };
    let err = peers::fetch(&studio, &code, &named, &Ctx::silent()).unwrap_err().to_string();
    assert!(err.contains(&format!("library {name:?} on the other computer (it has ")), "{err}");
    nothing_here();
    let err = peers::fetch(&studio, &code, &opts, &Ctx::silent()).unwrap_err().to_string();
    let why = if missing { "is missing a stored file a snapshot lists" } else { "came back damaged" };
    assert!(err.contains(why), "{err}");
    nothing_here();

    // The store's own code, the library it lacks and the bad file are told apart after
    // connecting.
    let (events, report) = served.stop(3);
    let count = |kind: &str| events.iter().filter(|e| serde_json::to_value(e).unwrap()["event"] == kind).count();
    assert_eq!((count("ready"), count("refused"), count("connected")), (1, 1, 3), "{events:?}");
    assert_eq!((count("done"), report.peers), (3, 3));
}
