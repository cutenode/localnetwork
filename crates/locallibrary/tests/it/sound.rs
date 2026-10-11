//! Scoring rekordbox's keys and tempos, and the analysis's, against GiantSteps-style annotations.
//! The drive's analyses are put in the store's cache beforehand, as an earlier run would have.

use crate::common::{self, add_track as add};

use std::fs;
use std::path::Path;

use locallibrary::api::{self, AnalysisBenchRequest};
use locallibrary::harmony::Camelot;
use locallibrary::progress::Ctx;
use locallibrary::sound::bench::{BenchOptions, ESTIMATE, REKORDBOX};
use locallibrary::sound::{self, Analysis, Estimate, KeyOutcome, TempoOutcome, VERSION};
use locallibrary::{Error, Store, paths, testing};
use rekordbox_pdb::NewTrack;

fn estimate(bpm: f32, key: &str) -> Analysis {
    Analysis {
        version: VERSION,
        duration_secs: 120.0,
        estimate: Some(Estimate { bpm, bpm_confidence: 0.9, key: Camelot::parse(key).unwrap(), key_confidence: 0.8 }),
        loudness: None,
    }
}

fn write(dir: &Path, name: &str, text: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(name), text).unwrap();
}

/// A drive holding three GiantSteps previews as rekordbox exported them, with their analyses
/// cached, and the datasets' annotations for them (and for one more track).
fn setup(dir: &Path) -> (std::path::PathBuf, Store) {
    let root = common::bigger_drive(dir, "GIANTSTEPS");
    let store = Store::open_or_init(dir.join("store")).unwrap();
    testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    let cache = sound::Cache::in_store(&store);
    for (id, key, bpm, heard) in [
        ("1001", "8A", 128.0, estimate(128.0, "8A")),
        ("1002", "8B", 64.0, estimate(128.0, "8A")),
        ("1003", "", 0.0, estimate(90.0, "3A")),
    ] {
        let path = format!("/Contents/UnknownArtist/UnknownAlbum/{id}.LOFI.mp3");
        let track = NewTrack { key: Some(key.into()), tempo: (bpm * 100.0) as u32, ..NewTrack::new(id, path.as_str()) };
        add(&root, track, id.as_bytes());
        let file = paths::to_fs(&root, &paths::key(&path));
        cache.put(&sound::Cache::key(&file).unwrap(), &heard).unwrap();
    }
    let keys = dir.join("keys");
    for (id, key) in [("1001", "A minor"), ("1002", "A minor"), ("1003", "C major"), ("9999", "D major")] {
        write(&keys, &format!("{id}.LOFI.key"), key);
    }
    write(&keys, "README", "not an annotation");
    let tempos = dir.join("tempos");
    for (id, bpm) in [("1001", "128"), ("1002", "128.0"), ("1003", "0.0")] {
        write(&tempos, &format!("{id}.LOFI.bpm"), bpm);
    }
    (root, store)
}

#[test]
fn rekordbox_and_the_analysis_are_scored_on_the_same_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _store) = setup(dir.path());
    let options =
        BenchOptions { keys: Some(dir.path().join("keys")), tempos: Some(dir.path().join("tempos")), audio: None };
    let req = AnalysisBenchRequest { drive: Some(root.display().to_string()), options };
    let r = api::analysis_bench(&req, &Ctx::silent()).unwrap();

    let rb = &r.keys[REKORDBOX];
    assert_eq!(rb.tracks, 3);
    assert_eq!(
        (rb.count(KeyOutcome::Correct), rb.count(KeyOutcome::Relative), rb.count(KeyOutcome::Missing)),
        (1, 1, 1)
    );
    assert!((rb.score - 1.3 / 3.0).abs() < 1e-9, "{rb:?}");
    assert_eq!(rb.misses, [("1002".to_string(), KeyOutcome::Relative), ("1003".to_string(), KeyOutcome::Missing)]);
    let heard = &r.keys[ESTIMATE];
    assert_eq!((heard.count(KeyOutcome::Correct), heard.count(KeyOutcome::Other)), (2, 1));

    // 1003 has no tempo in the annotations (v2's 0.0).
    let rb = &r.tempos[REKORDBOX];
    assert_eq!((rb.tracks, rb.count(TempoOutcome::Correct), rb.count(TempoOutcome::Half)), (2, 1, 1));
    assert_eq!((rb.score, r.accuracy2[REKORDBOX]), (0.5, 100.0));
    assert_eq!(r.tempos[ESTIMATE].score, 1.0);
    // 9999 is annotated but isn't on the drive.
    assert_eq!(r.unmatched, 1);
    assert!(r.failed.is_empty());
}

