### Fixed

- **`drive verify` no longer takes a scan's word for a file.** A `check` before it could record a
  damaged database as the file's hash, and the damage then passed as verified. What a scan only
  read now comes after older snapshots, and a file matching only that counts as unverified, though
  a later change to it is found.
- **`store verify` checks snapshots too.** A manifest that no longer hashes to its id is reported,
  with exit status 4.
