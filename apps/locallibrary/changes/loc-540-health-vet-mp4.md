### Fixed

- `check --audio` no longer aborts on an M4A whose atoms claim more than the file holds when the
  file is smaller than the library says. It's reported as a file that won't open.