/// A track whose audio is missing from the drive is left out of the analysis's scores (rekordbox's
/// stored values are still scored), not counted as the analysis's miss.
#[test]
fn tracks_without_audio_are_left_out_of_the_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _store) = setup(dir.path());
    fs::remove_file(paths::to_fs(&root, "Contents/UnknownArtist/UnknownAlbum/1003.LOFI.mp3")).unwrap();
    let options = BenchOptions { keys: Some(dir.path().join("keys")), ..Default::default() };
    let req = AnalysisBenchRequest { drive: Some(root.display().to_string()), options };
    let r = api::analysis_bench(&req, &Ctx::silent()).unwrap();
    assert_eq!(r.keys[REKORDBOX].tracks, 3);
    let heard = &r.keys[ESTIMATE];
    assert_eq!((heard.tracks, heard.count(KeyOutcome::Missing)), (2, 0), "{heard:?}");
    // 9999 isn't on the drive, and 1003 has no audio to analyse.
    assert_eq!(r.unmatched, 2);
}

/// Audio in a folder is analysed instead of the drive's (a file that isn't audio fails), and the
/// MTG dataset's table is read for its single-key, confident rows.
#[test]
fn a_folder_of_audio_and_the_mtg_table() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, _store) = setup(dir.path());
    let audio = dir.path().join("audio");
    write(&audio, "1001.LOFI.mp3", "not audio");
    write(&audio, "1002.LOFI.mp3", "not audio either");
    write(&audio, "notes.txt", "skipped");
    write(dir.path(), "annotations.txt", "ID\tMANUAL KEY\tC\n1001\tA minor\t2\n1002\tA minor\t1\n1003\t-\t0\n");
    let options = BenchOptions { keys: Some(dir.path().join("annotations.txt")), tempos: None, audio: Some(audio) };
    let r = api::analysis_bench(&AnalysisBenchRequest { drive: None, options }, &Ctx::silent()).unwrap();
    assert!(!r.keys.contains_key(REKORDBOX) && r.tempos.is_empty());
    let heard = &r.keys[ESTIMATE];
    assert_eq!((heard.tracks, heard.count(KeyOutcome::Missing)), (1, 1));
    assert_eq!(r.failed.len(), 1, "{:?}", r.failed);
    assert!(r.failed[0].0.ends_with("1001.LOFI.mp3"));
}

#[test]
fn missing_or_unreadable_annotations_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _store) = setup(dir.path());
    let drive = Some(root.display().to_string());
    let bench =
        |options, drive: Option<String>| api::analysis_bench(&AnalysisBenchRequest { drive, options }, &Ctx::silent());

    let empty = dir.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    let none = BenchOptions { tempos: Some(empty.clone()), ..Default::default() };
    assert!(matches!(bench(none, drive.clone()), Err(Error::Invalid(_))));
    let keys = BenchOptions { keys: Some(dir.path().join("keys")), ..Default::default() };
    assert!(matches!(bench(keys, None), Err(Error::Invalid(_))));

    write(&empty, "1.LOFI.bpm", "fast");
    let bad = BenchOptions { tempos: Some(empty), ..Default::default() };
    let err = bench(bad, drive.clone()).unwrap_err();
    assert!(matches!(&err, Error::Parse { path, .. } if path.ends_with("1.LOFI.bpm")), "{err}");
    let gone = BenchOptions { keys: Some(dir.path().join("gone")), ..Default::default() };
    assert!(matches!(bench(gone, drive), Err(Error::Io { .. })));
}
