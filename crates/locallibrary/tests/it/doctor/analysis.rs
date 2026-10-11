//! A second opinion on tempos, keys and levels: doctor analyses the audio and lists stored values
//! the analysis disagrees with, the check before a set warns about levels from what's cached, and
//! ordering a set trusts keys the analysis questions less. Most analyses are put in the store's
//! cache beforehand, as an earlier run would have left them, so only one track is decoded.

use crate::common::{self, add_track as add};

use std::f64::consts::PI;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use locallibrary::check::{self, CheckOptions, Level};
use locallibrary::device::Device;
use locallibrary::doctor::{self, DoctorOptions, Section};
use locallibrary::edit::Edit;
use locallibrary::harmony::Camelot;
use locallibrary::progress::{CancelToken, Ctx, Event, Phase, Progress};
use locallibrary::sound::{
    self, Analysis, Estimate, KeyOutcome, Loudness, LoudnessProblem, Plausible, TempoRelation, VERSION,
};
use locallibrary::{Store, paths, testing};
use loclib_mixid::test_util::wav_of;
use rekordbox_pdb::NewTrack;

fn track(title: &str, n: u32, key: &str, bpm: f64) -> NewTrack {
    NewTrack {
        artist: Some("Someone".into()),
        key: Some(key.into()),
        tempo: (bpm * 100.0) as u32,
        analyze_path: format!("/PIONEER/USBANLZ/P0AA/0000000{n}/ANLZ0000.DAT"),
        duration: 30,
        ..NewTrack::new(title, format!("/Contents/Analysed/{title}.wav"))
    }
}

/// What an earlier analysis found: `bpm` and `key`, at `lufs`.
fn analysis(bpm: f32, key: &str, lufs: f64) -> Analysis {
    Analysis {
        version: VERSION,
        duration_secs: 30.0,
        estimate: Some(Estimate { bpm, bpm_confidence: 0.9, key: Camelot::parse(key).unwrap(), key_confidence: 0.8 }),
        loudness: Some(Loudness {
            integrated_lufs: Some(lufs),
            range_lu: 4.0,
            sample_peak: 0.9,
            true_peak: 0.95,
            clipped_runs: 0,
            clipped_samples: 0,
        }),
    }
}

/// Put `a` in `store`'s cache for the track at `device_path`, as an earlier run would have.
fn cached(store: &Store, root: &Path, device_path: &str, a: &Analysis) {
    let file = paths::to_fs(root, &paths::key(device_path));
    sound::Cache::in_store(store).put(&sound::Cache::key(&file).unwrap(), a).unwrap();
}

/// A 100 Hz sine driven to twice full scale: clipped flat on every half cycle.
fn clipped_wav(secs: f64) -> Vec<u8> {
    let n = (44_100.0 * secs) as usize;
    let pcm: Vec<i32> = (0..n)
        .map(|i| ((2.0 * (2.0 * PI * 100.0 * i as f64 / 44_100.0).sin()).clamp(-1.0, 1.0) * 32_767.0) as i32)
        .collect();
    wav_of(44_100, 16, pcm.into_iter())
}

/// A store for this thread's drive lookups, which keeps the analyses.
fn store(dir: &Path) -> Store {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    store
}

#[derive(Default)]
struct Decoded(AtomicUsize);

