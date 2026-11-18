### Changed

- **The profile counts rekordbox's collection on this computer.** `profile`, `profile publish` and
  the localnetwork tab read `master.db` (as the player does) when no drive or library is named, so
  the collection's size, genres, artists, era and energy match what rekordbox shows now, not the last
  drive you pulled. Drives and the store's snapshots still find your sets' and histories' tracks.
  `--no-collection` leaves it out. Coming back to the localnetwork tab after rekordbox saved builds
  the profile again.
