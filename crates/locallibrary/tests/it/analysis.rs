//! Waveforms, beat grids and cues read from real rekordbox analysis files.

use crate::common;

use locallibrary::Device;
use locallibrary::anlz::{self, Mood, PhraseKind, WaveformStyle};

#[test]
fn reads_waveforms_and_grid_from_demo_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open_export(common::demo_drive(dir.path(), "USB")).unwrap();
    let lib = d.library().unwrap();
    assert!(!lib.tracks.is_empty());
    for t in &lib.tracks {
        let a = anlz::load(&d, t);
        // Colour preview from .EXT wins over the .DAT's blue one.
        let preview = a.preview.as_ref().expect("preview waveform");
        assert_eq!(preview.style, WaveformStyle::Rgb);
        assert_eq!(preview.columns.len(), 1200);
        assert!(preview.columns_per_sec.is_none());
        assert!(preview.columns.iter().any(|c| c.back.height > 0.3));
        assert!(preview.columns.iter().all(|c| (0.0..=1.0).contains(&c.back.height)));
        assert!(preview.columns.iter().all(|c| c.front.height <= c.back.height));

        // 150 columns per second, covering the whole track.
        let detail = a.detail.as_ref().expect("detail waveform");
        assert_eq!(detail.style, WaveformStyle::Rgb);
        assert_eq!(detail.columns_per_sec, Some(150.0));
        let secs = detail.columns.len() as f64 / 150.0;
        assert!((secs - t.duration_secs as f64).abs() < 2.0, "{secs} vs {}", t.duration_secs);

        assert!(!a.beats.is_empty());
        assert!(a.beats.windows(2).all(|w| w[0].time_ms < w[1].time_ms));

        // Phrases (rekordbox 6 masks them) tile the grid from beat 1.
        let st = a.structure.as_ref().expect("phrase analysis");
        assert_eq!(st.mood, Mood::Mid);
        assert_eq!(st.phrases[0].start_beat, 1);
        assert!(st.phrases.windows(2).all(|w| w[0].end_beat == w[1].start_beat));
        assert!(st.phrases.last().unwrap().end_beat as usize <= a.beats.len());

        // Grid-only loading skips the waveforms and phrases.
        let g = anlz::load_grid(&d, t);
        assert!(g.preview.is_none() && g.detail.is_none() && g.structure.is_none());
        assert_eq!(g.beats, a.beats);
    }
}

#[test]
fn falls_back_to_blue_preview_without_ext() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let t = &lib.tracks[0];
    let dat = d.path(&locallibrary::paths::key(&t.analyze_path));
    std::fs::remove_file(dat.with_extension("EXT")).unwrap();
    let a = anlz::load(&d, t);
    let preview = a.preview.expect("blue preview from .DAT");
    assert_eq!(preview.style, WaveformStyle::Blue);
    assert_eq!(preview.columns.len(), 400);
    assert!(a.detail.is_none());
}

#[test]
fn names_phrases_like_rekordbox() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open_export(common::demo_drive(dir.path(), "USB")).unwrap();
    let lib = d.library().unwrap();
    let t = lib.tracks.iter().find(|t| t.title == "Demo Track 2").unwrap();
    let st = anlz::load(&d, t).structure.unwrap();
    let kinds: Vec<PhraseKind> = st.phrases.iter().map(|p| p.kind).collect();
    assert_eq!(kinds.len(), 11);
    assert_eq!(kinds[0], PhraseKind::Intro);
    assert_eq!(kinds[1], PhraseKind::Verse(1));
    assert_eq!(kinds[5], PhraseKind::Bridge);
    assert_eq!(kinds[10], PhraseKind::Outro);
    assert_eq!((st.phrases[10].start_beat, st.phrases[10].end_beat), (221, 257));
    assert_eq!(PhraseKind::Verse(2).name(), "Verse 2");
}
