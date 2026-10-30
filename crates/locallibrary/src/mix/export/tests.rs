use super::*;
use crate::mix::{CoverSource, MixInfo, MixWaveform, Span, Transition};

fn report() -> MixReport {
    let track = Track {
        key: "Fm".into(),
        comment: "private note".into(),
        duration_secs: 172,
        rating: 5,
        play_count: 3,
        ..loclib_model::test_util::track(1, "Loopmasters", "Demo Track 1", "/Contents/Loopmasters/Demo Track 1.mp3")
    };
    let play = Play {
        track,
        start: 4.123,
        end: 134.5,
        track_start: 2.0,
        track_end: 130.0,
        tempo: 1.0234567,
        played_bpm: Some(131.0),
        key_lock: Some(true),
        semitones: 0.0,
        key: Some("4A".into()),
        played_key: Some("4A".into()),
        confidence: 0.91,
        votes: 120,
        presence: vec![0.2, 0.8, 1.0],
        presence_step: 2.0,
        jumps: 1,
        alternates: vec![],
    };
    MixReport {
        mix: MixInfo {
            path: "/Users/someone/Mixes/set.wav".into(),
            kind: "audio",
            title: "  Friday  ".into(),
            artist: Some("DJ".into()),
            genre: Some(String::new()),
            date: None,
            recorder: None,
            duration_secs: 1335.0,
            sample_rate: 44100,
            channels: 2,
            labels: vec![],
            cover: None,
        },
        drive: "DNA".into(),
        drive_id: None,
        library: Default::default(),
        plays: vec![play.clone(), play],
        transitions: vec![Transition {
            from: 0,
            to: 1,
            at: 130.0,
            overlap: 4.5,
            harmony: Some("same key".into()),
            tempo_change: Some(0.04),
        }],
        unidentified: vec![Span { start: 300.0, end: 340.0 }],
        coverage: 0.9876,
        waveform: MixWaveform {
            columns_per_sec: 4.0,
            peak: vec![0; 5340],
            low: (0..5340).map(|i| (i % 256) as u8).collect(),
            mid: vec![1; 5340],
            high: vec![2; 5340],
        },
        tempo_range: 0.16,
        analysed_at: chrono::Utc::now(),
        elapsed_secs: 1.0,
    }
}

#[test]
fn exports_what_a_page_needs_and_nothing_local() {
    let e = build(&report(), None, &ProfilePrefs::default());
    assert_eq!((e.format, e.version), (FORMAT, VERSION));
    assert_eq!(e.mix.title, "Friday");
    assert_eq!(e.mix.genre, None);
    assert_eq!(e.waveform.low.len(), MIX_COLUMNS);
    assert!((e.waveform.seconds_per_column - 1335.0 / 1200.0).abs() < 0.001);
    assert_eq!(e.plays[0].start, 4.12);
    assert_eq!(e.plays[0].tempo, 1.0235);
    assert_eq!(e.plays[0].presence, [20, 80, 100]);
    assert_eq!(e.coverage, 0.988);
    let json = serde_json::to_string(&e).unwrap();
    for leak in
        ["/Users/someone", "/Contents/", "private note", "rating", "file_path", "filePath", "DNA", "\"library\""]
    {
        assert!(!json.contains(leak), "{leak} leaked into the export");
    }
    assert!(json.contains("\"playedBpm\":131.0"));
    assert!(json.contains("\"keyLock\":true"));
}

/// A track the profile hides is "ID - ID", with nothing else that names it; a cover found
/// next to the recording (a guess) stays out.
#[test]
fn hidden_tracks_and_guessed_covers_stay_out() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("friday.png");
    image::RgbImage::new(32, 32).save(&png).unwrap();
    let mut r = report();
    r.plays[1].track.album = "White label".into();
    r.mix.cover = Some(CoverSource::File(png));
    let mut prefs = ProfilePrefs::default();
    prefs.hidden.push("loopmasters - demo track 1".into());
    let e = build(&r, None, &prefs);
    let t = &e.plays[1].track;
    assert_eq!((t.title.as_str(), t.artist.as_deref(), t.album.as_deref()), (UNNAMED, Some(UNNAMED), None));
    assert_eq!(t.bpm, Some(128.0), "how it was played still shows");
    assert!(e.mix.cover.is_none());
    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("Demo Track") && !json.contains("White label"), "{json}");
}

