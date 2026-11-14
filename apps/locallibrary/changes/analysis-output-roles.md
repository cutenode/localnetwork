### Added

- **Choosing whose beat grid, phrases and waveform you get.** `plugin roles grid`, `phrases` and
  `waveform` choose the analyzer each is read from, as a role is chosen (`plugin roles waveform
  PLUGIN.ANALYZER`, `--none`, `--reset`), kept as `analysis.roles.waveform` and so on. The player
  draws that analyzer's, or another's for a track it hasn't analysed, and `analyze --apply` writes
  only that analyzer's without `--plugin` and `--analyzer`. Unchosen, the player draws the first
  with one for the track, as before. An analyzer that doesn't work one out is refused. The
  sidecar's `plugins.roles` lists them too, and `plugins.apply_analysis.plan` takes no `plugin` or
  `analyzer` to write each from the analyzer chosen for it alone.

### Changed

- The docs say the `identity` role is reserved: nothing reads it yet.
