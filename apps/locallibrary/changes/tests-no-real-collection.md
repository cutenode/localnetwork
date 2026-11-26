### Changed

- No test reads rekordbox's own collection on the computer it runs on. A debug build (every
  test's) uses only the `master.db` that `LOCALLIBRARY_MASTERDB` names and never falls back to
  rekordbox's, and opening rekordbox's own without that variable naming it fails the test.
  Release builds still find rekordbox's collection as before.
