use std::io::pipe;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use loclib_mixid::test_util::{resample, synth, wav_of};
use loclib_plugin::{Incoming, PROTOCOL_VERSION, Peer, Pending, code, method};
use loclib_progress::Ctx;
use serde_json::{Value, json};

use super::*;

/// The host's end of the plugin, served on a thread over in-memory pipes and past `hello`.
struct Session {
    host: Peer,
    _incoming: Receiver<Incoming>,
    served: JoinHandle<()>,
}

fn start() -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    Session { host, _incoming: incoming, served }
}

impl Session {
    /// `track/analyze` with `sound`, as the host sends it.
    fn analyze(&self, audio: &Path) -> Pending {
        let track = json!({ "id": 1, "file_path": "/Contents/track.wav", "audio_path": audio });
        let req = json!({ "analyzer": "sound", "track": track, "context": { "drive": "USB" } });
        self.host.send_request(method::ANALYZE_TRACK, &req).unwrap()
    }

    /// Closes the plugin's input: it stops.
    fn stop(self) {
        self.host.close();
        self.served.join().unwrap();
    }
}

fn manifest() -> Manifest {
    Manifest::parse(MANIFEST).unwrap()
}

/// `secs` of loclib-mixid's test signal `seed` at `rate`, scaled by `gain` (12,000 is a
/// recording's level, more clips) and written as a 16-bit stereo WAV: the recordings the CLI's
/// `doctor --analyze` tests use.
fn recording(dir: &Path, seed: u32, secs: f64, rate: u32, gain: f32) -> PathBuf {
    let path = dir.join(format!("{seed}.wav"));
    let pcm: Vec<i32> = resample(&synth(secs, seed), loclib_mixid::fingerprint::RATE, rate)
        .iter()
        .map(|s| ((s * gain) as i32).clamp(-32_768, 32_767))
        .collect();
    std::fs::write(&path, wav_of(rate, 16, pcm.into_iter())).unwrap();
    path
}

#[test]
fn describes_one_analyzer_and_asks_for_nothing() {
    let manifest = manifest();
    manifest.validate(true).unwrap();
    assert!(manifest.capabilities.is_empty(), "{manifest:?}");
    let [analyzer] = &manifest.contributes.analyzers[..] else { panic!("{manifest:?}") };
    assert_eq!(analyzer.name, "sound");
    let mut columns: Vec<&str> = analyzer.fields.keys().map(String::as_str).collect();
    columns.sort();
    let expected =
        ["bpm", "bpm_confidence", "clipped_runs", "integrated_lufs", "key", "key_confidence", "true_peak_dbtp"];
    assert_eq!(columns, expected);
    let s = start();
    let described: Manifest = s.host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    s.stop();
}

/// The host keeps fields by the plugin's version, and loclib-analysis's `VERSION` changes
/// whenever its results would: a new one needs a new plugin version (and this test updated).
#[test]
fn the_version_moves_with_loclib_analysis() {
    let pair = (manifest().version, loclib_analysis::VERSION);
    assert_eq!(pair, ("0.1.0".to_string(), 3), "bump the plugin's version with loclib-analysis's");
}

