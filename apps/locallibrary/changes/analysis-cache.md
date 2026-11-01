### Changed

- `profile`, `mix` and the history reads behind `plays`, `sets` and the profile keep their work in
  the store's cache and say what came from it: the history lists of each snapshot's databases (by
  the databases' hashes), what the profile reads in tracks' analysis files, what's heard in a
  recording, and each drive's fingerprint index, until the prints it was built from change. A
  new version of what makes any of them reads none of the old ones, and `store gc` removes
  fingerprints, recordings heard and indexes older versions made.
- Without declared sets, the profile's sessions are the ones `sets` and `plays` find, with the
  same windows: imported logs count too (dated by when they were played), and sessions you
  dismissed don't. `plays` reads each drive's databases once.
