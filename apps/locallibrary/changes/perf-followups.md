### Changed

- `profile` looks for each track's analysis files once, and a file that isn't there costs one
  look: on a 3,886-track drive without analysis files it takes about a quarter of the time it
  did, where reading every track's `.EXT` (for hot cues D to H) had made it half again slower.
- Applying an edit for every track (an import's keys and tempos, `analyze --apply`) finds each
  edit's tracks without going through the whole library, and so does planning a replay of one,
  whose edits name tracks by file.
