### Changed

- **A waveform written to a drive keeps rekordbox's colour overview.** `set_analysis` and
  `analyze --apply waveform` took a track whose files hold rekordbox's colour overview (`PWV4`) or
  3-band gains (`PWVC`) to be one they can't write, and every track rekordbox analyses has them.
  Both are kept now, and the 3-band waveforms are written at the levels rekordbox's had, so the
  gains still fit them.

### Fixed

- A new beat grid moves the track's phrases with it, to the beats nearest their times, instead of
  leaving them numbered against the old grid's beats. The plan names phrases that move by more
  than a beat, and those that end up on one beat with the next and go.
