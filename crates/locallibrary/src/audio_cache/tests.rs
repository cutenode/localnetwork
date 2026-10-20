use super::*;
use crate::mix::index::Prints;

/// Keys and paths stay what fingerprint caches were written with, so users' caches still read,
/// and each kind keeps to its own folder and version.
#[test]
fn caches_keep_their_layout() {
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("Track.MP3");
    fs::write(&audio, b"12345").unwrap();
    let mtime = fs::metadata(&audio).unwrap().modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap();
    let recipe = format!("2\u{1f}track.mp3\u{1f}5\u{1f}{}", mtime.as_secs());
    let expected = blake3::hash(recipe.as_bytes()).to_hex()[..32].to_string();
    assert_eq!(Cache::<Prints>::key(&audio).as_deref(), Some(expected.as_str()));
    assert_eq!(Cache::<Prints>::key(&dir.path().join("gone.mp3")), None);

    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let prints = Cache::<Prints>::in_store(&store).clone();
    assert_eq!(prints.root(), store.root().join("cache/fingerprints"));
    assert_eq!(prints.file("abcdef", "fp"), prints.root().join("ab/abcdef.fp"));
}

/// A cached value that can't be read back (cut short) counts as not cached.
#[test]
fn unreadable_values_are_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::<Lengths>::new(dir.path());
    fs::create_dir_all(dir.path().join("ab")).unwrap();
    fs::write(cache.file("abcdef", Lengths::EXT), b"12").unwrap();
    assert_eq!(cache.get("abcdef"), Some(12));
    fs::write(cache.file("abcdef", Lengths::EXT), b"1x").unwrap();
    assert!(cache.get("abcdef").is_none());
}

// -----------------------------------------------------------------------------------------
// Properties
// -----------------------------------------------------------------------------------------

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use hegel::TestCase;
use hegel::generators as gs;

use crate::progress::{CancelToken, Event};

/// File lengths: a cache kind whose values the test can check without decoding audio.
struct Lengths;

impl Kind for Lengths {
    const DIR: &'static str = "lengths";
    const EXT: &'static str = "len";
    const WHAT: &'static str = "a length";
    const VERSION: u32 = 1;
    type Value = u64;
    fn encode(value: &u64) -> Vec<u8> {
        value.to_string().into_bytes()
    }
    fn decode(bytes: &[u8]) -> Option<u64> {
        std::str::from_utf8(bytes).ok()?.parse().ok()
    }
}

/// What working a file out does: the first byte of the file says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// Its length.
    Decodes,
    /// A parse error: the file doesn't decode.
    Garbled,
    /// The decoder panics, which counts as not decoding.
    Crashes,
    /// An I/O error, as from a drive pulled out mid-read.
    Unreadable,
    /// Not there at all.
    Missing,
}

impl Outcome {
    const ALL: [Outcome; 5] =
        [Outcome::Decodes, Outcome::Garbled, Outcome::Crashes, Outcome::Unreadable, Outcome::Missing];

    fn mark(self) -> u8 {
        b"dgcum"[Outcome::ALL.iter().position(|&o| o == self).unwrap()]
    }

    /// Doesn't decode, so the failure is remembered.
    fn remembered(self) -> bool {
        matches!(self, Outcome::Garbled | Outcome::Crashes)
    }
}

/// Stands in for a decoder: does what the file's first byte says, counting the calls.
fn work_out(path: &Path, calls: &AtomicUsize) -> Result<u64> {
    calls.fetch_add(1, Ordering::Relaxed);
    let bytes = fs::read(path).map_err(|e| Error::Io { path: path.into(), source: e })?;
    match bytes[0] {
        b'd' => Ok(bytes.len() as u64),
        b'g' => Err(Error::parse(path, "no audio in it")),
        b'c' => panic!("decoder bug"),
        _ => Err(Error::Io { path: path.into(), source: std::io::ErrorKind::UnexpectedEof.into() }),
    }
}

/// Files whose first byte is their drawn outcome, the rest padding to a drawn length; the
/// missing ones aren't written.
#[hegel::test_helper]
fn audio_files(tc: &TestCase, dir: &Path) -> Vec<(Audio, Outcome)> {
    let outcomes = tc.draw(gs::vecs(gs::sampled_from(Outcome::ALL.to_vec())).max_size(6));
    outcomes
        .into_iter()
        .enumerate()
        .map(|(i, outcome)| {
            let path = dir.join(format!("Track {i}.mp3"));
            let len = tc.draw(gs::integers::<usize>().min_value(1).max_value(64));
            if outcome != Outcome::Missing {
                let mut bytes = vec![b'.'; len];
                bytes[0] = outcome.mark();
                fs::write(&path, bytes).unwrap();
            }
            let id = 10 + i as u32;
            (Audio { id, path, name: format!("Track {i}"), bytes: len as u64 }, outcome)
        })
        .collect()
}

