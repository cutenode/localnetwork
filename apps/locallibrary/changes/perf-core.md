### Changed

- Every command starts faster: the plugins' manifests no longer go through TOML and JSON on each
  run, the command line is built once rather than twice, and it's built in full only when help
  may be shown. `--version` answers before any of that, and so does each plugin as it starts.
- `check` and `doctor` take about half the time they did on a drive without its audio, since
  plugins' reads no longer search every string they return afresh for every path.
- Reading a library copies each track's strings once instead of twice.
- Hashing a small file no longer sets up a megabyte to read it through.
- `pull` on macOS waits for the disk once per volume rather than once per file: a 10,000-track
  drive takes less than half the time it did. Syncs, packs and writes to drives flush this way too.
- Merging finds each new track, and a playlist's or a list's tracks, without going through the
  whole library each time.
- Release archives and the binary in them are smaller.
