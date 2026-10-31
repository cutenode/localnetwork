use super::*;
use loclib_mixid::fingerprint::{FrameLevels, Peak};

#[test]
fn prints_round_trip() {
    let p = TrackPrint {
        duration_secs: 312.5,
        peaks: vec![Peak { t: 0, f: 3 }, Peak { t: 0, f: 200 }, Peak { t: 130, f: 7 }, Peak { t: 70_000, f: 255 }],
    };
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    cache.put("abcdef", &p).unwrap();
    assert_eq!(cache.get("abcdef"), Some(p));
    assert_eq!(cache.get("abcdeg"), None);
}

#[test]
fn failures_are_remembered_by_the_version_that_failed() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path());
    cache.put_failure("abcdef", "could not parse x.wav: no audio track").unwrap();
    assert_eq!(cache.failure("abcdef").as_deref(), Some("could not parse x.wav: no audio track"));
    assert_eq!(cache.get("abcdef"), None);
    std::fs::write(cache.file("abcdef", "err"), "0.0.1\nan older decoder's").unwrap();
    assert_eq!(cache.failure("abcdef"), None, "a newer version tries again");
}

/// A file the decoder can't make sense of (a WAV saying 0 Hz panics symphonia) is reported
/// and remembered, so the next run neither crashes nor decodes it again. Tracks as long as a
/// recorded set aren't indexed.
#[test]
fn unreadable_and_long_tracks_are_left_out() {
    use crate::probe::tests::wav;
    use crate::progress::{CancelToken, Event};
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drive");
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    std::fs::write(root.join("Contents/zero.wav"), wav(0, 16, 0.0)).unwrap();
    std::fs::write(root.join("Contents/set.wav"), wav(8_000, 16, 1.0)).unwrap();
    let device = Device::open(&root).unwrap();
    let mut set = track(2, "DJ", "Live", "/Contents/set.wav");
    set.duration_secs = LONGEST_TRACK_SECS + 1;
    let library = Library { tracks: vec![track(1, "A", "Zero", "/Contents/zero.wav"), set], ..Default::default() };
    let cache = Cache::new(dir.path().join("cache"));

    let decoded = std::sync::atomic::AtomicUsize::new(0);
    let count = |e: &Event| {
        if matches!(e, Event::Item { phase: Phase::Fingerprint, .. }) {
            decoded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    };
    let ctx = Ctx::new(&count, CancelToken::new());
    for run in 0..2 {
        let (prints, report) = fingerprint_library(&device, &library, &cache, &ctx).unwrap();
        assert!(prints.is_empty());
        assert_eq!(report.long.iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
        assert_eq!(report.failed.len(), 1, "run {run}");
        assert!(report.failed[0].error.contains("crashed"), "{}", report.failed[0].error);
    }
    assert_eq!(decoded.into_inner(), 1, "decoded once, on the first run");
}

/// A failure is remembered only for the file it came from: one that went away or changed
/// while it was read (a drive pulled out) is tried again when it's back as it was.
#[test]
fn a_failure_from_a_file_changed_mid_read_is_not_remembered() {
    use crate::probe::tests::wav;
    use crate::progress::{CancelToken, Event};
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drive");
    let file = root.join("Contents/a.wav");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, wav(8_000, 16, 1.0)).unwrap();
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    let device = Device::open(&root).unwrap();
    let library = Library { tracks: vec![track(1, "A", "One", "/Contents/a.wav")], ..Default::default() };
    let cache = Cache::new(dir.path().join("cache"));

    // Swapped for one the decoder crashes on as it's about to be read.
    let swap = |e: &Event| {
        if matches!(e, Event::Item { phase: Phase::Fingerprint, .. }) {
            std::fs::write(&file, wav(0, 16, 0.0)).unwrap();
        }
    };
    let (_, report) = fingerprint_library(&device, &library, &cache, &Ctx::new(&swap, CancelToken::new())).unwrap();
    assert_eq!(report.failed.len(), 1);

    std::fs::write(&file, wav(8_000, 16, 1.0)).unwrap();
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(modified).unwrap();
    let (prints, report) = fingerprint_library(&device, &library, &cache, &Ctx::silent()).unwrap();
    assert!(report.failed.is_empty() && prints.len() == 1, "{:?}", report.failed);
}

use hegel::TestCase;
use hegel::generators as gs;

/// Fingerprinting a drive cancelled before it starts stops as cancelled as soon as there's a
/// file to look at, and remembers nothing; run again, it sorts every track into exactly one of
/// indexed, missing, unreadable or too long, and a file it couldn't read isn't decoded twice.
#[hegel::test(test_cases = 20)]
fn fingerprinting_sorts_every_track_and_a_cancelled_run_keeps_nothing(tc: TestCase) {
    use crate::progress::{CancelToken, Event};
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drive");
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    // Each track: its file is there (not audio), missing, or it's as long as a recorded set.
    let kinds: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(2)).max_size(5));
    let mut library = Library::default();
    for (i, kind) in kinds.iter().enumerate() {
        let path = format!("/Contents/{i}.mp3");
        let mut t = track(i as u32 + 1, "A", &format!("T{i}"), &path);
        match kind {
            0 => std::fs::write(root.join(&path[1..]), b"ID3 not really audio").unwrap(),
            1 => {}
            _ => t.duration_secs = LONGEST_TRACK_SECS + 1,
        }
        library.tracks.push(t);
    }
    let ids = |k: u8| kinds.iter().enumerate().filter(|(_, x)| **x == k).map(|(i, _)| i as u32 + 1).collect::<Vec<_>>();
    let device = Device::open(&root).unwrap();
    let cache_dir = dir.path().join("cache");
    let cache = Cache::new(&cache_dir);

    let cancel = CancelToken::new();
    cancel.cancel();
    let cancelled = fingerprint_library(&device, &library, &cache, &Ctx::new(&crate::progress::NoProgress, cancel));
    match cancelled {
        Err(Error::Cancelled) => assert!(!ids(0).is_empty()),
        Ok((prints, report)) => {
            assert!(ids(0).is_empty() && prints.is_empty());
            assert_eq!(report.missing.iter().map(|t| t.id).collect::<Vec<_>>(), ids(1));
        }
        Err(e) => panic!("{e}"),
    }
    assert!(!cache_dir.exists(), "a cancelled run remembers nothing");

    let decoded = std::sync::atomic::AtomicUsize::new(0);
    let count = |e: &Event| {
        if matches!(e, Event::Item { phase: Phase::Fingerprint, .. }) {
            decoded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    };
    let ctx = Ctx::new(&count, CancelToken::new());
    for _ in 0..2 {
        let (prints, report) = fingerprint_library(&device, &library, &cache, &ctx).unwrap();
        assert!(prints.is_empty());
        assert_eq!((report.tracks, report.indexed), (kinds.len(), 0));
        assert_eq!(report.missing.iter().map(|t| t.id).collect::<Vec<_>>(), ids(1));
        assert_eq!(report.long.iter().map(|t| t.id).collect::<Vec<_>>(), ids(2));
        let mut failed: Vec<u32> = report.failed.iter().map(|f| f.track.id).collect();
        failed.sort_unstable();
        assert_eq!(failed, ids(0));
    }
    assert_eq!(decoded.into_inner(), ids(0).len(), "each unreadable file decoded once");
}

