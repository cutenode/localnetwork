### Fixed

- On Windows, an edit to the drive open in the player could fail with "Access is denied" now and
  then. Once a second the player checks that the drive still has its databases, and a check that
  landed on a database being replaced held it open. Those checks now wait for the replacement, as
  the drive watcher's already did.
