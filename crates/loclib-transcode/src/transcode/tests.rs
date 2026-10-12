use std::f64::consts::PI;

use std::sync::Mutex;

use flacenc::component::BitRepr;
use flacenc::error::Verify;
use loclib_mixid::test_util::wav_of;
use loclib_progress::CancelToken;
use lofty::config::WriteOptions;
use lofty::id3::v2::{Frame, Id3v2Tag, PrivateFrame};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, TagExt};

use super::*;
use crate::Dither;

/// A 1×1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D,
    0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn recipe(container: Container, sample_rate: u32, bit_depth: u16, dither: Dither) -> Recipe {
    Recipe { container, sample_rate, bit_depth, dither }
}

/// A file decoded back: interleaved samples at full 32-bit scale, so depths compare directly.
struct Decoded {
    rate: u32,
    channels: usize,
    samples: Vec<i32>,
}

fn decode(path: &Path) -> Decoded {
    let mut samples = Vec::new();
    let format = decode_native(path, Damaged::Fail, &Ctx::silent(), |s: &[i32], _, _| -> Result<_> {
        samples.extend_from_slice(s);
        Ok(ControlFlow::Continue(()))
    })
    .unwrap();
    let mut out = Decoded { rate: format.sample_rate, channels: usize::from(format.channels), samples };
    // As `transcode` reads AIFF, past symphonia's misreading of SSND's length.
    if let Some(frames) = aiff_frames(path) {
        out.samples.truncate(frames as usize * out.channels);
    }
    out
}

/// Tones between the octaves, so no lag but 0 lines them all up, at `rate` for `secs`.
fn tones(rate: u32, secs: f64) -> Vec<f64> {
    let parts = [(997.0, 0.3, 0.0), (3_163.7, 0.2, 1.0), (7_919.3, 0.15, 2.0), (15_013.0, 0.1, 0.5)];
    (0..(secs * f64::from(rate)).round() as usize)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            parts.iter().map(|(f, a, phase)| a * (2.0 * PI * f * t + phase).sin()).sum()
        })
        .collect()
}

/// A stereo WAV of `signal` (in [-1, 1)) in both channels.
fn wav(path: &Path, rate: u32, bits: u16, signal: &[f64]) {
    let scale = f64::from(1u32 << (bits - 1));
    std::fs::write(path, wav_of(rate, bits, signal.iter().map(|s| (s * scale).round() as i32))).unwrap();
}

/// Deterministic noise in a depth's range, interleaved.
fn noise(len: usize, bits: u16) -> Vec<i32> {
    let mut state = 0x1234_5678u32;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> (33 - bits)) as i32 - (1 << (bits - 2))
        })
        .collect()
}

/// A FLAC of interleaved `samples`, encoded by flacenc in one go (not this crate's streaming).
fn flac(path: &Path, samples: &[i32], channels: usize, bits: usize, rate: usize) {
    let source = flacenc::source::MemSource::from_samples(samples, channels, bits, rate);
    let config = flacenc::config::Encoder::default().into_verified().unwrap();
    let mut stream = flacenc::encode_with_fixed_block_size(&config, source, 4096).unwrap();
    // flacenc counts the short last block in the minimum, which makes symphonia take the stream
    // for one of variable block sizes and refuse it.
    stream.stream_info_mut().set_block_sizes(4096, 4096).unwrap();
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).unwrap();
    std::fs::write(path, sink.as_slice()).unwrap();
}

fn tagged(path: &Path) -> Tag {
    lofty::read_from_path(path).unwrap().primary_tag().unwrap().clone()
}

fn transcode_quietly(source: &Path, recipe: &Recipe, out: &Path) -> Result<Transcoded> {
    transcode(source, recipe, out, &Ctx::silent())
}

