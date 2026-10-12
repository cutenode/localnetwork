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
mod tests;
