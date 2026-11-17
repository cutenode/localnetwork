### Changed

- **rekordbox's collection is the player's default.** With rekordbox on this computer, the player
  opens its collection at start, even with a drive plugged in, unless a drive was open at the last
  quit and is here. A drive holding a copy of the whole collection (from `collection export`) is
  never opened in its place, and the Sources pane marks it `copy, <date>`.

### Added

- **The collection stays current.** While it's open, the player watches `master.db` and its WAL:
  a couple of seconds after rekordbox saves, it reads the collection again, keeping your place.
