use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use loclib_mixid::test_util::{resample, synth, wav_of};
use loclib_plugin::test_util::{FakeHost, Response, Sent};
use loclib_plugin::{EventName, FieldType, Listener, Manifest, Output, Role, Suggestion, code, method};
use loclib_progress::Ctx;
use serde_json::{Value, json};

use super::*;

/// The plugin served by a fake host.
struct Session {
    host: FakeHost,
}

fn start() -> Session {
    Session { host: FakeHost::start(MANIFEST, serve) }
}

impl Session {
    /// `track/analyze` with `tempo-key-loudness`, as the host sends it, with a scratch folder of
    /// its own.
    fn analyze(&self, audio: &Path) -> Sent {
        self.send(audio, json!({ "drive": "USB" }))
    }

    /// `track/analyze`, with `scratch` as its scratch folder (none from a host without `blobs`).
    fn analyze_in(&self, audio: &Path, scratch: Option<&Path>) -> Sent {
        self.send(audio, json!({ "drive": "USB", "scratch": scratch }))
    }

    /// `track/analyze` of `audio`, in `context`.
    fn send(&self, audio: &Path, context: Value) -> Sent {
        let track = json!({ "id": 1, "file_path": "/Contents/track.wav", "audio_path": audio });
        let req = json!({ "analyzer": "tempo-key-loudness", "track": track, "context": context });
        self.host.send(method::ANALYZE_TRACK, &req)
    }

    /// Closes the plugin's input: it stops.
    fn stop(self) {
        self.host.stop();
    }
}

/// The waveform `sent` answered, read from its file in the request's scratch folder.
fn waveform(sent: &Sent, value: &FieldValue) -> Vec<u8> {
    let Ok(Some(Output::Waveform(w))) = Output::of(FieldType::Waveform, value) else { panic!("{value:?}") };
    assert_eq!(w.columns_per_sec, COLUMNS_PER_SEC);
    std::fs::read(sent.scratch().unwrap().join(w.blob)).unwrap()
}

fn manifest() -> Manifest {
    Manifest::parse(MANIFEST).unwrap()
}

/// `secs` of loclib-mixid's test signal `seed` at `rate`, scaled by `gain` (12,000 is a
/// recording's level, more clips) and written as a 16-bit stereo WAV: the recordings the CLI's
/// `doctor --analyze` tests use.
fn recording(dir: &Path, seed: u32, secs: f64, rate: u32, gain: f32) -> PathBuf {
    let path = dir.join(format!("{seed}.wav"));
    std::fs::write(&path, wav_of(rate, 16, pcm(seed, secs, rate, gain).into_iter())).unwrap();
    path
}

/// The 16-bit samples [`recording`] writes to each of its two channels.
fn pcm(seed: u32, secs: f64, rate: u32, gain: f32) -> Vec<i32> {
    resample(&synth(secs, seed), loclib_mixid::fingerprint::RATE, rate)
        .iter()
        .map(|s| ((s * gain) as i32).clamp(-32_768, 32_767))
        .collect()
}

/// The clipped runs loclib-analysis counts in a recording of `pcm`: runs of three or more
/// full-scale samples in a row (32,767 or more either way, as 16-bit audio decodes), in each of
/// its two channels. Gain over full scale doesn't make one: a short or quiet stretch can clip
/// only one or two samples at a time.
fn clipped_runs(pcm: &[i32]) -> u64 {
    let full = |s: &i32| s.abs() >= 32_767;
    let runs = pcm.chunk_by(|a, b| full(a) == full(b)).filter(|run| full(&run[0]) && run.len() >= 3);
    2 * runs.count() as u64
}

