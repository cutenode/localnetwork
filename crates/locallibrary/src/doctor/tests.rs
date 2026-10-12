use super::*;

#[test]
fn sync_manager_names() {
    assert_eq!(sync_manager_base("House1-1"), "House1");
    assert_eq!(sync_manager_base("House1-12"), "House1");
    assert_eq!(sync_manager_base("Track (2)"), "Track");
    assert_eq!(sync_manager_base("Track"), "Track");
    assert_eq!(sync_manager_base("-1"), "-1");
    assert_eq!(sync_manager_base("(1)"), "(1)");
    assert_eq!(sync_manager_base("Track (live)"), "Track (live)");
    assert_eq!(sync_manager_base("Track 2"), "Track 2");
}

/// Regression: tracks sharing a missing cover were reported as one, so `check` counted one
/// track without artwork for a whole album.
#[test]
fn each_track_missing_a_shared_file_is_reported_once() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    let track = |id, path: &str| Track {
        artwork_path: Some("/PIONEER/Artwork/00001/a1.jpg".into()),
        ..loclib_model::test_util::track(id, "A", "T", path)
    };
    let lib = Library { tracks: vec![track(1, "/Contents/a.mp3"), track(2, "/Contents/b.mp3")], ..Default::default() };
    // Both databases list both tracks.
    let dbs = Databases { pdb: Some(lib.clone()), plus: Some(lib), plus_error: None, images: Vec::new() };
    let found = missing(&d, &dbs, &FileTree::default(), &Ctx::silent()).unwrap();
    let said: Vec<(Problem, u32, &str)> = found.iter().map(|b| (b.problem, b.track.id, b.database)).collect();
    assert_eq!(
        said,
        [
            (Problem::MissingAudio, 1, "device_library"),
            (Problem::MissingArtwork, 1, "device_library"),
            (Problem::MissingAudio, 2, "device_library"),
            (Problem::MissingArtwork, 2, "device_library"),
        ]
    );
}

/// The name Sync Manager's second copy of a file has (`Name-1`, `Name (1)`) gives back the
/// first's, any number of digits on; a name without such an ending is its own.
#[hegel::test(test_cases = 300)]
fn a_sync_manager_copys_name_gives_back_the_first(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let stem: String = tc.draw(gs::text().alphabet("ab -()1é").min_size(1).max_size(8));
    let n = tc.draw(gs::integers::<u32>().max_value(1000));
    assert_eq!(sync_manager_base(&format!("{stem}-{n}")), stem);
    assert_eq!(sync_manager_base(&format!("{stem} ({n})")), stem);
    let ends_like_a_copy = stem.ends_with(')') || stem.ends_with(|c: char| c.is_ascii_digit());
    if !ends_like_a_copy {
        assert_eq!(sync_manager_base(&stem), stem);
    }
}

/// Why the keeper of a group of copies ranks ahead of the next: the first thing, best first,
/// they differ in (lossless audio, a lossy file's bitrate, sample rate, bit depth, cues,
/// playlists, history lists).
#[hegel::test(test_cases = 300)]
fn the_keeper_is_kept_for_the_first_thing_it_has_more_of(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let draw = |tc: &hegel::TestCase| DuplicateTrack {
        track: TrackRef { id: 1, name: "A - T".into(), file_path: "/Contents/t.mp3".into() },
        playlists: tc.draw(gs::integers::<usize>().max_value(2)),
        history: tc.draw(gs::integers::<usize>().max_value(2)),
        cues: tc.draw(gs::integers::<usize>().max_value(2)),
        play_count: 0,
        lossless: tc.draw(gs::booleans()),
        bitrate: tc.draw(gs::sampled_from(vec![128, 320])),
        sample_rate: tc.draw(gs::sampled_from(vec![44_100, 48_000])),
        bit_depth: tc.draw(gs::sampled_from(vec![16, 24])),
        file_size: 1,
        date_added: String::new(),
    };
    let (a, b) = (draw(&tc), draw(&tc));
    let (keep, next) = if a.rank() >= b.rank() { (a, b) } else { (b, a) };
    let why = keep.reason_over(&next);
    let lossy_bitrate = !keep.lossless && keep.bitrate != next.bitrate;
    let first = if keep.lossless != next.lossless {
        "lossless over lossy"
    } else if lossy_bitrate {
        "kbps over"
    } else if keep.sample_rate != next.sample_rate {
        "Hz over"
    } else if keep.bit_depth != next.bit_depth {
        "-bit over"
    } else if keep.cues != next.cues {
        "cue(s) over"
    } else if keep.playlists != next.playlists {
        "playlist(s) over"
    } else if keep.history != next.history {
        "history list(s) over"
    } else {
        ""
    };
    assert!(first.is_empty() || why.contains(first), "{why:?} for {keep:?} over {next:?}");
}

