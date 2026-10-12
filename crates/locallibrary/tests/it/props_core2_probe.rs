//! Reading compressed audio and audio in other containers: MP3s whose last frames are damaged are
//! undecodable when damage outweighs sound, FLAC and AIFF copies made from drawn WAVs read back
//! as written, a FLAC whose end is overwritten stops where its sound does, and PCM under a name
//! that says nothing is read with no codec.

use crate::common::media;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::equipment::Codec;
use locallibrary::probe::{self, AudioProblem};
use locallibrary::progress::Ctx;
use loclib_transcode::{Container, Dither, Recipe};

/// Samples in an MPEG-1 Layer III frame.
const FRAME_SAMPLES: f64 = 1152.0;

/// `n` MPEG-1 Layer III frames (128 kbit/s, 44.1 kHz), silent up to frame `sound` and from
/// there with a payload no decoder takes (an invalid block type in its side information).
fn mp3(n: usize, sound: usize) -> Vec<u8> {
    let frame = &media::mp3()[..417];
    let broken: Vec<u8> = (0..413).map(|i| (i * 7919 % 251) as u8).collect();
    let mut out = Vec::new();
    for f in 0..n {
        match f < sound {
            true => out.extend_from_slice(frame),
            false => out.extend(frame[..4].iter().chain(&broken)),
        }
    }
    out
}

/// An MP3 whose last frames don't decode: whole when none are damaged, undecodable when the
/// damaged ones outnumber the rest, and otherwise heard as it is (a glitch at the end of a set
/// isn't a broken track). Its length is the frames', damaged or not.
#[hegel::test(test_cases = 30)]
#[hegel::explicit_test_case(n = 100usize, sound = 0usize)]
#[hegel::explicit_test_case(n = 100usize, sound = 30usize)]
fn mp3s_damaged_at_the_end_are_undecodable_when_damage_outweighs_sound(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().min_value(20).max_value(300));
    let sound = tc.draw(gs::integers::<usize>().max_value(n));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Track.mp3");
    let bytes = mp3(n, sound);
    fs::write(&path, &bytes).unwrap();
    let secs = n as f64 * FRAME_SAMPLES / 44_100.0;
    let f = probe::format(&path).unwrap();
    assert_eq!((f.codec, f.sample_rate), (Some(Codec::Mp3), 44_100));
    assert!((f.duration_secs.unwrap() - secs).abs() < 0.03, "{f:?}");
    let p = probe::health(&path, secs, bytes.len() as u64);
    match p {
        None => assert!(sound > 0 && n - sound <= sound, "{sound} of {n} frames sound"),
        Some(p @ AudioProblem::Undecodable { .. }) => {
            assert!(n - sound > sound, "{sound} of {n} frames sound");
            assert_eq!(p.kind(), "undecodable");
            assert!(!p.is_warning());
            assert!(p.to_string().starts_with("its end doesn't decode: "), "{p}");
        }
        other => panic!("{other:?} for {sound} of {n} frames"),
    }
}

/// A drawn WAV copied to FLAC or AIFF reads back with the WAV's format and length, and is whole;
/// under a name that says nothing, AIFF is read with no codec (its samples could be anything's),
/// FLAC by its stream. A FLAC whose last part is overwritten stops before it, cut short.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn flac_and_aiff_copies_read_back_as_written(tc: TestCase) {
    let rate = tc.draw(gs::sampled_from(vec![22_050u32, 44_100]));
    let secs = f64::from(tc.draw(gs::integers::<u32>().min_value(10).max_value(14)));
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.wav");
    fs::write(&src, loclib_mixid::test_util::wav(rate, 16, secs)).unwrap();
    for container in [Container::Flac, Container::Aiff] {
        let out = dir.path().join(format!("copy.{}", container.extension()));
        let recipe = Recipe { container, sample_rate: rate, bit_depth: 16, dither: Dither::None };
        loclib_transcode::transcode(&src, &recipe, &out, &Ctx::silent()).unwrap();
        let bytes = fs::read(&out).unwrap();
        let f = probe::format(&out).unwrap();
        assert_eq!((f.codec, f.sample_rate, f.bit_depth, f.channels), (Some(container.codec()), rate, 16, 2));
        assert!((f.duration_secs.unwrap() - secs).abs() < 0.01, "{f:?}");
        assert!(probe::health(&out, secs, bytes.len() as u64).is_none());

        let elsewhere = dir.path().join("copy.bin");
        fs::write(&elsewhere, &bytes).unwrap();
        let codec = probe::format(&elsewhere).unwrap().codec;
        assert_eq!(codec, (container == Container::Flac).then_some(Codec::Flac));

        if container == Container::Flac {
            // Overwrite from somewhere in the middle on, more than the 2 s a check allows: no
            // frame there is found.
            let from = bytes.len() * tc.draw(gs::integers::<usize>().min_value(30).max_value(60)) / 100;
            let mut damaged = bytes.clone();
            damaged[from..].iter_mut().enumerate().for_each(|(i, b)| *b = (i * 7919 % 251) as u8);
            fs::write(&out, &damaged).unwrap();
            match probe::health(&out, secs, bytes.len() as u64) {
                Some(AudioProblem::Truncated { ends_secs, expected_secs }) => {
                    assert_eq!(expected_secs, secs);
                    assert!(ends_secs < secs * from as f64 / bytes.len() as f64 + 0.5, "{ends_secs} of {secs}");
                }
                other => panic!("{other:?}"),
            }
        }
    }
}