/// A resampled copy starts where the source does and is as long, to the frame: its samples
/// line up with the same tones made at the new rate at lag 0, and match them closely.
#[test]
fn resampled_copies_keep_time_and_length() {
    let dir = tempfile::tempdir().unwrap();
    for (from, to) in [(96_000, 48_000), (88_200, 44_100), (44_100, 48_000)] {
        let source = dir.path().join(format!("{from}.wav"));
        let signal = tones(from, 1.25);
        wav(&source, from, 24, &signal);
        let out = dir.path().join(format!("{from}-{to}.aiff"));
        let made = transcode_quietly(&source, &recipe(Container::Aiff, to, 24, Dither::Tpdf), &out).unwrap();

        let expected = (signal.len() as u64 * u64::from(to) + u64::from(from) / 2) / u64::from(from);
        let copy = decode(&out);
        assert_eq!((copy.rate, copy.channels), (to, 2));
        assert_eq!(made, Transcoded { frames: expected, channels: 2 }, "{from} -> {to}");
        assert_eq!(copy.samples.len() as u64 / 2, expected, "{from} -> {to}");

        let left: Vec<f64> = copy.samples.iter().step_by(2).map(|&s| f64::from(s) / 2_147_483_648.0).collect();
        let reference = tones(to, 1.25);
        // Away from the edges, where the abrupt start and end ring.
        let middle = to as usize / 10..left.len() - to as usize / 10;
        let correlation =
            |lag: isize| -> f64 { middle.clone().map(|i| left[i] * reference[(i as isize + lag) as usize]).sum() };
        let best = (-8..=8).max_by(|a, b| correlation(*a).total_cmp(&correlation(*b))).unwrap();
        assert_eq!(best, 0, "{from} -> {to}");
        // Within a frame, the 15 kHz tone's phase says how far within it: rubato's kernel sits
        // 1/128 of an input sample early, and nothing else should move it.
        let (f, phase) = (15_013.0, 0.5);
        let (mut sin, mut cos) = (0.0, 0.0);
        for i in middle {
            let w = 2.0 * PI * f * i as f64 / f64::from(to) + phase;
            (sin, cos) = (sin + left[i] * w.sin(), cos + left[i] * w.cos());
        }
        let early = cos.atan2(sin) / (2.0 * PI * f) * f64::from(to);
        assert!(early.abs() < 0.02, "{from} -> {to}: {early} frames early");
    }
}

/// Changing only the container (or going up to 24 bit) loses nothing: a 16-bit FLAC made by
/// flacenc decodes back sample for sample from every container.
#[test]
fn lossless_copies_are_bit_exact() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("master.flac");
    let samples = noise(10_001 * 2, 16);
    flac(&source, &samples, 2, 16, 44_100);
    let master = decode(&source);
    assert_eq!(master.samples.len(), samples.len());
    for container in Container::ALL {
        for depth in [16, 24] {
            let out = dir.path().join(format!("copy-{depth}.{}", container.extension()));
            let made = transcode_quietly(&source, &recipe(container, 44_100, depth, Dither::None), &out).unwrap();
            assert_eq!(made.frames, 10_001);
            assert!(decode(&out).samples == master.samples, "{container:?} at {depth} bit");
        }
    }
}

/// Tags and artwork come along in each container's own form, from Vorbis comments to ID3v2 and
/// back, and from one ID3v2 to another. The source is mono 24 bit with an odd number of frames,
/// so WAV and AIFF need a pad byte before the tag.
#[test]
fn tags_and_art_survive() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("master.flac");
    flac(&source, &noise(1_001, 24), 1, 24, 48_000);
    let mut tag = Tag::new(TagType::VorbisComments);
    tag.set_title("Night Drive".into());
    tag.set_artist("Ana".into());
    tag.push_picture(
        Picture::unchecked(PNG.to_vec()).pic_type(PictureType::CoverFront).mime_type(MimeType::Png).build(),
    );
    tag.save_to_path(&source, WriteOptions::default()).unwrap();
    let master = decode(&source);

    let check = |path: &Path, tag_type| {
        let tag = tagged(path);
        assert_eq!(tag.tag_type(), tag_type, "{}", path.display());
        assert_eq!((tag.title().as_deref(), tag.artist().as_deref()), (Some("Night Drive"), Some("Ana")));
        assert_eq!(tag.pictures().len(), 1, "{}", path.display());
        assert_eq!((tag.pictures()[0].data(), tag.pictures()[0].pic_type()), (PNG, PictureType::CoverFront));
        let copy = decode(path);
        assert!(
            copy.samples == master.samples,
            "{}: {} samples of {}",
            path.display(),
            copy.samples.len(),
            master.samples.len()
        );
    };
    for container in Container::ALL {
        let out = dir.path().join(format!("copy.{}", container.extension()));
        transcode_quietly(&source, &recipe(container, 48_000, 24, Dither::None), &out).unwrap();
        if container == Container::Flac {
            check(&out, TagType::VorbisComments);
            continue;
        }
        check(&out, TagType::Id3v2);
        // Readers are lenient about it, but chunks start on even offsets.
        let bytes = std::fs::read(&out).unwrap();
        let id3 = bytes.windows(4).rposition(|w| w.eq_ignore_ascii_case(b"ID3 ")).unwrap();
        assert_eq!(id3 % 2, 0, "{container:?}'s tag chunk is at {id3}");
    }
    // An ID3v2 tag going to ID3v2 keeps frames the generic tag has no field for.
    let aiff = dir.path().join("copy.aiff");
    let mut id3 = Id3v2Tag::from(tagged(&aiff));
    id3.insert(Frame::Private(PrivateFrame::new("rekordbox", vec![1, 2, 3])));
    id3.save_to_path(&aiff, WriteOptions::default()).unwrap();
    let again = dir.path().join("again.wav");
    transcode_quietly(&aiff, &recipe(Container::Wav, 48_000, 24, Dither::None), &again).unwrap();
    check(&again, TagType::Id3v2);
    let private = Id3v2Tag::from(tagged(&again)).into_iter().find_map(|f| match f {
        Frame::Private(p) => Some(p.private_data.into_owned()),
        _ => None,
    });
    assert_eq!(private, Some(vec![1, 2, 3]));
}

