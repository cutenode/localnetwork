### Added

- The sound plugin works out a beat grid from a track's audio: one tempo throughout, to 0.01 BPM,
  with bars from the first beat. The player draws it for a track whose drive has no grid, and
  `analyze --apply grid` writes it to a drive's analysis files, so tracks rekordbox never
  analysed get one. `sound.bpm` is now the grid's tempo, and tracks are analysed again.
