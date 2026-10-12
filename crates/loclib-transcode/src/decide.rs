//! Which copy a set of players needs: the file as it is, a lossless copy at a rate and depth they
//! all play, or none.

use std::fmt;

use loclib_equipment::{Codec, Model};
use serde::{Deserialize, Serialize};

/// Bumped whenever [`transcode`](crate::transcode) would write different audio for the same
/// [`Recipe`] (a new resampler setting, dither seed or container layout), so stored copies made
/// by an older version are redone.
pub const RECIPE_VERSION: u32 = 1;

/// What a source file holds, as its headers say (loclib-equipment's [`Codec`] names the format).
/// A rate or depth of 0 means unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    pub codec: Codec,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub channels: u16,
}

/// A lossless container a copy is written in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Container {
    /// rekordbox and every player read its ID3 tags.
    #[default]
    Aiff,
    Wav,
    Flac,
}

impl Container {
    /// In the order tried after the preferred one.
    pub const ALL: [Container; 3] = [Container::Aiff, Container::Wav, Container::Flac];

    pub fn codec(self) -> Codec {
        match self {
            Container::Aiff => Codec::Aiff,
            Container::Wav => Codec::Wav,
            Container::Flac => Codec::Flac,
        }
    }

    /// The copy's file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Container::Aiff => "aiff",
            Container::Wav => "wav",
            Container::Flac => "flac",
        }
    }
}

/// How a copy's samples are rounded to its bit depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dither {
    /// Plain rounding, exact when nothing was resampled and the depth didn't drop.
    None,
    /// Triangular dither of ±1 LSB, seeded so the same source always makes the same copy.
    Tpdf,
}

/// How to make a copy. Its [`Display`](fmt::Display) form (`aiff-48000-24-tpdf-v1`) is stable:
/// store it with the source's content hash, and a copy whose stored form differs from today's
/// recipe, or whose master changed, needs making again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Recipe {
    pub container: Container,
    pub sample_rate: u32,
    /// 16 or 24.
    pub bit_depth: u16,
    pub dither: Dither,
}

impl fmt::Display for Recipe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dither = match self.dither {
            Dither::None => "round",
            Dither::Tpdf => "tpdf",
        };
        let Recipe { container, sample_rate, bit_depth, .. } = self;
        write!(f, "{}-{sample_rate}-{bit_depth}-{dither}-v{RECIPE_VERSION}", container.extension())
    }
}

/// Choices [`decide`] leaves to the caller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// The container tried first; the others follow in [`Container::ALL`]'s order.
    pub container: Container,
    /// Decode a lossy file some target can't play into a lossless container: a bigger file, but
    /// no further loss. Lossy files are never encoded lossy again.
    pub decode_lossy: bool,
}

/// What a set of players needs for one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Every target plays the file as it is.
    Copy,
    Transcode(Recipe),
    Unplayable(Reason),
}

/// Why a file can't be made playable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// It's lossy, some target doesn't play it, and [`Options::decode_lossy`] is off.
    Lossy,
    /// No lossless container plays on every target.
    NoLosslessFormat,
    /// Its sample rate isn't known, so no rate the targets take can be chosen for a copy.
    UnknownRate,
}