/// Dither makes the same copy every time, one step at most from plain rounding.
#[test]
fn dithered_copies_are_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("master.wav");
    wav(&source, 44_100, 24, &tones(44_100, 0.5));
    let make = |name: &str, dither| {
        let out = dir.path().join(name);
        transcode_quietly(&source, &recipe(Container::Aiff, 44_100, 16, dither), &out).unwrap();
        out
    };
    let (once, twice, rounded) =
        (make("a.aiff", Dither::Tpdf), make("b.aiff", Dither::Tpdf), make("c.aiff", Dither::None));
    assert_eq!(std::fs::read(&once).unwrap(), std::fs::read(&twice).unwrap());
    let (dithered, rounded) = (decode(&once).samples, decode(&rounded).samples);
    assert_ne!(dithered, rounded);
    let step = 1 << 16;
    assert!(dithered.iter().zip(&rounded).all(|(a, b)| (a - b).abs() <= step));
}

#[derive(Default)]
struct Recorder(Mutex<Vec<Event>>);

impl loclib_progress::Progress for Recorder {
    fn on_event(&self, event: &Event) {
        self.0.lock().unwrap().push(event.clone());
    }
}

/// Progress adds up to the source's size as its audio is decoded, and cancelling stops the copy.
#[test]
fn progress_and_cancelling() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("long.wav");
    wav(&source, 44_100, 16, &tones(44_100, 10.0));
    let out = dir.path().join("copy.aiff");
    let exact = recipe(Container::Aiff, 44_100, 16, Dither::None);

    let recorder = Recorder::default();
    transcode(&source, &exact, &out, &Ctx::new(&recorder, CancelToken::new())).unwrap();
    let events = recorder.0.into_inner().unwrap();
    let bytes: u64 =
        events.iter().map(|e| if let Event::Bytes { phase: Phase::Copy, bytes } = e { *bytes } else { 0 }).sum();
    assert!(events.len() >= 2, "{events:?}");
    assert_eq!(bytes, std::fs::metadata(&source).unwrap().len(), "the source's size, in step with the audio");

    let cancel = CancelToken::new();
    cancel.cancel();
    let err = transcode(&source, &exact, &out, &Ctx::new(&loclib_progress::NoProgress, cancel)).unwrap_err();
    assert!(matches!(err, Error::Cancelled(_)), "{err}");
}

