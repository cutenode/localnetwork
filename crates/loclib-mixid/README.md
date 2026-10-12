# loclib-mixid

Finds which known tracks a recorded DJ mix plays: when each comes in and goes out, which part of
the track was used, at what speed, with or without key lock, and how sure the match is. It was
split out of [locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which matches a set
against the tracks on a rekordbox USB drive, but all it needs is audio files and a recording.

It follows Dan Ellis's audfprint, in the line of Wang's Shazam paper. Audio is decoded through
symphonia (MP3, AAC and ALAC in `.m4a`, FLAC, WAV, AIFF), averaged to mono and resampled to
11,025 Hz as it streams, so an hour-long recording never sits in memory at full rate. Spectral
peaks are paired into landmark hashes, and every hash in the mix is looked up through a range of
playback speeds, both time-stretched (key lock) and pitched with the tempo (no key lock). Votes
that agree on where a track sits in the mix become a play, refined with a line fitted through
them.

```rust,no_run
use loclib_mixid::{LibraryIndex, fingerprint_file, listen, matcher};
use loclib_progress::Ctx;

# fn main() -> loclib_mixid::Result<()> {
let ctx = Ctx::silent();
// Your tracks, each with an id of your choosing. Keep the prints (`TrackPrint::encode`):
// decoding is the slow part.
let tracks = [(1, "one.flac"), (2, "two.mp3"), (3, "three.wav")];
let prints = tracks
    .iter()
    .map(|&(id, path)| Ok((id, fingerprint_file(path.as_ref(), &ctx)?)))
    .collect::<loclib_mixid::Result<Vec<_>>>()?;
let index = LibraryIndex::build(prints);

let (_, heard) = listen("set.wav".as_ref(), &ctx)?;
// Search playback speeds within ±8%.
for play in matcher::find(&index, &heard.peaks, heard.frames, 0.08, None, &ctx)? {
    println!(
        "track {} from {:.0}s to {:.0}s, {:+.1}%, confidence {:.2}",
        index.track_ids[play.slot as usize],
        play.start,
        play.end,
        (play.tempo - 1.0) * 100.0,
        play.confidence(),
    );
}
# Ok(())
# }
```

`matcher::find` reports `Phase::Match` progress through the `Ctx`, one item per ~4 s slice of
the mix. Everything checks the `Ctx` for cancellation; reporting the other phases
(`Fingerprint`, `Index`, `Listen`) is left to the caller, which knows how many files it has.

`matcher::align` compares two tracks' prints at their original speed: the offset most of their
landmarks agree on and how much of each is heard there. Two encodings of one recording (a FLAC
and an MP3 made from it) line up over all of both; an edit with a stretch cut out lines up only in
part at any one offset.

```rust,no_run
# fn main() -> loclib_mixid::Result<()> {
use loclib_mixid::{fingerprint_file, matcher};
use loclib_progress::Ctx;

let ctx = Ctx::silent();
let (flac, mp3) = (fingerprint_file("a.flac".as_ref(), &ctx)?, fingerprint_file("a.mp3".as_ref(), &ctx)?);
if let Some(a) = matcher::align(&flac, &mp3).filter(|a| a.coverage.0 > 0.8 && a.coverage.1 > 0.8) {
    // An MP3's encoder delay: about 25 ms later.
    println!("the same recording, {:+.0} ms", a.offset * 1000.0);
}
# Ok(())
# }
```

`refine` takes that offset to a fraction of a sample, for moving cue points and beat grids,
which rekordbox keeps in whole milliseconds. It decodes only the first minute or so of each file,
at 44.1 kHz, and cross-correlates a few loud stretches of one with the other within 10 ms of the
estimate. Below `REFINE_MIN_CORRELATION` the two aren't the same audio there, and the offset
shouldn't be used.

```rust,no_run
# fn main() -> loclib_mixid::Result<()> {
# let a = loclib_mixid::matcher::Alignment { offset: 0.025, drift: 0.0, drift_noise: 0.0, pairs: 900, shifts: 900, coverage: (1.0, 1.0) };
use loclib_mixid::{REFINE_MIN_CORRELATION, refine};
use loclib_progress::Ctx;

// `a`: a precise `Alignment` of the two, as above.
let r = refine("a.flac".as_ref(), "a.mp3".as_ref(), a.offset, &Ctx::silent())?;
if r.correlation >= REFINE_MIN_CORRELATION {
    println!("{:+.3} ms", r.offset * 1000.0);
}
# Ok(())
# }
```

## Accuracy and limits

- Speeds are searched within the range given, up to ±25%. Key-locked speeds are searched on a
  coarse grid and pitched ones on a fine one; a tempo measured from a long enough stretch (8 s or
  more) replaces the grid's.
- Key lock is told from which kind of speed voted more; it's `None` when the speed is within
  0.6% of the original or the votes are too even.
- A play needs at least 16 s and enough votes. Shorter matches are taken for a shared sample.
- Two files with the same audio (a re-download, a remaster) both match: the stronger is kept and
  the other is listed in `alternates`. A remix and its original can do the same.
- Loops and cue jumps start a new chain inside one play and are counted in `jumps`.
- `align`'s offset is measured to within 4 ms (frames are 23 ms apart; the mean over many
  matched peaks resolves a fraction of one), on copies shifted by 2 to 70 ms and copies made as
  MP3, AAC and 48 kHz FLAC. `Alignment::is_precise` says whether it was measured from enough
  peaks (100, about 13 s of the test signal), agreeing over the whole track.
- `refine`'s offset is within a microsecond of the truth on synthetic copies shifted by
  fractions of a sample, resampled to 48 kHz, or low-passed and quantised, from an estimate 4 ms
  out. It looks only at the first minute, so an edit later on goes unnoticed.
- `Found::confidence()` comes from vote density and how continuously the track was heard. It is a
  heuristic score, not a probability.
- Hashes shared by too many tracks (a bare kick pattern) are dropped from the index, and peaks
  below ~65 Hz are ignored.

## Features

- `test-util`: synthetic audio and WAV files for tests. Not a stable API.

## What it doesn't do

- Know anything about a DJ library: track names, keys, BPMs, where files live. You give each
  track an id and look your metadata up from it.
- Store prints or indexes on disk. `TrackPrint::encode` and `decode` give bytes to keep;
  where and under which key is up to you.
- Read DJ software's project files or audio editors' projects. Push their audio into a
  `fingerprint::Analyzer` yourself.
- Identify tracks it hasn't been given: there's no online lookup.
- Run in real time. It works on a whole recording.

## Licence

MIT.
