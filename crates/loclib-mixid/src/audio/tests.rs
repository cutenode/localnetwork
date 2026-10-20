use super::*;

fn tone(freq: f64, rate: u32, secs: f64) -> Vec<f32> {
    (0..(rate as f64 * secs) as usize).map(|i| (2.0 * PI * freq * i as f64 / rate as f64).sin() as f32).collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    let mut r = Resampler::new(from, to);
    let mut out = Vec::new();
    // Odd chunk sizes exercise the streaming bookkeeping.
    for chunk in x.chunks(1013) {
        r.push(chunk, &mut |s| out.extend_from_slice(s));
    }
    r.finish(&mut |s| out.extend_from_slice(s));
    out
}

#[test]
fn keeps_passband_and_length() {
    for from in [44_100, 48_000, 96_000, 22_050] {
        let out = resample(&tone(1000.0, from, 2.0), from, 11_025);
        let expected = 2.0 * 11_025.0;
        assert!((out.len() as f64 - expected).abs() < 3.0, "{from}: {} samples", out.len());
        let mid = &out[2000..out.len() - 2000];
        assert!((rms(mid) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.03, "{from}: rms {}", rms(mid));
    }
}

/// A sink that has heard enough stops decoding: the rest of the file isn't read.
#[test]
fn decoding_stops_when_the_sink_has_enough() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.wav");
    std::fs::write(&path, crate::test_util::wav(44_100, 16, 30.0)).unwrap();
    let mut heard = 0;
    let start = decode_until(&path, 11_025, &Ctx::silent(), &mut |s| {
        heard += s.len();
        if heard < 11_025 { ControlFlow::Continue(()) } else { ControlFlow::Break(()) }
    })
    .unwrap();
    assert!(start.duration_secs < 1.5 && heard < 2 * 11_025, "{start:?}, {heard} samples");
    let whole = decode(&path, 11_025, &Ctx::silent(), &mut |_| {}).unwrap();
    assert!((whole.duration_secs - 30.0).abs() < 0.01, "{whole:?}");
}

/// At the file's own rate, the samples come interleaved as they are, with their rate and
/// channel count; cancelling stops the decode without reading the rest.
#[test]
fn native_decoding_hands_on_the_file_and_stops_when_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.wav");
    let samples: Vec<i32> = (0..8_000 * 60).map(|i| (i % 2000) - 1000).collect();
    std::fs::write(&path, crate::test_util::wav_of(8_000, 16, samples.iter().copied())).unwrap();
    let mut heard = Vec::new();
    let format = decode_native(&path, Damaged::Fail, &Ctx::silent(), |s: &[i32], rate, channels| -> Result<_> {
        assert_eq!((rate, channels), (8_000, 2));
        heard.extend(s.iter().step_by(2).map(|v| v >> 16));
        Ok(ControlFlow::Continue(()))
    })
    .unwrap();
    assert_eq!((format.sample_rate, format.channels), (8_000, 2));
    assert!(heard == samples, "every sample, unchanged");

    let ctx = Ctx::silent();
    ctx.cancel.cancel();
    let mut frames = 0;
    let result = decode_native(&path, Damaged::Skip, &ctx, |s: &[i32], _, channels| -> Result<_> {
        frames += s.len() / channels;
        Ok(ControlFlow::Continue(()))
    });
    assert!(matches!(result, Err(Error::Cancelled(_))), "{result:?}");
    assert!(frames < samples.len(), "{frames}");
}

/// Fed a sample at a time, the output is the one fed whole, to the bit: 11025 to 48000 Hz
/// puts outputs on exact ties between two of the kernel's phases, which rounding a drifting
/// position used to tip either way.
#[test]
fn chunks_dont_change_the_output() {
    let x = tone(50.0, 11_025, 0.25);
    let mut r = Resampler::new(11_025, 48_000);
    let mut one_by_one = Vec::new();
    for s in &x {
        r.push(std::slice::from_ref(s), &mut |s| one_by_one.extend_from_slice(s));
    }
    r.finish(&mut |s| one_by_one.extend_from_slice(s));
    assert!(crate::test_util::resample(&x, 11_025, 48_000) == one_by_one);
}

#[test]
fn removes_what_would_alias() {
    // 9 kHz is above the 5.5 kHz output Nyquist rate: it must not fold back down.
    let out = resample(&tone(9000.0, 44_100, 1.0), 44_100, 11_025);
    assert!(rms(&out[1000..out.len() - 1000]) < 0.01);
}

use hegel::generators as gs;
use hegel::{Generator, TestCase};

