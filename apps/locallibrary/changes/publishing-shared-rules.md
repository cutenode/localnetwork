### Added

- Publishing a set to Mixcloud (and its dry run) warns about Mixcloud's licensing rules, as
  `sets export --format mixcloud` does: a show under 15 minutes, more than 3 tracks from one
  release (2 in a row), more than 4 by one artist (3 in a row). It counts the tracks published
  with their names; the export counts every track.
- Plugins publishing a set get each named track's album and length (`album`, `duration_seconds`)
  in the tracklist.

### Changed

- `sets export --format youtube` writes the description the youtube plugin uploads. A time that
  would break YouTube's chapters (out of order, under 10 seconds after the chapter before, or in
  the recording's last 10 seconds) is left off its line, where it used to be written and warned
  about; a track without a start time is listed without one instead of the export failing; `<`
  and `>` are left out; past 5,000 bytes it lists the tracks that fit and how many more there
  are; and a track with no artist is `ID - Title`.
- `sets export --format mixcloud` no longer says a set is under 15 minutes when a track's length
  isn't known and the last track has no start time.
