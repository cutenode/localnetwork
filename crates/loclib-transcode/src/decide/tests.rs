use super::*;
use hegel::Generator;
use hegel::generators as gs;
use loclib_equipment::{Audio, Exfat, Kind, MODELS, Spec, find};

fn model(name: &str) -> &'static Model {
    find(name).unwrap()
}

fn source(codec: Codec, sample_rate: u32, bit_depth: u16) -> Source {
    Source { codec, sample_rate, bit_depth, channels: 2 }
}

fn recipe(container: Container, sample_rate: u32, bit_depth: u16, dither: Dither) -> Decision {
    Decision::Transcode(Recipe { container, sample_rate, bit_depth, dither })
}

/// What real players get, from loclib-equipment's table.
#[test]
fn decisions_for_real_players() {
    let (nxs2, nxs, cdj3000, omnis, xz) =
        (model("CDJ-2000NXS2"), model("CDJ-2000NXS"), model("CDJ-3000"), model("OMNIS-DUO"), model("XDJ-XZ"));
    let lossless = Options::default();
    let decode = Options { decode_lossy: true, ..Options::default() };
    let flac = Options { container: Container::Flac, ..Options::default() };
    let cases = [
        // The NXS2 plays FLAC up to 96 kHz; the NXS plays no FLAC and nothing over 48 kHz.
        (source(Codec::Flac, 96_000, 24), vec![nxs2], lossless, Decision::Copy),
        (source(Codec::Flac, 96_000, 24), vec![nxs], lossless, recipe(Container::Aiff, 48_000, 24, Dither::Tpdf)),
        (source(Codec::Flac, 88_200, 24), vec![nxs2, nxs], lossless, recipe(Container::Aiff, 44_100, 24, Dither::Tpdf)),
        (
            source(Codec::Flac, 44_100, 16),
            vec![cdj3000, nxs],
            lossless,
            recipe(Container::Aiff, 44_100, 16, Dither::None),
        ),
        (source(Codec::Flac, 192_000, 24), vec![nxs2], lossless, recipe(Container::Aiff, 96_000, 24, Dither::Tpdf)),
        (source(Codec::Flac, 176_400, 24), vec![cdj3000], flac, recipe(Container::Flac, 88_200, 24, Dither::Tpdf)),
        (source(Codec::Flac, 96_000, 24), vec![omnis], flac, recipe(Container::Flac, 48_000, 24, Dither::Tpdf)),
        // FLAC asked for but the NXS doesn't play it: AIFF, the next in line.
        (source(Codec::Flac, 48_000, 24), vec![nxs], flac, recipe(Container::Aiff, 48_000, 24, Dither::None)),
        // 32-bit masters come down to 24.
        (source(Codec::Wav, 44_100, 32), vec![cdj3000], lossless, recipe(Container::Aiff, 44_100, 24, Dither::Tpdf)),
        // The XDJ-XZ lists no ALAC.
        (source(Codec::Alac, 44_100, 16), vec![xz], lossless, recipe(Container::Aiff, 44_100, 16, Dither::None)),
        // MPEG-2 MP3s play on the NXS2, not the CDJ-3000; decoding one is opt-in.
        (source(Codec::Mp3, 22_050, 16), vec![nxs2], lossless, Decision::Copy),
        (source(Codec::Mp3, 22_050, 16), vec![cdj3000], lossless, Decision::Unplayable(Reason::Lossy)),
        (source(Codec::Mp3, 22_050, 0), vec![cdj3000], decode, recipe(Container::Aiff, 44_100, 16, Dither::Tpdf)),
        (source(Codec::Aac, 32_000, 16), vec![cdj3000], decode, recipe(Container::Aiff, 48_000, 16, Dither::Tpdf)),
        // An unknown depth (an ALAC row) gets the highest the players take, dithered rather
        // than called exact; an unknown rate can't be planned.
        (source(Codec::Alac, 44_100, 0), vec![xz], lossless, recipe(Container::Aiff, 44_100, 24, Dither::Tpdf)),
        (source(Codec::Alac, 0, 16), vec![xz], lossless, Decision::Unplayable(Reason::UnknownRate)),
        // A mixer plays nothing from a drive and isn't held against a file.
        (source(Codec::Flac, 96_000, 24), vec![nxs2, model("DJM-V10")], lossless, Decision::Copy),
        (source(Codec::Flac, 96_000, 24), vec![], lossless, Decision::Copy),
    ];
    for (source, targets, options, expected) in cases {
        let names: Vec<&str> = targets.iter().map(|m| m.name).collect();
        assert_eq!(decide(&source, &targets, &options), expected, "{source:?} on {names:?} with {options:?}");
    }
}

