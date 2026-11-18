//! Scoring analyzers against annotations (`analysis bench`) through the host: the analyzers chosen
//! for the key and bpm roles measure the drive's tracks, or audio files on no drive, keeping
//! nothing.

use locallibrary::api::AnalysisBenchRequest;
use locallibrary::fields::{Analyzer, FieldStore, FieldValue, Role, roles};
use locallibrary::harmony::Camelot;
use locallibrary::plugin::Audio;
use locallibrary::sound::bench::{BenchOptions, BenchReport, REKORDBOX};
use locallibrary::sound::{KeyOutcome, TempoOutcome};
use locallibrary::{Track, paths};

use super::roles::heard;
use super::*;

impl Fixture {
    /// Two plugins with the `heard` analyzer, `echo` and `second`, whose roles this thread reads.
    fn heard_twice() -> Fixture {
        let f = Fixture::new(
            |dir| vec![spec(dir, &heard("echo"), &[]), spec(dir, &heard("second"), &[])],
            Options::default(),
        );
        locallibrary::testing::roles_on_this_thread(Some(f.plugins.roles()));
        f
    }

    /// `analysis bench` with `options`, on the drive or not, measured through the host.
    fn bench(&self, options: BenchOptions, on_drive: bool) -> Result<BenchReport> {
        let req = AnalysisBenchRequest { drive: on_drive.then(|| common::path(&self.drive)), options };
        let (plugins, store) = (&self.plugins, &self.store);
        api::analysis_bench(
            &req,
            store,
            |a, audio| plugins.measure(&a.plugin, &a.analyzer, audio, store, &Ctx::silent()),
            &Ctx::silent(),
        )
    }

    /// Key and tempo annotations for the drive's `tracks`, by their file names: each its own key
    /// and tempo, as rekordbox stored them. Answers the folders.
    fn annotate(&self, tracks: &[Track]) -> (PathBuf, PathBuf) {
        let (keys, tempos) = (self._dir.path().join("keys"), self._dir.path().join("tempos"));
        for (dir, ext) in [(&keys, "key"), (&tempos, "bpm")] {
            std::fs::create_dir_all(dir).unwrap();
            for t in tracks {
                let id = t.file_path.rsplit('/').next().unwrap().split('.').next().unwrap();
                let text = match ext {
                    "key" => Camelot::parse(&t.key).unwrap().note(),
                    _ => t.bpm.to_string(),
                };
                std::fs::write(dir.join(format!("{id}.{ext}")), text).unwrap();
            }
        }
        (keys, tempos)
    }
}

/// On a drive, the analyzer chosen for the key and bpm roles hears each track through the host,
/// scored under its name beside rekordbox by what it heard, and keeps nothing; choosing another
/// for a role scores that one for it.
#[test]
fn a_drives_tracks_are_scored_through_the_chosen_analyzer() {
    let f = Fixture::heard_twice();
    let before = common::library(&f.drive).tracks;
    let key = |t: &Track| Camelot::parse(&t.key).unwrap();
    // The first heard as it's annotated; the second in the parallel key, at double the tempo,
    // after reading the drive's tracks as the analyzer may.
    let read = json!(["tracks.get", { "drive": "USB", "ids": [before[1].id] }]);
    let tracks = f.hear(&[
        json!({ "tone": key(&before[0]).code(), "tempo": before[0].bpm }),
        json!({ "tone": key(&before[1]).parallel().code(), "tempo": before[1].bpm * 2.0, "read": read }),
    ]);
    let (keys, tempos) = f.annotate(&tracks);
    let options = BenchOptions { keys: Some(keys), tempos: Some(tempos), audio: None };
    let r = f.bench(options.clone(), true).unwrap();
    assert_eq!(r.keys.keys().collect::<Vec<_>>(), [REKORDBOX, "echo.heard"]);
    assert_eq!(r.keys[REKORDBOX].score, 1.0);
    let heard = &r.keys["echo.heard"];
    assert_eq!((heard.count(KeyOutcome::Correct), heard.count(KeyOutcome::Parallel)), (1, 1), "{heard:?}");
    let heard = &r.tempos["echo.heard"];
    assert_eq!((heard.count(TempoOutcome::Correct), heard.count(TempoOutcome::Double)), (1, 1), "{heard:?}");
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    let echo = Analyzer { plugin: "echo".into(), analyzer: "heard".into(), version: "1".into() };
    let audio = paths::to_fs(&f.drive, &paths::key(&tracks[0].file_path));
    assert!(FieldStore::in_store(&f.store).get(&audio, &[echo]).is_empty(), "nothing is kept");

    roles::choose(&f.store, Role::Key, Some("second.heard")).unwrap();
    let r = f.bench(options, true).unwrap();
    assert_eq!(r.keys.keys().collect::<Vec<_>>(), [REKORDBOX, "second.heard"]);
    assert_eq!(r.tempos.keys().collect::<Vec<_>>(), [REKORDBOX, "echo.heard"]);
}

