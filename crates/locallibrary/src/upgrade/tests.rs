use super::*;

use std::fs;

use hegel::generators as gs;
use loclib_mixid::matcher::Alignment;
use loclib_mixid::test_util::{resample, synth, wav_of};

use crate::equipment::MODELS;

/// A row of the drive's database for `path`, an MP3 at 44.1 kHz.
fn row(path: &str, analyze_path: &str) -> Track {
    Track {
        id: 1,
        title: "Song".into(),
        artist: "Someone".into(),
        album: String::new(),
        album_artist: String::new(),
        genre: String::new(),
        label: String::new(),
        key: String::new(),
        remixer: String::new(),
        original_artist: String::new(),
        composer: String::new(),
        comment: String::new(),
        mix_name: String::new(),
        isrc: String::new(),
        bpm: 0.0,
        duration_secs: 20,
        year: 0,
        track_number: 0,
        disc_number: 0,
        rating: 0,
        color: None,
        play_count: 0,
        bitrate: 320,
        sample_rate: 44_100,
        sample_depth: 0,
        file_size: 0,
        file_path: path.into(),
        analyze_path: analyze_path.into(),
        date_added: String::new(),
        release_date: String::new(),
        artwork_id: None,
        artwork_path: None,
    }
}

/// `secs` of test signal `seed` at `rate` after `delay_ms` of silence, as a stereo WAV of `bits`.
fn recording(seed: u32, secs: f64, rate: u32, bits: u16, delay_ms: u32) -> Vec<u8> {
    let mut pcm = vec![0.0f32; (rate * delay_ms / 1000) as usize];
    pcm.extend(resample(&synth(secs, seed), loclib_mixid::fingerprint::RATE, rate));
    let scale = 12_000.0 * f32::from(1u16 << (bits - 16));
    wav_of(rate, bits, pcm.iter().map(|s| (s * scale) as i32))
}

fn precise(offset: f64) -> Alignment {
    Alignment { offset, drift: 0.0, drift_noise: 0.0, pairs: 10_000, shifts: 10_000, coverage: (1.0, 1.0) }
}

/// The copy's name: the first of `Song.wav`, `Song (2).wav`... that no track uses and that isn't
/// on the drive, or that is and holds the lossless file's audio already (an upgrade undone left
/// it), which is then used where it is. A folder that would leave the drive is refused.
#[hegel::test(test_cases = 10)]
#[hegel::explicit_test_case(taken = vec![true, false, false, false], there = vec![2u8, 0, 0, 0])]
#[hegel::explicit_test_case(taken = vec![false, true, false, false], there = vec![1u8, 0, 3, 2])]
#[hegel::explicit_test_case(taken = vec![false, false, false, false], there = vec![3u8, 0, 0, 0])]
fn the_copy_takes_the_first_free_name(tc: hegel::TestCase) {
    // Per candidate name: whether a track uses it, and what the drive has there (nothing, the
    // same audio, other audio of the same size, other audio of another size).
    let taken: Vec<bool> = tc.draw(gs::vecs(gs::booleans()).min_size(4).max_size(4));
    let there: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(3)).min_size(4).max_size(4));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("USB");
    fs::create_dir_all(root.join("Contents/Sound")).unwrap();
    let device = Device::open(&root).unwrap();
    let lossless = dir.path().join("Song.wav");
    let audio = b"lossless audio".to_vec();
    fs::write(&lossless, &audio).unwrap();
    let names = ["Song.wav", "Song (2).wav", "Song (3).wav", "Song (4).wav"];
    let mut taken_keys = FxHashSet::default();
    for (n, name) in names.iter().enumerate() {
        let key = format!("Contents/Sound/{name}");
        if taken[n] {
            taken_keys.insert(key.to_lowercase());
        }
        let bytes: Option<&[u8]> = match there[n] {
            1 => Some(&audio),
            2 => Some(b"other  audio  "),
            3 => Some(b"other"),
            _ => None,
        };
        if let Some(b) = bytes {
            fs::write(device.path(&key), b).unwrap();
        }
    }
    let size = audio.len() as u64;
    let got = free_key(&device, "Contents/Sound", "Song.wav", &lossless, size, &taken_keys, &Ctx::silent()).unwrap();
    let want = (0..)
        .find_map(|n: usize| {
            let (is_taken, on_drive) = (taken.get(n).copied().unwrap_or(false), there.get(n).copied().unwrap_or(0));
            let key = match n {
                0 => "Contents/Sound/Song.wav".to_string(),
                n => format!("Contents/Sound/Song ({}).wav", n + 1),
            };
            match (is_taken, on_drive) {
                (true, _) => None,
                (false, 0) => Some((key, false)),
                (false, 1) => Some((key, true)),
                _ => None,
            }
        })
        .unwrap();
    assert_eq!(got, want);
    assert!(matches!(
        free_key(&device, "Contents/../..", "Song.wav", &lossless, size, &taken_keys, &Ctx::silent()),
        Err(Error::Invalid(m)) if m.contains("unsafe")
    ));
    // Naming no folder is refused before anything is read.
    let none = plan(&device, &UpgradeOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(none.to_string().contains("folders"), "{none}");
}

