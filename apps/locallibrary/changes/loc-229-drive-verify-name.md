### Fixed

- `drive verify` no longer calls `exportLibrary.db` damaged, or its WAL missing, when the drive
  holds the library it saved: after `drive player`, a sync that kept the drive's name, or a player
  checkpointing the database. `provision --player-name` no longer stops at its check.
- `drive verify --repair` keeps the drive's name and colour when it rewrites `exportLibrary.db`.
