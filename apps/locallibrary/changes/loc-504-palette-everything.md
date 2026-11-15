### Added

- **The command line from the player's palette.** `:` finds `doctor` (and `--by-sound`), `drive
  space` and `plays stats`, which show their report in a panel, and `track dedupe`, `track
  relocate`, `track upgrade` and `refresh`, which show their plan first and are undone with `u`.
  For `drive format`, `fsck`, `firmware`, `provision`, the store's upkeep and `bug` it shows the
  command to run in a terminal, the drive filled in. Commands have words they're found by too:
  "duplicates", "MP3 to FLAC", "free space", and a plugin's operation its command line name
  (`clean`).

### Changed

- **`?` lists keys only.** Its top line says `:` searches everything, the commands with no key
  are in the palette alone, and enter on a search no key matches searches the palette instead.
