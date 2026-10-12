use super::*;

#[test]
fn markers_go_into_wav_and_aiff_tags() {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, FrameFlags, GeneralEncapsulatedObject, Id3v2Tag};

    use serato::{Markers, write_markers};

    let dir = tempfile::tempdir().unwrap();
    let m = Markers { colour: [0xff; 3], cues: vec![(3, 1234, [0xcc, 0xcc, 0x00], "x".into())], ..Default::default() };
    let (wav, aiff) = (crate::probe::tests::wav(44_100, 16, 0.0001), crate::probe::tests::aiff(4));
    let geob = |tag: &Id3v2Tag| -> Vec<Vec<u8>> {
        tag.into_iter()
            .filter_map(|f| match f {
                Frame::Binary(b) if b.id().as_str() == "GEOB" => {
                    GeneralEncapsulatedObject::parse(&b.data, FrameFlags::default()).ok().map(|g| g.data)
                }
                _ => None,
            })
            .collect()
    };
    let options = ParseOptions::new().read_properties(false);
    let path = dir.path().join("a.wav");
    std::fs::write(&path, &wav).unwrap();
    // Twice: the second replaces the first rather than adding another.
    write_markers(&path, &path, &m.encode()).unwrap();
    write_markers(&path, &path, &m.encode()).unwrap();
    let file = lofty::iff::wav::WavFile::read_from(&mut std::fs::File::open(&path).unwrap(), options).unwrap();
    assert_eq!(geob(file.id3v2().unwrap()), [m.encode()]);

    let path = dir.path().join("a.aiff");
    std::fs::write(&path, &aiff).unwrap();
    write_markers(&path, &path, &m.encode()).unwrap();
    let file = lofty::iff::aiff::AiffFile::read_from(&mut std::fs::File::open(&path).unwrap(), options).unwrap();
    assert_eq!(geob(file.id3v2().unwrap()), [m.encode()]);

    let flac = dir.path().join("a.flac");
    std::fs::write(&flac, b"fLaC").unwrap();
    assert_eq!(Error::from(write_markers(&flac, &flac, &m.encode()).unwrap_err()).code(), "tags");
}
