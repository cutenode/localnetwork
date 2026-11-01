### Added

- **Fixes for the check's own findings.** A drive behind or ahead of the first one checked, history
  lists the store lacks and tracks without artwork (when a plugin finds cover art) carry a fix,
  offered in the player's palette as plugins' fixes are: syncing the library's drives, saving a
  snapshot, or the cover art fetch form for those tracks. The sidecar's `check` items have it as
  `fix: {name, params}` (`sync`, `pull`, `art.fetch`).
- **Suggestions for new tracks.** Once a drive is plugged in or its databases change, the coverart plugin suggests
  fetching covers for its tracks without artwork, and the sound plugin analysing the tracks it has
  no tempo, key or loudness for. A suggestion reviewed or dismissed is remembered in the store
  (`plugins/seen.json`), and the same again, or one still waiting, is left out: only tracks not
  suggested before come up.
- **Keys for plugins' commands.** `plugin settings ID keys.NAME KEY` (or the plugin's settings
  form) gives one of its commands or operations a key in the player: a character, `ctrl-` and a
  letter, or `f1` to `f12`. A key the player already uses, or one bound to another command, is
  refused, saying why.