/// What can't be copied says why, and tags that can't be read only cost a warning.
#[test]
fn failures() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("master.wav");
    wav(&source, 44_100, 16, &tones(44_100, 0.1));
    let out = dir.path().join("copy.aiff");
    let exact = recipe(Container::Aiff, 44_100, 16, Dither::None);

    let odd_depth = transcode_quietly(&source, &recipe(Container::Aiff, 44_100, 20, Dither::None), &out).unwrap_err();
    assert!(matches!(&odd_depth, Error::Invalid(m) if m.contains("aiff-44100-20-round")), "{odd_depth}");
    let no_rate = transcode_quietly(&source, &recipe(Container::Wav, 0, 16, Dither::None), &out).unwrap_err();
    assert!(matches!(no_rate, Error::Invalid(_)));

    let gone = dir.path().join("gone.wav");
    let err = transcode_quietly(&gone, &exact, &out).unwrap_err();
    assert!(matches!(&err, Error::Decode(loclib_mixid::Error::Io { path, .. }) if *path == gone), "{err}");

    // symphonia panics reading a WAV that says 0 Hz.
    let zero = dir.path().join("zero.wav");
    std::fs::write(&zero, wav_of(0, 16, [0; 10].into_iter())).unwrap();
    let err = transcode_quietly(&zero, &exact, &out).unwrap_err();
    assert!(err.to_string().ends_with(loclib_mixid::audio::CRASHED), "{err}");

    let empty = dir.path().join("empty.wav");
    wav(&empty, 44_100, 16, &[]);
    let err = transcode_quietly(&empty, &exact, &out).unwrap_err();
    assert!(err.to_string().ends_with("it holds no audio"), "{err}");

    let nowhere = dir.path().join("no such folder").join("copy.aiff");
    let err = transcode_quietly(&source, &exact, &nowhere).unwrap_err();
    assert!(matches!(&err, Error::Io { path, .. } if *path == nowhere), "{err}");

    // symphonia finds the WAV inside by its content; lofty goes by the name.
    let unnamed = dir.path().join("master.bin");
    std::fs::copy(&source, &unnamed).unwrap();
    let recorder = Recorder::default();
    transcode(&unnamed, &exact, &out, &Ctx::new(&recorder, CancelToken::new())).unwrap();
    let warned = recorder
        .0
        .into_inner()
        .unwrap()
        .iter()
        .any(|e| matches!(e, Event::Warning { message } if message.contains("tags weren't copied")));
    assert!(warned);
    assert_eq!(decode(&out).samples, decode(&source).samples);
}

/// Planned sizes come close to what copies take.
#[test]
fn size_estimates_match_copies() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("master.wav");
    wav(&source, 96_000, 24, &tones(96_000, 1.0));
    let facts = crate::Source { codec: loclib_equipment::Codec::Wav, sample_rate: 96_000, bit_depth: 24, channels: 2 };
    let source_bytes = std::fs::metadata(&source).unwrap().len();
    let out = dir.path().join("copy.aiff");
    let aiff = recipe(Container::Aiff, 48_000, 24, Dither::Tpdf);
    transcode_quietly(&source, &aiff, &out).unwrap();
    let estimate = crate::estimate_size(&facts, source_bytes, 1.0, &aiff) as f64;
    let actual = std::fs::metadata(&out).unwrap().len() as f64;
    assert!((estimate / actual - 1.0).abs() < 0.001, "{estimate} for {actual}");

    let master = dir.path().join("master.flac");
    flac(&master, &noise(44_100 * 2, 16).iter().map(|s| s / 64).collect::<Vec<_>>(), 2, 16, 44_100);
    let facts = crate::Source { codec: loclib_equipment::Codec::Flac, sample_rate: 44_100, bit_depth: 16, channels: 2 };
    let out = dir.path().join("copy.flac");
    let flac_24 = recipe(Container::Flac, 44_100, 24, Dither::None);
    transcode_quietly(&master, &flac_24, &out).unwrap();
    let estimate = crate::estimate_size(&facts, std::fs::metadata(&master).unwrap().len(), 1.0, &flac_24) as f64;
    let actual = std::fs::metadata(&out).unwrap().len() as f64;
    assert!((estimate / actual - 1.0).abs() < 0.6, "{estimate} for {actual}");
}

