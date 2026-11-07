use super::*;

pub use loclib_mixid::test_util::{wav, wav_of};

#[test]
fn formats_from_headers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.wav");
    std::fs::write(&path, wav(96_000, 24, 0.5)).unwrap();
    let f = format(&path).unwrap();
    assert_eq!((f.codec, f.sample_rate, f.bit_depth), (Some(Codec::Wav), 96_000, 24));
    assert!((f.duration_secs.unwrap() - 0.5).abs() < 0.01, "{f:?}");
    // PCM in a container players don't list.
    let odd = dir.path().join("a.bin");
    std::fs::copy(&path, &odd).unwrap();
    assert_eq!(format(&odd).unwrap().codec, None);
    std::fs::write(dir.path().join("b.wav"), b"not audio").unwrap();
    assert!(format(&dir.path().join("b.wav")).is_err());
}

#[test]
fn whole_cut_short_and_unreadable_files() {
    let dir = tempfile::tempdir().unwrap();
    let whole = wav(8_000, 16, 30.0);
    let path = dir.path().join("whole.wav");
    std::fs::write(&path, &whole).unwrap();
    let size = whole.len() as u64;
    assert_eq!(health(&path, 30.0, size), None);
    assert_eq!(health(&path, 31.0, size), None, "the database keeps whole seconds");
    assert_eq!(health(&path, 0.0, 0), None, "no length: the header's");

    // An interrupted copy: the header still promises 30 s.
    let cut = dir.path().join("cut.wav");
    std::fs::write(&cut, &whole[..whole.len() * 2 / 5]).unwrap();
    match health(&cut, 30.0, size) {
        Some(AudioProblem::Truncated { ends_secs, expected_secs }) => {
            assert!((ends_secs - 12.0).abs() < 0.5, "{ends_secs}");
            assert_eq!(expected_secs, 30.0);
        }
        other => panic!("{other:?}"),
    }

    // A whole file that isn't the length the database says: another file took its place.
    match health(&path, 60.0, size) {
        Some(AudioProblem::LengthDiffers { file_secs, expected_secs }) => {
            assert!((file_secs - 30.0).abs() < 0.01 && expected_secs == 60.0)
        }
        other => panic!("{other:?}"),
    }
    assert!(health(&path, 60.0, size).unwrap().is_warning());
    // ...unless it lost as much of its size as of its length: a cut copy whose length comes
    // from its size (as a constant bit rate MP3's does).
    assert!(matches!(health(&path, 60.0, size * 2), Some(AudioProblem::Truncated { .. })));

    let junk = dir.path().join("junk.mp3");
    std::fs::write(&junk, b"not audio at all").unwrap();
    assert!(matches!(health(&junk, 200.0, 0), Some(AudioProblem::Unreadable { .. })));
    assert!(matches!(health(&dir.path().join("gone.mp3"), 1.0, 0), Some(AudioProblem::Unreadable { .. })));
}

/// An M4A cut short in its audio doesn't open (its last box runs past the end): it's truncated
/// when it's smaller than the database's size, or than its own boxes say. With no length to give,
/// it's still a file that won't open.
#[test]
fn an_m4a_cut_short_is_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let whole = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/silence.m4a")).unwrap();
    let path = dir.path().join("whole.m4a");
    std::fs::write(&path, &whole).unwrap();
    let size = whole.len() as u64;
    assert_eq!(health(&path, 20.0, size), None);
    assert_eq!(mp4_len(&path), Some(size));
    // Half its audio: the index is first, and the frames after it.
    let mdat = whole.windows(4).position(|w| w == b"mdat").unwrap() + 4;
    let cut = dir.path().join("cut.m4a");
    std::fs::write(&cut, &whole[..mdat + (whole.len() - mdat) / 2]).unwrap();
    for bytes in [size, 0] {
        match health(&cut, 20.0, bytes) {
            Some(AudioProblem::Truncated { ends_secs, expected_secs }) => {
                assert!(ends_secs > 0.0 && ends_secs < 20.0 && expected_secs == 20.0, "{ends_secs}")
            }
            other => panic!("{bytes}: {other:?}"),
        }
    }
    assert!(matches!(health(&cut, 0.0, size), Some(AudioProblem::Unreadable { .. })));
    // A file that won't open for what's in it isn't cut short, smaller than it should be or not.
    let junk = dir.path().join("junk.m4a");
    std::fs::write(&junk, b"not audio at all").unwrap();
    for bytes in [16, 1000] {
        assert!(matches!(health(&junk, 20.0, bytes), Some(AudioProblem::Unreadable { .. })));
    }
    assert_eq!(mp4_len(&junk), None);
}