const RATES: [u32; 8] = [8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 96_000];

#[hegel::test_helper]
fn rate(tc: &TestCase) -> u32 {
    tc.draw(gs::sampled_from(RATES.to_vec()))
}

/// Between any two rates, whatever the chunks the input comes in: the output is the input's
/// length at the new rate (rounded up: an output for every position inside the input), the
/// same sample for sample however it was chunked; a tone well inside both rates' bands keeps
/// its level.
#[hegel::test(test_cases = 40)]
fn resampling_keeps_length_and_level(tc: TestCase) {
    let (from, to) = (rate(&tc), rate(&tc));
    let freq = tc.draw(gs::floats::<f64>().min_value(50.0).max_value(0.2 * from.min(to) as f64));
    let x = tone(freq, from, tc.draw(gs::floats::<f64>().min_value(0.0).max_value(0.3)));
    let chunk = tc.draw(gs::integers::<usize>().min_value(1).max_value(5000));
    let mut chunked = Vec::new();
    let mut r = Resampler::new(from, to);
    for c in x.chunks(chunk) {
        r.push(c, &mut |s| chunked.extend_from_slice(s));
    }
    r.finish(&mut |s| chunked.extend_from_slice(s));
    let expected = (x.len() as u64 * u64::from(to)).div_ceil(u64::from(from));
    assert_eq!(chunked.len() as u64, expected);
    assert!(crate::test_util::resample(&x, from, to) == chunked, "chunks of {chunk} change it");
    // Away from the ends, where the kernel runs off the signal.
    let edge = (to as usize / 50).max(64);
    if chunked.len() > 4 * edge {
        let level = rms(&chunked[edge..chunked.len() - edge]);
        assert!((level - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.03, "rms {level} at {freq} Hz");
    }
}

/// Each output is the kernel at its exact position, to the bit: every input within the kernel's
/// half-width, weighed by the table entry nearest its distance (ties away from 0), summed in
/// order. The same rate passes the input through.
#[hegel::test(test_cases = 40)]
fn each_output_is_the_kernel_at_its_position(tc: TestCase) {
    let (from, to) = (rate(&tc), rate(&tc));
    let x: Vec<f32> = tc.draw(gs::vecs(gs::floats::<f32>().min_value(-1.0).max_value(1.0)).max_size(2000));
    let r = Resampler::new(from, to);
    let (unit, half, table, n) = (r.unit, r.half, &r.table, x.len() as u64);
    let at = |pos: u64| {
        let lo = pos.saturating_sub(half).div_ceil(unit);
        let hi = ((pos + half) / unit).min(n - 1);
        (lo..=hi).fold(0.0f32, |acc, i| {
            let k = ((2 * (i * unit).abs_diff(pos) * PHASES as u64 + unit) / (2 * unit)) as usize;
            acc + x[i as usize] * table[k.min(table.len() - 1)]
        })
    };
    let model: Vec<f32> = match from == to {
        true => x.clone(),
        false => (0..).map(|o| o * r.step).take_while(|&pos| pos < n * unit).map(at).collect(),
    };
    assert!(crate::test_util::resample(&x, from, to) == model, "{from} Hz to {to} Hz");
}

/// A WAV of any rate, depth and length decodes to its samples exactly, at its own rate and
/// to mono at it, and says how long it is; a sink that has heard enough stops the decode a
/// packet or so later.
#[hegel::test(test_cases = 40)]
fn wavs_decode_to_their_samples(tc: TestCase) {
    let rate = rate(&tc);
    let bits = tc.draw(gs::sampled_from(vec![16u16, 24, 32]));
    let top = (1i64 << (bits - 1)) - 1;
    let samples: Vec<i32> = tc
        .draw(gs::vecs(gs::integers::<i64>().min_value(-top - 1).max_value(top)).max_size(3000))
        .into_iter()
        .map(|v| v as i32)
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.wav");
    std::fs::write(&path, crate::test_util::wav_of(rate, bits, samples.iter().copied())).unwrap();

    let mut heard = Vec::new();
    let format = decode_native(&path, Damaged::Fail, &Ctx::silent(), |s: &[i32], r, channels| -> Result<_> {
        assert_eq!((r, channels), (rate, 2));
        heard.extend(s.iter().step_by(2).map(|v| v >> (32 - bits)));
        Ok(ControlFlow::Continue(()))
    })
    .unwrap();
    assert_eq!(heard, samples);
    assert_eq!((format.sample_rate, format.channels), (rate, 2));
    assert_eq!(format.duration_secs, samples.len() as f64 / rate as f64);

    let mut mono = Vec::new();
    decode(&path, rate, &Ctx::silent(), &mut |s| mono.extend_from_slice(s)).unwrap();
    let full = (1u64 << (bits - 1)) as f32;
    assert_eq!(mono.len(), samples.len());
    assert!(mono.iter().zip(&samples).all(|(m, s)| (m - *s as f32 / full).abs() < 1e-6));

    let enough = tc.draw(gs::integers::<usize>().min_value(1).max_value(samples.len().max(1)));
    let mut got = 0;
    let start = decode_until(&path, rate, &Ctx::silent(), &mut |s| {
        got += s.len();
        if got < enough { ControlFlow::Continue(()) } else { ControlFlow::Break(()) }
    })
    .unwrap();
    assert!(got >= enough.min(samples.len()) && got < enough + 8192, "{got} for {enough}");
    assert!(start.duration_secs * rate as f64 <= got as f64 + 0.5);
}

/// A format is told by the file's last extension alone, in any case: not by the rest of its
/// name or its folder. rekordbox's formats are decodable; other formats and names without an
/// extension aren't.
#[hegel::test(test_cases = 200)]
fn formats_are_told_by_extension_in_any_case(tc: TestCase) {
    let named = [
        ("a.mp3", true),
        ("B.FLAC", true),
        ("c.Wave", true),
        ("d.aif", true),
        ("e.M4A", true),
        ("f.ogg", false),
        ("g.opus", false),
        ("h.mp3.txt", false),
        ("i.txt.wav", true),
        ("mp3", false),
        (".mp3", false),
        ("j.mp3 ", false),
    ];
    for (name, decodable) in named {
        assert_eq!(is_decodable(Path::new(name)), decodable, "{name}");
    }
    let ext: String = tc.draw(hegel::one_of!(
        gs::sampled_from(vec!["mp3", "flac", "wave", "aiff", "m4a", "ogg"]).map(str::to_string),
        gs::text().alphabet("acdefilmopvwx34. ").max_size(6)
    ));
    let cased: String = ext.chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c }).collect();
    let dir = tc.draw(gs::sampled_from(vec!["/music", "/a.flac", "set.mp3"]));
    let stem = tc.draw(gs::sampled_from(vec!["set", "a.b", "mp3", "x.flac"]));
    let plain = is_decodable(&Path::new("/music").join(format!("set.{ext}")));
    let path = Path::new(dir).join(format!("{stem}.{cased}"));
    assert_eq!(is_decodable(&path), plain, "{} and set.{ext}", path.display());
}

