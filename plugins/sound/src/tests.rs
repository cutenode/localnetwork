use std::io::pipe;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use loclib_mixid::test_util::{resample, synth, wav_of};
use loclib_plugin::loclib_model::analysis::Beat;
use loclib_plugin::{FieldType, Incoming, Manifest, Output, PROTOCOL_VERSION, Peer, Pending, Role, code, method};
use loclib_progress::Ctx;
use serde_json::{Value, json};

use super::*;

/// The host's end of the plugin, served on a thread over in-memory pipes and past `hello`.
struct Session {
    host: Peer,
    _incoming: Receiver<Incoming>,
    served: JoinHandle<()>,
    /// The scratch folder its requests give the plugin.
    scratch: tempfile::TempDir,
}

fn start() -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    Session { host, _incoming: incoming, served, scratch: tempfile::tempdir().unwrap() }
}

impl Session {
    /// `track/analyze` with `tempo-key-loudness`, as the host sends it, with a scratch folder.
    fn analyze(&self, audio: &Path) -> Pending {
        self.analyze_in(audio, Some(self.scratch.path()))
    }

    /// `track/analyze`, with `scratch` as its scratch folder (none from a host without `blobs`).
    fn analyze_in(&self, audio: &Path, scratch: Option<&Path>) -> Pending {
        let track = json!({ "id": 1, "file_path": "/Contents/track.wav", "audio_path": audio });
        let context = json!({ "drive": "USB", "scratch": scratch });
        let req = json!({ "analyzer": "tempo-key-loudness", "track": track, "context": context });
        self.host.send_request(method::ANALYZE_TRACK, &req).unwrap()
    }

    /// The waveform a request answered, read from its file in the scratch folder.
    fn waveform(&self, value: &FieldValue) -> Vec<u8> {
        let Ok(Some(Output::Waveform(w))) = Output::of(FieldType::Waveform, value) else { panic!("{value:?}") };
        assert_eq!(w.columns_per_sec, COLUMNS_PER_SEC);
        std::fs::read(self.scratch.path().join(w.blob)).unwrap()
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
    assert_eq!((manifest.id.as_str(), analyzer.name.as_str()), ("sound", "tempo-key-loudness"));
    let (structured, columns): (Vec<(&str, FieldType)>, Vec<_>) =
        analyzer.fields.iter().map(|(name, kind)| (name.as_str(), *kind)).partition(|(_, kind)| kind.is_structured());
    let mut columns: Vec<&str> = columns.into_iter().map(|(name, _)| name).collect();
    columns.sort();
    let expected =
        ["bpm", "bpm_confidence", "clipped_runs", "integrated_lufs", "key", "key_confidence", "true_peak_dbtp"];
    assert_eq!(columns, expected);
    // Drawn, and written to analysis files when asked, not columns.
    assert_eq!(structured, [("grid", FieldType::Beats), ("waveform", FieldType::Waveform)]);
    // Every role but identity: doctor reads them all.
    let roles: Vec<Role> = analyzer.roles.keys().copied().collect();
    assert!(Role::ALL.iter().all(|r| roles.contains(r) != (*r == Role::Identity)), "{roles:?}");
    let s = start();
    let described: Manifest = s.host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    s.stop();
}

/// The host keeps fields by the analyzer's version, and loclib-analysis's `VERSION` changes
/// whenever its results would: the analyzer's is it.
#[test]
fn the_analyzers_version_is_loclib_analysis_s() {
    let version = manifest().contributes.analyzers.remove(0).version;
    assert_eq!(version, loclib_analysis::VERSION.to_string(), "the analyzer's version is loclib-analysis's");
}

/// Any recording, quiet or clipped, silent or too short for a tempo, gets the values doctor's
/// `--analyze` reports from loclib-analysis's own analysis of the same file (a tempo finding's
/// `estimate` and `confidence`, a key finding's Camelot `estimate` and `confidence`, a loudness
/// finding's `integrated_lufs`, `true_peak_dbtp` and `clipped_runs`), and the grid and waveform
/// it measured, each of the type the manifest declares: every one, null when it has nothing to
/// show.
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
    let drawn = (fields["waveform"] != FieldValue::Null).then(|| s.waveform(&fields["waveform"]));
    s.stop();