/// Two copies of one recording, the second starting `delay` ms later, are measured that far
/// apart to well under a millisecond; a different recording, a file that isn't there and a
/// cancelled run each come back as why not (the last as a cancel), never as an offset.
#[hegel::test(test_cases = 4)]
fn measured_offsets_are_the_delay_or_say_why_not(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use loclib_mixid::test_util::{resample, synth, wav_of};
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, seed: u32, delay_ms: u32| {
        let mut pcm = vec![0.0f32; (44_100 * delay_ms / 1000) as usize];
        pcm.extend(resample(&synth(6.0, seed), loclib_mixid::fingerprint::RATE, 44_100));
        let path = dir.path().join(name);
        std::fs::write(&path, wav_of(44_100, 16, pcm.iter().map(|s| (s * 12_000.0) as i32))).unwrap();
        path
    };
    let seed = tc.draw(gs::integers::<u32>().min_value(1).max_value(10_000));
    let delay_ms = tc.draw(gs::integers::<u32>().max_value(300));
    let a = write("a.wav", seed, 0);
    let b = write("b.wav", seed, delay_ms);
    let other = write("other.wav", seed + 1, 0);
    let delay = f64::from(delay_ms) / 1000.0;
    let silent = Ctx::silent();

    let found = measured_offset(&a, &b, delay, &silent).unwrap().unwrap();
    assert!((found - delay).abs() < 0.0005, "{found} for {delay}");
    let back = measured_offset(&b, &a, -delay, &silent).unwrap().unwrap();
    assert!((back + delay).abs() < 0.0005, "{back} for {}", -delay);
    assert!(measured_offset(&a, &other, 0.0, &silent).unwrap().is_err());
    assert!(measured_offset(&a, &dir.path().join("gone.wav"), delay, &silent).unwrap().is_err());

    let cancel = crate::progress::CancelToken::new();
    cancel.cancel();
    let cancelled = Ctx::new(&crate::progress::NoProgress, cancel);
    assert!(matches!(measured_offset(&a, &b, delay, &cancelled), Err(Error::Cancelled)));
}

/// A track's audio is lossless by its extension, or for an MPEG-4 file, only when its header
/// says Apple Lossless: one that can't be read (missing, or not MPEG-4) isn't.
#[hegel::test(test_cases = 30)]
fn lossless_audio_is_told_by_its_extension_or_its_header(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    let ext =
        tc.draw(gs::sampled_from(vec!["flac", "fla", "wav", "aiff", "aif", "mp3", "aac", "m4a", "mp4", "ogg", "xyz"]));
    let path = format!("/Contents/t.{ext}");
    if tc.draw(gs::booleans()) {
        std::fs::create_dir_all(dir.path().join("Contents")).unwrap();
        std::fs::write(dir.path().join(format!("Contents/t.{ext}")), b"not audio").unwrap();
    }
    let track = loclib_model::test_util::track(1, "A", "T", &path);
    let expected = matches!(ext, "flac" | "fla" | "wav" | "aiff" | "aif");
    assert_eq!(lossless(&d, &track), expected, "{path}");
}

/// Two tracks of one recording, one starting later, are that far apart by their sound (when
/// they're long enough for the prints to place each other; too short, that's why not); tracks of
/// different recordings, or one whose audio doesn't decode, have no offset, and say why.
#[hegel::test(test_cases = 2)]
#[hegel::explicit_test_case(secs = 8.0, seed = 1u32, delay_ms = 2u32)]
fn tracks_are_offset_only_when_they_sound_like_one_recording(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use loclib_mixid::fingerprint::RATE;
    use loclib_mixid::test_util::{resample, synth, wav_of};
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Contents")).unwrap();
    let d = Device::open(dir.path()).unwrap();
    let secs = tc.draw(gs::sampled_from(vec![8.0, 20.0]));
    let seed = tc.draw(gs::integers::<u32>().min_value(1).max_value(10_000));
    let delay_ms = tc.draw(gs::integers::<u32>().max_value(200));
    let add = |id: u32, rate: u32, pcm: &[f32], delay_ms: u32| {
        let silence = std::iter::repeat_n(0, (rate * delay_ms / 1000) as usize);
        let samples: Vec<i32> = silence.chain(pcm.iter().map(|s| (s * 12_000.0) as i32)).collect();
        let path = format!("/Contents/{id}.wav");
        std::fs::write(d.path(&paths::key(&path)), wav_of(rate, 16, samples.into_iter())).unwrap();
        loclib_model::test_util::track(id, "A", "T", &path)
    };
    // Unoptimised, resampling takes most of a case: both copies share one, and another
    // recording, told apart by its prints alone, is written at their rate, needing none.
    let sound = resample(&synth(secs, seed), RATE, 44_100);
    let (a, b) = (add(1, 44_100, &sound, 0), add(2, 44_100, &sound, delay_ms));
    let other = add(3, RATE, &synth(secs, seed + 1), 0);
    let silent = Ctx::silent();
    match offset_between(&d, &a, &b, &silent).unwrap() {
        Ok(found) => assert!((found - f64::from(delay_ms) / 1000.0).abs() < 0.0005, "{found} for {delay_ms} ms"),
        Err(why) => {
            assert!(secs < 10.0, "{secs} s: {why}");
            assert_eq!(why, "the offset between them couldn't be estimated");
        }
    }
    assert_eq!(offset_between(&d, &a, &other, &silent).unwrap(), Err("they don't sound like one recording"));
    // `a` is the one broken: the other recording, at the prints' rate, decodes far faster.
    std::fs::write(d.path("Contents/1.wav"), b"RIFF not a wave").unwrap();
    assert!(offset_between(&d, &a, &other, &silent).unwrap().is_err());
}