/// A WAV damaged anywhere (bytes changed, cut short) never panics the decoder: it decodes,
/// saying how much it decoded, or is that file's error, in either mode.
#[hegel::test(test_cases = 60)]
fn damaged_files_are_that_files_error(tc: TestCase) {
    let frames = tc.draw(gs::integers::<usize>().min_value(1).max_value(2000));
    let mut bytes = crate::test_util::wav_of(rate(&tc), 16, (0..frames as i32).map(|i| i * 7 - 3000));
    let flips = tc.draw(gs::vecs(gs::tuples!(gs::integers::<usize>(), gs::integers::<u8>())).max_size(4));
    for (at, v) in flips {
        let n = bytes.len();
        bytes[at % n] = v;
    }
    let keep = tc.draw(gs::integers::<usize>().max_value(bytes.len()));
    bytes.truncate(keep);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.wav");
    std::fs::write(&path, &bytes).unwrap();
    for damaged in [Damaged::Skip, Damaged::Fail] {
        let mut heard = 0;
        let result = decode_native(&path, damaged, &Ctx::silent(), |s: &[i16], _, channels| -> Result<_> {
            heard += s.len() / channels;
            Ok(ControlFlow::Continue(()))
        });
        match result {
            Ok(format) => assert!(format.duration_secs * format.sample_rate as f64 <= heard as f64 + 0.5),
            Err(Error::Parse { path: at, .. }) => assert_eq!(at, path),
            Err(e) => panic!("{e}"),
        }
    }
}

/// The packets a WAV of `frames` decodes in: symphonia hands on at most 1152 frames at a time.
fn packets(frames: usize) -> usize {
    frames.div_ceil(1152)
}

