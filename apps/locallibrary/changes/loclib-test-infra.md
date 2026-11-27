### Fixed

- A sync stopped by an error leaves the same files on the drive every time. Small files already
  being copied could finish or not depending on timing, so the next sync planned a different
  number of copies. Now every small copy finishes, the large ones stop, and the error given is the
  first in the plan's order.
- `pack --transcode --container flac` no longer makes a file nothing can read from loud 24-bit
  audio. The encoder could pick a coding tens of megabytes long for one block; such a block is now
  written as its samples.
- A drive built from a Traktor collection keeps every beat of its grid. The beat just before a
  grid marker could be left out.
