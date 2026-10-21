use super::*;

#[test]
fn hot_cue_colour_codes_count_from_one() {
    assert_eq!((hot_cue_palette(1), hot_cue_palette(62)), (Some(HOT_CUE_PALETTE[0]), Some(HOT_CUE_PALETTE[61])));
    assert_eq!((hot_cue_palette(0), hot_cue_palette(63)), (None, None));
}

#[test]
fn phrases_are_named_as_rekordbox_names_them() {
    let names = [PhraseKind::Up, PhraseKind::Verse(2), PhraseKind::Unknown(42)].map(PhraseKind::name);
    assert_eq!(names, ["Up", "Verse 2", "Phrase 42"]);
}

use hegel::TestCase;
use hegel::generators as gs;

/// Verses and unknown phrases are named with their number; the others have names of their
/// own, all different. Each kind serialises in snake case, a numbered one with its number.
#[hegel::test(test_cases = 100)]
fn phrases_are_named_and_serialised_with_their_number(tc: TestCase) {
    let (verse, unknown) = (tc.draw(gs::integers::<u8>()), tc.draw(gs::integers::<u16>()));
    assert_eq!(PhraseKind::Verse(verse).name(), format!("Verse {verse}"));
    assert_eq!(PhraseKind::Unknown(unknown).name(), format!("Phrase {unknown}"));
    let named = [
        PhraseKind::Intro,
        PhraseKind::Up,
        PhraseKind::Down,
        PhraseKind::Bridge,
        PhraseKind::Chorus,
        PhraseKind::Outro,
    ];
    let mut names: Vec<String> = named.iter().map(|k| k.name()).collect();
    for (k, name) in named.iter().zip(&names) {
        assert_eq!(serde_json::to_value(k).unwrap(), name.to_lowercase());
    }
    names.sort();
    names.dedup();
    assert_eq!(names.len(), named.len());
    assert_eq!(serde_json::to_value(PhraseKind::Verse(verse)).unwrap(), serde_json::json!({ "verse": verse }));
    assert_eq!(serde_json::to_value(PhraseKind::Unknown(unknown)).unwrap(), serde_json::json!({ "unknown": unknown }));
}

/// A cue's loop length serialises as `[numerator, denominator]` (or null), as its schema says,
/// and the rest of an analysis keeps its fields' names.
#[hegel::test(test_cases = 100)]
fn analyses_serialise_as_their_schema_describes(tc: TestCase) {
    let loop_beats = tc.draw(gs::optional(gs::tuples!(gs::integers::<u16>(), gs::integers::<u16>())));
    let hot_cue = tc.draw(gs::optional(gs::integers::<u8>().max_value(15)));
    let cue = Cue {
        hot_cue,
        start_ms: tc.draw(gs::integers()),
        end_ms: tc.draw(gs::optional(gs::integers())),
        comment: tc.draw(gs::text().max_size(6)),
        color: hot_cue_palette(tc.draw(gs::integers())),
        color_id: tc.draw(gs::integers::<u8>().max_value(8)),
        loop_beats,
    };
    let json = serde_json::to_value(&cue).unwrap();
    let expected = loop_beats.map_or(serde_json::Value::Null, |(n, d)| serde_json::json!([n, d]));
    assert_eq!(json["loop_beats"], expected);
    assert_eq!(json["hot_cue"], serde_json::json!(hot_cue));
    assert_eq!(json["comment"], cue.comment.as_str());

    let rgb = tc.draw(gs::booleans());
    let analysis = TrackAnalysis {
        beats: vec![Beat {
            beat_number: tc.draw(gs::integers::<u16>().min_value(1).max_value(4)),
            bpm: 128.0,
            time_ms: 0,
        }],
        cues: vec![cue],
        preview: Some(Waveform {
            style: if rgb { WaveformStyle::Rgb } else { WaveformStyle::Blue },
            columns_per_sec: None,
            columns: vec![WaveColumn::default()],
        }),
        detail: None,
        structure: Some(SongStructure {
            mood: Mood::High,
            phrases: vec![Phrase { kind: PhraseKind::Chorus, start_beat: 1, end_beat: 17, fill_beat: None }],
        }),
    };
    let json = serde_json::to_value(&analysis).unwrap();
    assert_eq!(json["preview"]["style"], if rgb { "rgb" } else { "blue" });
    assert_eq!(json["structure"]["mood"], "high");
    assert_eq!(json["structure"]["phrases"][0]["kind"], "chorus");
    assert_eq!(json["cues"][0]["loop_beats"], expected);
    assert_eq!(json["detail"], serde_json::Value::Null);
}