/// A drive of tracks synthesized from `seeds`, and a recording on this computer made of a stretch
/// of each, in that order.
fn drive_and_set(dir: &std::path::Path, seeds: &[u32]) -> (Device, std::sync::Arc<Library>, std::path::PathBuf) {
    use crate::probe::tests::wav_of;
    use loclib_mixid::fingerprint::RATE;
    use loclib_mixid::test_util::synth;
    use loclib_model::test_util::track;
    let root = dir.join("drive");
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    let pcm = |x: &[f32]| wav_of(RATE, 16, x.iter().map(|s| (s * 20_000.0) as i32));
    let mut library = Library::default();
    let mut set = Vec::new();
    for (i, &seed) in seeds.iter().enumerate() {
        let audio = synth(30.0, seed);
        let path = format!("/Contents/{i}.wav");
        std::fs::write(root.join(&path[1..]), pcm(&audio)).unwrap();
        library.tracks.push(track(i as u32 + 1, "A", &format!("Track {i}"), &path));
        set.extend_from_slice(&audio[RATE as usize * 2..RATE as usize * 26]);
    }
    let recording = dir.join("set.wav");
    std::fs::write(&recording, pcm(&set)).unwrap();
    (Device::open(&root).unwrap(), std::sync::Arc::new(library), recording)
}