/// A run keeps to every file's outcome: a file that isn't there is missing, one that decodes is
/// done with its value, in the order the files came, and one that doesn't decode (or crashes
/// the decoder) or can't be read is a failure. A second run, through a clone of the cache,
/// decodes only what the first couldn't settle: unreadable files, which may read next time,
/// and everything when the cache can't be written, which warns once per value it couldn't keep.
#[hegel::test(test_cases = 60)]
fn runs_decode_each_file_once_and_remember_what_doesnt_decode(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let files = audio_files(&tc, dir.path());
    let writable = tc.draw(gs::booleans());
    let root = dir.path().join("cache");
    if !writable {
        // A file where the cache's folder should be: nothing can be written under it.
        fs::write(&root, b"").unwrap();
    }
    let cache = Cache::<Lengths>::new(&root);
    let warnings = Mutex::new(Vec::new());
    let sink = |e: &Event| {
        if let Event::Warning { message } = e {
            warnings.lock().unwrap().push(message.clone());
        }
    };
    let ctx = Ctx::new(&sink, CancelToken::new());
    let audio: Vec<Audio> = files
        .iter()
        .map(|(a, _)| Audio { id: a.id, path: a.path.clone(), name: a.name.clone(), bytes: a.bytes })
        .collect();
    let count = |o: fn(Outcome) -> bool| files.iter().filter(|(_, x)| o(*x)).count();
    let present = count(|o| o != Outcome::Missing);
    let decodes: Vec<(u32, u64)> =
        files.iter().filter(|(_, o)| *o == Outcome::Decodes).map(|(a, _)| (a.id, a.bytes)).collect();
    let failing: Vec<u32> = {
        let mut f: Vec<u32> =
            files.iter().filter(|(_, o)| o.remembered() || *o == Outcome::Unreadable).map(|(a, _)| a.id).collect();
        f.sort_unstable();
        f
    };
    let missing: Vec<u32> = files.iter().filter(|(_, o)| *o == Outcome::Missing).map(|(a, _)| a.id).collect();

    let check = |cache: &Cache<Lengths>, computed: usize, calls: usize| {
        let counter = AtomicUsize::new(0);
        warnings.lock().unwrap().clear();
        let run = for_files(&audio, cache, Phase::Scan, |p, _| work_out(p, &counter), &ctx).unwrap();
        assert_eq!(run.done.iter().map(|d| (d.id, d.value)).collect::<Vec<_>>(), decodes);
        assert!(
            run.done.iter().all(|d| Some(&d.key) == Cache::<Lengths>::key(&audio[(d.id - 10) as usize].path).as_ref())
        );
        let mut failed: Vec<u32> = run.failed.iter().map(|(id, _)| *id).collect();
        failed.sort_unstable();
        assert_eq!(failed, failing);
        assert_eq!(run.missing, missing);
        assert_eq!((run.computed, counter.load(Ordering::Relaxed)), (computed, calls));
        let unkept = if writable { 0 } else { calls - count(|o| o == Outcome::Unreadable) };
        let warned = warnings.lock().unwrap();
        assert_eq!(warned.len(), unkept, "{warned:?}");
        assert!(warned.iter().all(|w| w.starts_with("couldn't cache a length: ")), "{warned:?}");
    };
    check(&cache, decodes.len(), present);
    let again = cache.clone();
    assert_eq!(again.root(), cache.root());
    if writable {
        check(&again, 0, count(|o| o == Outcome::Unreadable));
    } else {
        check(&again, decodes.len(), present);
    }
}

/// A run cancelled while one file is being worked out stops with the cancellation, and keeps
/// what it finished: every value it cached is that file's own, and a run after it finishes the
/// job, working out only what the cancelled one didn't.
#[hegel::test(test_cases = 40)]
fn a_cancelled_run_keeps_what_it_finished(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(12));
    let files: Vec<Audio> = (0..n)
        .map(|i| {
            let path = dir.path().join(format!("{i}.wav"));
            let mut bytes = vec![b'.'; i + 1];
            bytes[0] = b'd';
            fs::write(&path, bytes).unwrap();
            Audio { id: i as u32, path, name: i.to_string(), bytes: i as u64 + 1 }
        })
        .collect();
    let stop = tc.draw(gs::integers::<usize>().max_value(n - 1));
    let cache = Cache::<Lengths>::new(dir.path().join("cache"));
    let cancel = CancelToken::new();
    let ctx = Ctx::new(&crate::progress::NoProgress, cancel.clone());
    let calls = AtomicUsize::new(0);
    let r = for_files(
        &files,
        &cache,
        Phase::Scan,
        |p, _| {
            if *p == files[stop].path {
                cancel.cancel();
                return Err(Error::Cancelled);
            }
            work_out(p, &calls)
        },
        &ctx,
    );
    assert!(matches!(r, Err(Error::Cancelled)), "{:?}", r.map(|r| r.computed));
    let kept: Vec<&Audio> =
        files.iter().filter(|f| cache.get(&Cache::<Lengths>::key(&f.path).unwrap()).is_some()).collect();
    assert!(kept.iter().all(|f| cache.get(&Cache::<Lengths>::key(&f.path).unwrap()) == Some(f.bytes)));
    assert!(!kept.iter().any(|f| f.id as usize == stop));
    assert_eq!(kept.len(), calls.load(Ordering::Relaxed), "every value worked out was kept");

    let after = AtomicUsize::new(0);
    let run = for_files(&files, &cache, Phase::Scan, |p, _| work_out(p, &after), &Ctx::silent()).unwrap();
    assert_eq!(
        run.done.iter().map(|d| (d.id, d.value)).collect::<Vec<_>>(),
        files.iter().map(|f| (f.id, f.bytes)).collect::<Vec<_>>()
    );
    assert_eq!((run.computed, after.load(Ordering::Relaxed)), (n - kept.len(), n - kept.len()));
}
