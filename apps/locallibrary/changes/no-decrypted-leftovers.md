### Fixed

- Reading rekordbox's collection no longer leaves a decrypted copy of it in the temporary folder
  when locallibrary is killed, crashes or quits mid-read: `master.db` is decrypted in memory. What
  still needs a file (copying the collection, a drive's database being edited) goes in a private
  `locallibrary-decrypted-*` folder, and one left by a process that ended is removed the next time
  locallibrary decrypts one, with the copies earlier versions left of `master.db` and of drives'
  databases. Only your own folders holding nothing but those copies are removed.