/// A cover given for the recording (`mix --cover`) is shared like an embedded one.
#[test]
fn a_given_cover_is_shared() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("cover.png");
    image::RgbImage::from_pixel(32, 32, image::Rgb([200, 40, 90])).save(&png).unwrap();
    let mut r = report();
    r.mix.cover = Some(CoverSource::Given(png));
    let cover = build(&r, None, &ProfilePrefs::default()).mix.cover;
    assert!(cover.is_some_and(|c| c.starts_with("data:image/jpeg;base64,")));
}

#[test]
fn downsampling_keeps_peaks() {
    assert_eq!(downsample(&[1, 9, 2, 3, 8, 4], 3), [9, 3, 8]);
    assert_eq!(downsample(&[1, 2], 3), [1, 2]);
}

use crate::anlz::{
    Beat, Cue, Mood, Phrase, PhraseKind, SongStructure, TrackAnalysis, WaveColumn, WaveLayer, Waveform, WaveformStyle,
};
use crate::device::Device;
use crate::mix::report::{PlayExtras, extras};
use crate::model::Library;
use hegel::TestCase;
use hegel::generators as gs;

/// Analysis files as a drive has them: an overview, a beat grid every half second, phrases
/// on it (some running past its end) and cues, hot and memory.
#[hegel::test_helper]
fn analysis(tc: &TestCase) -> TrackAnalysis {
    let heights = tc.draw(gs::vecs(gs::floats::<f32>().min_value(0.0).max_value(1.2)).max_size(700));
    let columns = heights
        .iter()
        .map(|&h| WaveColumn { back: WaveLayer { height: h, color: [9, 99, 199] }, front: WaveLayer::default() })
        .collect();
    let beats: Vec<Beat> = (0..tc.draw(gs::integers::<u32>().max_value(64)))
        .map(|i| Beat { beat_number: (i % 4) as u16 + 1, bpm: 120.0, time_ms: i * 500 })
        .collect();
    let phrases = tc
        .draw(
            gs::vecs(gs::tuples!(gs::integers::<u16>().max_value(80), gs::integers::<u16>().max_value(80))).max_size(5),
        )
        .into_iter()
        .map(|(start_beat, end_beat)| Phrase { kind: PhraseKind::Chorus, start_beat, end_beat, fill_beat: None })
        .collect();
    let cues = tc
        .draw(
            gs::vecs(gs::tuples!(
                gs::optional(gs::integers::<u8>().max_value(10)),
                gs::integers::<u32>().max_value(300_000)
            ))
            .max_size(6),
        )
        .into_iter()
        .map(|(hot_cue, start_ms)| Cue {
            hot_cue,
            start_ms,
            end_ms: None,
            comment: "drop".into(),
            color: hot_cue.map(|h| [h, 0, 255]),
            color_id: 0,
            loop_beats: None,
        })
        .collect();
    TrackAnalysis {
        beats,
        cues,
        preview: tc.draw(gs::booleans()).then_some(Waveform {
            style: WaveformStyle::Rgb,
            columns_per_sec: None,
            columns,
        }),
        detail: None,
        structure: Some(SongStructure { mood: Mood::High, phrases }),
    }
}

