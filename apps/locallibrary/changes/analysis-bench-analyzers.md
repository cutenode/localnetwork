### Changed

- `analysis bench` scores the analyzers chosen for the key and bpm roles (`plugin roles`), each
  under its own name (`sound.tempo-key-loudness` by default), instead of stratum-dsp under
  `stratum-dsp`. They run through the plugin host on the drive's tracks or the files in `--audio`,
  and keep nothing. A file in `--audio` is on no drive, so an analyzer that reads the library as it
  analyses fails on it. With no analyzer for those roles, a drive's rekordbox values are scored
  alone, with a warning.
- The core library no longer links stratum-dsp, ebur128 or biquad: loclib-analysis puts them
  behind its `dsp` feature, which only the sound plugin turns on.
