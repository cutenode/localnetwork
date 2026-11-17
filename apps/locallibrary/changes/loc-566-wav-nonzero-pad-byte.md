### Fixed

- `--write-tags` no longer plans cues for a WAV or AIFF whose last pad byte isn't 0 and then
  fails writing them. It's copied without cues, saying why, like one missing that byte.
