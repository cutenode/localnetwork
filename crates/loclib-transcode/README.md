# loclib-transcode

Player-safe copies of a lossless DJ library. A DJ keeps masters in whatever format and resolution
they bought them in (FLAC at 24/96, ALAC, 32-bit WAV); a drive for a booth's CDJs needs files those
players load. This crate decides, per file, whether every target player plays it as it is, and if
not, makes a lossless copy they do: AIFF by default (rekordbox and every player read its ID3 tags),
or WAV or FLAC. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which uses it
when packing a drive, but all it needs is audio files and
[loclib-equipment](https://github.com/cutenode/localnetwork/tree/main/crates/loclib-equipment)'s table of what each player reads.

```rust
use loclib_equipment::{Codec, find};
use loclib_transcode::{Container, Decision, Dither, Options, Recipe, Source, decide};

let master = Source { codec: Codec::Flac, sample_rate: 96_000, bit_depth: 24, channels: 2 };
// The NXS2 plays FLAC up to 96 kHz; the NXS plays no FLAC, and WAV and AIFF up to 48 kHz.
let booth = [find("CDJ-2000NXS2").unwrap(), find("CDJ-2000NXS").unwrap()];
let Decision::Transcode(recipe) = decide(&master, &booth, &Options::default()) else { panic!() };
assert_eq!(recipe, Recipe { container: Container::Aiff, sample_rate: 48_000, bit_depth: 24, dither: Dither::Tpdf });
// Store this with the master's content hash: a copy is current while both still match.
assert_eq!(recipe.to_string(), "aiff-48000-24-tpdf-v1");

assert_eq!(decide(&master, &booth[..1], &Options::default()), Decision::Copy);
```

Then make the copy, into a temp file renamed into place once it's done:

```rust,no_run
use loclib_progress::Ctx;
use loclib_transcode::{Container, Dither, Recipe, transcode};

# fn main() -> loclib_transcode::Result<()> {
let recipe = Recipe { container: Container::Aiff, sample_rate: 48_000, bit_depth: 24, dither: Dither::Tpdf };
let copy = transcode("master.flac".as_ref(), &recipe, ".copy.aiff.tmp".as_ref(), &Ctx::silent())?;
println!("{} frames of {} channels", copy.frames, copy.channels);
std::fs::rename(".copy.aiff.tmp", "copy.aiff").unwrap();
# Ok(())
# }
```

## What it decides

- **Copy** when every target plays the file as it is (mixers are left out: they play nothing from
  a drive).
- **Transcode** a lossless file to the first container every target plays: the one asked for
  (AIFF unless `Options::container` says otherwise), then AIFF, WAV and FLAC. The rate and depth
  stay when the targets take them; otherwise it's the highest they take below the source's, in
  the same family where there is one (176.4 and 88.2 kHz go to 88.2 or 44.1, 192 and 96 to 96 or
  48), else the lowest above it. Dither is added whenever the samples change: a new rate, a lower
  depth or a lossy source. A copy that only changes the container is bit for bit the master.
- **Unplayable** for a lossy file (MP3, AAC) a target can't play, unless `Options::decode_lossy`
  asks for it to be decoded into a lossless container: bigger, but no further loss. Lossy files
  are never encoded lossy again. There's no MP3 or AAC encoder here: LAME is LGPL, this crate
  takes permissive licences only, and a lossy encoder's delay would shift every cue.

`estimate_size` says about how big a copy will be, for planning a drive's space.

## How copies are made

Audio is decoded through symphonia (as in
[loclib-mixid](https://github.com/cutenode/localnetwork/tree/main/crates/loclib-mixid)) at the
source's own rate, streamed in packets so a long 24/96 file never sits in memory, resampled with
[rubato](https://crates.io/crates/rubato)'s sinc resampler when the rate changes, rounded to the
copy's depth with TPDF dither of ±1 LSB from a fixed seed (the same master and recipe always make
the same bytes), and written as AIFF or WAV by hand, or as FLAC through
[flacenc](https://crates.io/crates/flacenc). Tags and artwork come along through
[lofty](https://crates.io/crates/lofty): ID3v2.3 in AIFF and WAV, Vorbis comments and pictures in
FLAC.

rekordbox keeps cue points and beat grids in milliseconds from the start, so a copy has to keep
time. The resampler's delay is taken off the front (a little silence fed in first makes it a
whole number of frames) and its tail is flushed: the copy's first sample is the master's, to
within a hundredth of a frame (rubato's filter sits 1/128 of a source sample early), and the copy
is as long as the master to the nearest frame at the new rate. The tests check both against the
same tones made at the new rate. A master too short to fill one frame at the new rate (a single
sample at 96 kHz, copied at 44.1) is refused with `Error::TooShort`: a copy of no frames is a
file nothing reads back.

Progress is reported as the bytes of the source read, through loclib-progress's `Ctx`, which also
cancels a copy between packets.

## Recipes

`Recipe`'s `Display` form (`aiff-48000-24-tpdf-v1`) names the container, rate, depth, dither and
`RECIPE_VERSION`, which goes up whenever the same recipe would make different bytes. A caller
storing it with the master's content hash knows a copy needs redoing when either changes.

## What it doesn't do

- Look at drives, or decide which players a drive is for. You bring the file's format (from its
  headers, say) and the players.
- Encode lossy formats, or downmix channels.
- Keep ID3v2's binary frames (Serato's markers, say) when the tag changes format: an AIFF master's
  ID3v2 tag is copied whole to an AIFF or WAV copy, but going to FLAC's Vorbis comments keeps what
  both formats hold.

## Licence

MIT.
