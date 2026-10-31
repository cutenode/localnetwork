### Fixed

- `--write-tags` no longer breaks a WAV or AIFF whose header size is 0 (a streamed recording
  whose header was never filled in) or ends inside one of its chunks: lofty wrote Serato's tag
  where that size said the file ended, and nothing could read the copy. Such a file is copied
  untagged, with a warning naming it.

### Security

- An M4A whose sample tables claim more entries than the file holds (4 billion asks for 16 GiB),
  or whose tag runs past the atom holding it, is reported as unreadable by `mix`, `check --audio`,
  analysis and the player. symphonia asked for the memory up front, which aborts the process on
  Windows and on Linux without overcommit.
- A file claiming a sample rate over 768 kHz is refused when it's decoded, and resampling an odd
  rate keeps at most 16 MiB of filter weights. A crafted rate made the fingerprints' resampler
  grow by about 17 floats per decoded sample, to gigabytes for a long file.
