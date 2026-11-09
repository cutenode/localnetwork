### Fixed

- `library push` checks that the store still holds every file it would copy before touching the
  drive. A store missing one now fails at the plan, naming the tracks it belongs to, instead of
  stopping part way through the copy with the drive marked as being written.