#[test]
fn describes_one_analyzer_and_hears_of_drives_changing() {
    let manifest = manifest();
    manifest.validate(true).unwrap();
    let capabilities: Vec<String> = manifest.capabilities.iter().map(ToString::to_string).collect();
    assert_eq!(capabilities, ["library:read", "analysis:read", "events:drives"]);
    let on = vec![EventName::DriveMounted, EventName::DriveChanged];
    assert_eq!(manifest.contributes.events, [Listener { on }]);
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
    assert_eq!(structured, [("waveform", FieldType::Waveform)]);
    // Every role of a track's sound: doctor reads them all.
    let roles: Vec<Role> = analyzer.roles.keys().copied().collect();
    let not_heard = [Role::Identity, Role::Recording, Role::Release, Role::Artists, Role::Isrc, Role::Credits];
    assert!(Role::ALL.iter().all(|r| roles.contains(r) != not_heard.contains(r)), "{roles:?}");
    // Started, the fake host checks that `describe` answers the manifest.
    start().stop();
}

/// The plugin served by a fake host, told `events` in order while the host answers each
/// `drive.library` with tracks 1 to `tracks` and each `track.fields` with those of them in
/// `analysed`: what it suggests, and the reads it asked for (each one's method and how many tracks).
fn told(events: &[EventNotify], tracks: u32, analysed: &[u32]) -> (Vec<HostNotify>, Vec<(String, usize)>) {
    told_failing(events, tracks, analysed, None)
}

/// [`told`], with the host failing each read of the method `failing` (`not_found`, as for a drive
/// that has gone).
fn told_failing(
    events: &[EventNotify],
    tracks: u32,
    analysed: &[u32],
    failing: Option<&str>,
) -> (Vec<HostNotify>, Vec<(String, usize)>) {
    let host = FakeHost::start(MANIFEST, serve);
    let fails = |method: &str| failing == Some(method);
    let (library_fails, fields_fail) = (fails("drive.library"), fails("track.fields"));
    let gone = || Err::<Value, _>(RpcError::failed("not_found", "not found: drive USB"));
    let (last, handled) = std::sync::mpsc::channel();
    host.on_call("drive.library", move |params| {
        if params["drive"] == "LAST" {
            let _ = last.send(());
            return Response::Hold;
        }
        if library_fails {
            return Response::Answer(gone());
        }
        let held = (1..=tracks).map(|id| loclib_plugin::loclib_model::Track { id, ..Default::default() });
        let library = loclib_plugin::loclib_model::Library { tracks: held.collect(), ..Default::default() };
        Response::Answer(Ok(json!({ "library": library, "page": { "total": tracks } })))
    });
    let analysed = analysed.to_vec();
    host.on_read("track.fields", move |params| {
        if fields_fail {
            return gone();
        }
        let asked: Vec<u32> = serde_json::from_value(params["tracks"].clone()).unwrap();
        let kept: Vec<Value> = asked
            .iter()
            .filter(|t| analysed.contains(t))
            .map(|t| json!({ "track": t, "plugins": { "sound": {} } }))
            .collect();
        Ok(json!({ "tracks": kept, "missing": [] }))
    });
    for e in events {
        host.event(e);
    }
    // One last event: its read comes once every event before it has been handled, and is held,
    // so stopping the plugin then cuts no read short.
    host.event(&databases_changed("LAST"));
    handled.recv().unwrap();
    host.stop();
    let reads = host
        .calls()
        .into_iter()
        .filter(|(_, params)| params["drive"] != "LAST")
        .map(|(method, params)| {
            let asked = match method.as_str() {
                "drive.library" => 0,
                "track.fields" => {
                    assert_eq!(params["plugin"], "sound");
                    serde_json::from_value::<Vec<u32>>(params["tracks"].clone()).unwrap().len()
                }
                other => panic!("{other}"),
            };
            (method, asked)
        })
        .collect();
    (host.notices(), reads)
}

/// `drive.changed` for `drive`, its databases changed.
fn databases_changed(drive: &str) -> EventNotify {
    EventNotify {
        event: EventName::DriveChanged,
        drive: Some(drive.into()),
        payload: json!({ "changed": ["databases"] }),
    }
}

