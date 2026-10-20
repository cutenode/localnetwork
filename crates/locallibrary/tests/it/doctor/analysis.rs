//! A second opinion on tempos, keys and levels: doctor lists stored values the analysis disagrees
//! with, the check before a set warns about levels, and ordering a set trusts keys the analysis
//! questions less, each from the values analyzers kept for the roles (put in the store
//! beforehand, as `locallibrary analyze` would have). Nothing is analysed here.

use crate::common::{self, add_track as add, heard};

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use locallibrary::check::{self, CheckOptions, Level};
use locallibrary::device::Device;
use locallibrary::doctor::{self, DoctorOptions, Section};
use locallibrary::edit::Edit;
use locallibrary::fields::{FieldValue, Role};
use locallibrary::progress::{CancelToken, Ctx, Event, Phase, Progress};
use locallibrary::sound::{KeyOutcome, LoudnessProblem, Plausible, TempoRelation};
use locallibrary::{Store, testing};
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

/// What an earlier analysis heard: `bpm` and `key`, at `lufs`, peaking at `peak` dBTP with
/// `clipped` runs of samples at full scale, as the sound plugin answers them.
fn analysis(bpm: f32, key: &str, lufs: f64, peak: f64, clipped: i64) -> Vec<(Role, FieldValue)> {
    vec![
        (Role::Bpm, FieldValue::Number(bpm.into())),
        (Role::BpmConfidence, FieldValue::Number(0.9f32.into())),
        (Role::Key, FieldValue::String(key.into())),
        (Role::KeyConfidence, FieldValue::Number(0.8f32.into())),
        (Role::Loudness, FieldValue::Number(lufs)),
        (Role::TruePeak, FieldValue::Number(peak)),
        (Role::ClippedRuns, FieldValue::Integer(clipped)),
    ]
}

/// Heard as stored, at the library's level.
fn steady(bpm: f32, key: &str, lufs: f64) -> Vec<(Role, FieldValue)> {
    analysis(bpm, key, lufs, -0.4, 0)
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
        heard(&store, &root, &format!("/Contents/Analysed/{title}.wav"), &steady(128.0, "8A", -8.0));
    }
    let half = add(&root, track("Half", 4, "8A", 64.0), &silence);
    heard(&store, &root, "/Contents/Analysed/Half.wav", &steady(128.0, "8A", -8.5));
    let relative = add(&root, track("Relative", 5, "8A", 128.0), &silence);
    heard(&store, &root, "/Contents/Analysed/Relative.wav", &steady(128.0, "8B", -7.5));
    let quiet = add(&root, track("Quiet", 6, "8A", 128.0), &silence);
    heard(&store, &root, "/Contents/Analysed/Quiet.wav", &steady(128.0, "8A", -20.0));
    let hot = add(&root, track("Hot", 7, "8A", 128.0), &silence);
    heard(&store, &root, "/Contents/Analysed/Hot.wav", &analysis(128.0, "8A", -6.0, 1.5, 4));
    // Silence: worked out, with no tempo, key, level or peak.
    let silent = add(&root, track("Silent", 8, "8A", 128.0), &silence);
    let nothing = [Role::Bpm, Role::Key, Role::Loudness, Role::TruePeak].map(|r| (r, FieldValue::Null));
    heard(
        &store,
        &root,
        "/Contents/Analysed/Silent.wav",
        &[&nothing[..], &[(Role::ClippedRuns, FieldValue::Integer(0))]].concat(),
    );

    let d = Device::open_export(&root).unwrap();
    let opts = DoctorOptions { only: vec![Section::Analysis], ..Default::default() };
    let decoded = Decoded::default();
    let r = doctor::run(&d, &opts, &Ctx::new(&decoded, CancelToken::new())).unwrap();
    assert!(r.duplicates.is_none() && r.missing.is_none(), "only the analysis");
    let a = r.analysis.unwrap();
    // The fixture's own tracks were never analysed, and nothing is analysed here.
    assert_eq!((a.analyzed, a.decoded, a.failed.len()), (8, 0, 0), "{a:#?}");
    assert_eq!(decoded.0.load(Ordering::Relaxed), 0);

    let tempo: Vec<(u32, TempoRelation, Option<Plausible>)> =
        a.tempo.iter().map(|f| (f.track.id, f.relation, f.likelier)).collect();
    assert_eq!(tempo, [(half, TempoRelation::Half, Some(Plausible::Estimate))], "{a:#?}");
    assert_eq!((a.tempo[0].estimate, a.tempo[0].confidence), (128.0, Some(0.9)), "as the analyzer has them");
    let keys: Vec<(u32, KeyOutcome)> = a.key.iter().map(|f| (f.track.id, f.relation)).collect();
    assert_eq!(keys, [(relative, KeyOutcome::Relative)]);
    assert_eq!((a.key[0].estimate.as_str(), a.key[0].confidence), ("8B", Some(0.8)));
    let level = |id| a.loudness.iter().find(|f| f.track.id == id).map(|f| f.problems.clone()).unwrap_or_default();
    assert_eq!(level(quiet), [LoudnessProblem::FarFromMedian], "{a:#?}");
    assert_eq!(level(hot), [LoudnessProblem::OverZero, LoudnessProblem::Clipped], "{a:#?}");
    assert_eq!((level(half), level(silent)), (vec![], vec![]));
    assert_eq!(a.median_lufs, Some(-8.0));
    // Not asked for, there's no analysis.
    assert!(doctor::run(&d, &DoctorOptions::default(), &Ctx::silent()).unwrap().analysis.is_none());

    // The check before a set warns about the levels, from what was kept.
    let report =
        check::check(std::slice::from_ref(&d), Some(&store), &CheckOptions::default(), &Ctx::silent()).unwrap();
    let item = report.items.iter().find(|i| i.check == "loudness").unwrap();
    assert_eq!(item.level, Level::Warning, "{item:?}");
    assert!(item.message.contains("Quiet") && item.message.contains("Hot"), "{}", item.message);
    let warned: Vec<u32> = report.loudness[0].tracks.iter().map(|f| f.track.id).collect();
    assert_eq!(warned, [quiet, hot]);
    // Without a store there's nothing kept to go on, and no item.
    let report = check::check(std::slice::from_ref(&d), None, &CheckOptions::default(), &Ctx::silent()).unwrap();
    assert!(!report.items.iter().any(|i| i.check == "loudness"));
    // Without an analyzer for the roles, nothing is read: no analysis, and no item.
    testing::roles_on_this_thread(Some(Default::default()));
    let a = doctor::run(&d, &opts, &Ctx::silent()).unwrap().analysis.unwrap();
    assert_eq!((a.analyzed, a.tempo.len()), (0, 0));
    let report =
        check::check(std::slice::from_ref(&d), Some(&store), &CheckOptions::default(), &Ctx::silent()).unwrap();
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
/// the other's parallel; without one kept, it orders as before.
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
    heard(&store, &root, &paths[2], &steady(124.0, "8A", -8.0));
    common::apply(&root, &[plan("Reordered")]);
    assert_eq!(order("Reordered"), [start, parallel, far]);
}