/// A player that plays no lossless format at all leaves nothing to convert to.
#[test]
fn no_lossless_format() {
    static LOSSY_ONLY: Model = Model {
        name: "LOSSY-1",
        kind: Kind::Player,
        export_pdb: Spec { value: true, inferred: false },
        onelibrary: Spec { value: false, inferred: false },
        exfat: Spec { value: Exfat::No, inferred: false },
        audio: &[Audio { codec: Codec::Mp3, rates: &[44_100], depths: &[16], inferred: false }],
        audio_inferred: false,
        notes: &[],
        source: "a test",
    };
    let d = decide(&source(Codec::Flac, 44_100, 16), &[&LOSSY_ONLY], &Options::default());
    assert_eq!(d, Decision::Unplayable(Reason::NoLosslessFormat));
}

#[test]
fn recipes_print_stably() {
    let r = Recipe { container: Container::Aiff, sample_rate: 48_000, bit_depth: 24, dither: Dither::Tpdf };
    assert_eq!(r.to_string(), "aiff-48000-24-tpdf-v1");
    let r = Recipe { container: Container::Flac, sample_rate: 44_100, bit_depth: 16, dither: Dither::None };
    assert_eq!(r.to_string(), "flac-44100-16-round-v1");
    assert_eq!(Container::Wav.extension(), "wav");
}

#[test]
fn size_estimates() {
    let aiff_48 = Recipe { container: Container::Aiff, sample_rate: 48_000, bit_depth: 24, dither: Dither::Tpdf };
    let flac_48 = Recipe { container: Container::Flac, ..aiff_48 };
    // A minute of 24/96 stereo WAV with a 44-byte header: the copy's samples plus that header.
    let wav = source(Codec::Wav, 96_000, 24);
    assert_eq!(estimate_size(&wav, 34_560_044, 60.0, &aiff_48), 17_280_044);
    // A FLAC master at 60% keeps that ratio as FLAC, and is its samples as AIFF.
    let flac = source(Codec::Flac, 96_000, 24);
    assert_eq!(estimate_size(&flac, 20_736_000, 60.0, &flac_48), 10_368_000);
    assert_eq!(estimate_size(&flac, 20_736_000, 60.0, &aiff_48), 17_280_000);
    // A lossy source's size says nothing about its samples.
    let aac = source(Codec::Aac, 48_000, 0);
    let aiff_16 = Recipe { bit_depth: 16, ..aiff_48 };
    assert_eq!(estimate_size(&aac, 1_920_000, 60.0, &aiff_16), 11_520_000);
}

fn any_source() -> impl hegel::PrintableGenerator<Source> {
    let rates = vec![0, 8_000, 16_000, 22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 384_000];
    hegel::compose!(|tc| {
        Source {
            codec: tc.draw(gs::sampled_from(Codec::ALL.to_vec()).print_as_debug()),
            sample_rate: tc
                .draw(hegel::one_of!(gs::sampled_from(rates.clone()), gs::integers::<u32>().max_value(800_000))),
            bit_depth: tc.draw(gs::sampled_from(vec![0, 8, 16, 20, 24, 32])),
            channels: tc.draw(gs::integers::<u16>().min_value(1).max_value(8)),
        }
    })
    .print_as_debug()
}