/// A drive's track whose audio file is gone is measured as missing, and the others as heard.
#[test]
fn a_track_whose_audio_is_gone_is_measured_as_missing() {
    let f = Fixture::heard_twice();
    let before = common::library(&f.drive).tracks;
    let key = |t: &Track| Camelot::parse(&t.key).unwrap();
    let tracks = f.hear(&[
        json!({ "tone": key(&before[0]).code(), "tempo": before[0].bpm }),
        json!({ "tone": key(&before[1]).code(), "tempo": before[1].bpm }),
    ]);
    std::fs::remove_file(paths::to_fs(&f.drive, &paths::key(&tracks[0].file_path))).unwrap();
    let ids = [tracks[0].id, tracks[1].id];
    let audio = Audio::Tracks { drive: "USB", tracks: &ids };
    let measured = f.plugins.measure("echo", "heard", audio, &f.store, &Ctx::silent()).unwrap();
    let e = measured[0].as_ref().unwrap_err().to_string();
    assert!(e.ends_with("its audio file isn't on the drive"), "{e}");
    assert!(measured[1].is_ok(), "{:?}", measured[1]);
}

/// Audio files on no drive are handed to the analyzer as tracks with no library: what it hears
/// in each is scored, a file it fails on is listed, and the reads of a library it makes meanwhile
/// are refused, whatever drive they name, while the same read from a drive's track is answered.
#[test]
fn files_on_no_drive_are_measured_with_no_library_read() {
    let f = Fixture::heard_twice();
    let folder = f._dir.path().join("audio");
    std::fs::create_dir_all(&folder).unwrap();
    let read = json!(["tracks.get", { "drive": "USB", "ids": [1] }]);
    let files = [("1.LOFI.wav", json!({ "tone": "8A" })), ("2.LOFI.wav", json!({ "tone": "8A", "read": read }))];
    let files: Vec<PathBuf> = files
        .iter()
        .map(|(name, heard)| {
            std::fs::write(folder.join(name), heard.to_string()).unwrap();
            folder.join(name)
        })
        .collect();
    let measured = f.plugins.measure("echo", "heard", Audio::Files(&files), &f.store, &Ctx::silent()).unwrap();
    assert_eq!(measured[0].as_ref().unwrap()["tone"], FieldValue::String("8A".into()));
    let refused = measured[1].as_ref().unwrap_err().to_string();
    assert!(
        refused.contains("tracks.get reads a library, and the audio file the plugin was handed is on no drive"),
        "{refused}"
    );

    // The same read, from a drive's track, is answered.
    let t = &f.hear(&[json!({ "tone": "8A", "read": read })])[0];
    let on_drive = Audio::Tracks { drive: "USB", tracks: &[t.id] };
    let measured = f.plugins.measure("echo", "heard", on_drive, &f.store, &Ctx::silent()).unwrap();
    assert_eq!(measured[0].as_ref().unwrap()["tone"], FieldValue::String("8A".into()));

    // `analysis bench --audio`: both are scored, the one refused as a miss.
    let keys = f._dir.path().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(keys.join("1.LOFI.key"), "A minor").unwrap();
    std::fs::write(keys.join("2.LOFI.key"), "A minor").unwrap();
    let r = f.bench(BenchOptions { keys: Some(keys), tempos: None, audio: Some(folder) }, false).unwrap();
    let heard = &r.keys["echo.heard"];
    assert_eq!((heard.tracks, heard.count(KeyOutcome::Correct), heard.count(KeyOutcome::Missing)), (2, 1, 1));
    assert_eq!(r.failed.len(), 1, "{:?}", r.failed);
    assert!(r.failed[0].0.ends_with("2.LOFI.wav") && r.failed[0].1.contains("on no drive"), "{:?}", r.failed);
}

/// For any choice of analyzer for the key and bpm roles (either plugin's, the default, or none),
/// the drive's tracks are scored by the one chosen for each, and by none for a role turned off.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_the_chosen_analyzers_are_scored(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::heard_twice();
    let before = common::library(&f.drive).tracks;
    let answers: Vec<Value> =
        before.iter().map(|t| json!({ "tone": Camelot::parse(&t.key).unwrap().code(), "tempo": t.bpm })).collect();
    let (keys, tempos) = f.annotate(&f.hear(&answers));
    let mut expected = Vec::new();
    for role in [Role::Key, Role::Bpm] {
        let choice = tc.draw(gs::sampled_from(vec![None, Some("echo.heard"), Some("second.heard"), Some("off")]));
        let scored = match choice {
            Some("off") => {
                roles::turn_off(&f.store, role).unwrap();
                vec![REKORDBOX]
            }
            Some(name) => {
                roles::choose(&f.store, role, Some(name)).unwrap();
                vec![REKORDBOX, name]
            }
            None => vec![REKORDBOX, "echo.heard"],
        };
        expected.push(scored);
    }
    let r = f.bench(BenchOptions { keys: Some(keys), tempos: Some(tempos), audio: None }, true).unwrap();
    assert_eq!(r.keys.keys().collect::<Vec<_>>(), expected[0]);
    assert_eq!(r.tempos.keys().collect::<Vec<_>>(), expected[1]);
    // Each analyzer scored hears every track as rekordbox stored it.
    assert!(r.keys.values().all(|k| k.score == 1.0) && r.tempos.values().all(|t| t.score == 1.0), "{r:?}");
}
