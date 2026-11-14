use super::*;

/// Without a waveform, an MP3 is taken to start at its first frame and lossless audio at its
/// first sample; AAC can't be told.
#[test]
fn without_a_waveform() {
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let lead = |path: &str| {
        let t = Track { file_path: path.into(), ..Default::default() };
        lead(&device, &t, &Ctx::silent()).unwrap()
    };
    assert_eq!(lead("/Contents/a.mp3"), Some(0.0));
    assert_eq!(lead("/Contents/a.flac"), Some(0.0));
    assert_eq!(lead("/Contents/a.aac"), None);
}

/// Without a waveform, where rekordbox's timeline starts is told by the audio's kind alone,
/// whatever case its extension is written in: an MP3 at its first frame, lossless audio at its
/// first sample, AAC (an MPEG-4 file that isn't Apple Lossless, or can't be read) not at all.
#[hegel::test(test_cases = 30)]
fn without_a_waveform_the_lead_is_told_by_the_kind_of_audio(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let (ext, want) = tc.draw(gs::sampled_from(vec![
        ("mp3", Some(0.0)),
        ("aac", None),
        ("m4a", None),
        ("mp4", None),
        ("flac", Some(0.0)),
        ("wav", Some(0.0)),
        ("aiff", Some(0.0)),
        ("ogg", Some(0.0)),
    ]));
    let ext: String = ext.chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c }).collect();
    let t = Track { file_path: format!("/Contents/a.{ext}"), ..Default::default() };
    assert_eq!(lead(&device, &t, &Ctx::silent()).unwrap(), want, "{ext}");
}

/// With a waveform to measure against, a track's lead is told from its decoded audio: silent
/// audio matches every candidate equally (MP3 a frame at most, AAC its encoder's priming), so the
/// lead is one of them; audio that can't be read, isn't there or would leave the drive fails.
#[hegel::test(test_cases = 20)]
fn prop_with_a_waveform_the_lead_is_measured_from_the_audio(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = tempfile::tempdir().unwrap();
    let anlz = "PIONEER/USBANLZ/P016/0000875E/ANLZ0000";
    let demo = fixtures.join("rekordcrate/demo_tracks").join(anlz);
    std::fs::create_dir_all(dir.path().join(anlz).parent().unwrap()).unwrap();
    for ext in ["DAT", "EXT"] {
        std::fs::copy(demo.with_extension(ext), dir.path().join(format!("{anlz}.{ext}"))).unwrap();
    }
    let device = Device::open(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path().join("Contents")).unwrap();
    let case = tc.draw(gs::integers::<u8>().max_value(4));
    let ext = if case == 1 { "m4a" } else { "mp3" };
    let file_path = match case {
        3 => "/Contents/../../outside.mp3".to_string(),
        _ => format!("/Contents/a.{ext}"),
    };
    let audio = match case {
        0 => Some(loclib_testkit::audio::mp3(tc.draw(gs::integers::<u32>().min_value(1).max_value(200)), None)),
        1 => Some(std::fs::read(fixtures.join("audio/silence.m4a")).unwrap()),
        2 => Some(b"not audio".to_vec()),
        _ => None,
    };
    if let Some(bytes) = &audio {
        std::fs::write(dir.path().join(&file_path[1..]), bytes).unwrap();
    }
    let t = Track { file_path, analyze_path: format!("/{anlz}.DAT"), ..Default::default() };
    let got = lead(&device, &t, &Ctx::silent());
    match case {
        0 | 1 => {
            let lead = got.unwrap().expect("measured");
            let rate = f64::from(crate::probe::format(&device.path(&paths::key(&t.file_path))).unwrap().sample_rate);
            let candidates: &[f64] = if case == 0 { &[0.0, 1152.0] } else { &[0.0, 1024.0, 2112.0] };
            assert!(candidates.iter().any(|c| (lead + c / rate).abs() < 1e-9), "{lead}");
        }
        3 => assert!(matches!(got, Err(crate::Error::Invalid(_))), "{got:?}"),
        _ => assert!(got.is_err(), "{got:?}"),
    }
}