/// Whatever the file, players and options: a file is copied only when every player plays it;
/// a copy is lossless, in a container, rate and depth every player takes, made of a lossy
/// file only when asked, keeping the source's rate when the players take it in that
/// container, and rounded without dither only when nothing is lost.
#[hegel::test(test_cases = 1000)]
fn decisions_hold_for_any_file_and_players(tc: hegel::TestCase) {
    let source = tc.draw(any_source());
    let picks = tc.draw(gs::vecs(gs::integers::<usize>().max_value(MODELS.len() - 1)).max_size(5));
    let targets: Vec<&Model> = picks.iter().map(|&i| &MODELS[i]).collect();
    let options = Options {
        container: tc.draw(gs::sampled_from(Container::ALL.to_vec()).print_as_debug()),
        decode_lossy: tc.draw(gs::booleans()),
    };
    let players = targets.iter().filter(|m| !m.is_mixer());
    let plays_everywhere =
        players.clone().all(|m| m.plays(source.codec, source.sample_rate, source.bit_depth).result.is_ok());
    let lossy = !source.codec.lossless();
    match decide(&source, &targets, &options) {
        Decision::Copy => assert!(plays_everywhere),
        Decision::Transcode(r) => {
            assert!(!plays_everywhere);
            assert!(!lossy || options.decode_lossy, "a lossy file was transcoded without asking");
            assert!(r.container.codec().lossless());
            assert!(players.clone().all(|m| m.plays(r.container.codec(), r.sample_rate, r.bit_depth).result.is_ok()));
            let takes_source_rate = players
                .clone()
                .all(|m| m.audio(r.container.codec()).is_some_and(|a| a.rates.contains(&source.sample_rate)));
            assert_eq!(r.sample_rate == source.sample_rate, takes_source_rate, "{r:?}");
            if r.dither == Dither::None {
                assert!(!lossy && r.sample_rate == source.sample_rate && r.bit_depth >= source.bit_depth);
                assert!(source.bit_depth > 0 && source.sample_rate > 0, "exact needs known depth and rate");
            }
        }
        Decision::Unplayable(Reason::Lossy) => assert!(lossy && !options.decode_lossy && !plays_everywhere),
        Decision::Unplayable(Reason::UnknownRate) => assert_eq!(source.sample_rate, 0),
        Decision::Unplayable(Reason::NoLosslessFormat) => panic!("every player plays some lossless format"),
    }
}

/// For a source whose size goes with its length (a header and its samples, a FLAC
/// compressing them at some ratio, a lossy file at some bitrate), a longer copy is estimated
/// no smaller; and a WAV or AIFF copy of a WAV or AIFF that changes nothing but the container
/// is the size of the source.
#[hegel::test(test_cases = 300)]
fn size_estimates_grow_with_length(tc: hegel::TestCase) {
    let source = tc.draw(any_source());
    let recipe = Recipe {
        container: tc.draw(gs::sampled_from(Container::ALL.to_vec()).print_as_debug()),
        sample_rate: tc.draw(gs::sampled_from(vec![44_100, 48_000, 96_000])),
        bit_depth: tc.draw(gs::sampled_from(vec![16, 24])),
        dither: Dither::Tpdf,
    };
    let header = tc.draw(gs::integers::<u64>().max_value(1 << 20));
    let ratio = tc.draw(gs::floats::<f64>().min_value(0.05).max_value(1.0));
    let master =
        |secs: f64| secs * f64::from(source.sample_rate) * f64::from(source.channels) * f64::from(source.bit_depth / 8);
    let bytes = |secs: f64| match source.codec {
        Codec::Wav | Codec::Aiff => master(secs).ceil() as u64 + header,
        Codec::Flac | Codec::Alac => (master(secs) * ratio) as u64,
        _ => (secs * 40_000.0 * ratio) as u64,
    };
    let (a, b) = (
        tc.draw(gs::floats::<f64>().min_value(0.0).max_value(3600.0)),
        tc.draw(gs::floats::<f64>().min_value(0.0).max_value(3600.0)),
    );
    let (short, long) = (a.min(b), a.max(b));
    let estimate = |secs: f64, r: &Recipe| estimate_size(&source, bytes(secs), secs, r);
    // A byte either way: the source's size is whole bytes.
    assert!(estimate(short, &recipe) <= estimate(long, &recipe) + 1, "{short} s and {long} s");
    if matches!(source.codec, Codec::Wav | Codec::Aiff) && recipe.container != Container::Flac {
        let same = Recipe { sample_rate: source.sample_rate, bit_depth: source.bit_depth, ..recipe };
        assert!(estimate(a, &same).abs_diff(bytes(a)) <= 1, "{} for a {}-byte source", estimate(a, &same), bytes(a));
    }
}