/// A sink's own error stops the decode at the packet it was handed and is what the decode
/// returns, in either mode; a sink that never fails hears every packet.
#[hegel::test(test_cases = 30)]
fn a_sinks_error_stops_the_decode(tc: TestCase) {
    let frames = tc.draw(gs::integers::<usize>().min_value(1).max_value(6000));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.wav");
    std::fs::write(&path, crate::test_util::wav_of(rate(&tc), 16, (0..frames as i32).map(|i| i % 900))).unwrap();
    let fail = tc.draw(gs::integers::<usize>().min_value(1).max_value(packets(frames) + 1));
    let damaged = tc.draw(gs::sampled_from(vec![Damaged::Skip, Damaged::Fail]));
    let mut calls = 0;
    let result = decode_native(&path, damaged, &Ctx::silent(), |_: &[i16], _, _| -> Result<_> {
        calls += 1;
        if calls == fail { Err(Error::Invalid(format!("enough at {calls}"))) } else { Ok(ControlFlow::Continue(())) }
    });
    if fail <= packets(frames) {
        assert!(matches!(&result, Err(Error::Invalid(m)) if *m == format!("enough at {fail}")), "{result:?}");
        assert_eq!(calls, fail);
    } else {
        assert!(result.is_ok() && calls == packets(frames), "{result:?} after {calls}");
    }
}

/// However a WAV's header is broken, every way of decoding it (at its own rate in either mode,
/// to mono, to mono until it's enough) fails as that file's error: I/O for a file that isn't
/// there, a parse error for a block size of 0, the decoder's crash for a rate of 0, which
/// panics symphonia. Intact, it decodes whatever it's called: it's told by its content.
#[hegel::test(test_cases = 60)]
fn broken_headers_are_that_files_error(tc: TestCase) {
    let frames = tc.draw(gs::integers::<usize>().min_value(1).max_value(500));
    let rate = rate(&tc);
    let broken = tc.draw(gs::sampled_from(vec!["intact", "missing", "block size 0", "rate 0"]));
    let name = tc.draw(gs::sampled_from(vec!["x.wav", "x", "x.bin"]));
    let samples = (0..frames as i32).map(|i| i * 5 - 1000);
    let mut bytes = crate::test_util::wav_of(if broken == "rate 0" { 0 } else { rate }, 16, samples);
    if broken == "block size 0" {
        bytes[32..34].fill(0);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    if broken != "missing" {
        std::fs::write(&path, &bytes).unwrap();
    }
    let ctx = Ctx::silent();
    let native =
        |damaged| decode_native(&path, damaged, &ctx, |_: &[f32], _, _| -> Result<_> { Ok(ControlFlow::Continue(())) });
    let results = [
        native(Damaged::Skip),
        native(Damaged::Fail),
        decode(&path, 11_025, &ctx, &mut |_| {}),
        decode_until(&path, 11_025, &ctx, &mut |_| ControlFlow::Break(())),
    ];
    for result in results {
        match (broken, result) {
            ("intact", Ok(format)) => assert_eq!((format.sample_rate, format.channels), (rate, 2)),
            ("missing", Err(Error::Io { path: at, .. })) => assert_eq!(at, path),
            ("block size 0", Err(Error::Parse { path: at, .. })) => assert_eq!(at, path),
            ("rate 0", Err(Error::Parse { path: at, message })) => {
                assert!(at == path && message == CRASHED, "{message}")
            }
            (broken, result) => panic!("{broken} {name}: {result:?}"),
        }
    }
}

/// Cancelled at any packet of a long decode, it stops at the next check (every 256 packets)
/// with [`Error::Cancelled`], handing on nothing after; cancelled after the last check, it
/// decodes to the end.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn cancelling_stops_a_decode_at_the_next_check(tc: TestCase) {
    let frames = tc.draw(gs::integers::<usize>().min_value(250 * 1152).max_value(520 * 1152));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.wav");
    std::fs::write(&path, crate::test_util::wav_of(8_000, 16, (0..frames as i32).map(|i| i % 300))).unwrap();
    let at = tc.draw(gs::integers::<usize>().min_value(1).max_value(packets(frames)));
    let damaged = tc.draw(gs::sampled_from(vec![Damaged::Skip, Damaged::Fail]));
    let ctx = Ctx::silent();
    let mut heard = 0;
    let result = decode_native(&path, damaged, &ctx, |_: &[i16], _, _| -> Result<_> {
        heard += 1;
        if heard == at {
            ctx.cancel.cancel();
        }
        Ok(ControlFlow::Continue(()))
    });
    let check = (at / 256 + 1) * 256;
    if check <= packets(frames) {
        assert!(matches!(result, Err(Error::Cancelled(_))), "{result:?}");
        assert_eq!(heard, check - 1);
    } else {
        assert!(result.is_ok() && heard == packets(frames), "{result:?} after {heard}");
    }
}