/// A damaged frame in a master fails its copy (leaving it out would move every cue after it),
/// where decoding to fingerprint it skips the frame.
/// A source under one frame at the recipe's rate has no copy: one would hold no audio, and
/// nothing reads that back (a FLAC of only its header, an AIFF symphonia calls damaged).
#[test]
fn sources_too_short_for_a_frame_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("blip.flac");
    flac(&source, &[100, -100], 2, 16, 96_000);
    for container in Container::ALL {
        let out = dir.path().join(format!("copy.{}", container.extension()));
        let err = transcode_quietly(&source, &recipe(container, 44_100, 16, Dither::None), &out).unwrap_err();
        assert!(
            matches!(&err, Error::TooShort { path, frames: 1, source_rate: 96_000, rate: 44_100 } if path == &source),
            "{err}"
        );
    }
    // A frame at the source's own rate is a frame of the copy.
    let out = dir.path().join("copy.aiff");
    let made = transcode_quietly(&source, &recipe(Container::Aiff, 96_000, 16, Dither::None), &out).unwrap();
    assert_eq!(made, Transcoded { frames: 1, channels: 2 });
    assert_eq!(decode(&out).samples, [100 << 16, -100 << 16]);
}

#[test]
fn a_damaged_frame_fails_the_copy() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("master.flac");
    flac(&source, &noise(3 * 4096 * 2, 16), 2, 16, 44_100);
    let master = std::fs::read(&source).unwrap();
    // Inside the middle frame's audio, and the last one's: their checksums no longer match, and
    // the reader drops them.
    for at in [master.len() / 2, master.len() - 100] {
        let mut bytes = master.clone();
        for b in &mut bytes[at..at + 64] {
            *b ^= 0x5a;
        }
        std::fs::write(&source, bytes).unwrap();
        let out = dir.path().join("copy.aiff");
        let err = transcode_quietly(&source, &recipe(Container::Aiff, 44_100, 16, Dither::None), &out).unwrap_err();
        assert!(matches!(err, Error::Decode(loclib_mixid::Error::Parse { .. })), "{err}");
        let skipped = loclib_mixid::audio::decode(&source, 44_100, &Ctx::silent(), &mut |_| {});
        assert!(skipped.is_ok(), "{skipped:?}");
    }
}