/// Once a drive's databases change, it suggests its analyzer for the tracks the host keeps none
/// of its fields for (the host leaves out those it suggested before), asking for the fields a
/// thousand tracks at a time. A drive whose tracks are all analysed, and a change to a drive's
/// state alone, suggest nothing: the state alone isn't even read.
#[test]
fn a_drive_whose_databases_changed_has_its_new_tracks_suggested_for_analysing() {
    let (suggested, reads) = told(&[databases_changed("USB")], 2500, &[1, 3, 2400]);
    let tracks: Vec<u32> = (1..=2500).filter(|t| ![1, 3, 2400].contains(t)).collect();
    let analyze = AnalyzeAction { analyzer: "tempo-key-loudness".into(), drive: "USB".into(), tracks };
    let expected = HostNotify::action("new tracks have no tempo, key or loudness", &HostAction::Analyze(analyze));
    assert_eq!(suggested, std::slice::from_ref(&expected));
    let read = |m: &str, n| (m.to_string(), n);
    assert_eq!(
        reads,
        [read("drive.library", 0), read("track.fields", 1000), read("track.fields", 1000), read("track.fields", 500)]
    );
    // The host takes it: it's its own analyzer.
    let taken = expected.suggestion(&manifest()).unwrap();
    assert!(matches!(taken, Some(Suggestion::Action(HostAction::Analyze(_)))), "{taken:?}");
    let state = EventNotify { payload: json!({ "changed": ["state"] }), ..databases_changed("USB") };
    let (suggested, reads) = told(&[state, databases_changed("USB")], 2, &[1, 2]);
    assert_eq!((suggested, reads.len()), (vec![], 2), "{reads:?}");
}

/// A drive plugged in with an export (perhaps exported on another computer) has its tracks
/// without fields suggested as one whose databases changed does; one without an export isn't
/// read.
#[test]
fn a_drive_plugged_in_has_its_new_tracks_suggested_for_analysing() {
    let mounted = |has_export: bool| EventNotify {
        event: EventName::DriveMounted,
        drive: Some("USB".into()),
        payload: json!({ "name": "USB", "has_export": has_export }),
    };
    let (suggested, reads) = told(&[mounted(false), mounted(true)], 3, &[2]);
    let analyze = AnalyzeAction { analyzer: "tempo-key-loudness".into(), drive: "USB".into(), tracks: vec![1, 3] };
    let expected = HostNotify::action("new tracks have no tempo, key or loudness", &HostAction::Analyze(analyze));
    assert_eq!((suggested, reads.len()), (vec![expected], 2), "{reads:?}");
}

/// Whichever of its reads the host fails (the drive's library, or the fields it keeps), nothing
/// is suggested for that drive, nothing more is read for it, and the events after it are still
/// handled; a change to a drive's state alone isn't read at all.
#[hegel::test(test_cases = 10)]
fn prop_a_read_that_fails_suggests_nothing(tc: TestCase) {
    let failing = tc.draw(gs::sampled_from(vec!["drive.library", "track.fields"]));
    let tracks = tc.draw(gs::integers::<u32>().min_value(1).max_value(2500));
    let state = EventNotify { payload: json!({ "changed": ["state"] }), ..databases_changed("USB") };
    let (suggested, reads) = told_failing(&[state, databases_changed("USB")], tracks, &[], Some(failing));
    assert_eq!(suggested, []);
    let methods: Vec<&str> = reads.iter().map(|(m, _)| m.as_str()).collect();
    let read = match failing {
        "drive.library" => &["drive.library"][..],
        _ => &["drive.library", "track.fields"][..],
    };
    assert_eq!(methods, read);
}