impl Progress for Decoded {
    fn on_event(&self, e: &Event) {
        if matches!(e, Event::Item { phase: Phase::Analyze, .. }) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[test]
fn doctor_questions_tempos_keys_and_levels() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let store = store(dir.path());
    let silence = vec![0u8; 64];
    // Three tracks the analysis agrees with set the library's usual tempo and level.
    for (n, title) in [(1, "Steady 1"), (2, "Steady 2"), (3, "Steady 3")] {
        add(&root, track(title, n, "8A", 128.0), &silence);
        cached(&store, &root, &format!("/Contents/Analysed/{title}.wav"), &analysis(128.0, "8A", -8.0));
    }
    let half = add(&root, track("Half", 4, "8A", 64.0), &silence);
    cached(&store, &root, "/Contents/Analysed/Half.wav", &analysis(128.0, "8A", -8.5));
    let relative = add(&root, track("Relative", 5, "8A", 128.0), &silence);
    cached(&store, &root, "/Contents/Analysed/Relative.wav", &analysis(128.0, "8B", -7.5));
    let quiet = add(&root, track("Quiet", 6, "8A", 128.0), &silence);
    cached(&store, &root, "/Contents/Analysed/Quiet.wav", &analysis(128.0, "8A", -20.0));
    // Analysed for real.
    let hot = add(&root, track("Hot", 7, "8A", 128.0), &clipped_wav(3.0));

    let d = Device::open_export(&root).unwrap();
    let opts = DoctorOptions { only: vec![Section::Analysis], ..Default::default() };
    let decoded = Decoded::default();
    let r = doctor::run(&d, &opts, &Ctx::new(&decoded, CancelToken::new())).unwrap();
    assert!(r.duplicates.is_none() && r.missing.is_none(), "only the analysis");
    let a = r.analysis.unwrap();
    // The fixture's placeholder audio doesn't decode; the new tracks' did (or came from the cache).
    assert_eq!(a.analyzed, 7, "{a:#?}");
    assert!(!a.failed.is_empty());
    assert!(decoded.0.load(Ordering::Relaxed) >= 1);

    let tempo: Vec<(u32, TempoRelation, Option<Plausible>)> =
        a.tempo.iter().filter(|f| f.track.id != hot).map(|f| (f.track.id, f.relation, f.likelier)).collect();
    assert_eq!(tempo, [(half, TempoRelation::Half, Some(Plausible::Estimate))], "{a:#?}");
    let keys: Vec<(u32, KeyOutcome)> =
        a.key.iter().filter(|f| f.track.id != hot).map(|f| (f.track.id, f.relation)).collect();
    assert_eq!(keys, [(relative, KeyOutcome::Relative)]);
    let level = |id| a.loudness.iter().find(|f| f.track.id == id).map(|f| f.problems.clone()).unwrap_or_default();
    assert_eq!(level(quiet), [LoudnessProblem::FarFromMedian], "{a:#?}");
    assert!(level(hot).contains(&LoudnessProblem::Clipped), "{a:#?}");
    assert_eq!(level(half), []);

    // Analysed once: the store keeps the results, and failures.
    let again = Decoded::default();
    doctor::run(&d, &opts, &Ctx::new(&again, CancelToken::new())).unwrap();
    assert_eq!(again.0.load(Ordering::Relaxed), 0);
    // Not asked for, nothing is analysed.
    assert!(doctor::run(&d, &DoctorOptions::default(), &Ctx::silent()).unwrap().analysis.is_none());

    // The check before a set warns about the levels, from the cache alone.
    let report =
        check::check(std::slice::from_ref(&d), Some(&store), &CheckOptions::default(), &Ctx::silent()).unwrap();
    let item = report.items.iter().find(|i| i.check == "loudness").unwrap();
    assert_eq!(item.level, Level::Warning, "{item:?}");
    assert!(item.message.contains("Quiet") && item.message.contains("Hot"), "{}", item.message);
    let warned: Vec<u32> = report.loudness[0].tracks.iter().map(|f| f.track.id).collect();
    assert!(warned.contains(&quiet) && warned.contains(&hot));
    // Without a store there's nothing cached to go on, and no item.
    let report = check::check(std::slice::from_ref(&d), None, &CheckOptions::default(), &Ctx::silent()).unwrap();
    assert!(!report.items.iter().any(|i| i.check == "loudness"));
}

/// A drive never analysed says nothing about levels before a set.
#[test]
fn the_check_is_quiet_about_levels_never_measured() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let store = store(dir.path());
    let d = Device::open_export(&root).unwrap();
    let report =
        check::check(std::slice::from_ref(&d), Some(&store), &CheckOptions::default(), &Ctx::silent()).unwrap();
    assert!(!report.items.iter().any(|i| i.check == "loudness") && report.loudness.is_empty());
}

/// Ordering a set goes easier on a clash between keys when the analysis says one of them may be
/// the other's parallel; without an analysis cached, it orders as before.
#[test]
fn ordering_a_set_trusts_questioned_keys_less() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let store = store(dir.path());
    let silence = vec![0u8; 64];
    let paths: Vec<String> =
        ["Start", "Far", "Parallel"].iter().map(|t| format!("/Contents/Analysed/{t}.wav")).collect();
    let start = add(&root, track("Start", 1, "8A", 124.0), &silence);
    let far = add(&root, track("Far", 2, "5A", 124.0), &silence);
    let parallel = add(&root, track("Parallel", 3, "11B", 124.0), &silence);
    let fill = Edit::SetEntries { playlist: "Set".into(), paths: paths.clone(), create: true };
    let plan = |into: &str| Edit::PlanSet {
        playlist: "Set".into(),
        start: Some(locallibrary::query::TrackSelector { ids: vec![start], ..Default::default() }),
        into: Some(into.into()),
    };
    let order = |into: &str| {
        let lib = Device::open_export(&root).unwrap().library().unwrap();
        lib.find_playlist(into).unwrap().track_ids.clone()
    };
    common::apply(&root, &[fill, plan("Ordered")]);
    assert_eq!(order("Ordered"), [start, far, parallel], "a tie keeps the playlist's order");

    // The analysis hears the third track in A minor, the parallel of its A major.
    cached(&store, &root, &paths[2], &analysis(124.0, "8A", -8.0));
    common::apply(&root, &[plan("Reordered")]);
    assert_eq!(order("Reordered"), [start, parallel, far]);
}
