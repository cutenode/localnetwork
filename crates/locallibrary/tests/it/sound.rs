//! Scoring rekordbox's keys and tempos, and the analysis's, against GiantSteps-style annotations.
//! The drive's audio is a few seconds of a chord on a beat, whose tempo the test hears first (as
//! the benchmark does every run) for the annotations. In audio this short the analysis's best
//! major and minor keys tie, so no key is heard (see loclib-analysis's README).

use crate::common::{self, add_track as add};

use std::f64::consts::PI;
use std::fs;
use std::path::Path;

use locallibrary::api::{self, AnalysisBenchRequest};
use locallibrary::progress::Ctx;
use locallibrary::sound::bench::{BenchOptions, ESTIMATE, REKORDBOX};
use locallibrary::sound::{KeyOutcome, TempoOutcome};
use locallibrary::{Error, paths};
use loclib_analysis::{Want, analyze};
use loclib_mixid::test_util::wav_of;
use rekordbox_pdb::NewTrack;

fn write(dir: &Path, name: &str, text: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(name), text).unwrap();
}

/// Six seconds of a chord of `notes` (Hz) with a click on every beat at `bpm`, as a WAV: music
/// the analysis hears a key and a tempo in.
fn recording(notes: &[f64], bpm: f64) -> Vec<u8> {
    const RATE: u32 = 22_050;
    let beat = 60.0 / bpm;
    let pcm = (0..RATE * 6).map(|i| {
        let t = f64::from(i) / f64::from(RATE);
        let chord: f64 = notes.iter().map(|f| (2.0 * PI * f * t).sin()).sum::<f64>() / notes.len() as f64;
        let since = t % beat;
        let click = if since < 0.01 { (2.0 * PI * 2_000.0 * since).sin() * (1.0 - since / 0.01) } else { 0.0 };
        ((0.3 * chord + 0.5 * click) * 32_767.0) as i32
    });
    wav_of(RATE, 16, pcm.collect::<Vec<_>>().into_iter())
}

/// The tempo the analysis hears in `audio`, put in `dir` to be heard.
fn heard(dir: &Path, audio: &[u8]) -> f64 {
    let file = dir.join("heard.wav");
    fs::write(&file, audio).unwrap();
    let e = analyze(&file, Want { estimate: true, loudness: false }, &Ctx::silent()).unwrap().estimate.unwrap();
    assert!(e.bpm > 0.0, "a tempo to score: {e:?}");
    e.bpm.into()
}

/// A drive holding three GiantSteps previews as rekordbox exported them, and the datasets'
/// annotations for them (and for one more track): rekordbox stored 1001's key and tempo as
/// annotated, 1002 in the relative key at half the tempo, and no key or tempo for 1003, which has
/// no tempo annotated either. The tempos annotated are those the analysis hears.
fn setup(dir: &Path) -> std::path::PathBuf {
    let root = common::bigger_drive(dir, "GIANTSTEPS");
    let (keys, tempos) = (dir.join("keys"), dir.join("tempos"));
    let chords = [
        ("1001", "8A", 1.0, [220.0, 261.63, 329.63], 128.0, "A minor"),
        ("1002", "8B", 0.5, [261.63, 329.63, 392.0], 120.0, "A minor"),
        ("1003", "", 0.0, [164.81, 196.0, 246.94], 100.0, "C major"),
    ];
    for (id, key, stored, notes, bpm, annotated) in chords {
        let audio = recording(&notes, bpm);
        let tempo = heard(dir, &audio) * if id == "1003" { 0.0 } else { 1.0 };
        let path = format!("/Contents/UnknownArtist/UnknownAlbum/{id}.LOFI.wav");
        let track = NewTrack {
            key: Some(key.into()),
            tempo: (tempo * stored * 100.0) as u32,
            ..NewTrack::new(id, path.as_str())
        };
        add(&root, track, &audio);
        write(&keys, &format!("{id}.LOFI.key"), annotated);
        write(&tempos, &format!("{id}.LOFI.bpm"), &tempo.to_string());
    }
    write(&keys, "9999.LOFI.key", "D major");
    write(&keys, "README", "not an annotation");
    root
}

#[test]
fn rekordbox_and_the_analysis_are_scored_on_the_same_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let root = setup(dir.path());
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
    assert_eq!((heard.tracks, heard.count(KeyOutcome::Missing)), (3, 3), "{heard:?}");

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
    let root = setup(dir.path());
    fs::remove_file(paths::to_fs(&root, "Contents/UnknownArtist/UnknownAlbum/1003.LOFI.wav")).unwrap();
    let options = BenchOptions { keys: Some(dir.path().join("keys")), ..Default::default() };
    let req = AnalysisBenchRequest { drive: Some(root.display().to_string()), options };
    let r = api::analysis_bench(&req, &Ctx::silent()).unwrap();
    assert_eq!(r.keys[REKORDBOX].tracks, 3);
    let heard = &r.keys[ESTIMATE];
    // The two with audio, each with no key (see the top): 1003 isn't counted as a miss.
    assert_eq!((heard.tracks, heard.count(KeyOutcome::Missing)), (2, 2), "{heard:?}");
    // 9999 isn't on the drive, and 1003 has no audio to analyse.
    assert_eq!(r.unmatched, 2);
}

/// Audio in a folder is analysed instead of the drive's (a file that isn't audio fails), and the
/// MTG dataset's table is read for its single-key, confident rows.
#[test]
fn a_folder_of_audio_and_the_mtg_table() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
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
    let root = setup(dir.path());
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