/// For any library, what it suggests is its tracks without fields, in the library's order, the
/// first [`MAX_SUGGESTED_TRACKS`] of them, and nothing when there are none.
#[hegel::test(test_cases = 20)]
fn it_suggests_the_first_tracks_without_fields(tc: TestCase) {
    let n: u32 = tc.draw(gs::sampled_from(vec![0, 1, 3, 40, MAX_SUGGESTED_TRACKS as u32 * 2]));
    let every = tc.draw(gs::integers::<u32>().min_value(1).max_value(4));
    let analysed: Vec<u32> = (1..=n).filter(|t| t % every == 0).collect();
    let (suggested, _) = told(&[databases_changed("USB")], n, &analysed);
    let expected: Vec<u32> = (1..=n).filter(|t| !analysed.contains(t)).take(MAX_SUGGESTED_TRACKS).collect();
    let tracks_of =
        |n: &HostNotify| serde_json::from_value::<Vec<u32>>(n.action.clone().unwrap().params["tracks"].clone());
    let got: Vec<Vec<u32>> = suggested.iter().map(|n| tracks_of(n).unwrap()).collect();
    assert_eq!(got, if expected.is_empty() { vec![] } else { vec![expected] });
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
/// finding's `integrated_lufs`, `true_peak_dbtp` and `clipped_runs`), and the waveform it
/// measured, each of the type the manifest declares: every one, null when it has nothing to show.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(seed = 7919u32, secs = 8.0f64, rate = 44_100u32, gain = 12_000.0f32)]
#[hegel::explicit_test_case(seed = 31u32, secs = 5.0f64, rate = 11_025u32, gain = 200_000.0f32)]
#[hegel::explicit_test_case(seed = 5u32, secs = 3.0f64, rate = 48_000u32, gain = 0.0f32)]
#[hegel::explicit_test_case(seed = 5u32, secs = 0.05f64, rate = 44_100u32, gain = 12_000.0f32)]
#[hegel::explicit_test_case(seed = 44_276_527u32, secs = 0.0625f64, rate = 11_025u32, gain = 200_000.0f32)]
fn fields_are_what_doctor_reports(tc: TestCase) {
    let seed = tc.draw(gs::integers::<u32>());
    let secs = tc.draw(gs::floats::<f64>().min_value(0.05).max_value(8.0));
    let rate = tc.draw(gs::sampled_from(vec![11_025u32, 44_100, 48_000]));
    let gain = tc.draw(gs::sampled_from(vec![0.0f32, 12_000.0, 200_000.0]));
    let dir = tempfile::tempdir().unwrap();
    let path = recording(dir.path(), seed, secs, rate, gain);
    let s = start();
    let sent = s.analyze(&path);
    let fields = sent.wait::<Analysis>().unwrap().fields;
    let drawn = (fields["waveform"] != FieldValue::Null).then(|| waveform(&sent, &fields["waveform"]));
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
    assert_eq!(l.clipped_runs, clipped_runs(&pcm(seed, secs, rate, gain)), "{fields:?}");
    // A waveform but for silence.
    let columns = a.waveform.unwrap().columns;
    assert_eq!(drawn, columns.iter().any(|c| *c != [0; 3]).then(|| columns.as_flattened().to_vec()));
    if gain == 0.0 {
        // Silence: no tempo, key, level or waveform, and nothing clipped.
        let shown: Vec<(&String, &FieldValue)> = fields.iter().filter(|(_, v)| **v != FieldValue::Null).collect();
        assert_eq!(shown, [(&"clipped_runs".to_string(), &FieldValue::Integer(0))]);
    } else if gain > 32_768.0 {
        // What doctor calls over 0 dBTP.
        assert!(number("true_peak_dbtp").unwrap() > 0.0, "{fields:?}");
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
/// `bpm`.
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

/// The waveform `audio` answers, three bytes a column.
fn waveform_of(audio: &Path) -> Vec<u8> {
    let s = start();
    let sent = s.analyze(audio);
    let fields = sent.wait::<Analysis>().unwrap().fields;
    let bands = waveform(&sent, &fields["waveform"]);
    s.stop();
    bands
}

/// Clicks after a silence have a waveform with a column every 23 ms, the silence's empty and the
/// clicks' drawn.
#[test]
fn a_click_track_has_a_waveform() {
    let dir = tempfile::tempdir().unwrap();
    let bands = waveform_of(&clicks(dir.path(), 120.0, 0.7, 8.0));
    let columns = bands.len() / 3;
    assert_eq!(bands.len() % 3, 0);
    assert!((columns as f64 - 8.7 * COLUMNS_PER_SEC).abs() <= 2.0, "{columns} columns");
    let lead = (0.6 * COLUMNS_PER_SEC) as usize;
    assert!(bands[..lead * 3].iter().all(|&b| b == 0) && bands[lead * 3..].iter().any(|&b| b > 0));
}

/// Any click track at a tempo DJs play, after any silence, has a waveform as long as the track,
/// its silence empty.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(bpm = 150.0f64, lead = 2.5f64)]
fn waveforms_follow_the_audio(tc: TestCase) {
    let bpm = tc.draw(gs::floats::<f64>().min_value(90.0).max_value(150.0));
    let lead = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(3.0));
    let dir = tempfile::tempdir().unwrap();
    let bands = waveform_of(&clicks(dir.path(), bpm, lead, 6.0));
    // A column every 1024 samples, the last however few.
    let (silence, samples) = ((lead * 44_100.0) as usize, (6.0 * 44_100.0) as usize);
    assert_eq!(bands.len(), (silence + samples).div_ceil(1024) * 3);
    assert!(bands[..silence / 1024 * 3].iter().all(|&b| b == 0), "{lead} s of silence");
    // Each click (4 kHz) is drawn in the column it starts in, in the highs far over the mids.
    let beat = 60.0 / bpm * 44_100.0;
    for at in (0..).map(|k| (k as f64 * beat).ceil() as usize).take_while(|at| *at < samples) {
        let drawn = |at: usize| {
            let c = (silence + at) / 1024 * 3;
            bands.get(c..c + 3).is_some_and(|c| c[2] > 0 && c[2] > 2 * c[1])
        };
        // (Or from the next sample, rounding aside.)
        assert!(drawn(at) || drawn(at + 1), "the click {at} samples in");
    }
}

/// The host takes no more than a day's waveform: a longer one is left out.
#[hegel::test(test_cases = 100)]
#[hegel::explicit_test_case(columns = 3_720_937usize)]
#[hegel::explicit_test_case(columns = 3_720_938usize)]
fn waveforms_are_kept_for_up_to_a_day(tc: TestCase) {
    let columns = tc.draw(gs::integers::<usize>().max_value(10_000_000));
    // A day at 44.1 kHz, 1024 samples a column.
    assert_eq!(within_a_day(columns), columns as u64 * 1024 <= 86_400 * 44_100, "{columns}");
}

/// Whatever the recording, its waveform goes in the request's scratch folder, a file of its
/// columns; without one (a host without `blobs`) it's left out and the rest answered, and one
/// that's gone fails the track as `io`, naming the file.
#[hegel::test(test_cases = 4)]
fn prop_the_waveform_is_a_file_in_the_scratch_folder(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let seed = tc.draw(gs::integers::<u32>().max_value(10_000));
    let audio = recording(dir.path(), seed, 1.0, 44_100, 12_000.0);
    let s = start();
    let gone = dir.path().join("gone");
    for case in 0..3 {
        match case {
            0 => {
                let sent = s.analyze(&audio);
                let fields = sent.wait::<Analysis>().unwrap().fields;
                assert!(!waveform(&sent, &fields["waveform"]).is_empty());
            }
            1 => {
                let fields = s.analyze_in(&audio, None).wait::<Analysis>().unwrap().fields;
                assert!(!fields.contains_key("waveform") && fields.contains_key("clipped_runs"), "{fields:?}");
            }
            _ => {
                let e = s.analyze_in(&audio, Some(&gone)).wait::<Analysis>().unwrap_err();
                assert_eq!((e.code, e.kind()), (code::FAILED, Some("io")), "{e:?}");
                assert!(e.message.starts_with(&gone.join(WAVEFORM_BLOB).display().to_string()), "{e:?}");
            }
        }
    }
    s.stop();
}