/// Any lossless source, as any recipe a player takes: the copy is at the recipe's rate, as long
/// as the source at it to the nearest frame, and in the recipe's container and depth. One that
/// changes neither rate nor depth (nor dithers) holds the source's samples exactly, one that
/// drops from 24 to 16 bit rounds each to the nearest step, and a copy of a copy made that way
/// is the copy again. A source under a frame at the recipe's rate is refused.
#[hegel::test(test_cases = 30)]
fn copies_hold_the_source_at_the_recipe(tc: hegel::TestCase) {
    use hegel::Generator;
    use hegel::generators as gs;
    let rates = vec![44_100, 48_000, 88_200, 96_000];
    // Masters come at twice and four times those too.
    let source_rates = vec![192_000, 176_400, 96_000, 88_200, 48_000, 44_100];
    let (rate, bits) = (tc.draw(gs::sampled_from(source_rates)), tc.draw(gs::sampled_from(vec![16u16, 24])));
    // Often a single frame, which at a lower rate can be under one.
    let frames = tc.draw(hegel::one_of!(gs::just(1), gs::integers::<usize>().min_value(1).max_value(5000)));
    let dir = tempfile::tempdir().unwrap();
    // The same in both channels, as `wav_of` writes them.
    let mono = noise(frames, bits);
    let samples: Vec<i32> = mono.iter().flat_map(|s| [*s, *s]).collect();
    // flacenc, which writes the FLAC sources, takes up to 96 kHz.
    let source = dir.path().join(if rate > 96_000 || tc.draw(gs::booleans()) { "master.wav" } else { "master.flac" });
    let wav = source.extension().unwrap() == "wav";
    if wav {
        std::fs::write(&source, wav_of(rate, bits, mono.iter().copied())).unwrap();
    } else {
        flac(&source, &samples, 2, usize::from(bits), rate as usize);
    }
    let title = tc.draw(gs::optional(gs::text().alphabet("Night drive é").min_size(1).max_size(9)));
    if let Some(title) = &title {
        let mut tag = Tag::new(if wav { TagType::Id3v2 } else { TagType::VorbisComments });
        tag.set_title(title.clone());
        tag.save_to_path(&source, WriteOptions::default()).unwrap();
    }
    let containers = Container::ALL.to_vec();
    let r = Recipe {
        container: tc.draw(gs::sampled_from(containers.clone()).print_as_debug()),
        sample_rate: tc.draw(gs::sampled_from(rates)),
        bit_depth: tc.draw(gs::sampled_from(vec![16u16, 24])),
        dither: tc.draw(gs::sampled_from(vec![Dither::None, Dither::Tpdf]).print_as_debug()),
    };
    assert!(r.to_string().starts_with(r.container.extension()) && r.to_string().ends_with("-v1"), "{r}");
    let copy = dir.path().join(format!("copy.{}", r.container.extension()));
    let expected = (frames as u64 * u64::from(r.sample_rate) + u64::from(rate) / 2) / u64::from(rate);
    let recorder = Recorder::default();
    let made = transcode(&source, &r, &copy, &Ctx::new(&recorder, CancelToken::new()));
    if expected == 0 {
        let err = made.unwrap_err();
        let want = (frames as u64, rate, r.sample_rate);
        let refused = matches!(&err, Error::TooShort { path, frames, source_rate, rate }
            if path == &source && (*frames, *source_rate, *rate) == want);
        return assert!(refused, "{r}: {err}");
    }
    let made = made.unwrap();
    let bytes: u64 = recorder
        .0
        .into_inner()
        .unwrap()
        .iter()
        .map(|e| if let Event::Bytes { bytes, .. } = e { *bytes } else { 0 })
        .sum();
    assert_eq!(bytes, std::fs::metadata(&source).unwrap().len(), "progress is the source's size");
    // The tags come along, in the form the container holds them.
    let kept =
        lofty::read_from_path(&copy).ok().and_then(|t| t.primary_tag().and_then(|t| t.title().map(|t| t.into_owned())));
    assert_eq!(kept, title, "{r}");
    assert_eq!(made, Transcoded { frames: expected, channels: 2 }, "{r}");
    let back = decode(&copy);
    assert_eq!((back.rate, back.channels, back.samples.len() as u64), (r.sample_rate, 2, 2 * expected), "{r}");
    if r.container == Container::Aiff {
        assert_eq!(aiff_frames(&copy), Some(expected));
    }
    let shift = 32 - u32::from(bits);
    if r.sample_rate == rate && r.dither == Dither::None {
        let step = 1i64 << (32 - u32::from(r.bit_depth));
        for (got, s) in back.samples.iter().zip(&samples) {
            let full = i64::from(*s) << shift;
            let nearest = ((full as f64 / step as f64).round() as i64)
                .clamp(i64::from(i32::MIN) / step, i64::from(i32::MAX) / step)
                * step;
            assert_eq!(i64::from(*got), nearest, "{r}");
        }
    }
    // Again, changing only the container: the copy's samples exactly.
    let again = Recipe { container: tc.draw(gs::sampled_from(containers).print_as_debug()), dither: Dither::None, ..r };
    let second = dir.path().join(format!("again.{}", again.container.extension()));
    transcode_quietly(&copy, &again, &second).unwrap();
    assert_eq!(decode(&second).samples, back.samples, "{r} then {again}");
}

/// A recipe no player takes (a depth other than 16 or 24 bit, an unknown rate) is refused
/// before anything is written, and a source with no audio has no copy.
#[hegel::test(test_cases = 30)]
fn recipes_no_player_takes_are_refused(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let bit_depth = tc.draw(gs::integers::<u16>().max_value(40));
    let sample_rate = tc.draw(gs::sampled_from(vec![0, 44_100]));
    tc.assume(!matches!(bit_depth, 16 | 24) || sample_rate == 0);
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("copy.aiff");
    let r = Recipe { container: Container::Aiff, sample_rate, bit_depth, dither: Dither::None };
    let err = transcode_quietly(&dir.path().join("master.wav"), &r, &out).unwrap_err();
    assert!(matches!(&err, Error::Invalid(m) if m.contains(&r.to_string())), "{err}");
    assert!(!out.exists());
    let empty = dir.path().join("empty.wav");
    std::fs::write(&empty, wav_of(44_100, 16, Vec::new().into_iter())).unwrap();
    let playable = Recipe { sample_rate: 44_100, bit_depth: tc.draw(gs::sampled_from(vec![16u16, 24])), ..r };
    let err = transcode_quietly(&empty, &playable, &out).unwrap_err();
    assert!(
        matches!(&err, Error::Decode(loclib_mixid::Error::Parse { message, .. }) if message == "it holds no audio"),
        "{err}"
    );
}