/// A lossless file is refused when one of the players can't play it, naming the first such
/// player and the file's format, rate and depth; one whose format can't be told, never.
#[hegel::test(test_cases = 100)]
#[hegel::explicit_test_case(given = vec![0usize], codec = Some(Codec::Wav), rate = 96_000u32, depth = 24u16)]
fn a_lossless_file_a_player_cant_play_is_refused(tc: hegel::TestCase) {
    use hegel::Generator;
    let given: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().max_value(MODELS.len() - 1)).max_size(3));
    let codec = tc
        .draw(gs::optional(gs::sampled_from(vec![Codec::Wav, Codec::Aiff, Codec::Flac, Codec::Alac])).print_as_debug());
    let rate = tc.draw(gs::sampled_from(vec![44_100u32, 48_000, 96_000, 192_000]));
    let depth = tc.draw(gs::sampled_from(vec![16u16, 24, 32]));
    let models: Vec<&Model> = given.iter().map(|&i| &MODELS[i]).collect();
    let file = AudioFile { size: 1, sample_rate: rate, bit_depth: depth, bitrate: 0 };
    let refusing = codec.and_then(|c| models.iter().find(|m| m.plays(c, rate, depth).result.is_err()));
    match (unplayable(&models, codec, &file), refusing) {
        (None, None) => {}
        (Some(why), Some(m)) => {
            let c = codec.unwrap();
            assert_eq!(why, format!("the {} doesn't play it ({} at {rate} Hz, {depth} bit)", m.name, c.name()));
        }
        (said, want) => panic!("{said:?} but {:?}", want.map(|m| m.name)),
    }
}