/// Any recording, quiet or clipped, silent or too short for a tempo, gets the values doctor's
/// `--analyze` reports from loclib-analysis's own analysis of the same file (a tempo finding's
/// `estimate` and `confidence`, a key finding's Camelot `estimate` and `confidence`, a loudness
/// finding's `integrated_lufs`, `true_peak_dbtp` and `clipped_runs`), each of the type the
/// manifest declares, leaving out those with nothing to show.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(seed = 7919u32, secs = 8.0f64, rate = 44_100u32, gain = 12_000.0f32)]
#[hegel::explicit_test_case(seed = 31u32, secs = 5.0f64, rate = 11_025u32, gain = 200_000.0f32)]
#[hegel::explicit_test_case(seed = 5u32, secs = 3.0f64, rate = 48_000u32, gain = 0.0f32)]
#[hegel::explicit_test_case(seed = 5u32, secs = 0.05f64, rate = 44_100u32, gain = 12_000.0f32)]
fn fields_are_what_doctor_reports(tc: TestCase) {
    let seed = tc.draw(gs::integers::<u32>());
    let secs = tc.draw(gs::floats::<f64>().min_value(0.05).max_value(8.0));
    let rate = tc.draw(gs::sampled_from(vec![11_025u32, 44_100, 48_000]));
    let gain = tc.draw(gs::sampled_from(vec![0.0f32, 12_000.0, 200_000.0]));
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), seed, secs, rate, gain);
    let s = start();
    let fields = s.analyze(&path).wait::<Analysis>().unwrap().fields;
    s.stop();

    let declared = &manifest().contributes.analyzers[0].fields;
    for (name, value) in &fields {
        assert!(declared.get(name).is_some_and(|t| t.accepts(value)), "{name}: {value:?}");
    }
    let number = |name: &str| match fields.get(name) {
        None => None,
        Some(FieldValue::Number(n)) => Some(*n),
        Some(other) => panic!("{name}: {other:?}"),
    };
    let a = analyze(&path, Want::ALL, &Ctx::silent()).unwrap();
    let l = a.loudness.unwrap();
    // Doctor's tempo findings leave out a tempo of 0 (`TempoRelation::between` has none for it).
    let tempo = a.estimate.as_ref().filter(|e| e.bpm > 0.0);
    assert_eq!(number("bpm"), tempo.map(|e| f64::from(e.bpm)), "{fields:?}");
    assert_eq!(number("bpm_confidence"), tempo.map(|e| f64::from(e.bpm_confidence)));
    assert_eq!(number("key_confidence"), a.estimate.as_ref().map(|e| f64::from(e.key_confidence)));
    let key = |e: &loclib_analysis::Estimate| FieldValue::String(e.key.code());
    match a.estimate.as_ref() {
        // Keys that tie (confidence 0) are picked from in no fixed order, so either of two
        // analyses of one file may have any of them.
        Some(e) if e.key_confidence == 0.0 => assert!(matches!(fields.get("key"), Some(FieldValue::String(_)))),
        estimate => assert_eq!(fields.get("key"), estimate.map(key).as_ref()),
    }
    assert_eq!(number("integrated_lufs"), l.integrated_lufs);
    assert_eq!(number("true_peak_dbtp"), Some(l.true_peak_dbtp()).filter(|db| db.is_finite()));
    assert_eq!(fields.get("clipped_runs"), Some(&FieldValue::Integer(l.clipped_runs as i64)));
    if gain == 0.0 {
        // Silence: no tempo, key or level, and nothing clipped.
        assert_eq!(fields.into_iter().collect::<Vec<_>>(), [("clipped_runs".to_string(), FieldValue::Integer(0))]);
    } else if gain > 32_768.0 {
        // What doctor calls over 0 dBTP and clipped.
        assert!(number("true_peak_dbtp").unwrap() > 0.0 && l.clipped_runs > 0, "{fields:?}");
    }
}

/// A file that isn't there or isn't audio fails that track alone, as the sidecar's `io` and
/// `parse` errors, and the plugin goes on to the next.
#[test]
fn unreadable_audio_fails_the_track_alone() {
    let dir = tempfile::tempdir().unwrap();
    // What the CLI tests' demo drives hold for audio.
    let text = dir.path().join("Demo Track 1.mp3");
    std::fs::write(&text, "audio Demo Track 1".repeat(100)).unwrap();
    // A rate the loudness meter refuses.
    let slow = dir.path().join("slow.wav");
    std::fs::write(&slow, wav_of(8, 16, [1_000, -1_000].into_iter())).unwrap();
    let s = start();
    let gone = dir.path().join("gone.wav");
    for (audio, kind, message) in [
        (&gone, "io", gone.display().to_string()),
        (&text, "parse", format!("could not parse {}", text.display())),
        (&slow, "parse", format!("could not analyse {}", slow.display())),
    ] {
        let e = s.analyze(audio).wait::<Analysis>().unwrap_err();
        assert_eq!((e.code, e.kind()), (code::FAILED, Some(kind)), "{e:?}");
        assert!(e.message.starts_with(&message), "{e:?}");
    }
    let fine = recording(dir.path(), 7919, 1.0, 44_100, 12_000.0);
    assert!(s.analyze(&fine).wait::<Analysis>().unwrap().fields.contains_key("clipped_runs"));
    s.stop();
}

/// Cancelling stops a decode under way: the audio is a FIFO the test feeds for as long as the
/// plugin reads, so only the cancel can end it.
#[cfg(unix)]
#[test]
fn cancelling_stops_the_decode() {
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("endless.wav");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let s = start();
    let pending = s.analyze(&fifo);
    // Opening blocks until the plugin opens it to read: its decode has begun.
    let mut audio = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
    pending.cancel();
    // A WAV header saying it's about as long as a WAV can be, then a second of tone at a time
    // until the plugin stops reading.
    let mut header = wav_of(44_100, 16, std::iter::empty());
    let data_len: u32 = 0xFFFF_FF00;
    header[4..8].copy_from_slice(&(data_len + 36).to_le_bytes());
    header[40..44].copy_from_slice(&data_len.to_le_bytes());
    let second = wav_of(44_100, 16, (0..44_100).map(|i| ((i as f64 * 0.05).sin() * 8_000.0) as i32));
    let mut written = audio.write_all(&header);
    while written.is_ok() {
        written = audio.write_all(&second[44..]);
    }
    assert_eq!(written.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
    assert_eq!(pending.wait::<Analysis>().unwrap_err(), RpcError::cancelled());
    s.stop();
}
