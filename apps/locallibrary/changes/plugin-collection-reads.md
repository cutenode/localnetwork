### Added

- Plugins can back up rekordbox's library with the new `collection:files` capability. Its reads
  say where rekordbox's files are (`collection.locations`) and whether rekordbox is running
  (`collection.status`). They copy `master.db` into the request's scratch folder, still encrypted,
  leaving out the cloud agent's sign-in unless asked (`collection.copy`). And they list each
  track's audio, analysis and artwork files, with its artist and album (`collection.tracks`).
  None of them writes to rekordbox's folders.