/// Packed bands unpack to themselves, and only whole columns unpack; pooled, each band's loudest
/// stays the loudest; drawn, a column is its two tallest bands, the taller behind.
#[hegel::test(test_cases = 100)]
fn bands_pack_pool_and_draw(tc: TestCase) {
    let columns = tc.draw(gs::vecs(gs::tuples!(gs::integers(), gs::integers(), gs::integers())).max_size(40));
    let bands = Bands(columns.into_iter().map(|(l, m, h)| [l, m, h]).collect());
    let packed = bands.pack();
    assert_eq!(packed.len(), 3 * bands.0.len());
    assert_eq!(Bands::unpack(&packed), Some(bands.clone()));
    let extra = tc.draw(gs::integers::<usize>().min_value(1).max_value(2));
    assert_eq!(Bands::unpack(&[packed, vec![0; extra]].concat()), None);

    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(60));
    let pooled = bands.pooled(n);
    assert_eq!(pooled.0.len(), n);
    for band in 0..3 {
        let loudest = |b: &Bands| b.0.iter().map(|c| c[band]).max().unwrap_or(0);
        assert_eq!(loudest(&pooled), loudest(&bands), "band {band}");
    }

    let w = bands.waveform(Some(150.0));
    assert_eq!((w.style, w.columns_per_sec, w.columns.len()), (WaveformStyle::ThreeBand, Some(150.0), bands.0.len()));
    for (c, b) in w.columns.iter().zip(&bands.0) {
        let mut scaled: Vec<f32> = (0..3).map(|i| f32::from(b[i]) / 255.0 * crate::tokens::bands::SCALES[i]).collect();
        scaled.sort_by(|a, b| b.total_cmp(a));
        assert_eq!((c.back.height, c.front.height), (scaled[0], scaled[1]));
        assert!(c.back.height <= 1.0);
    }
}

#[test]
fn bands_pool_into_previews() {
    let bands = Bands(vec![[1, 0, 0], [0, 9, 0], [0, 0, 3], [4, 4, 4]]);
    assert_eq!(bands.pooled(2), Bands(vec![[1, 9, 0], [4, 4, 4]]));
    assert_eq!(bands.pooled(8).0[..2], [[1, 0, 0], [1, 0, 0]]);
    assert_eq!(Bands::default().pooled(3), Bands(vec![[0; 3]; 3]));
    // The lows behind, the mids (as tall as the highs) in front.
    let w = Bands(vec![[255, 0, 0]]).waveform(None);
    assert_eq!(
        (w.columns[0].back.color, w.columns[0].front.color),
        (crate::tokens::bands::LOW, crate::tokens::bands::MID)
    );
    assert_eq!(w.columns[0].front.height, 0.0);
}

/// Beats and cues read back as they're written, as a plugin reads them from the host.
#[test]
fn beats_and_cues_read_back() {
    let beat = Beat { beat_number: 1, bpm: 128.5, time_ms: 25 };
    let cue = Cue {
        hot_cue: Some(4),
        start_ms: 60_025,
        end_ms: Some(60_259),
        comment: "Vocal ♪".into(),
        color: Some([0xe6, 0x28, 0x28]),
        color_id: 0,
        loop_beats: Some((1, 2)),
    };
    let json = serde_json::to_string(&(beat, &cue)).unwrap();
    assert_eq!(serde_json::from_str::<(Beat, Cue)>(&json).unwrap(), (beat, cue));
}