/// A WAV cut short in its header doesn't open: it's truncated when it's smaller than the
/// database's size, and otherwise a file that won't open.
#[test]
fn a_file_cut_short_in_its_header_is_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let whole = wav(8_000, 16, 30.0);
    let cut = dir.path().join("cut.wav");
    std::fs::write(&cut, &whole[..30]).unwrap();
    match health(&cut, 30.0, whole.len() as u64) {
        Some(AudioProblem::Truncated { ends_secs, expected_secs }) => {
            assert!(ends_secs < 0.1 && expected_secs == 30.0, "{ends_secs}")
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(health(&cut, 30.0, 0), Some(AudioProblem::Unreadable { .. })));
}

/// An M4A cut anywhere is truncated, whether it still opens or not. Only one cut too short to say what it is (under 8 bytes), or cut right before its index,
/// is a file that won't open: nothing in it says it should go on.
#[hegel::test(test_cases = 100)]
fn an_m4a_cut_anywhere_is_truncated(tc: hegel::TestCase) {
    let whole = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/silence.m4a")).unwrap();
    let keep = tc.draw(hegel::generators::integers::<usize>().max_value(whole.len() - 1));
    let index = whole.windows(4).position(|w| w == b"moov").unwrap() - 4;
    let dir = tempfile::tempdir().unwrap();
    let cut = dir.path().join("cut.m4a");
    std::fs::write(&cut, &whole[..keep]).unwrap();
    match health(&cut, 20.0, whole.len() as u64) {
        Some(AudioProblem::Truncated { ends_secs, expected_secs }) => {
            assert!(ends_secs < 20.0 && expected_secs == 20.0, "{ends_secs}")
        }
        Some(AudioProblem::Unreadable { .. }) if keep < 8 || keep == index => {}
        other => panic!("{keep}: {other:?}"),
    }
}

/// symphonia panics on a WAV saying 0 Hz: reading its format is an error about that file.
#[test]
fn a_file_that_crashes_the_decoder_is_its_own_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zero.wav");
    std::fs::write(&path, wav(0, 16, 0.0)).unwrap();
    let err = format(&path).unwrap_err();
    assert!(matches!(err.inner(), Error::Parse { message, .. } if message == CRASHED), "{err}");
}

