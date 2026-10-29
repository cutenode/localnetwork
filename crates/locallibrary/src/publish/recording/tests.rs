use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// `secs` seconds of silence as a WAV file (44.1 kHz, 16-bit stereo).
fn wav(secs: u32) -> Vec<u8> {
    let len = secs * 44_100 * 4;
    crate::recordings::tests::wav(len, len)
}

/// `frames` silent MP3 frames (128 kbit/s, 44.1 kHz), about 26 ms each.
fn mp3(frames: usize) -> Vec<u8> {
    [&[0xff, 0xfb, 0x90, 0x64][..], &[0; 413]].concat().repeat(frames)
}

/// A recording goes up as it is exactly when browsers play it and it fits. Otherwise it's AAC at
/// 128 kbit/s, or the most that fits, never under 64, and about no bigger than the limit; and a
/// recording too long for 64 to fit is refused.
#[hegel::test(test_cases = 500)]
fn a_recording_goes_up_as_it_is_only_when_browsers_play_it_and_it_fits(tc: TestCase) {
    let limit = tc.draw(gs::integers::<u64>().min_value(1).max_value(MAX_BYTES));
    let bytes = tc.draw(gs::integers::<u64>().max_value(2 * MAX_BYTES));
    let duration = tc.draw(gs::integers::<u64>().max_value(24 * 3600));
    let playable = tc.draw(gs::booleans());
    let room = limit as f64 * HEADROOM;
    match encoding(playable, bytes, duration, limit) {
        Ok(Encoding::AsIs) => assert!(playable && bytes <= limit),
        Ok(Encoding::Aac { kbps, bytes: about }) => {
            assert!(!(playable && bytes <= limit));
            assert!((LEAST_KBPS..=KBPS).contains(&kbps), "{kbps}");
            assert_eq!(about, aac_bytes(kbps, duration.max(1)));
            assert!(about as f64 <= room, "{about} for {limit}");
            // The most that fits: a kbit/s more wouldn't.
            assert!(kbps == KBPS || aac_bytes(kbps + 1, duration.max(1)) as f64 > room, "{kbps}");
        }
        Err(e) => {
            assert!(!(playable && bytes <= limit));
            assert!(aac_bytes(LEAST_KBPS, duration.max(1)) as f64 > room, "{e}");
            assert!(e.to_string().starts_with("the recording is too long to upload"), "{e}");
        }
    }
}

/// What's read of a recording: its format, size and length, and whether browsers play it as it
/// is. What isn't audio is refused, saying what to do.
#[test]
fn a_recording_is_read_for_its_format_and_length() {
    let dir = tempfile::tempdir().unwrap();
    let (w, m, t) = (dir.path().join("set.wav"), dir.path().join("set.mp3"), dir.path().join("set.txt"));
    std::fs::write(&w, wav(2)).unwrap();
    std::fs::write(&m, mp3(40)).unwrap();
    std::fs::write(&t, "not audio").unwrap();
    let a = probe(&w).unwrap();
    assert_eq!((a.format.as_str(), a.bytes, a.duration_secs, a.playable), ("WAV", wav(2).len() as u64, 2, None));
    let a = probe(&m).unwrap();
    assert_eq!((a.format.as_str(), a.duration_secs, a.playable.as_deref()), ("MP3", 1, Some("audio/mpeg")));
    let e = probe(&t).unwrap_err().to_string();
    assert!(e.contains("isn't audio locallibrary can read") && e.contains("--recording FILE"), "{e}");
}

/// A recording record links the upload and says how long it plays, as its Lexicon takes it; a
/// file the Lexicon doesn't take (not audio, or too big) doesn't fit.
#[test]
fn a_recording_record_fits_its_lexicon() {
    let blob = |mime: &str, size: u64| {
        let cid = "bafkreibme22gw2h7y2h7tg2fhqotaqjucnbc24deqo72b6mkl2egezxhvy";
        json!({ "$type": "blob", "ref": { "$link": cid }, "mimeType": mime, "size": size })
    };
    let now = Utc::now();
    let r = record(blob("audio/mp4", 1000), 3600, now);
    lexicon::validate(lexicon::RECORDING, &r).unwrap();
    assert_eq!(r["durationSeconds"], 3600);
    assert_eq!(r["createdAt"], crate::profile::timestamp(now));
    assert_eq!(record(blob("audio/mp4", 1000), 1, now)["durationSeconds"], 1);
    assert!(record(blob("audio/mp4", 1000), 0, now).get("durationSeconds").is_none(), "too short to say");
    // An MP4 file a data server reads as video.
    lexicon::validate(lexicon::RECORDING, &record(blob("video/mp4", 1000), 60, now)).unwrap();
    for wrong in [blob("image/png", 1000), blob("audio/mp4", MAX_BYTES + 1)] {
        assert_eq!(lexicon::validate(lexicon::RECORDING, &record(wrong, 60, now)).unwrap_err().code(), "invalid");
    }
    assert_eq!(document(lexicon::RECORDING)["audio"]["maxSize"], MAX_BYTES, "the Lexicon's limit is this one");
}

fn document(nsid: &str) -> &'static Value {
    &lexicon::document(nsid).unwrap()["defs"]["main"]["record"]["properties"]
}

/// Encoding, with a stand-in ffmpeg on `PATH` (a shell script, so not on Windows).
#[cfg(unix)]
mod with_ffmpeg {
    use std::sync::Mutex;

    use super::*;
    use crate::format::tests::fake_tools::FakeTools;
    use crate::progress::Progress;

