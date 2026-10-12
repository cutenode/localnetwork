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
