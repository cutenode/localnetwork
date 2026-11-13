### Fixed

- The player no longer saves its defaults over a state file it couldn't read as it started (one
  from a newer build, or one it had no permission to read then). It runs on its defaults and
  leaves the file as it is until the next start.