    /// The progress events an operation reported.
    #[derive(Default)]
    struct Recorded(Mutex<Vec<Event>>);

    impl Progress for Recorded {
        fn on_event(&self, event: &Event) {
            self.0.lock().unwrap().push(event.clone());
        }
    }

    /// A stand-in ffmpeg that notes its arguments, writes `out` to the file it's told to, says how
    /// much it has written as ffmpeg's `-progress` does, and exits with `status` (saying `why` on
    /// standard error when that isn't 0).
    fn fake_ffmpeg(fake: &FakeTools, out: &str, status: u8, why: &str) {
        fake.script(
            "ffmpeg",
            &format!(
                "printf '%s\\n' \"$@\" >\"$d/ffmpeg-args\"\nfor last; do :; done\nprintf '%s' '{out}' >\"$last\"\n\
                 printf 'total_size=N/A\\nprogress=continue\\ntotal_size=3\\nprogress=continue\\ntotal_size={}\\nprogress=end\\n'\n\
                 printf '%s\\n' '{why}' >&2\nexit {status}\n",
                out.len()
            ),
        );
    }

    /// Over formats and limits, with a stand-in ffmpeg: ffmpeg runs exactly when the recording can't go
    /// up as it is, as AAC at the bitrate the plan chose with its index at the front, and what goes
    /// up is what it wrote, as audio/mp4, reported as it was written; otherwise what goes up is the
    /// recording itself, as its own type.
    #[hegel::test(test_cases = 30)]
    fn ffmpeg_encodes_only_what_cant_go_up_as_it_is(tc: TestCase) {
        let fake = FakeTools::new();
        fake_ffmpeg(&fake, "encoded", 0, "");
        let is_mp3 = tc.draw(gs::booleans());
        let (name, bytes) = if is_mp3 {
            ("set.mp3", mp3(tc.draw(gs::integers::<usize>().min_value(40).max_value(200))))
        } else {
            ("set.wav", wav(tc.draw(gs::integers::<u32>().min_value(1).max_value(3))))
        };
        let path = fake.dir().join(name);
        std::fs::write(&path, &bytes).unwrap();
        let limit = tc.draw(gs::integers::<u64>().min_value(8_000).max_value(2 * bytes.len() as u64));
        let audio = probe(&path).unwrap();
        let Ok(how) = encoding(audio.playable.is_some(), audio.bytes, audio.duration_secs, limit) else {
            return;
        };
        let recorded = Recorded::default();
        let upload = prepare(&audio, how, limit, &Ctx::new(&recorded, Default::default())).unwrap();
        let args = std::fs::read_to_string(fake.dir().join("ffmpeg-args")).ok();
        match how {
            Encoding::AsIs => {
                assert!(is_mp3 && bytes.len() as u64 <= limit);
                assert_eq!((upload.path(), upload.mime.as_str()), (path.as_path(), "audio/mpeg"));
                assert!(args.is_none(), "ffmpeg didn't run");
            }
            Encoding::Aac { kbps, .. } => {
                assert!(!is_mp3 || bytes.len() as u64 > limit);
                assert_eq!(std::fs::read_to_string(upload.path()).unwrap(), "encoded");
                assert_eq!(upload.mime, "audio/mp4");
                let args = args.unwrap();
                let expected =
                    format!("-i\n{}\n-vn\n-c:a\naac\n-b:a\n{kbps}k\n-movflags\n+faststart\n", path.display());
                assert!(args.contains(&expected), "{args}");
                assert!(args.ends_with(&format!("{}\n", upload.path().display())), "{args}");
                let written: u64 = recorded
                    .0
                    .lock()
                    .unwrap()
                    .iter()
                    .filter_map(|e| match e {
                        Event::Bytes { phase: Phase::Write, bytes } => Some(*bytes),
                        _ => None,
                    })
                    .sum();
                assert_eq!(written, "encoded".len() as u64);
                // The encoding goes once the upload's done with it.
                let encoded = upload.path().to_path_buf();
                drop(upload);
                assert!(!encoded.exists());
            }
        }
    }

    /// Without ffmpeg, a recording that has to be encoded says to install it; an ffmpeg that fails
    /// says why; and an encoding that comes out bigger than the data server takes isn't uploaded.
    #[test]
    fn encoding_says_what_went_wrong() {
        let fake = FakeTools::new();
        let path = fake.dir().join("set.wav");
        std::fs::write(&path, wav(1)).unwrap();
        let audio = probe(&path).unwrap();
        let how = Encoding::Aac { kbps: KBPS, bytes: 16_000 };
        let err = prepare(&audio, how, MAX_BYTES, &Ctx::silent()).err().unwrap();
        assert_eq!(err.code(), "not_found");
        assert!(err.to_string().contains("ffmpeg, which encodes the recording (WAV)"), "{err}");
        assert!(err.to_string().contains("https://ffmpeg.org"), "{err}");

        fake_ffmpeg(&fake, "", 1, "set.wav: Invalid data found when processing input");
        let err = prepare(&audio, how, MAX_BYTES, &Ctx::silent()).err().unwrap();
        assert_eq!(err.code(), "external");
        assert!(err.to_string().ends_with("Invalid data found when processing input"), "{err}");

        fake_ffmpeg(&fake, "far too much", 0, "");
        let err = prepare(&audio, how, 5, &Ctx::silent()).err().unwrap();
        assert_eq!(err.to_string(), "ffmpeg's encoding of the recording came to 12 B, more than 5 B");
    }
}
