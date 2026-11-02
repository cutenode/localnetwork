//! A mix report's page and portable export with what the drive adds to each play: cover art,
//! the overview waveform, hot cues and phrases, from rekordbox's own analysis files (the demo
//! export's), with cues and art put there first so what comes out can be told.

use crate::common::{self, media::png, media::wav};

use std::path::{Path, PathBuf};

use locallibrary::anlz::{self, CueFile, CuePoint};
use locallibrary::artwork::{self, Artwork};
use locallibrary::device::Device;
use locallibrary::mix::export::{self, UNNAMED};
use locallibrary::mix::report::{self, Drive};
use locallibrary::mix::{CoverSource, MixInfo, MixReport, Play};
use locallibrary::model::{Library, Track};
use locallibrary::paths;
use locallibrary::profile::ProfilePrefs;

use serde_json::Value;

/// The demo export, with art on track 1 only, and on track 1 a memory cue and hot cue A (red,
/// named "Vocal") at 15 s.
fn drive(dir: &Path) -> (Device, Library) {
    let root = common::demo_drive(dir, "DNA");
    common::media::set_art(&root, &[1]);
    let d = Device::open_export(root).unwrap();
    let cues = [
        CuePoint { start_ms: 5_000, comment: "Intro".into(), color_id: 6, ..Default::default() },
        CuePoint {
            hot_cue: Some(0),
            start_ms: 15_000,
            comment: "Vocal".into(),
            color_code: 0x2a,
            color_rgb: [0xe6, 0x28, 0x28],
            ..Default::default()
        },
    ];
    let library = d.library().unwrap();
    let dat = d.path(&paths::key(&library.track(1).unwrap().analyze_path));
    for (path, kind) in [(dat.with_extension("EXT"), CueFile::Ext), (dat, CueFile::Dat)] {
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, anlz::write_cue_points(&bytes, kind, &cues).unwrap()).unwrap();
    }
    (d, library)
}

fn play(track: &Track, start: f64) -> Play {
    Play {
        track: track.clone(),
        start,
        end: start + 120.0,
        track_start: 0.0,
        track_end: 120.0,
        tempo: 1.0,
        speed_range: None,
        played_bpm: Some(track.bpm),
        key_lock: None,
        semitones: 0.0,
        key: None,
        played_key: None,
        confidence: 0.9,
        votes: 300,
        presence: vec![1.0],
        presence_step: 2.0,
        jumps: 0,
        alternates: Vec::new(),
    }
}

/// A report of a mix playing the drive's two tracks, then one that's no longer on it.
fn mix(library: &Library, recording: PathBuf) -> MixReport {
    let mut gone = library.tracks[0].clone();
    gone.file_path = "/Contents/gone.mp3".into();
    MixReport {
        mix: MixInfo {
            path: recording,
            kind: "audio",
            title: "Friday".into(),
            artist: None,
            genre: None,
            date: None,
            recorder: None,
            duration_secs: 400.0,
            sample_rate: 44_100,
            channels: 2,
            labels: Vec::new(),
            cover: None,
        },
        drive: "DNA".into(),
        drive_id: None,
        library: Default::default(),
        plays: vec![play(&library.tracks[0], 0.0), play(&library.tracks[1], 110.0), play(&gone, 230.0)],
        transitions: Vec::new(),
        unidentified: Vec::new(),
        coverage: 0.9,
        waveform: Default::default(),
        tempo_range: 0.16,
        analysed_at: chrono::Utc::now(),
        elapsed_secs: 1.0,
    }
}

fn hiding(t: &Track) -> ProfilePrefs {
    let mut prefs = ProfilePrefs::default();
    prefs.hidden_artists.push(t.artist.clone());
    prefs
}