/// Whatever a report found, its export has as many plays, transitions and unidentified
/// stretches, every time to the hundredth, a mix waveform of at most 1,200 columns keeping
/// its loudest that spans the same time, and each play's presence in percent. The tracks the
/// profile hides are "ID - ID", and from the drive each track has its overview (at most 200
/// columns), its hot cues and its phrases on the grid, as the HTML report has them.
#[hegel::test(test_cases = 60)]
fn exports_count_what_the_report_found(tc: TestCase) {
    let mut r = report();
    let base = r.plays[0].clone();
    r.plays = (0..tc.draw(gs::integers::<u32>().max_value(4)))
        .map(|i| {
            let start = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(3600.0));
            let mut p = Play {
                start,
                end: start + tc.draw(gs::floats::<f64>().min_value(0.0).max_value(600.0)),
                tempo: tc.draw(gs::floats::<f64>().min_value(0.84).max_value(1.16)),
                presence: tc.draw(gs::vecs(gs::floats::<f32>().min_value(-0.5).max_value(1.5)).max_size(20)),
                ..base.clone()
            };
            p.track.id = i + 1;
            p.track.artist = format!("Artist {i}");
            p.track.file_path = format!("/Contents/{i}.mp3");
            p
        })
        .collect();
    r.transitions = (1..r.plays.len())
        .map(|i| Transition {
            from: i - 1,
            to: i,
            at: r.plays[i].start,
            overlap: r.plays[i - 1].end - r.plays[i].start,
            harmony: None,
            tempo_change: Some(1.23),
        })
        .collect();
    r.unidentified = tc
        .draw(gs::vecs(gs::floats::<f64>().min_value(0.0).max_value(3600.0)).max_size(4))
        .into_iter()
        .map(|start| Span { start, end: start + 31.0 })
        .collect();
    let columns = tc.draw(gs::integers::<usize>().max_value(3000));
    let seed = tc.draw(gs::integers::<usize>());
    let band: Vec<u8> = (0..columns).map(|i| (i.wrapping_mul(2_654_435_761) ^ seed) as u8).collect();
    r.waveform = MixWaveform {
        columns_per_sec: tc.draw(gs::sampled_from(vec![0.0, 4.0])),
        peak: band.clone(),
        low: band.clone(),
        mid: band.clone(),
        high: band,
    };
    let mut prefs = ProfilePrefs::default();
    let hidden = tc.draw(gs::vecs(gs::integers::<u32>().max_value(3)).max_size(2));
    prefs.hidden_artists.extend(hidden.iter().map(|i| format!("Artist {i}")));

    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let library = Library { tracks: r.plays.iter().map(|p| p.track.clone()).collect(), ..Default::default() };
    let analyses: Vec<TrackAnalysis> = r.plays.iter().map(|_| analysis(&tc)).collect();
    let on_drive = tc.draw(gs::booleans());
    let drive =
        on_drive.then_some(Drive { device: &device, library: &library, analyses: Some(&analyses), fields: None });
    let e = build(&r, drive, &prefs);

    assert_eq!(
        (e.plays.len(), e.transitions.len(), e.unidentified.len()),
        (r.plays.len(), r.transitions.len(), r.unidentified.len())
    );
    let n = columns.min(MIX_COLUMNS);
    assert!([&e.waveform.low, &e.waveform.mid, &e.waveform.high].iter().all(|w| w.len() == n));
    assert_eq!(e.waveform.low.iter().max(), r.waveform.low.iter().max());
    if n > 0 && r.waveform.columns_per_sec > 0.0 {
        let secs = columns as f64 / r.waveform.columns_per_sec;
        assert!((e.waveform.seconds_per_column * n as f64 - secs).abs() <= 5e-5 * n as f64 + 1e-9, "{secs} s");
    }
    let hundredth = |a: f64, b: f64| (a - b).abs() <= 0.005 + 1e-9;
    for (x, p) in e.plays.iter().zip(&r.plays) {
        assert!(hundredth(x.start, p.start) && hundredth(x.end, p.end) && (x.tempo - p.tempo).abs() <= 5e-5);
        let presence: Vec<u8> = p.presence.iter().map(|v| (v.clamp(0.0, 1.0) * 100.0).round() as u8).collect();
        assert_eq!(x.presence, presence);
        let named = may_name(&prefs, &p.track);
        assert_eq!(named, x.track.title != UNNAMED);
        assert_eq!(x.track.artist.as_deref(), Some(if named { p.track.artist.as_str() } else { UNNAMED }));
    }
    assert!(
        e.transitions.iter().zip(&r.transitions).all(|(x, t)| hundredth(x.at, t.at) && hundredth(x.overlap, t.overlap))
    );
    assert!(
        e.unidentified.iter().zip(&r.unidentified).all(|(x, s)| hundredth(x.start, s.start) && hundredth(x.end, s.end))
    );
    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("/Contents/") && !json.contains("private note"));

    for (i, (x, a)) in e.plays.iter().zip(&analyses).enumerate() {
        let hot = a.cues.iter().filter(|c| c.hot_cue.is_some()).count();
        let phrases = a
            .structure
            .as_ref()
            .unwrap()
            .phrases
            .iter()
            .filter(|ph| (1..=a.beats.len()).contains(&(ph.start_beat as usize).max(1)))
            .count();
        let preview = a.preview.as_ref().filter(|w| !w.columns.is_empty());
        let page = extras(
            Drive { device: &device, library: &library, analyses: Some(&analyses), fields: None },
            &r.plays[i],
            i,
            false,
        );
        if !on_drive {
            assert!(x.track.overview.is_none() && x.track.cues.is_empty() && x.track.phrases.is_empty());
            continue;
        }
        let overview = x.track.overview.as_ref();
        assert_eq!(overview.map(|o| o.heights.len()), preview.map(|w| w.columns.len().min(TRACK_COLUMNS)));
        assert_eq!(overview.map(|o| o.colors.len()), overview.map(|o| o.heights.len()));
        assert_eq!(
            page.wave.as_ref().map(|w| BASE64_STANDARD.decode(w).unwrap().len()),
            preview.map(|w| 4 * w.columns.len().min(400))
        );
        assert_eq!((x.track.cues.len(), page.cues.len()), (hot, hot));
        assert_eq!((x.track.phrases.len(), page.phrases.len()), (phrases, phrases));
        assert!(
            x.track.phrases.iter().zip(&page.phrases).all(|(a, b)| hundredth(a.start, b.start) && a.kind == b.kind)
        );
        assert_eq!(page.art, None, "no art on this drive");
    }
    assert_eq!(extras(Drive::new(&device, &library), &play_elsewhere(), 0, true), PlayExtras::default());
}

