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
    std::fs::write(&path, loclib_testkit::audio::wav(44_100, 16, 30.0)).unwrap();
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
    std::fs::write(&path, loclib_testkit::audio::wav_of(8_000, 16, samples.iter().copied())).unwrap();
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
    assert!(loclib_testkit::audio::resample(&x, 11_025, 48_000) == one_by_one);
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
    assert!(loclib_testkit::audio::resample(&x, from, to) == chunked, "chunks of {chunk} change it");
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
    assert!(loclib_testkit::audio::resample(&x, from, to) == model, "{from} Hz to {to} Hz");
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
    std::fs::write(&path, loclib_testkit::audio::wav_of(rate, bits, samples.iter().copied())).unwrap();

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

/// An AIFF decodes to the frames its COMM chunk counts, exactly, at its own rate and to mono,
/// whether a chunk (an ID3 tag) follows them or the file ends with them. symphonia reads 8 bytes
/// past the samples: noise from the chunk after them, and when none follows, a length that calls
/// the file cut short wherever its samples end within 8 bytes of a whole number of 1152-frame
/// packets (1151 or 1152 frames of 16-bit stereo). Either way of treating damage stops there.
#[hegel::test(test_cases = 40)]
#[hegel::explicit_test_case(channels = 2u16, bits = 16u16, frames = 1151usize, tagged = false)]
#[hegel::explicit_test_case(channels = 1u16, bits = 24u16, frames = 2304usize, tagged = false)]
#[hegel::explicit_test_case(channels = 2u16, bits = 16u16, frames = 1000usize, tagged = true)]
fn aiffs_decode_to_their_frames(tc: TestCase) {
    let channels = tc.draw(gs::integers::<u16>().min_value(1).max_value(2));
    let bits = tc.draw(gs::sampled_from(vec![16u16, 24]));
    let frames = tc.draw(gs::integers::<usize>().max_value(2400));
    let tagged = tc.draw(gs::booleans());
    let (width, half) = (usize::from(bits / 8), 1i64 << (bits - 1));
    let samples: Vec<i32> =
        (0..frames * usize::from(channels)).map(|i| ((i as i64 * 7_919) % (2 * half) - half) as i32).collect();
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_be_bytes()[4 - width..].to_vec()).collect();
    let mut chunks = [
        b"COMM".as_slice(),
        &18u32.to_be_bytes(),
        &channels.to_be_bytes(),
        &(frames as u32).to_be_bytes(),
        &bits.to_be_bytes(),
        // 44.1 kHz as an 80-bit extended float.
        &[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0],
        b"SSND",
        &(data.len() as u32 + 8).to_be_bytes(),
        &[0; 8],
        &data,
    ]
    .concat();
    if data.len() % 2 == 1 {
        chunks.push(0);
    }
    if tagged {
        chunks.extend_from_slice(b"ID3 \0\0\0\x0aID3\x03\0\0\0\0\0\0");
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.aiff");
    std::fs::write(&path, [b"FORM".as_slice(), &(chunks.len() as u32 + 4).to_be_bytes(), b"AIFF", &chunks].concat())
        .unwrap();

    let mut heard = Vec::new();
    let format = decode_native(&path, Damaged::Fail, &Ctx::silent(), |s: &[i32], r, c| -> Result<_> {
        assert_eq!((r, c), (44_100, usize::from(channels)));
        heard.extend(s.iter().map(|v| v >> (32 - bits)));
        Ok(ControlFlow::Continue(()))
    })
    .unwrap();
    assert_eq!(heard, samples);
    assert_eq!(format.duration_secs, frames as f64 / 44_100.0);
    let mut mono = 0;
    decode(&path, 44_100, &Ctx::silent(), &mut |s| mono += s.len()).unwrap();
    assert_eq!(mono, frames);
    // Through the sink the other properties decode with, in either mode, as 16-bit samples.
    for damaged in [Damaged::Skip, Damaged::Fail] {
        let (result, heard) = native(&path, damaged, Sink::Takes);
        assert_eq!(heard.samples.len(), samples.len());
        if bits == 16 {
            assert!(heard.samples.iter().zip(&samples).all(|(h, s)| i32::from(*h) == *s));
        }
        assert_eq!(result.unwrap().duration_secs, frames as f64 / 44_100.0);
    }
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
    let mut bytes = loclib_testkit::audio::wav_of(rate(&tc), 16, (0..frames as i32).map(|i| i * 7 - 3000));
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
    std::fs::write(&path, loclib_testkit::audio::wav_of(rate(&tc), 16, (0..frames as i32).map(|i| i % 900))).unwrap();
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
    let mut bytes = loclib_testkit::audio::wav_of(if broken == "rate 0" { 0 } else { rate }, 16, samples);
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
    std::fs::write(&path, loclib_testkit::audio::wav_of(8_000, 16, (0..frames as i32).map(|i| i % 300))).unwrap();
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

/// [`aiff_frames`] reads the frame count from the COMM chunk wherever it is among the chunks
/// (each padded to an even length), in an AIFF or AIFC; anything else, a file cut short before
/// it and a file that isn't there have none.
#[hegel::test(test_cases = 200)]
fn aiff_frames_are_found_among_any_chunks(tc: TestCase) {
    let frames = tc.draw(gs::integers::<u32>());
    let kind = tc.draw(gs::sampled_from(vec![*b"AIFF", *b"AIFC", *b"WAVE"]));
    let mut bytes = [b"FORM".as_slice(), &0u32.to_be_bytes(), &kind].concat();
    for len in tc.draw(gs::vecs(gs::integers::<u32>().max_value(9)).max_size(4)) {
        bytes.extend_from_slice(b"FVER");
        bytes.extend_from_slice(&len.to_be_bytes());
        bytes.extend(std::iter::repeat_n(0x55, (len + len % 2) as usize));
    }
    let comm_end = bytes.len() + 14;
    bytes.extend_from_slice(b"COMM");
    bytes.extend_from_slice(&18u32.to_be_bytes());
    bytes.extend_from_slice(&2u16.to_be_bytes());
    bytes.extend_from_slice(&frames.to_be_bytes());
    bytes.extend_from_slice(&[0, 16, 0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.aiff");
    let cut = tc.draw(gs::integers::<usize>().max_value(bytes.len()));
    std::fs::write(&path, &bytes[..cut]).unwrap();
    let found = aiff_frames(&path);
    assert_eq!(found, (kind != *b"WAVE" && cut >= comm_end).then_some(u64::from(frames)), "cut at {cut}");
    assert_eq!(aiff_frames(&dir.path().join("gone.aiff")), None);
}

/// A WAV saying any rate at all decodes at it, to mono at the fingerprints' rate too, when it's
/// at most [`MAX_RATE`]; above it (a crafted header, which resampling would take memory in
/// proportion to) every way of decoding refuses it as that file's error.
#[hegel::test(test_cases = 60)]
#[hegel::explicit_test_case(rate = 4_294_967_291u32, frames = 32usize)]
#[hegel::explicit_test_case(rate = 768_001u32, frames = 32usize)]
#[hegel::explicit_test_case(rate = 767_999u32, frames = 32usize)]
fn rates_past_the_highest_are_refused(tc: TestCase) {
    let rate = tc.draw(gs::integers::<u32>().min_value(1));
    let frames = tc.draw(gs::integers::<usize>().min_value(1).max_value(32));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.wav");
    std::fs::write(&path, loclib_testkit::audio::wav_of(rate, 16, (0..frames as i32).map(|i| i * 99))).unwrap();
    let ctx = Ctx::silent();
    let results = [
        decode_native(&path, Damaged::Fail, &ctx, |_: &[f32], _, _| -> Result<_> { Ok(ControlFlow::Continue(())) }),
        decode(&path, loclib_testkit::audio::RATE, &ctx, &mut |_| {}),
    ];
    for result in results {
        match result {
            Ok(format) if rate <= MAX_RATE => assert_eq!(format.sample_rate, rate),
            Err(Error::Parse { path: at, message }) if rate > MAX_RATE => {
                assert!(at == path && message.contains("768 kHz"), "{message}")
            }
            result => panic!("{rate} Hz: {result:?}"),
        }
    }
}

/// Between any two rates up to [`MAX_RATE`] (to one a decode asks for), the weights a resampler
/// keeps stay under [`CACHED_WEIGHTS`] however much it's fed, and its output is the same to the
/// bit as one that keeps none.
#[hegel::test(test_cases = 60)]
#[hegel::explicit_test_case(from = 767_999u32, to = 11_025u32, chunk = 4096usize)]
#[hegel::explicit_test_case(from = 1u32, to = 768_000u32, chunk = 1usize)]
fn the_weight_cache_is_bounded_and_changes_nothing(tc: TestCase) {
    let from = tc.draw(gs::integers::<u32>().min_value(1).max_value(MAX_RATE));
    let to = tc.draw(gs::integers::<u32>().min_value(8_000).max_value(MAX_RATE));
    let chunk = tc.draw(gs::integers::<usize>().min_value(1).max_value(4096));
    // About 20,000 outputs, whatever the rates.
    let n = (20_000 * u64::from(from) / u64::from(to)).clamp(1, 100_000) as usize;
    let x: Vec<f32> = (0..n).map(|i| ((i * 7_919) % 2_001) as f32 / 1_000.0 - 1.0).collect();
    let (mut cached, mut fresh) = (Resampler::new(from, to), Resampler::new(from, to));
    fresh.phases.clear();
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for c in x.chunks(chunk) {
        cached.push(c, &mut |s| a.extend_from_slice(s));
        fresh.push(c, &mut |s| b.extend_from_slice(s));
    }
    cached.finish(&mut |s| a.extend_from_slice(s));
    fresh.finish(&mut |s| b.extend_from_slice(s));
    let kept: usize = cached.phases.iter().map(Vec::len).sum();
    assert!(kept as u64 <= CACHED_WEIGHTS, "{from} Hz to {to} Hz keeps {kept} weights");
    assert!(a == b, "{from} Hz to {to} Hz");
}

/// What [`native`]'s sink does: take everything, or at its `n`th packet (from 1) stop the
/// decode, fail it, or cancel it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Sink {
    Takes,
    Breaks(usize),
    Fails(usize),
    Cancels(usize),
}

/// What [`native`] heard: the samples, and each packet's rate and channels.
#[derive(Debug, Default)]
struct Heard {
    samples: Vec<i16>,
    formats: Vec<(u32, usize)>,
}

/// `path` decoded at its own rate as `i16`, in `damaged` mode, its packets handed to a sink that
/// does as `sink` says. Every property below decodes through this one sink: coverage counts a
/// generic function by its best instance, and each closure is one.
fn native(path: &Path, damaged: Damaged, sink: Sink) -> (Result<SourceFormat>, Heard) {
    let ctx = Ctx::silent();
    let mut heard = Heard::default();
    let result = decode_native(path, damaged, &ctx, |s: &[i16], rate, channels| -> Result<_> {
        heard.samples.extend_from_slice(s);
        heard.formats.push((rate, channels));
        let n = heard.formats.len();
        match sink {
            Sink::Breaks(at) if at == n => return Ok(ControlFlow::Break(())),
            Sink::Fails(at) if at == n => return Err(Error::Invalid(format!("enough at {n}"))),
            Sink::Cancels(at) if at == n => ctx.cancel.cancel(),
            _ => {}
        }
        Ok(ControlFlow::Continue(()))
    });
    (result, heard)
}

/// An ALAC track's frames decode to their samples at the rate its setup says, whichever mode,
/// and a second track's are left out. A frame the decoder can't read (its header garbled, or cut
/// short) is skipped, or fails the decode, by the mode. A setup saying 0 Hz or over [`MAX_RATE`]
/// (its container saying 8 kHz) is refused at the first frame that decodes; with none, the decode
/// is the container's 8 kHz. A sample table counting a sample the others don't place fails the
/// decode there, after the frames it has.
#[hegel::test(test_cases = 60)]
// A frame cut short, listened past and copied.
#[hegel::explicit_test_case(
    rate = 8_000u32,
    tracks = 1usize,
    frames = 1usize,
    damaged = Damaged::Skip,
    packets = vec![(vec![(1i16, 2i16)], 2u8), (vec![(3, 4)], 0)],
    overcount = false
)]
#[hegel::explicit_test_case(
    rate = 8_000u32,
    tracks = 1usize,
    frames = 1usize,
    damaged = Damaged::Fail,
    packets = vec![(vec![(1i16, 2i16)], 2u8)],
    overcount = false
)]
// Two tracks, a garbled frame listened past, then a frame at 0 Hz.
#[hegel::explicit_test_case(
    rate = 0u32,
    tracks = 2usize,
    frames = 1usize,
    damaged = Damaged::Skip,
    packets = vec![(vec![(1i16, 2i16)], 1u8), (vec![(3, 4)], 0)],
    overcount = false
)]
// A sample the table counts but doesn't place, after one it does.
#[hegel::explicit_test_case(
    rate = 8_000u32,
    tracks = 1usize,
    frames = 1usize,
    damaged = Damaged::Fail,
    packets = vec![(vec![(1i16, 2i16)], 0u8)],
    overcount = true
)]
fn prop_alac_frames_decode_at_the_rate_their_setup_says(tc: TestCase) {
    use loclib_testkit::audio::{alac_frame, m4a_with_audio};

    let rate = tc.draw(gs::sampled_from(vec![0, 8_000, 44_100, MAX_RATE + 1]));
    let tracks = tc.draw(gs::integers::<usize>().min_value(1).max_value(2));
    let frames = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let damaged = tc.draw(gs::sampled_from(vec![Damaged::Skip, Damaged::Fail]));
    let sample = gs::tuples!(gs::integers::<i16>(), gs::integers::<i16>());
    // Each frame intact (0), its header garbled (1) or cut short (2).
    let damage = gs::sampled_from(vec![0, 0, 0, 1, 2]);
    let packets: Vec<(Vec<(i16, i16)>, u8)> =
        tc.draw(gs::vecs(gs::tuples!(gs::vecs(sample).min_size(frames).max_size(frames), damage)).max_size(4));
    let overcount = tc.draw(gs::booleans());
    let overcounted = overcount && !packets.is_empty();
    let bytes: Vec<Vec<u8>> = packets
        .iter()
        .map(|(samples, damage)| {
            let mut frame = alac_frame(samples);
            match damage {
                // The 12 bits after the element's tag, which are 0.
                1 => frame[1] = 0xff,
                // Inside the count of frames it holds.
                2 => frame.truncate(4),
                _ => {}
            }
            frame
        })
        .collect();
    let mut file = m4a_with_audio(rate, tracks, &bytes, frames as u32);
    if overcounted {
        // `stts` counts one more sample than `stsz` sizes.
        let count = file.windows(4).position(|w| w == b"stts").unwrap() + 12;
        let n = u32::from_be_bytes(file[count..count + 4].try_into().unwrap());
        file[count..count + 4].copy_from_slice(&(n + 1).to_be_bytes());
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.m4a");
    std::fs::write(&path, file).unwrap();

    let (result, heard) = native(&path, damaged, Sink::Takes);
    assert!(heard.formats.iter().all(|f| *f == (rate, 2)), "{:?}", heard.formats);
    let (mut want, mut refused) = (Vec::new(), None);
    for (samples, damage) in &packets {
        if *damage > 0 {
            if damaged == Damaged::Fail {
                refused = Some(if *damage == 1 { "unused header bits not 0" } else { "end of bitstream" });
                break;
            }
            continue;
        }
        if rate == 0 || rate > MAX_RATE {
            refused = Some("it decodes to 0 Hz, over 768 kHz or no channels");
            break;
        }
        want.extend(samples.iter().flat_map(|&(l, r)| [l, r]));
    }
    if overcounted && refused.is_none() {
        refused = Some("isomp4: missing");
    }
    match (result, refused) {
        (Ok(format), None) => {
            assert_eq!(heard.samples, want);
            let decoded = want.len() / 2;
            // With nothing decoded, the container's rate (its ALAC entry leaves the channels unsaid).
            let (at, channels) = if decoded > 0 { (rate, 2) } else { (8_000, 0) };
            assert_eq!((format.sample_rate, format.channels), (at, channels));
            assert_eq!(format.duration_secs, decoded as f64 / f64::from(at));
        }
        (Err(Error::Parse { path: at, message }), Some(why)) => {
            assert!(at == path && message.contains(why), "{message}");
            assert_eq!(heard.samples, want, "what came before");
        }
        (result, refused) => panic!("{rate} Hz, {damaged:?}, {packets:?}: {result:?}, not {refused:?}"),
    }
}

/// An ALAC setup no decoder takes (a version past 0, a bit depth over 32), a track of a codec
/// none knows, a file that isn't there and a WAV saying 0 Hz (which panics the decoder) are that
/// file's error, however it's decoded.
#[hegel::test(test_cases = 20)]
fn prop_files_no_decoder_takes_are_that_files_error(tc: TestCase) {
    use loclib_testkit::audio::{m4a, mp4_empty_tables, wav_of};

    let (field, value) = tc.draw(hegel::one_of!(
        gs::integers::<u8>().min_value(1).map(|v| (4, v)),
        gs::integers::<u8>().min_value(33).map(|v| (5, v)),
        gs::sampled_from(vec![(0, 0), (1, 0), (2, 0)]),
    ));
    let mut bytes = m4a(8_000, &mp4_empty_tables(), &[]);
    // The setup follows the inner `alac` atom's version and flags.
    let cookie = bytes.windows(8).rposition(|w| w == b"alac\0\0\0\0").unwrap() + 8;
    match field {
        // The sample entry, of a codec no one knows.
        0 => {
            let entry = bytes.windows(4).position(|w| w == b"alac").unwrap();
            bytes[entry..entry + 4].copy_from_slice(b"zzzz");
        }
        // Not there.
        1 => {}
        2 => bytes = wav_of(0, 16, (0..64).map(|i| i * 100)),
        _ => bytes[cookie + field] = value,
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(if field == 2 { "x.wav" } else { "x.m4a" });
    if field != 1 {
        std::fs::write(&path, &bytes).unwrap();
    }
    let damaged = tc.draw(gs::sampled_from(vec![Damaged::Skip, Damaged::Fail]));
    let results = [
        native(&path, damaged, Sink::Takes).0,
        decode(&path, loclib_testkit::audio::RATE, &Ctx::silent(), &mut |_| {}),
    ];
    for result in results {
        match (field, result) {
            (1, Err(Error::Io { path: at, .. })) => assert_eq!(at, path),
            (_, Err(Error::Parse { path: at, message })) => {
                let why = match field {
                    0 => "no audio track",
                    2 => CRASHED,
                    _ => "alac",
                };
                assert!(at == path && message.contains(why), "{message}")
            }
            (_, result) => panic!("{field} at {value}: {result:?}"),
        }
    }
}

/// A folder that can't be opened, named as audio, is that path's I/O error however it's read.
#[cfg(unix)]
#[hegel::test(test_cases = 5)]
fn prop_a_folder_that_cant_be_opened_is_its_io_error(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(tc.draw(gs::sampled_from(vec!["x.mp3", "x.m4a", "x"])));
    std::fs::create_dir(&path).unwrap();
    /// Opens the folder up again, failing assertions or not, so the temporary folder goes.
    struct Reopen<'a>(&'a Path);
    impl Drop for Reopen<'_> {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let _reopen = Reopen(&path);
    let results = [
        open(&path).map(drop),
        native(&path, Damaged::Skip, Sink::Takes).0.map(drop),
        decode(&path, loclib_testkit::audio::RATE, &Ctx::silent(), &mut |_| {}).map(drop),
    ];
    for result in results {
        assert!(matches!(&result, Err(Error::Io { path: at, .. }) if *at == path), "{result:?}");
    }
}

/// A sink stops a decode at the packet it says, with what it was handed so far: by breaking (the
/// decode is then what it heard) or failing (its own error is the decode's). Cancelled at a
/// packet, the decode stops at the next check (every 256 packets) with the packets before it
/// handed on; cancelled after the last check, it decodes to the end.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
#[hegel::explicit_test_case(frames = 300_000usize, at = 3usize, sink = Sink::Cancels(3), damaged = Damaged::Skip)]
#[hegel::explicit_test_case(frames = 3_000usize, at = 2usize, sink = Sink::Fails(2), damaged = Damaged::Fail)]
#[hegel::explicit_test_case(frames = 3_000usize, at = 2usize, sink = Sink::Breaks(2), damaged = Damaged::Fail)]
fn prop_a_sink_or_cancelling_stops_a_decode_where_it_says(tc: TestCase) {
    let frames = tc.draw(gs::integers::<usize>().min_value(1).max_value(260 * 1152));
    let total = packets(frames);
    let at = tc.draw(gs::integers::<usize>().min_value(1).max_value(total + 1));
    let sink = tc.draw(gs::sampled_from(vec![Sink::Breaks(at), Sink::Fails(at), Sink::Cancels(at)]));
    let damaged = tc.draw(gs::sampled_from(vec![Damaged::Skip, Damaged::Fail]));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.wav");
    std::fs::write(&path, loclib_testkit::audio::wav_of(8_000, 16, (0..frames as i32).map(|i| i % 300))).unwrap();
    let (result, heard) = native(&path, damaged, sink);
    let handed = heard.formats.len();
    let reached = at <= total;
    match (sink, result) {
        (Sink::Breaks(_), Ok(format)) => {
            assert_eq!(handed, at.min(total));
            assert_eq!(format.duration_secs, (heard.samples.len() / 2) as f64 / 8_000.0);
        }
        (Sink::Fails(_), Err(Error::Invalid(m))) => assert!(reached && m == format!("enough at {at}") && handed == at),
        (Sink::Fails(_), Ok(_)) => assert!(!reached && handed == total),
        (Sink::Cancels(_), Err(Error::Cancelled(_))) => {
            let check = (at / 256 + 1) * 256;
            assert!(check <= total && handed == check - 1, "cancelled at {at}, handed {handed} of {total}");
        }
        (Sink::Cancels(_), Ok(_)) => assert!((at / 256 + 1) * 256 > total && handed == total),
        (sink, result) => panic!("{sink:?} of {total}: {result:?}"),
    }
}

/// A FLAC of `blocks` (16-bit mono at 44.1 kHz, every block as long, 256 samples at most), each
/// frame verbatim with its CRCs.
fn flac(blocks: &[Vec<i16>]) -> Vec<u8> {
    fn crc8(bytes: &[u8]) -> u8 {
        bytes
            .iter()
            .fold(0u8, |crc, &b| (0..8).fold(crc ^ b, |c, _| if c & 0x80 != 0 { (c << 1) ^ 0x07 } else { c << 1 }))
    }
    fn crc16(bytes: &[u8]) -> u16 {
        bytes.iter().fold(0u16, |crc, &b| {
            (0..8).fold(crc ^ (u16::from(b) << 8), |c, _| if c & 0x8000 != 0 { (c << 1) ^ 0x8005 } else { c << 1 })
        })
    }
    let size = blocks[0].len() as u64;
    let total = size * blocks.len() as u64;
    // The rate, channels less one, bits less one and total samples, packed into 64 bits.
    let packed = (44_100u64 << 44) | (15 << 36) | total;
    let mut out = b"fLaC\x80\0\0\x22".to_vec();
    out.extend_from_slice(&(size as u16).to_be_bytes());
    out.extend_from_slice(&(size as u16).to_be_bytes());
    // Frame sizes unknown.
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&packed.to_be_bytes());
    // No MD5.
    out.extend_from_slice(&[0; 16]);
    for (n, block) in blocks.iter().enumerate() {
        // Fixed blocks; the block size in a byte after the header, 44.1 kHz; mono, 16 bits; frame n.
        let mut frame = vec![0xff, 0xf8, 0x69, 0x08, n as u8, (size - 1) as u8];
        frame.push(crc8(&frame));
        // A verbatim subframe.
        frame.push(0x02);
        frame.extend(block.iter().flat_map(|s| s.to_be_bytes()));
        let crc = crc16(&frame);
        frame.extend_from_slice(&crc.to_be_bytes());
        out.extend(frame);
    }
    out
}