/// An AIFF file of `frames` frames of 48 kHz mono 16-bit silence.
pub(crate) fn aiff(frames: u32) -> Vec<u8> {
    let mut comm = 1u16.to_be_bytes().to_vec();
    comm.extend_from_slice(&frames.to_be_bytes());
    comm.extend_from_slice(&16u16.to_be_bytes());
    comm.extend_from_slice(&[0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]); // 48000 as an 80-bit float
    let data = vec![0u8; frames as usize * 2];
    let mut body = b"AIFFCOMM".to_vec();
    body.extend_from_slice(&18u32.to_be_bytes());
    body.extend_from_slice(&comm);
    body.extend_from_slice(b"SSND");
    body.extend_from_slice(&(8 + data.len() as u32).to_be_bytes());
    body.extend_from_slice(&[0; 8]);
    body.extend_from_slice(&data);
    let mut out = b"FORM".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// PCM in AIFF, which players list as AIFF.
#[test]
fn aiff_headers_give_the_format() {
    let dir = tempfile::tempdir().unwrap();
    let aif = dir.path().join("a.aif");
    std::fs::write(&aif, aiff(24_000)).unwrap();
    let f = format(&aif).unwrap();
    assert_eq!((f.codec, f.sample_rate, f.bit_depth), (Some(Codec::Aiff), 48_000, 16));
    assert!((f.duration_secs.unwrap() - 0.5).abs() < 0.01, "{f:?}");
    assert_eq!(health(&aif, 0.0, 0), None);
}

#[test]
fn problems_read_as_sentences() {
    let cases = [
        (AudioProblem::Unreadable { error: "no audio track".into() }, "unreadable", "can't be opened: no audio track"),
        (AudioProblem::Truncated { ends_secs: 72.4, expected_secs: 187.0 }, "truncated", "stops at 1:12 of 3:07"),
        (
            AudioProblem::Undecodable { error: "invalid main_data".into() },
            "undecodable",
            "its end doesn't decode: invalid main_data",
        ),
        (
            AudioProblem::LengthDiffers { file_secs: 241.0, expected_secs: 187.0 },
            "length_differs",
            "4:01 long, the database says 3:07",
        ),
    ];
    for (problem, kind, text) in cases {
        assert_eq!((problem.kind(), problem.to_string().as_str()), (kind, text));
        assert_eq!(problem.is_warning(), kind == "length_differs");
    }
    assert_eq!(minutes(-3.0), "0:00");
}

#[test]
fn parallel_work_stops_when_cancelled() {
    let cancel = crate::progress::CancelToken::new();
    cancel.cancel();
    let ctx = Ctx::new(&crate::progress::NoProgress, cancel);
    let r = parallel(&[1, 2, 3], Phase::Verify, |i| (i.to_string(), 0), |i| i * 2, &ctx);
    assert!(matches!(r, Err(Error::Cancelled)));
    let none: Vec<u32> = Vec::new();
    assert!(parallel(&none, Phase::Verify, |i| (i.to_string(), 0), |i| i * 2, &ctx).unwrap().is_empty());
}

#[test]
fn parallel_keeps_order() {
    let items: Vec<u32> = (0..100).collect();
    let out = parallel(&items, Phase::Scan, |i| (i.to_string(), 0), |i| i * 2, &Ctx::silent()).unwrap();
    assert_eq!(out, items.iter().map(|i| i * 2).collect::<Vec<_>>());
}

/// Audio in a codec there's no decoder for here (Opus in an MP4) has its format read from its
/// headers, with no codec players list, and can't be opened for its health, whatever length and
/// size the database gives: it isn't cut short.
#[hegel::test(test_cases = 20)]
fn prop_audio_without_a_decoder_is_read_but_unreadable(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let secs = tc.draw(gs::sampled_from(vec![0.0, 1.0, 30.0]));
    let bytes = tc.draw(gs::sampled_from(vec![0u64, 1 << 20]));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/silence-opus.mp4");
    let f = format(&path).unwrap();
    assert_eq!((f.codec, f.sample_rate), (None, 48_000), "{f:?}");
    assert!(matches!(health(&path, secs, bytes), Some(AudioProblem::Unreadable { .. })), "{secs} {bytes}");
}

/// AAC and ALAC in an M4A are told apart by their headers, which give their rate, channels and
/// length too; a file of two tracks is read, and checked, by its first. Each is whole.
#[hegel::test(test_cases = 10)]
fn prop_m4a_codecs_are_told_apart_by_their_headers(tc: hegel::TestCase) {
    let (name, codec, secs) = tc.draw(hegel::generators::sampled_from(vec![
        ("silence.m4a", Codec::Aac, 20.0),
        ("silence-alac.m4a", Codec::Alac, 1.0),
        ("two-tracks.m4a", Codec::Alac, 2.0),
    ]));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio").join(name);
    let f = format(&path).unwrap();
    assert_eq!((f.codec, f.sample_rate), (Some(codec), 8_000), "{f:?}");
    assert!((f.duration_secs.unwrap() - secs).abs() < 0.2, "{f:?}");
    assert_eq!(health(&path, secs, 0), None, "{name}");
}

/// A file named without an extension that's cut short in its header is told from one that won't
/// open for what's in it by its size alone, as one with an extension is.
#[test]
fn a_file_without_an_extension_cut_short_is_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let whole = wav(8_000, 16, 30.0);
    let cut = dir.path().join("cut");
    std::fs::write(&cut, &whole[..30]).unwrap();
    assert!(matches!(health(&cut, 30.0, whole.len() as u64), Some(AudioProblem::Truncated { .. })));
}