/// A play of a track the drive doesn't have.
fn play_elsewhere() -> Play {
    let mut p = report().plays.remove(0);
    p.track.file_path = "/Contents/gone.mp3".into();
    p
}

/// Downsampling keeps the loudest value, leaves a short series as it is, and makes as many
/// columns as asked of a long one, each the loudest of its stretch, stretches in order.
#[hegel::test(test_cases = 300)]
fn downsampling_keeps_the_loudest(tc: TestCase) {
    let values = tc.draw(gs::vecs(gs::integers::<u8>()).max_size(500));
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(300));
    let out = downsample(&values, n);
    assert_eq!(out.len(), values.len().min(n));
    assert_eq!(out.iter().max(), values.iter().max());
    if values.len() <= n {
        return assert_eq!(out, values);
    }
    for (i, v) in out.iter().enumerate() {
        let (lo, hi) = (i * values.len() / n, ((i + 1) * values.len() / n).max(i * values.len() / n + 1));
        assert_eq!(Some(v), values[lo..hi].iter().max());
    }
}

/// A PNG of `side` pixels square, or bytes that aren't an image when `side` is 0.
fn image_bytes(dir: &std::path::Path, side: u32) -> Vec<u8> {
    if side == 0 {
        return b"not an image".to_vec();
    }
    let png = dir.join(format!("art{side}.png"));
    image::RgbImage::from_fn(side, side, |x, y| image::Rgb([x as u8 * 3, y as u8 * 3, 90])).save(&png).unwrap();
    std::fs::read(png).unwrap()
}

