//! `locallibrary mix`: a drive's tracks found in a recorded set, from real (synthetic) audio.

use std::path::{Path, PathBuf};

use crate::support::{Env, path};

/// The rate fingerprints are made at: the audio needs nothing finer.
const RATE: u32 = 11_025;

/// A deterministic noise-like signal with structure (a decaying two-partial note every 120 ms),
/// as the library's own fingerprint tests make: each seed is a different "track".
fn synth(secs: u32, seed: u32) -> Vec<f32> {
    let n = (secs * RATE) as usize;
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    let mut rand = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f32 / u32::MAX as f32
    };
    let mut out = vec![0.0f32; n];
    let note = (0.12 * RATE as f64) as usize;
    for start in (0..n).step_by(note) {
        let f = 150.0 + rand() * 2500.0;
        let g = 0.2 + rand() * 0.3;
        for i in 0..(note * 3).min(n - start) {
            let t = i as f32 / RATE as f32;
            let tau = 2.0 * std::f32::consts::PI * f * t;
            out[start + i] += (-t * 12.0).exp() * g * (tau.sin() + 0.5 * (1.5 * tau).sin());
        }
    }
    out
}

/// A 16-bit mono WAV of `samples`.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|s| ((s * 12_000.0) as i16).to_le_bytes()).collect();
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + data.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    for (v, n) in [(16u32, 4), (1, 2), (1, 2), (RATE, 4), (RATE * 2, 4), (2, 2), (16, 2)] {
        wav.extend(&v.to_le_bytes()[..n]);
    }
    wav.extend(b"data");
    wav.extend((data.len() as u32).to_le_bytes());
    wav.extend(data);
    wav
}

/// The demo drive, both of whose tracks are the same 40 seconds of audio (as WAV, whatever the
/// file is called), and a recording that plays it and then 40 seconds the drive doesn't have.
pub(super) fn drive_and_recording(env: &Env) -> (PathBuf, PathBuf) {
    let drive = env.demo("Demo");
    let track = synth(40, 7919);
    for n in [1, 2] {
        std::fs::write(drive.join(format!("Contents/Loopmasters/UnknownAlbum/Demo Track {n}.mp3")), wav(&track))
            .unwrap();
    }
    let recording = env.outside("friday.wav");
    std::fs::write(&recording, wav(&[track, synth(40, 5 * 7919)].concat())).unwrap();
    (drive, recording)
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn a_recording_is_identified_into_a_tracklist_a_report_and_data() {
    let env = Env::ascii();
    let (drive, recording) = drive_and_recording(&env);
    let d = path(&drive);
    let pages = env.outside("pages");
    let data = env.outside("friday.json");
    // A folder to make, named as the system names one: `pages/`, or `pages\` on Windows.
    let folder = format!("{}{}", pages.display(), std::path::MAIN_SEPARATOR);
    let out = env.ok(&["mix", path(&recording), "--drive", d, "--html", &folder]);
    let tracklist: Vec<&str> = out.lines().collect();
    assert_eq!(tracklist[0], "friday", "{out}");
    assert!(tracklist[1].starts_with("1:20 - 1 tracks from Demo - "), "{out}");
    let play = tracklist.iter().position(|l| l.contains("Loopmasters - Demo Track 1")).unwrap();
    assert!(tracklist[play].starts_with("  1      0:0"), "{out}");
    assert!(tracklist[play].contains("128.0   +0.0%"), "{out}");
    // The other track is the same audio: said under the play.
    assert_eq!(tracklist[play + 1].trim(), "same audio as Loopmasters - Demo Track 2", "{out}");
    assert!(tracklist[play + 2].contains("0:40  not in the library (0:40)"), "{out}");
    assert!(out.contains("fingerprinted 2 new track(s) on Demo"), "{out}");
    let pages_written = files_in(&pages);
    assert!(pages_written[0].starts_with("friday_") && pages_written[0].ends_with(".html"), "{pages_written:?}");
    assert!(out.contains(&format!("report: {}", pages.join(&pages_written[0]).display())), "{out}");

    // The fingerprints are kept: indexing finds nothing new, and the data file is written as named.
    let out = env.ok(&["mix", "--drive", d]);
    assert!(out.contains("Demo: 2 of 2 tracks fingerprinted (0 new)"), "{out}");
    let audio = env.outside("friday.flac");
    std::fs::write(&audio, "the master").unwrap();
    let export = ["mix", path(&recording), "--drive", d, "--export", path(&data), "--audio", path(&audio)];
    let out = env.ok(&export);
    assert!(out.contains(&format!("data: {}", data.display())) && !out.contains("fingerprinted"), "{out}");
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&data).unwrap()).unwrap();
    assert_eq!(doc["plays"].as_array().unwrap().len(), 1, "{doc}");
}

