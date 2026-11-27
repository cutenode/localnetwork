### Added

- **Cues placed from the grid and phrases.** `cues auto` puts cues on a drive's tracks, picked by
  id, playlist, filter or album, in a style you choose (asked when `--style` isn't given). The
  `rekordbox` style puts hot cues A-H where each new phrase starts, named after the phrase.
  `intro-chorus` puts A in green on the intro and H, G and F in pink on the first three choruses.
  `first-cue` only adds the memory cue. Every style puts a memory cue named `1.1Bars` on the first
  downbeat, as rekordbox's first cue does, unless a memory cue sits within a bar of it already. Hot
  cues already in a slot a style uses are kept unless `--overwrite` is given; memory cues are never
  replaced. The drive's journal records the style, so `edits replay` places the cues
  again on the grid and phrases of a later export (journal version 6).
