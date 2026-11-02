### Changed

- `mix` keeps a drive's fingerprint index between runs only up to 512 MiB (about 3,000 tracks).
  It had no limit, and was rewritten whenever any track's print changed. A bigger one is built
  again each run, and one kept before goes the next time it's read.
  `store index-limit [MIB|default]` changes the limit (`0` keeps none), and `settings export` and
  `import` take it with the rest (`mix.index_limit_mib`).

### Fixed

- Where the pitch fader was ridden through a track, the player's Mixes view follows the ride: the
  playhead on the track's waveform is where its speeds put it (the average speed drifted by a
  second or more), and the tempo under the timeline steps between the speeds. The HTML report's
  tempo chart does the same, and a transition's tempo change compares the speeds the two tracks
  were at when the next came in, not the ridden track's average. Loops and cue jumps inside a
  play move the playhead too. Reports carry each play's `lines` for this; ones made before keep
  the average.