/// The pictures an export carries are square JPEG thumbnails, each of only what may be shared:
/// the recording's embedded cover (not one guessed from an image beside it) and the drive's
/// artwork of each track the profile lets it name. What isn't artwork (not an image, or under
/// 16 pixels) is left out.
#[hegel::test(test_cases = 12)]
fn exports_carry_thumbnails_of_only_what_may_be_shared(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut r = report();
    let set = dir.path().join("set.wav");
    std::fs::write(&set, crate::probe::tests::wav(8_000, 16, 0.2)).unwrap();
    let embedded = tc.draw(gs::booleans());
    r.mix.cover = if embedded {
        let png = dir.path().join("cover.png");
        std::fs::write(&png, image_bytes(dir.path(), tc.draw(gs::integers::<u32>().min_value(16).max_value(48))))
            .unwrap();
        crate::artwork::embed_in_file(&set, &Artwork::from_path(&png).unwrap()).unwrap();
        Some(CoverSource::Embedded(set.clone()))
    } else {
        Some(CoverSource::File(dir.path().join("set.png")))
    };
    let base = r.plays[0].clone();
    let device = Device::open(dir.path()).unwrap();
    let mut library = Library::default();
    let mut prefs = ProfilePrefs::default();
    // Each play: its artwork's size (0: not an image, None: no artwork), and whether it's hidden.
    let plays: Vec<(Option<u32>, bool)> = tc.draw(
        gs::vecs(gs::tuples!(gs::optional(gs::sampled_from(vec![0u32, 12, 16, 40])), gs::weighted_booleans(0.3)))
            .min_size(1)
            .max_size(3),
    );
    r.plays = plays
        .iter()
        .enumerate()
        .map(|(i, (art, hidden))| {
            let mut p = base.clone();
            p.track.id = i as u32 + 1;
            p.track.artist = format!("Artist {i}");
            p.track.file_path = format!("/Contents/{i}.mp3");
            if let Some(side) = art {
                let key = format!("PIONEER/Artwork/00001/a{i}.jpg");
                std::fs::create_dir_all(dir.path().join("PIONEER/Artwork/00001")).unwrap();
                std::fs::write(dir.path().join(&key), image_bytes(dir.path(), *side)).unwrap();
                p.track.artwork_path = Some(format!("/{key}"));
            }
            if *hidden {
                prefs.hidden_artists.push(p.track.artist.clone());
            }
            library.tracks.push(p.track.clone());
            p
        })
        .collect();
    let drive = Drive { device: &device, library: &library, analyses: None, fields: None };
    let e = build(&r, Some(drive), &prefs);

    let thumbnail_side = |url: &str| {
        let jpeg = BASE64_STANDARD.decode(url.strip_prefix("data:image/jpeg;base64,").unwrap()).unwrap();
        assert_eq!(image::guess_format(&jpeg).unwrap(), image::ImageFormat::Jpeg);
        let img = image::load_from_memory(&jpeg).unwrap();
        assert_eq!(img.width(), img.height());
        img.width()
    };
    assert_eq!(e.mix.cover.as_deref().map(thumbnail_side), embedded.then_some(COVER_PX));
    for (p, (art, hidden)) in e.plays.iter().zip(&plays) {
        let shared = !hidden && art.is_some_and(|side| side >= 16);
        assert_eq!(p.track.artwork.as_deref().map(thumbnail_side), shared.then_some(ARTWORK_PX), "{art:?} {hidden}");
    }
}

/// An export names the mix and each track it may name by their text without the space around
/// it, and what is only space as untitled; a track the profile hides is "ID" whatever it's called.
#[hegel::test(test_cases = 100)]
fn exports_name_what_has_a_name_and_call_the_rest_untitled(tc: TestCase) {
    let text = || gs::sampled_from(vec!["", " ", "\t\n", "Friday", "  Demo Track 1 ", "日本"]);
    let mut r = report();
    r.mix.title = tc.draw(text()).to_string();
    r.plays[0].track.title = tc.draw(text()).to_string();
    let hidden = tc.draw(gs::booleans());
    let mut prefs = ProfilePrefs::default();
    if hidden {
        prefs.hidden_artists.push(r.plays[0].track.artist.clone());
    }
    let e = build(&r, None, &prefs);
    let named = |s: &str, untitled: &str| match s.trim() {
        "" => untitled.to_string(),
        t => t.to_string(),
    };
    assert_eq!(e.mix.title, named(&r.mix.title, "Untitled mix"));
    let track = if hidden { UNNAMED.to_string() } else { named(&r.plays[0].track.title, "Untitled") };
    assert_eq!(e.plays[0].track.title, track);
}
