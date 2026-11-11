### Added

- **rekordbox library backups.** The new rekordbox-backup plugin backs up rekordbox's library on
  this computer the way rekordbox's own File > Library > Backup Library does:
  `locallibrary collection backup` writes `rekordbox_bak_YYYYMMDD.zip` into your Documents folder
  (the `destination` setting), and with `music` on copies the audio into `rekordbox_bak/` beside
  it. It shows its plan first. `collection backups` lists the backups there, and
  `collection verify` checks that one is whole. The `schedule` service makes one every
  `every_days` while the player, the sidecar or `watch` runs, keeping the newest `keep`. The
  backup holds rekordbox's and Dropbox's sign-in tokens, as rekordbox's own does, unless
  `credentials` is `strip`. Restore with rekordbox's File > Library > Restore Library.