    let declared = &manifest().contributes.analyzers[0].fields;
    assert!(fields.keys().eq(declared.keys()), "{fields:?}");
    for (name, value) in &fields {
        assert!(declared[name].accepts(value), "{name}: {value:?}");
    }
    let number = |name: &str| match &fields[name] {
        FieldValue::Null => None,
        FieldValue::Number(n) => Some(*n),
        other => panic!("{name}: {other:?}"),
    };
    let a = analyze(&path, Want::ALL, &Ctx::silent()).unwrap();
    let l = a.loudness.unwrap();
    // Doctor's tempo findings leave out a tempo of 0 (`TempoRelation::between` has none for it).
    let tempo = a.estimate.as_ref().filter(|e| e.bpm > 0.0);
    assert_eq!(number("bpm"), tempo.map(|e| f64::from(e.bpm)), "{fields:?}");
    assert_eq!(number("bpm_confidence"), tempo.map(|e| f64::from(e.bpm_confidence)));
    assert_eq!(number("key_confidence"), a.estimate.as_ref().map(|e| f64::from(e.key_confidence)));
    // Keys that tie (confidence 0) are none: two analyses of one file hear the same.
    let key = a.estimate.as_ref().and_then(|e| e.key);
    assert_eq!(fields["key"], key.map_or(FieldValue::Null, |k| FieldValue::String(k.code())), "{:?}", a.estimate);
    assert_eq!(number("integrated_lufs"), l.integrated_lufs);
    assert_eq!(number("true_peak_dbtp"), Some(l.true_peak_dbtp()).filter(|db| db.is_finite()));
    assert_eq!(fields["clipped_runs"], FieldValue::Integer(l.clipped_runs as i64));
    // A grid where the beats fall evenly enough, and a waveform but for silence.
    let stable = tempo.filter(|e| e.grid_stability >= MIN_GRID_STABILITY);
    assert_eq!(fields["grid"], stable.map_or(FieldValue::Null, |e| FieldValue::beats(&e.beats)));
    let columns = a.waveform.unwrap().columns;
    assert_eq!(drawn, columns.iter().any(|c| *c != [0; 3]).then(|| columns.as_flattened().to_vec()));
    if gain == 0.0 {
        // Silence: no tempo, key, level, grid or waveform, and nothing clipped.
        let shown: Vec<(&String, &FieldValue)> = fields.iter().filter(|(_, v)| **v != FieldValue::Null).collect();
        assert_eq!(shown, [(&"clipped_runs".to_string(), &FieldValue::Integer(0))]);
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

/// Audio drawn that can't be analysed: gone, text, a WAV at a rate the loudness meter refuses, or
/// (on Unix) a FIFO the decode reads for ever, until it's cancelled.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Unreadable {
    Gone,
    Text,
    Slow,
    Endless,
}

/// Any run of tracks whose audio can't be analysed fails each track alone, as the sidecar's `io`
/// (not there) and `parse` (no decoder plays it, or the meter can't measure it) errors, or
/// `cancelled` for a decode the host cancels, naming the file; the plugin answers the next one
/// all the same.
#[hegel::test(test_cases = 8)]
fn audio_that_cant_be_analysed_fails_its_track_alone(tc: TestCase) {
    let kinds = tc.draw(
        gs::vecs(gs::sampled_from(vec![Unreadable::Gone, Unreadable::Text, Unreadable::Slow, Unreadable::Endless]))
            .min_size(1)
            .max_size(3),
    );
    let dir = tempfile::tempdir().unwrap();
    let s = start();
    for (i, kind) in kinds.into_iter().enumerate() {
        let audio = dir.path().join(format!("{i}.wav"));
        let e = match kind {
            Unreadable::Gone => s.analyze(&audio).wait::<Analysis>().unwrap_err(),
            Unreadable::Text => {
                let words = tc.draw(gs::text().alphabet("abc \n").min_size(1).max_size(200));
                std::fs::write(&audio, format!("not audio {words}")).unwrap();
                s.analyze(&audio).wait::<Analysis>().unwrap_err()
            }
            Unreadable::Slow => {
                let rate = tc.draw(gs::integers::<u32>().min_value(1).max_value(8));
                // At least a sample: with none there is nothing for the meter to refuse, and an empty
                // recording is silence.
                let samples = tc.draw(
                    gs::vecs(gs::integers::<i32>().min_value(-32_768).max_value(32_767)).min_size(1).max_size(64),
                );
                std::fs::write(&audio, wav_of(rate, 16, samples.into_iter())).unwrap();
                s.analyze(&audio).wait::<Analysis>().unwrap_err()
            }
            Unreadable::Endless => endless(&s, &audio),
        };
        let failed = match kind {
            Unreadable::Gone => "io",
            Unreadable::Endless if cfg!(unix) => "cancelled",
            Unreadable::Endless => "io",
            _ => "parse",
        };
        assert_eq!((e.code, e.kind()), (code::FAILED, Some(failed)), "{kind:?}: {e:?}");
        if failed != "cancelled" {
            assert!(e.message.contains(&audio.display().to_string()), "{kind:?}: {e:?}");
        }
    }
    s.stop();
}

/// A track whose audio is a FIFO fed for as long as the plugin reads it, analysed and cancelled
/// once its decode has begun: the error it fails with. (Without FIFOs, the file isn't there.)
fn endless(s: &Session, fifo: &Path) -> RpcError {
    #[cfg(unix)]
    {
        use std::io::Write;

        assert!(std::process::Command::new("mkfifo").arg(fifo).status().unwrap().success());
        let pending = s.analyze(fifo);
        // Opening blocks until the plugin opens it to read: its decode has begun.
        let mut audio = std::fs::OpenOptions::new().write(true).open(fifo).unwrap();
        pending.cancel();
        let mut header = wav_of(44_100, 16, std::iter::empty());
        let data_len: u32 = 0xFFFF_FF00;
        header[4..8].copy_from_slice(&(data_len + 36).to_le_bytes());
        header[40..44].copy_from_slice(&data_len.to_le_bytes());
        let second = wav_of(44_100, 16, (0..44_100).map(|i| ((i as f64 * 0.05).sin() * 8_000.0) as i32));
        let mut written = audio.write_all(&header);
        while written.is_ok() {
            written = audio.write_all(&second[44..]);
        }
        pending.wait::<Analysis>().unwrap_err()
    }
    #[cfg(not(unix))]
    s.analyze(fifo).wait::<Analysis>().unwrap_err()
}

/// The player's columns, `track` and a mix's report show the fields as the manifest says: tempos
/// to a tenth, confidences as percentages, loudness in LUFS and true peaks signed, in dBTP.
#[test]
fn fields_show_with_their_units() {
    let display = manifest().contributes.analyzers.remove(0).display;
    let shown = |field: &str, v: FieldValue| display[field].show(&v);
    assert_eq!(shown("bpm", FieldValue::Number(123.814)), "123.8");
    assert_eq!(shown("bpm_confidence", FieldValue::Number(0.853)), "85%");
    assert_eq!(shown("key", FieldValue::String("8A".into())), "8A");
    assert_eq!(shown("key_confidence", FieldValue::Number(0.3)), "30%");
    assert_eq!(shown("integrated_lufs", FieldValue::Number(-8.24)), "-8.2 LUFS");
    assert_eq!(shown("true_peak_dbtp", FieldValue::Number(0.43)), "+0.4 dBTP");
    assert_eq!(shown("true_peak_dbtp", FieldValue::Number(-0.92)), "-0.9 dBTP");
    // A count shows as it is.
    assert!(!display.contains_key("clipped_runs"));
}

/// A 16-bit WAV of `lead` seconds of silence, then `secs` of a 4 kHz click on every beat at
/// `bpm`: a grid stratum-dsp tracks evenly at tempos DJs play.
fn clicks(dir: &Path, bpm: f64, lead: f64, secs: f64) -> PathBuf {
    let path = dir.join("clicks.wav");
    let rate = 44_100.0;
    let beat = 60.0 / bpm;
    let click = |t: f64| 0.8 * (-(t % beat) * 60.0).exp() * (std::f64::consts::TAU * 4_000.0 * (t % beat)).sin();
    let silence = std::iter::repeat_n(0.0, (lead * rate) as usize);
    let pcm: Vec<i32> = silence
        .chain((0..(secs * rate) as usize).map(|i| click(i as f64 / rate)))
        .map(|s| (s * 32_767.0).round() as i32)
        .collect();
    std::fs::write(&path, wav_of(44_100, 16, pcm.into_iter())).unwrap();
    path
}

/// What a click track answers: its grid's beats (none when it's null) and its waveform's
/// columns, three bytes each.
fn grid_and_waveform(audio: &Path) -> (FieldValue, Vec<Beat>, Vec<u8>) {
    let s = start();
    let fields = s.analyze(audio).wait::<Analysis>().unwrap().fields;
    let bands = s.waveform(&fields["waveform"]);
    s.stop();
    let beats = match Output::of(FieldType::Beats, &fields["grid"]).unwrap() {
        Some(Output::Beats(beats)) => beats,
        None => Vec::new(),
        other => panic!("{other:?}"),
    };
    (fields["bpm"].clone(), beats, bands)
}

/// Clicks after a silence have a grid the host can write to a track's analysis file (in order,
/// counted in fours from 1, at the track's tempo) that starts where they do, a beat apart; their
/// waveform has a column every 23 ms, the silence's empty.
#[test]
fn a_click_track_has_a_grid_and_a_waveform() {
    let dir = tempfile::tempdir().unwrap();
    let (bpm, beats, bands) = grid_and_waveform(&clicks(dir.path(), 120.0, 0.7, 8.0));
    let FieldValue::Number(bpm) = bpm else { panic!("{bpm:?}") };
    assert!(beats.len() > 4, "{beats:?}");
    let numbers: Vec<u16> = beats.iter().map(|b| b.beat_number).collect();
    assert_eq!(numbers, (0..beats.len()).map(|i| (i % 4 + 1) as u16).collect::<Vec<_>>());
    assert!(beats.iter().all(|b| b.bpm == bpm), "{beats:?}");
    // The first click is at 700 ms, and they're half a second apart.
    assert!((650..=1200).contains(&beats[0].time_ms), "{beats:?}");
    let apart = (60_000.0 / bpm).round() as u32;
    assert!(beats.windows(2).all(|w| (w[1].time_ms - w[0].time_ms).abs_diff(apart) <= 1), "{beats:?}");
    let columns = bands.len() / 3;
    assert_eq!(bands.len() % 3, 0);
    assert!((columns as f64 - 8.7 * COLUMNS_PER_SEC).abs() <= 2.0, "{columns} columns");
    let lead = (0.6 * COLUMNS_PER_SEC) as usize;
    assert!(bands[..lead * 3].iter().all(|&b| b == 0) && bands[lead * 3..].iter().any(|&b| b > 0));
}

/// Any click track at a tempo DJs play, after any silence: a grid, when it has one, that the host
/// can write (in order, counted in fours from 1, at the track's tempo) and that starts after the
/// silence; a waveform as long as the track, its silence empty.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(bpm = 150.0f64, lead = 2.5f64)]
fn grids_and_waveforms_follow_the_audio(tc: TestCase) {
    let bpm = tc.draw(gs::floats::<f64>().min_value(90.0).max_value(150.0));
    let lead = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(3.0));
    let dir = tempfile::tempdir().unwrap();
    let (heard, beats, bands) = grid_and_waveform(&clicks(dir.path(), bpm, lead, 6.0));
    for (i, b) in beats.iter().enumerate() {
        assert_eq!(b.beat_number, (i % 4 + 1) as u16, "{beats:?}");
        assert_eq!(FieldValue::Number(b.bpm), heard, "{beats:?}");
        // Onsets are framed 46 ms at a time.
        assert!(f64::from(b.time_ms) >= lead * 1000.0 - 100.0, "{lead} s of silence: {beats:?}");
    }
    assert!(beats.windows(2).all(|w| w[0].time_ms < w[1].time_ms), "{beats:?}");
    let columns = bands.len() / 3;
    assert!((columns as f64 - (lead + 6.0) * COLUMNS_PER_SEC).abs() <= 2.0, "{columns} columns");
    let silent = ((lead - 0.1).max(0.0) * COLUMNS_PER_SEC) as usize;
    assert!(bands[..silent * 3].iter().all(|&b| b == 0), "{lead} s of silence");
}

/// The waveform goes in the request's scratch folder: without one (a host without `blobs`) it's
/// left out and the rest answered, and one that's gone fails the track as `io`, naming the file.
#[test]
fn the_waveform_needs_a_scratch_folder() {
    let dir = tempfile::tempdir().unwrap();
    let audio = recording(dir.path(), 7919, 1.0, 44_100, 12_000.0);
    let s = start();
    let fields = s.analyze_in(&audio, None).wait::<Analysis>().unwrap().fields;
    assert!(!fields.contains_key("waveform") && fields.contains_key("clipped_runs"), "{fields:?}");
    let gone = dir.path().join("gone");
    let e = s.analyze_in(&audio, Some(&gone)).wait::<Analysis>().unwrap_err();
    assert_eq!((e.code, e.kind()), (code::FAILED, Some("io")), "{e:?}");
    assert!(e.message.starts_with(&gone.join(WAVEFORM_BLOB).display().to_string()), "{e:?}");
    s.stop();
}
