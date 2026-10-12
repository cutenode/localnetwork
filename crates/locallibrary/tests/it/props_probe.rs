//! Reading audio files: drawn WAV files report their format and length as written, and the
//! health check tells a whole file from one cut short, one the database gives another length,
//! and one that isn't audio at all, on a drive's tracks too.

use crate::common::media;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::equipment::Codec;
use locallibrary::probe::{self, AudioProblem};
use locallibrary::progress::Ctx;
use locallibrary::{Library, paths};

#[hegel::test(test_cases = 40, suppress_health_check = [HealthCheck::TooSlow])]
fn wav_files_are_read_and_checked_as_they_are(tc: TestCase) {
    let rate = tc.draw(gs::sampled_from(vec![22_050u32, 44_100, 48_000]));
    let bits = tc.draw(gs::sampled_from(vec![16u16, 24]));
    let channels = tc.draw(gs::integers::<u16>().min_value(1).max_value(2));
    let secs = f64::from(tc.draw(gs::integers::<u32>().min_value(1).max_value(14)));
    let bytes = media::wav_of(rate, bits, channels, secs);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Track.wav");
    fs::write(&path, &bytes).unwrap();

    let f = probe::format(&path).unwrap();
    assert_eq!((f.codec, f.sample_rate, f.bit_depth, f.channels), (Some(Codec::Wav), rate, bits, channels));
    assert!((f.duration_secs.unwrap() - secs).abs() < 1e-6, "{f:?}");
    // Whole, it's fine whether the database gives its length (to the second) or none.
    let rounded = secs.round();
    assert!(probe::health(&path, rounded, bytes.len() as u64).is_none());
    assert!(probe::health(&path, 0.0, 0).is_none());

    match tc.draw(gs::integers::<u8>().max_value(2)) {
        // The database says it's longer than the file says: another file replaced it, unless
        // it also lost as much of its size, which is a copy cut short.
        0 => {
            let expected = secs + f64::from(tc.draw(gs::integers::<u32>().min_value(5).max_value(60)));
            let cut_size = tc.draw(gs::booleans());
            let recorded = if cut_size { (bytes.len() as f64 * expected / secs) as u64 } else { 0 };
            match probe::health(&path, expected, recorded) {
                Some(AudioProblem::Truncated { ends_secs, expected_secs }) => {
                    assert!(cut_size);
                    assert!((ends_secs - secs).abs() < 0.5 && expected_secs == expected);
                }
                Some(p @ AudioProblem::LengthDiffers { .. }) => {
                    assert!(!cut_size);
                    assert!(p.is_warning() && p.kind() == "length_differs");
                }
                other => panic!("{other:?}"),
            }
        }
        // Cut short behind its header's back: it stops where the bytes do.
        1 => {
            let keep = 44 + (bytes.len() - 44) * tc.draw(gs::integers::<usize>().min_value(0).max_value(40)) / 100;
            fs::write(&path, &bytes[..keep]).unwrap();
            let left = (keep - 44) as f64 / f64::from(rate * u32::from(channels) * u32::from(bits / 8));
            let p = probe::health(&path, rounded, bytes.len() as u64);
            // Missing past the 2 s (and 0.5%) the check allows. Where a file stops is known to a
            // packet, so a cut within half a second of that limit may go either way.
            let short = secs - left - (2.0 + secs * 0.005);
            // With no audio left at all, nothing decodes: cut short, however little it should hold.
            if short > 0.5 || keep == 44 {
                let Some(AudioProblem::Truncated { ends_secs, .. }) = p else { panic!("{p:?}") };
                assert!((ends_secs - left).abs() < 0.5, "stops at {ends_secs}, not {left}");
                assert!(!AudioProblem::Truncated { ends_secs, expected_secs: rounded }.is_warning());
            } else if short < -0.5 {
                assert!(p.is_none(), "{left} of {secs} s left: {p:?}");
            }
        }
        // Not audio at all.
        _ => {
            fs::write(&path, b"RIFF this is no wave").unwrap();
            let p = probe::health(&path, rounded, 0).unwrap();
            assert_eq!(p.kind(), "unreadable", "{p}");
            assert!(probe::format(&path).is_err());
        }
    }
}

/// `m:ss`, rounding to the second, never negative.
#[hegel::test(test_cases = 200)]
fn minutes_read_as_a_clock(tc: TestCase) {
    let secs = tc.draw(gs::floats::<f64>().min_value(-10.0).max_value(10_000.0));
    let shown = probe::minutes(secs);
    let (m, s) = shown.split_once(':').unwrap();
    assert_eq!(s.len(), 2);
    let total = m.parse::<u64>().unwrap() * 60 + s.parse::<u64>().unwrap();
    assert_eq!(total, secs.max(0.0).round() as u64);
}

/// A drive's tracks are checked once per file: those cut short, or not audio, are found, the rest
/// pass, and files that aren't there are left to the other checks.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn a_drives_files_are_each_checked_once(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = crate::common::blank(dir.path(), "USB");
    fs::create_dir_all(root.join("PIONEER/rekordbox")).unwrap();
    let mut lib = Library::default();
    let mut want = Vec::new();
    for n in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        let path = format!("/Contents/T{n}.wav");
        let file = paths::to_fs(&root, &paths::key(&path));
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let wav = media::wav_of(22_050, 16, 1, 12.0);
        let kind = tc.draw(gs::integers::<u8>().max_value(3));
        match kind {
            0 => fs::write(&file, &wav).unwrap(),
            1 => fs::write(&file, &wav[..wav.len() / 4]).unwrap(),
            2 => fs::write(&file, b"not audio").unwrap(),
            _ => {}
        }
        let mut t = loclib_model::test_util::track(n as u32 + 1, "A", &format!("T{n}"), &path);
        t.duration_secs = 12;
        t.file_size = wav.len() as u64;
        lib.tracks.push(t.clone());
        if tc.draw(gs::booleans()) {
            // Another row for the same file is checked with it.
            lib.tracks.push(locallibrary::Track { id: 100 + n as u32, ..t });
        }
        want.push(kind);
    }
    let d = Device::open(&root).unwrap();
    let tree = d.scan(Some(&lib), &Ctx::silent()).unwrap();
    let (checked, findings) = probe::check_tracks(&d, &lib, &tree, &Ctx::silent()).unwrap();
    assert_eq!(checked, want.iter().filter(|k| **k != 3).count());
    let found: Vec<(String, &str)> = findings.iter().map(|f| (f.track.file_path.clone(), f.problem.kind())).collect();
    let expect: Vec<(String, &str)> = want
        .iter()
        .enumerate()
        .filter_map(|(n, k)| match k {
            1 => Some((format!("/Contents/T{n}.wav"), "truncated")),
            2 => Some((format!("/Contents/T{n}.wav"), "unreadable")),
            _ => None,
        })
        .collect();
    let mut found = found;
    found.sort();
    assert_eq!(found, expect);
}