/// What a report says, but for when it was made, how long that took and how many tracks were
/// fingerprinted for it.
fn said(r: &crate::mix::MixReport) -> serde_json::Value {
    let mut v = serde_json::to_value(r).unwrap();
    let o = v.as_object_mut().unwrap();
    o.remove("analysed_at");
    o.remove("elapsed_secs");
    o["library"].as_object_mut().unwrap().remove("fingerprinted");
    v
}

/// Identifying a mix again, with the drive's index and the recording heard kept from the first
/// time, decodes nothing and says exactly what the first time did, index and all.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn identifying_again_from_the_cache_says_what_the_first_time_did(tc: TestCase) {
    use crate::mix::{Identifier, Options};
    use crate::progress::{CancelToken, Event};
    let seeds: Vec<u32> =
        tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(100_000)).min_size(1).max_size(3));
    let dir = tempfile::tempdir().unwrap();
    let (device, library, recording) = drive_and_set(dir.path(), &seeds);
    let cache = Cache::new(dir.path().join("cache"));
    let decoded = std::sync::atomic::AtomicUsize::new(0);
    let count = |e: &Event| {
        if matches!(e, Event::Item { phase: Phase::Fingerprint, .. }) {
            decoded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    };
    let ctx = Ctx::new(&count, CancelToken::new());
    let options = Options { tempo_range: 0.02 };

    let cold = Identifier::with_library(&device, library.clone(), &cache, &ctx).unwrap();
    let first = cold.identify(&recording, &options, &ctx).unwrap();
    assert!(!cold.cache.index);
    assert_eq!(cold.cache.heard.load(std::sync::atomic::Ordering::Relaxed), 0);
    assert_eq!(decoded.swap(0, std::sync::atomic::Ordering::Relaxed), seeds.len());

    let warm = Identifier::with_library(&device, library.clone(), &cache, &ctx).unwrap();
    let again = warm.identify(&recording, &options, &ctx).unwrap();
    assert!(warm.cache.index, "the index is the one kept");
    assert_eq!(warm.cache.heard.load(std::sync::atomic::Ordering::Relaxed), 1, "the recording was heard before");
    assert_eq!(decoded.load(std::sync::atomic::Ordering::Relaxed), 0, "nothing decoded again");
    assert_eq!(warm.index.encode(), cold.index.encode());
    assert_eq!(said(&again), said(&first));
    assert_eq!(again.plays.len(), seeds.len(), "{:#?}", again.plays);

    // A track that changes changes the prints the index was built from: it's built again, and
    // kept in place of the old one.
    let changed = device.root().join("Contents/0.wav");
    let modified = std::fs::metadata(&changed).unwrap().modified().unwrap();
    std::fs::File::options()
        .write(true)
        .open(&changed)
        .unwrap()
        .set_modified(modified + std::time::Duration::from_secs(5))
        .unwrap();
    let rebuilt = Identifier::with_library(&device, library, &cache, &ctx).unwrap();
    assert!(!rebuilt.cache.index);
    assert_eq!(rebuilt.index.encode(), cold.index.encode(), "the same audio indexes the same");
    assert_eq!(std::fs::read_dir(cache.root().join(INDEX_DIR)).unwrap().count(), 1, "one index per drive");
}

/// An index reads back as it was written, and nothing else does: bytes cut short or added to,
/// or written by another version.
#[hegel::test(test_cases = 50)]
fn an_index_reads_back_as_it_was_written(tc: TestCase) {
    use hegel::Generator;
    let prints: Vec<(u32, TrackPrint)> = tc.draw(
        gs::vecs(
            gs::tuples!(
                gs::integers::<u32>(),
                gs::vecs(gs::tuples!(gs::integers::<u32>().max_value(5_000), gs::integers::<u8>())).max_size(60),
                gs::floats::<f32>().min_value(0.0).max_value(1_200.0)
            )
            .map(|(id, peaks, duration_secs)| {
                let mut peaks: Vec<Peak> = peaks.into_iter().map(|(t, f)| Peak { t, f }).collect();
                peaks.sort();
                (id, TrackPrint { duration_secs, peaks })
            }),
        )
        .max_size(6),
    );
    let index = LibraryIndex::build(prints);
    let bytes = index.encode();
    let back = LibraryIndex::decode(&bytes).expect("reads back");
    assert_eq!(back.encode(), bytes);
    assert_eq!(
        (&back.track_ids, back.durations.len(), back.hashes()),
        (&index.track_ids, index.durations.len(), index.hashes())
    );
    let cut = tc.draw(gs::integers::<usize>().max_value(bytes.len() - 1));
    assert!(LibraryIndex::decode(&bytes[..cut]).is_none(), "cut short at {cut}");
    assert!(LibraryIndex::decode(&[bytes.as_slice(), &[0]].concat()).is_none());
    for at in [4, 6] {
        let mut older = bytes.clone();
        older[at] = older[at].wrapping_sub(1);
        assert!(LibraryIndex::decode(&older).is_none(), "another version's, at {at}");
    }
}