/// An MP4's length is where its top-level boxes say they end, their sizes in 32 or 64 bits, or the
/// last running to the end of the file. Cut anywhere after its first header, its boxes run past
/// the cut, unless the cut is where a box starts or inside a last box running to the end. A file whose first
/// box isn't `ftyp`, holding a box smaller than a header, or whose boxes add up past what a
/// 64-bit offset holds isn't one; nor is a cut too short to say (an empty file ends at 0).
#[hegel::test(test_cases = 300)]
fn prop_mp4_boxes_say_how_long_the_file_is(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let mut file = Vec::new();
    let (mut starts, mut last) = (Vec::new(), (0, false));
    for i in 0..n {
        let payload = tc.draw(gs::integers::<usize>().max_value(40));
        let kind = if i == 0 { b"ftyp" } else { b"free" };
        let to_end = i == n - 1 && tc.draw(gs::weighted_booleans(0.2));
        last = (file.len(), to_end);
        starts.push(file.len());
        if !to_end && tc.draw(gs::booleans()) {
            file.extend_from_slice(&1u32.to_be_bytes());
            file.extend_from_slice(kind);
            file.extend_from_slice(&(16 + payload as u64).to_be_bytes());
        } else {
            file.extend_from_slice(&(if to_end { 0 } else { 8 + payload as u32 }).to_be_bytes());
            file.extend_from_slice(kind);
        }
        file.extend(std::iter::repeat_n(0, payload));
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.m4a");
    let len_of = |bytes: &[u8]| {
        std::fs::write(&path, bytes).unwrap();
        mp4_len(&path)
    };
    assert_eq!(len_of(&file), Some(file.len() as u64));

    let keep = tc.draw(gs::integers::<usize>().max_value(file.len() - 1));
    match len_of(&file[..keep]) {
        None => assert!(0 < keep && keep < 8, "{keep}"),
        Some(len) if keep == 0 => assert_eq!(len, 0, "an empty file ends where it starts"),
        // Cut where a box starts, it's the boxes before.
        Some(len) if starts.contains(&keep) || (last.1 && keep >= last.0 + 8) => assert_eq!(len, keep as u64),
        Some(len) => assert!(keep >= 8 && len > keep as u64, "{keep}: {len}"),
    }

    let mut broken = file.clone();
    match tc.draw(gs::sampled_from(vec!["not ftyp", "short box", "overflow"])) {
        "not ftyp" => broken[4..8].copy_from_slice(b"moov"),
        "short box" if !last.1 => {
            broken.extend_from_slice(&tc.draw(gs::integers::<u32>().min_value(2).max_value(7)).to_be_bytes());
            broken.extend_from_slice(b"free");
        }
        "overflow" if !last.1 => {
            broken.extend_from_slice(&1u32.to_be_bytes());
            broken.extend_from_slice(b"free");
            broken.extend_from_slice(&u64::MAX.to_be_bytes());
        }
        _ => return,
    }
    assert_eq!(len_of(&broken), None);
}

/// A time stamp reads as seconds by the track's time base when it has one, else as frames at its
/// sample rate, and as 0 with neither.
#[hegel::test(test_cases = 300)]
fn prop_time_stamps_read_as_seconds(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let ts = tc.draw(gs::integers::<u64>().max_value(1 << 40));
    let rate = tc.draw(gs::integers::<u32>().max_value(192_000));
    let base = tc.draw(gs::booleans()).then(|| TimeBase::new(1, rate.max(1)));
    let want = match (base, rate) {
        (None, 0) => 0.0,
        _ => ts as f64 / f64::from(rate.max(1)),
    };
    let got = seconds(base, rate, ts);
    assert!((got - want).abs() <= want * 1e-9 + 1e-9, "{got} {want}");
}

/// A panic in the work on any item goes on up from [`parallel`], as it was raised.
#[hegel::test(test_cases = 20)]
fn prop_a_panic_in_the_work_goes_on_up(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(40));
    let bad = tc.draw(gs::integers::<u32>().max_value(n - 1));
    let items: Vec<u32> = (0..n).collect();
    let work = |&i: &u32| if i == bad { std::panic::panic_any(i) } else { i };
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        parallel(&items, Phase::Verify, |i| (i.to_string(), 0), work, &Ctx::silent())
    }));
    assert_eq!(ran.unwrap_err().downcast_ref::<u32>(), Some(&bad));
}
