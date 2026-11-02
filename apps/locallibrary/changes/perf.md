### Changed

- Scanning a drive lists its folders in parallel, and `check` scans each drive once instead of
  twice. Adding tracks to `export.pdb` or `exportLibrary.db` no longer goes through every row for
  each one added, and sorting, library diffs, key suggestions, Serato and Engine exports,
  tracklist matching and finding tracks in a mix repeat less work per track.
