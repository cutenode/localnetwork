# loclib-mixid

Finds which known tracks a recorded DJ mix plays: when each comes in and goes out, which part of
the track was used, at what speed, with or without key lock, and how sure the match is. It was
split out of [locallibrary](https://github.com/cutenode/locallibrary), which matches a set
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