#[test]
fn the_export_carries_what_the_drive_says_about_each_track() {
    let dir = tempfile::tempdir().unwrap();
    let (d, library) = drive(dir.path());
    // A recording with a cover of its own: the export carries it.
    let recording = dir.path().join("friday.wav");
    std::fs::write(&recording, wav()).unwrap();
    artwork::embed_in_file(&recording, &Artwork::from_bytes(&png(400, 400, 99)).unwrap()).unwrap();
    let mut r = mix(&library, recording.clone());
    r.mix.cover = Some(CoverSource::Embedded(recording));

    let e = export::build(&r, Some(Drive::new(&d, &library)), &ProfilePrefs::default());
    assert!(e.mix.cover.as_deref().is_some_and(|c| c.starts_with("data:image/jpeg;base64,")));
    let (one, two, gone) = (&e.plays[0].track, &e.plays[1].track, &e.plays[2].track);
    assert!(one.artwork.as_deref().is_some_and(|a| a.starts_with("data:image/jpeg;base64,")));
    assert_eq!(two.artwork, None, "track 2 has no art");
    let cues: Vec<(f64, &str, Option<&str>)> =
        one.cues.iter().map(|c| (c.at, c.label.as_str(), c.color.as_deref())).collect();
    assert_eq!(cues, [(15.0, "A", Some("#e62828"))], "hot cues only");
    for t in [one, two] {
        let overview = t.overview.as_ref().unwrap();
        assert!(!overview.heights.is_empty() && overview.heights.len() <= 200);
        assert_eq!(overview.colors.len(), overview.heights.len());
        assert!(overview.colors.iter().all(|c| c.len() == 7 && c.starts_with('#')), "{:?}", overview.colors);
        assert!(!t.phrases.is_empty());
        // In order, within the track (its length is in whole seconds, so the last phrase can end
        // a moment past it).
        for (p, next) in t.phrases.iter().zip(t.phrases.iter().skip(1)) {
            assert!(0.0 <= p.start && p.start < p.end && p.end <= next.start, "{p:?} {next:?}");
        }
        assert!(t.phrases.last().unwrap().end < t.duration_secs + 1.0);
    }
    assert!(gone.overview.is_none() && gone.cues.is_empty(), "nothing from a file the drive no longer has");

    // A track the profile hides keeps what doesn't name it, and loses its art.
    let hidden = export::build(&r, Some(Drive::new(&d, &library)), &hiding(&library.tracks[0]));
    let t = &hidden.plays[0].track;
    assert_eq!((t.title.as_str(), t.artwork.as_deref()), (UNNAMED, None));
    assert!(t.overview.is_some() && t.cues.len() == 1);
}

/// The data block of a report page.
fn page_data(html: &str) -> Value {
    let data = html.split_once(r#"<script id="data" type="application/json">"#).unwrap().1;
    serde_json::from_str(data.split_once("</script>").unwrap().0).unwrap()
}

#[test]
fn the_page_shows_each_tracks_art_waveform_and_cues() {
    let dir = tempfile::tempdir().unwrap();
    let (d, library) = drive(dir.path());
    let r = mix(&library, dir.path().join("friday.wav"));
    let at = dir.path().join("reports/friday.html");

    let v = page_data(&report::page(&r, Some(Drive::new(&d, &library)), None, &at, &ProfilePrefs::default(), false));
    let extras = v["extras"].as_array().unwrap();
    assert!(extras[0]["art"].as_str().unwrap().starts_with("data:image/jpeg;base64,"));
    assert_eq!(extras[1]["art"], Value::Null);
    assert!(!extras[1]["wave"].as_str().unwrap().is_empty());
    assert_eq!(extras[0]["cues"], serde_json::json!([{ "t": 15.0, "label": "A", "color": "#e62828" }]));
    assert_eq!(extras[2], serde_json::json!({ "wave": null, "phrases": [], "cues": [] }));

    // A hidden track's art would name it.
    let v = page_data(&report::page(&r, Some(Drive::new(&d, &library)), None, &at, &hiding(&library.tracks[0]), false));
    assert_eq!((&v["extras"][0]["art"], v["extras"][0]["wave"].is_string()), (&Value::Null, true));

    // Followed along elsewhere, a cue says its name too.
    let extras = report::extras(Drive::new(&d, &library), &r.plays[0], 0, false);
    assert_eq!(extras.cues[0].label, "A · Vocal");
    assert_eq!(extras.art, None, "art is the page's");
}

#[test]
fn a_recording_browsers_play_is_its_own_audio() {
    assert_eq!(report::playable_audio(Path::new("/mixes/Set #1.MP3")), Some(PathBuf::from("/mixes/Set #1.MP3")));
}
