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
