### Added

- **Beat grids from the audio.** The sound plugin works out a beat grid for every track it
  analyses: one tempo, to 0.01 BPM, with the beats where rekordbox would put them and bars from the
  first beat. The player draws it for a track whose drive has none, and `analyze --apply grid`
  writes it into the track's analysis files, so a track rekordbox never analysed gets a grid
  players show. Against the grids rekordbox made for a library of electronic music, its tempo was
  rekordbox's for 87% of tracks and three in four had their beats within 10 ms of rekordbox's.

### Changed

- **`sound.bpm` is the grid's tempo**, to 0.01 BPM and in rekordbox's octave more often than
  before (87% of tracks against 79%), so doctor's `--analyze` and `analyze --apply bpm` use it.
  Every track is analysed again the next time the sound plugin runs.
