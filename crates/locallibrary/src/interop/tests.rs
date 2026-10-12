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

/// Regression (`export serato --write-tags`): on a drive whose audio isn't real (placeholders, as
/// the demo tracks are here), the plan said 0 cues but that both copies got rekordbox's hot cues,
/// and the export failed naming a temporary file. Now the placeholders are copied as they are,
/// with a warning naming one, a real file gets its tag and fits where it's planned, and a file
/// lofty only refuses as it writes stops the export naming the drive's file, leaving nothing.
#[test]
fn serato_copies_go_as_planned() {
    use rekordbox_pdb::{NewTrack, PdbEditor};

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().map(Result::unwrap) {
            match e.file_type().unwrap().is_dir() {
                true => copy_dir(&e.path(), &to.join(e.file_name())),
                false => _ = std::fs::copy(e.path(), to.join(e.file_name())).unwrap(),
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Main");
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks"), &root);
    let pdb = root.join("PIONEER/rekordbox/export.pdb");
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    ed.add_track(&NewTrack::new("Real".to_string(), "/Contents/Real.wav".to_string())).unwrap();
    ed.save(&pdb).unwrap();
    let d = Device::open(&root).unwrap();
    for t in &d.library().unwrap().tracks {
        let audio = match t.title.as_str() {
            "Real" => crate::probe::tests::wav(44_100, 16, 0.01),
            title => format!("audio {title}").repeat(100).into_bytes(),
        };
        let path = d.path(&paths::key(&t.file_path));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, audio).unwrap();
    }
    let out = dir.path().join("Serato");
    let opts = InteropOptions { write_tags: true, ..Default::default() };
    let plan = plan(&d, Software::Serato, &out, &opts, &Ctx::silent()).unwrap();
    assert_eq!(plan.cues, 0, "the demo tracks have no cues");
    assert!(plan.notes.iter().any(|n| n.contains("; 0 of them get")), "{:?}", plan.notes);
    let placeholder = d.path("Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3");
    let warning = format!(
        "2 track(s) can't take Serato's cue tag, so their copies get no cues (e.g. {}: ",
        placeholder.display()
    );
    assert!(plan.warnings.iter().any(|w| w.starts_with(&warning)), "{:?}", plan.warnings);

    let done = apply(&d, Software::Serato, &out, &opts, &Ctx::silent()).unwrap();
    for f in done.files.iter().filter(|f| f.what.starts_with("copy of the audio")) {
        let key = f.path.strip_prefix(fsutil::resolved(&out)).unwrap().to_str().unwrap();
        let (copy, from) = (std::fs::read(&f.path).unwrap(), std::fs::read(d.path(key)).unwrap());
        match f.what.ends_with("with Serato's cue tag") {
            true => assert!(copy.len() > from.len() && copy.len() as u64 <= f.bytes, "{}", f.path.display()),
            false => assert_eq!((copy, f.bytes), (from.clone(), from.len() as u64), "{}", f.path.display()),
        }
    }
    assert_eq!(done.files.iter().filter(|f| f.what.ends_with("with Serato's cue tag")).count(), 1);

    // An AIFF whose odd-sized last chunk has no pad byte: lofty reads it, but won't write into it.
    let rate = [0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0];
    let comm = [&1u16.to_be_bytes()[..], &74u32.to_be_bytes(), &16u16.to_be_bytes(), &rate].concat();
    let ssnd = [&[0; 8][..], &[1; 149]].concat();
    let mut form = b"AIFF".to_vec();
    for (id, data) in [(b"COMM", &comm), (b"SSND", &ssnd)] {
        form.extend_from_slice(id);
        form.extend_from_slice(&(data.len() as u32).to_be_bytes());
        form.extend_from_slice(data);
    }
    let aiff = [&b"FORM"[..], &(form.len() as u32).to_be_bytes(), &form].concat();
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    ed.add_track(&NewTrack::new("Odd".to_string(), "/Contents/Odd.aiff".to_string())).unwrap();
    ed.save(&pdb).unwrap();
    std::fs::write(root.join("Contents/Odd.aiff"), aiff).unwrap();
    let d = Device::open(&root).unwrap();
    let out = dir.path().join("Serato again");
    let err = apply(&d, Software::Serato, &out, &opts, &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "tags");
    assert!(err.to_string().starts_with(&format!("tags: {}: ", d.path("Contents/Odd.aiff").display())), "{err}");
    let left: Vec<_> =
        walkdir::WalkDir::new(&out).into_iter().map(Result::unwrap).filter(|e| e.file_type().is_file()).collect();
    assert!(left.is_empty(), "{left:?}");
}
