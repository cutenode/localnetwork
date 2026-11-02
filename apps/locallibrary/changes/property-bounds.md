### Fixed

- `--write-tags` also copies a WAV or AIFF untagged, with a warning, when its header size counts
  bytes after its last chunk that aren't a chunk (a stray byte, or junk). lofty wrote Serato's tag
  past them, where nothing reading the copy found it.