/// Players made up of any formats, rates and depths: a copy is planned in a lossless container
/// every player lists with a rate and a depth they all share, the one asked for when it's such
/// a container; when there's none, there's no lossless format to copy to.
#[hegel::test(test_cases = 500)]
fn made_up_players_share_a_format_or_have_none(tc: hegel::TestCase) {
    let codecs = vec![Codec::Aiff, Codec::Wav, Codec::Flac, Codec::Mp3];
    let players: Vec<&'static Model> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
        .map(|_| {
            let audio: Vec<Audio> = tc
                .draw(gs::samples(codecs.clone()).without_replacement().max_size(4).print_as_debug())
                .into_iter()
                .map(|codec| Audio {
                    codec,
                    rates: tc.draw(gs::samples(vec![44_100, 48_000, 96_000]).without_replacement().max_size(3)).leak(),
                    depths: tc.draw(gs::samples(vec![16, 24]).without_replacement().max_size(2)).leak(),
                    inferred: false,
                })
                .collect();
            &*Box::leak(Box::new(Model {
                name: "MADE-UP",
                kind: Kind::Player,
                export_pdb: Spec { value: true, inferred: false },
                onelibrary: Spec { value: false, inferred: false },
                exfat: Spec { value: Exfat::No, inferred: false },
                audio: audio.leak(),
                audio_inferred: false,
                notes: &[],
                source: "a test",
            }))
        })
        .collect();
    let source = Source {
        codec: tc.draw(gs::sampled_from(vec![Codec::Flac, Codec::Wav, Codec::Aiff]).print_as_debug()),
        sample_rate: tc.draw(gs::sampled_from(vec![44_100, 48_000, 96_000])),
        bit_depth: tc.draw(gs::sampled_from(vec![16, 24])),
        channels: 2,
    };
    let options =
        Options { container: tc.draw(gs::sampled_from(Container::ALL.to_vec()).print_as_debug()), decode_lossy: false };
    let shared = |c: Container| {
        let lists: Vec<&Audio> = players.iter().filter_map(|m| m.audio(c.codec())).collect();
        lists.len() == players.len()
            && lists[0].rates.iter().any(|r| lists.iter().all(|a| a.rates.contains(r)))
            && lists[0].depths.iter().any(|d| lists.iter().all(|a| a.depths.contains(d)))
    };
    let plays = |codec, rate, depth| players.iter().all(|m| m.plays(codec, rate, depth).result.is_ok());
    let copies = plays(source.codec, source.sample_rate, source.bit_depth);
    match decide(&source, &players, &options) {
        Decision::Copy => assert!(copies),
        Decision::Transcode(r) => {
            assert!(!copies && shared(r.container) && plays(r.container.codec(), r.sample_rate, r.bit_depth), "{r:?}");
            assert!(r.container == options.container || !shared(options.container), "{r:?}");
        }
        Decision::Unplayable(Reason::NoLosslessFormat) => {
            assert!(!copies && Container::ALL.iter().all(|c| !shared(*c)));
        }
        other => panic!("{other:?} for a lossless source at a known rate"),
    }
}