/// A FLAC frame whose checksum fails is dropped by the reader before the decoder sees it: decoding
/// to copy fails as a stretch of the file damaged (whether frames follow it or not), and
/// decoding to listen leaves it out and hears the rest.
#[hegel::test(test_cases = 40)]
// The last frame dropped, and one with frames after it.
#[hegel::explicit_test_case(size = 16usize, blocks = vec![vec![7i16; 16]; 2], broken = Some(1usize), damaged = Damaged::Fail)]
#[hegel::explicit_test_case(size = 16usize, blocks = vec![vec![7i16; 16]; 2], broken = Some(0usize), damaged = Damaged::Fail)]
fn prop_a_frame_the_reader_drops_fails_a_copy_but_not_a_listen(tc: TestCase) {
    let size = tc.draw(gs::integers::<usize>().min_value(16).max_value(64));
    let blocks: Vec<Vec<i16>> =
        tc.draw(gs::vecs(gs::vecs(gs::integers::<i16>()).min_size(size).max_size(size)).min_size(1).max_size(5));
    let broken = tc.draw(gs::optional(gs::integers::<usize>().max_value(blocks.len() - 1)));
    let damaged = tc.draw(gs::sampled_from(vec![Damaged::Skip, Damaged::Fail]));
    let mut bytes = flac(&blocks);
    if let Some(k) = broken {
        // A sample of frame k changed and its CRC-16 not: 42 bytes before the frames, each 7 of
        // header, 1 of subframe header, 2 a sample and 2 of CRC.
        bytes[42 + k * (10 + 2 * size) + 8] ^= 0x55;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.flac");
    std::fs::write(&path, &bytes).unwrap();
    let (result, heard) = native(&path, damaged, Sink::Takes);
    let kept: Vec<i16> =
        blocks.iter().enumerate().filter(|(i, _)| Some(*i) != broken).flat_map(|(_, b)| b.clone()).collect();
    match (result, broken, damaged) {
        (Ok(_), None, _) | (Ok(_), Some(_), Damaged::Skip) => assert_eq!(heard.samples, kept),
        (Err(Error::Parse { path: at, message }), Some(k), Damaged::Fail) => {
            assert!(at == path && message == "a stretch of it is damaged", "{message}");
            assert_eq!(heard.samples, blocks[..k].concat());
        }
        (result, broken, damaged) => panic!("{broken:?} in {damaged:?}: {result:?}"),
    }
}
