# loclib-analysis

A second opinion on a DJ library's tempos and keys, how loud each track is, and a way to score
both against annotated datasets. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which checks
the BPMs and keys rekordbox stored on a USB drive, but all it needs is audio files.

- `analyze` decodes a track once (through symphonia, like loclib-mixid: MP3, AAC and ALAC in
  `.m4a`, FLAC, WAV, AIFF) and measures what you ask for: tempo, key and a beat grid from
  [stratum-dsp](https://github.com/HLLMR/stratum-dsp); loudness from
  [ebur128](https://github.com/sdroege/ebur128): integrated loudness, loudness range, sample and
  true peak, and runs of clipped samples; and a waveform in three bands (lows, mids and highs, a
  column every 23 ms) from loclib-mixid's frame levels. The `Analysis` serialises, carrying the
  crate's `VERSION`, so you can cache it by the audio's hash and drop it when the version
  changes.
- `tempo` tells whether a stored tempo and an estimate agree or are off by half, double, 2/3 or
  3/2, and which of the two a library's other tempos make more likely.
- `benchmark` reads the GiantSteps key and tempo annotations and scores analyzers the way
  published results are: MIREX's weighted key score, tempo Accuracy1 and Accuracy2, with what
  each got wrong. Fifths count in either direction, as MIREX has scored them since 2017, so
  weighted scores published before then (the 2015 GiantSteps paper's among them) aren't directly
  comparable.

```rust,no_run
use loclib_analysis::{Want, analyze};
use loclib_progress::Ctx;

# fn main() -> loclib_analysis::Result<()> {
let analysis = analyze("track.flac".as_ref(), Want::ALL, &Ctx::silent())?;
if let Some(e) = &analysis.estimate {
    let key = e.key.map_or("no key".into(), |k| k.code());
    println!("{:.1} BPM ({:.2}), {key} ({:.2})", e.bpm, e.bpm_confidence, e.key_confidence);
}
if let Some(l) = &analysis.loudness {
    println!("{:?} LUFS, {:.1} dBTP, {} clipped runs", l.integrated_lufs, l.true_peak_dbtp(), l.clipped_runs);
}
# Ok(())
# }
```

Checking a stored tempo against an estimate:

```rust
use loclib_analysis::tempo::{Plausible, TempoPrior, TempoRelation};

// rekordbox says 87, the estimate 174: an octave apart.
assert_eq!(TempoRelation::between(87.0, 174.0), Some(TempoRelation::Half));
// In a library of drum and bass, 174 is the likelier.
let library = TempoPrior::library([172.0, 174.0, 175.0, 170.0]);
assert_eq!(library.more_plausible(87.0, 174.0), Some(Plausible::Estimate));
```

## Accuracy and limits

- rekordbox's stored values are a good baseline: on GiantSteps it gets about 72% of keys right
  and about 75% of tempos within 4% (89% forgiving half and double time). Tempo mistakes are
  mostly octave errors; key mistakes mostly the relative, parallel or fifth-related key.
- stratum-dsp's accuracy is self-reported (88% of 155 tracks within 2 BPM, 72% of 68 keys), so
  its estimate is a second opinion to compare with, not a replacement. Score both on the same
  files with `benchmark` before trusting either.
- Tracks are resampled to 44.1 kHz mono for the analyzer and held in memory whole (about 10 MB a
  minute) while it runs; loudness is measured on every channel at the file's own rate.
- Clipping is counted as runs of three or more samples in a row at full scale (32767/32768 or
  above) in one channel. A loud master that peaks at 0 dBFS without flattening doesn't count.
- The analyzer can't be cancelled once it starts on a decoded track.
- Silent audio has no estimate. A blip too short for a tempo gets one with a tempo of 0.
- The beat grid is stratum-dsp's, put back in the track's time (it tracks beats after trimming
  leading silence), with bars counted in fours from its first beat. Its `grid_stability` says
  how evenly the beats fall. Steady synthetic clicks score 1, beats spaced exactly by the
  estimated tempo, though the grid can stop before the clicks do; 14 released dance tracks
  scored 0.22 to 0.53, with beats missing and repeated. Check it before using the grid, and know
  that a stable grid spaced by a tempo off by a fraction of a BPM drifts from the music as the
  track goes on.
- Each band of the waveform is scaled against its own loudest columns, so it shows where a band
  is loud in the track, not how the bands compare.
- Where the best keys tie (confidence 0) there's no key. stratum-dsp scales major and minor keys'
  scores apart, so its best of each tie whenever it scores a track whole (one under about 12
  seconds, or with no stretch of a clear key), and it would pick one of them in no fixed order.

## What it doesn't do

- Download datasets: get the GiantSteps annotations and audio from their repositories.
- Store anything. Analyses serialise; where they go is up to you.
- Write tempos or keys anywhere. It says what it found and how sure it is.

## Licence

MIT.