/// The report shows what analyzers worked out of each track it plays, of the fields they say
/// how to show, as they say.
#[test]
fn the_report_shows_the_fields_analyzers_say_how_to_show() {
    let mut env = Env::ascii();
    let hinted = crate::support::echo_plugin().replace(
        "fields = { bytes = \"integer\" }",
        "fields = { bytes = \"integer\" }\ndisplay.bytes = { unit = \"B\" }",
    );
    env.plugins(&[&hinted]);
    let (drive, recording) = drive_and_recording(&env);
    let d = path(&drive);
    env.ok(&["analyze", d, "--analyzer", "size"]);
    let page = env.outside("friday.html");
    env.ok(&["mix", path(&recording), "--drive", d, "--html", path(&page)]);
    let html = std::fs::read_to_string(&page).unwrap();
    let bytes = std::fs::metadata(drive.join("Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3")).unwrap().len();
    assert!(html.contains(&format!(r#""fields":[{{"label":"echo.bytes","value":"{bytes} B"}}]"#)), "{html}");
}

#[test]
fn several_recordings_are_written_into_a_folder() {
    let env = Env::ascii();
    let (drive, recording) = drive_and_recording(&env);
    let again = env.outside("saturday.wav");
    std::fs::copy(&recording, &again).unwrap();
    let pages = env.outside("pages");
    let out = env.ok(&["mix", path(&recording), path(&again), "--drive", path(&drive), "--html", path(&pages)]);
    // One tracklist after the other.
    assert!(out.starts_with("friday\n") && out.contains("\n\nsaturday\n"), "{out}");
    let written = files_in(&pages);
    assert!(written[0].starts_with("friday_") && written[1].starts_with("saturday_"), "{written:?}");
}

#[test]
fn indexing_says_which_tracks_it_couldnt_fingerprint() {
    let env = Env::ascii();
    let drive = env.demo("Demo");
    std::fs::remove_file(drive.join("Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3")).unwrap();
    let out = env.ok(&["mix", "--drive", path(&drive)]);
    assert!(out.contains("Demo: 0 of 2 tracks fingerprinted (0 new)"), "{out}");
    assert!(out.contains("1 track(s) have no audio file on the drive"), "{out}");
    assert!(out.contains("couldn't read Loopmasters - Demo Track 1: "), "{out}");
}

#[test]
fn only_recordings_mix_can_read_are_taken() {
    let env = Env::ascii();
    let notes = env.outside("notes.txt");
    std::fs::write(&notes, "not audio").unwrap();
    let err = env.fails(1, &["mix", path(&env.outside("gone.wav"))]);
    assert!(err.contains("gone.wav doesn't exist"), "{err}");
    let err = env.fails(1, &["mix", path(&notes)]);
    assert!(err.contains("notes.txt isn't a format mix reads"), "{err}");
    let set = env.outside("set.wav");
    std::fs::write(&set, wav(&synth(1, 1))).unwrap();
    let err = env.fails(1, &["mix", path(&set), path(&set), "--audio", path(&set)]);
    assert!(err.contains("--audio goes with a single mix"), "{err}");
}
