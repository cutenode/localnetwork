### Added

- **A snapshot's library.** The sidecar's `snapshot.library` reads the library a drive had when
  a snapshot was taken, a page of tracks at a time as `drive.library` does, and a plugin with
  `store:read` can call it, so it can bring back one playlist from a snapshot.
