### Added

- `store browse` opens a past version of a drive as a folder of read-only links into the store, with nothing copied, and removes it on Ctrl-C. `--to` chooses the folder, `--keep` leaves it open, `store views` lists what's open and `store unbrowse` removes it. Not on Windows.

### Changed

- Files in the store are read-only from now on, so a write to one fails instead of leaving it under the wrong hash. Older ones change when a view shows them.