/// What `targets` need to play `source`. Mixers are ignored: they play nothing from a drive.
///
/// A copy keeps the source's rate and depth when the targets take them, else takes the highest
/// they do below it, in the same family (88.2 kHz becomes 44.1, 96 becomes 48) where there's
/// one, else the lowest above it. Lossy sources count as 16 bit, what their decoders put out, and
/// a lossless one of unknown depth gets the deepest the targets take, dithered. A source of
/// unknown rate that needs a copy can't have one planned.
pub fn decide(source: &Source, targets: &[&Model], options: &Options) -> Decision {
    let players: Vec<&Model> = targets.iter().copied().filter(|m| !m.is_mixer()).collect();
    if players.iter().all(|m| m.plays(source.codec, source.sample_rate, source.bit_depth).result.is_ok()) {
        return Decision::Copy;
    }
    let lossy = !source.codec.lossless();
    if lossy && !options.decode_lossy {
        return Decision::Unplayable(Reason::Lossy);
    }
    if source.sample_rate == 0 {
        return Decision::Unplayable(Reason::UnknownRate);
    }
    // An unknown depth (0) is taken as the deepest: nothing is lost whatever it was.
    let source_depth = match (lossy, source.bit_depth) {
        (true, _) => 16,
        (false, 0) => u16::MAX,
        (false, depth) => depth,
    };
    let in_family = |rate: u32| rate.is_multiple_of(11_025) == source.sample_rate.is_multiple_of(11_025);
    let containers =
        std::iter::once(options.container).chain(Container::ALL.into_iter().filter(|c| *c != options.container));
    for container in containers {
        let rates = common(&players, container.codec(), |a| a.rates.to_vec());
        let depths = common(&players, container.codec(), |a| a.depths.iter().map(|&d| u32::from(d)).collect());
        let rate = pick(&rates, source.sample_rate, in_family);
        let depth = pick(&depths, source_depth.into(), |_| true);
        let (Some(sample_rate), Some(depth)) = (rate, depth) else { continue };
        let bit_depth = depth as u16;
        let exact =
            !lossy && source.bit_depth > 0 && sample_rate == source.sample_rate && bit_depth >= source.bit_depth;
        let dither = if exact { Dither::None } else { Dither::Tpdf };
        return Decision::Transcode(Recipe { container, sample_rate, bit_depth, dither });
    }
    Decision::Unplayable(Reason::NoLosslessFormat)
}

/// The values every player lists for `codec` (`values` reads them from its entry); empty when
/// one doesn't play it.
fn common(players: &[&Model], codec: Codec, values: impl Fn(&loclib_equipment::Audio) -> Vec<u32>) -> Vec<u32> {
    let mut lists = players.iter().map(|m| m.audio(codec).map(&values).unwrap_or_default());
    let first = lists.next().unwrap_or_default();
    lists.fold(first, |acc, list| acc.into_iter().filter(|v| list.contains(v)).collect())
}

/// The best of `supported` for a source at `want`: in its family first, then `want` itself or
/// the highest below it, then the lowest above it.
fn pick(supported: &[u32], want: u32, in_family: impl Fn(u32) -> bool) -> Option<u32> {
    supported
        .iter()
        .copied()
        .max_by_key(|&v| (in_family(v), v <= want, if v <= want { i64::from(v) } else { -i64::from(v) }))
}

/// About how many bytes the copy `recipe` makes of `source` will take, for planning a drive's
/// space. `source_bytes` is the source file's size and `duration_secs` its length.
///
/// WAV and AIFF copies are their samples, plus whatever an uncompressed source holds besides
/// samples (headers, tags, art). A FLAC copy of FLAC or ALAC compresses about as well as its
/// master did; a FLAC copy of anything else is given as its samples, which it comes in under.
/// Tags and art of compressed sources copied to WAV or AIFF aren't counted.
pub fn estimate_size(source: &Source, source_bytes: u64, duration_secs: f64, recipe: &Recipe) -> u64 {
    let samples =
        |rate: u32, depth: u16| duration_secs * f64::from(rate) * f64::from(source.channels) * f64::from(depth / 8);
    let copy = samples(recipe.sample_rate, recipe.bit_depth);
    let master = samples(source.sample_rate, source.bit_depth);
    let estimate = match source.codec {
        Codec::Wav | Codec::Aiff => copy + (source_bytes as f64 - master).max(0.0),
        Codec::Flac | Codec::Alac if recipe.container == Container::Flac && master > 0.0 => {
            copy * source_bytes as f64 / master
        }
        _ => copy,
    };
    estimate.ceil() as u64
}

#[cfg(test)]
mod tests {
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
            (
                source(Codec::Flac, 88_200, 24),
                vec![nxs2, nxs],
                lossless,
                recipe(Container::Aiff, 44_100, 24, Dither::Tpdf),
            ),
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
            (
                source(Codec::Wav, 44_100, 32),
                vec![cdj3000],
                lossless,
                recipe(Container::Aiff, 44_100, 24, Dither::Tpdf),
            ),
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
                assert!(
                    players.clone().all(|m| m.plays(r.container.codec(), r.sample_rate, r.bit_depth).result.is_ok())
                );
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
        let master = |secs: f64| {
            secs * f64::from(source.sample_rate) * f64::from(source.channels) * f64::from(source.bit_depth / 8)
        };
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
            assert!(
                estimate(a, &same).abs_diff(bytes(a)) <= 1,
                "{} for a {}-byte source",
                estimate(a, &same),
                bytes(a)
            );
        }
    }
}