/// A lossy track and a lossless file of its recording: an upgrade pointing it at a copy next to
/// its file, moving its cues by the offset between their audio; or why not, for the first reason
/// that applies: two rows share its file, the offset couldn't be estimated, its file isn't under
/// `Contents/`, the lossless file is gone or isn't audio or a player can't play it, an AAC track
/// has no waveform to time it by, the two files' audio can't be compared, or its analysis can't
/// be rewritten. Every reason is tried on every run.
#[hegel::test(test_cases = 1, suppress_health_check = [hegel::HealthCheck::TooSlow])]
#[hegel::explicit_test_case(rate = 48_000u32, delay_ms = 26u32)]
fn an_upgrade_or_the_first_reason_it_isnt_one(tc: hegel::TestCase) {
    let rate = tc.draw(gs::sampled_from(vec![44_100u32, 48_000]));
    let delay_ms = tc.draw(gs::sampled_from(vec![0u32, 26, 50]));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("USB");
    fs::create_dir_all(root.join("Contents/Sound")).unwrap();
    let device = Device::open(&root).unwrap();
    let mp3 = "/Contents/Sound/Song.mp3";
    fs::write(device.path(&paths::key(mp3)), recording(7919, 12.0, 44_100, 16, delay_ms)).unwrap();
    let lossless = dir.path().join("Song.wav");
    fs::write(&lossless, recording(7919, 12.0, rate, 24, 0)).unwrap();
    let hi_res = dir.path().join("Hi.wav");
    fs::write(&hi_res, recording(7919, 2.0, 96_000, 24, 0)).unwrap();
    let not_audio = dir.path().join("Notes.wav");
    fs::write(&not_audio, b"not audio").unwrap();
    let garbage = "/Contents/Sound/Broken.mp3";
    fs::write(device.path(&paths::key(garbage)), b"not audio either").unwrap();
    let m4a = "/Contents/Sound/Song.m4a";
    fs::write(device.path(&paths::key(m4a)), b"not an mpeg-4 file").unwrap();
    let dat = "/PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT";
    fs::create_dir_all(device.path(&paths::key(dat)).parent().unwrap()).unwrap();
    fs::write(device.path(&paths::key(dat)), b"PMAI not an analysis file").unwrap();
    let coarse = -(f64::from(delay_ms) / 1000.0);
    let nxs = [crate::equipment::find("CDJ-2000NXS").unwrap()];

    let taken = FxHashSet::default();
    let ctx = Ctx::silent();
    for reason in 0..10 {
        let mut t = row(mp3, "");
        let (mut file, mut rows, mut coarse_al, mut players): (&Path, usize, Alignment, &[&Model]) =
            (&lossless, 1, precise(coarse), &[]);
        match reason {
            1 => rows = 2,
            2 => coarse_al.shifts = 0,
            3 => t.file_path = "/Music/Song.mp3".into(),
            4 => file = Path::new("/nonexistent/locallibrary/Song.wav"),
            5 => file = &not_audio,
            6 => (file, players) = (&hi_res, &nxs),
            7 => t.file_path = m4a.into(),
            8 => t.file_path = garbage.into(),
            9 => t.analyze_path = dat.into(),
            _ => {}
        }
        let found = Found { track: &t, file, coarse: coarse_al, rows };
        let result = upgrade(&device, &found, players, &taken, &ctx);
        let why = match result {
            Ok(Ok((u, edit))) => {
                assert_eq!(reason, 0);
                assert_eq!(
                    (u.to.as_str(), u.on_drive, u.size),
                    ("Contents/Sound/Song.wav", false, fs::metadata(&lossless).unwrap().len())
                );
                assert!((u.shift_ms + delay_ms as i32).abs() <= 2, "{} vs {delay_ms} ms", u.shift_ms);
                match edit {
                    Edit::SetPath { path, file: Some(f), shift_ms, .. } => {
                        assert_eq!((path.as_str(), shift_ms), ("/Contents/Sound/Song.wav", u.shift_ms));
                        assert_eq!((f.sample_rate, f.bit_depth, f.bitrate), (rate, 24, rate * 24 * 2 / 1000));
                    }
                    other => panic!("{other:?}"),
                }
                continue;
            }
            Ok(Err(why)) => why,
            Err(e) => {
                assert!(matches!(reason, 4 | 5), "{reason}: {e}");
                assert_eq!(matches!(e, Error::Io { .. }), reason == 4, "{e}");
                continue;
            }
        };
        let starts = match reason {
            1 => "another row of the database has its file",
            2 => "the offset between them couldn't be estimated",
            3 => "its file isn't under Contents/",
            6 => "the CDJ-2000NXS doesn't play it (WAV at 96000 Hz, 24 bit)",
            7 => timeline::UNTOLD,
            8 => "their audio",
            9 => "its analysis can't be rewritten",
            _ => panic!("{reason}: {why}"),
        };
        assert!(why.starts_with(starts), "{reason}: {why}");
    }
}
