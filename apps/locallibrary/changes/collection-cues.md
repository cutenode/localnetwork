### Fixed

- Tracks played from rekordbox's collection show their hot cues, memory cues and loops. rekordbox
  keeps them in `master.db`, and the player read them from the analysis files in its `share`
  folder, which rekordbox writes cues into only for a drive it exports to. Auto cue starts those
  tracks at their first memory cue too.
