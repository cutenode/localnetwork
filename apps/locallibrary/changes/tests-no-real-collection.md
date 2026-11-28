### Changed

- No test reads rekordbox's own collection or settings on the computer it runs on. A debug build
  (every test's), or anything nextest starts, uses only the `master.db` that
  `LOCALLIBRARY_MASTERDB` names and the settings folder `LOCALLIBRARY_REKORDBOX_SETTINGS` names,
  and never falls back to rekordbox's. Opening rekordbox's own `master.db` without
  `LOCALLIBRARY_MASTERDB` naming it fails the test, however the path is spelled and whatever
  `HOME` says. Release builds still find rekordbox's collection and settings as before.