/// What's heard in a recording reads back bit for bit, and nothing else does.
#[hegel::test(test_cases = 50)]
fn a_recording_heard_reads_back_bit_for_bit(tc: TestCase) {
    let f = || gs::floats::<f32>().allow_nan(false);
    let peaks: Vec<Peak> = tc
        .draw(gs::vecs(gs::tuples!(gs::integers::<u32>(), gs::integers::<u8>())).max_size(50))
        .into_iter()
        .map(|(t, f)| Peak { t, f })
        .collect();
    let levels: Vec<FrameLevels> = tc
        .draw(gs::vecs(gs::tuples!(f(), f(), f(), f())).max_size(50))
        .into_iter()
        .map(|(peak, a, b, c)| FrameLevels { peak, bands: [a, b, c] })
        .collect();
    let format = SourceFormat {
        sample_rate: tc.draw(gs::integers::<u32>()),
        channels: tc.draw(gs::integers::<u16>()),
        duration_secs: tc.draw(gs::floats::<f64>().allow_nan(false)),
    };
    let heard = Heard { frames: tc.draw(gs::integers::<u32>()), peaks, levels };
    let bytes = Recordings::encode(&(format, heard));
    let (format2, heard2) = Recordings::decode(&bytes).expect("reads back");
    assert_eq!(Recordings::encode(&(format2, heard2)), bytes);
    assert_eq!(Recordings::written_by(&bytes[..8]), Some(Recordings::VERSION));
    let cut = tc.draw(gs::integers::<usize>().max_value(bytes.len() - 1));
    assert!(Recordings::decode(&bytes[..cut]).is_none(), "cut short at {cut}");
    let mut older = bytes.clone();
    older[4] = older[4].wrapping_sub(1);
    assert!(Recordings::decode(&older).is_none(), "another version's");
}

/// `store gc` removes what older versions kept beside the fingerprints (prints, recordings heard
/// and indexes) and keeps what this one reads.
#[test]
fn gc_removes_what_older_versions_fingerprinted() {
    use crate::store::Store;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let cache = Cache::in_store(&store);
    let heard = recordings(&cache);
    let print = TrackPrint { duration_secs: 1.0, peaks: vec![Peak { t: 1, f: 2 }] };
    cache.put("aa01", &print).unwrap();
    let mut v1 = print.encode();
    v1[4..6].copy_from_slice(&(VERSION - 1).to_le_bytes());
    std::fs::write(cache.file("aa02", Prints::EXT), &v1).unwrap();
    let recorded = (SourceFormat::default(), Heard { peaks: Vec::new(), levels: Vec::new(), frames: 0 });
    heard.put("bb01", &recorded).unwrap();
    let mut old = Recordings::encode(&recorded);
    old[4..8].copy_from_slice(&(Recordings::VERSION - 1).to_le_bytes());
    std::fs::create_dir_all(cache.root().join("bb")).unwrap();
    std::fs::write(heard.file("bb02", Recordings::EXT), &old).unwrap();
    let (_, kept) = library_index(&cache, "Main", Vec::new(), &Ctx::silent());
    assert!(!kept);
    let mut stale = std::fs::read(index_file(&cache, "Main")).unwrap();
    stale[38..40].copy_from_slice(&(INDEX_VERSION - 1).to_le_bytes());
    std::fs::write(index_file(&cache, "Old"), &stale).unwrap();

    let older = vec![index_file(&cache, "Old"), cache.file("aa02", "fp"), heard.file("bb02", "heard")];
    let mut found = crate::audio_cache::unread(&store);
    found.sort();
    let mut want = older.clone();
    want.sort();
    assert_eq!(found, want);
    let report = store.gc(false).unwrap();
    assert_eq!(report.removed_objects, 3);
    assert!(older.iter().all(|f| !f.exists()));
    assert_eq!(cache.get("aa01"), Some(print));
    assert!(heard.get("bb01").is_some());
    assert!(library_index(&cache, "Main", Vec::new(), &Ctx::silent()).1, "the current index stays");
}
