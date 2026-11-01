### Fixed

- Commands that only look something up in the store no longer make an empty one first when
  there's none: `drive forget`, `role`, `retire` and `adopt --as`, `sets delete` and `playlist`,
  `smart rm` and `apply`, `profile unpin` and `unhide`, `store label`, `unlabel`, `gc`, `prune`
  and `copy`, `library prune` and `rename`, `push`, `art unembed`, and the sidecar's methods for
  the same. They say there's no store instead. `players set --dry-run` and `players clear` plan
  with nothing remembered, and `drive verify` and `fsck` (and the sidecar's `drive.verify`,
  `drive.fsck`, `players.set.plan`, `players.clear.plan` and `sets.export`) record only into a
  store that's there.
- A store that can't be opened is named in the error whichever command reached it. Reads and
  plugin commands left the path out.
- `provision --format` shows each of the format's warnings once, under the format's plan. The
  plan's own `warnings` no longer repeats them; the plan's top-level `warnings` still has both.
- `drive tidy` exits with the partial code (5) when it couldn't remove some files.
- `setup --role` for the drive that becomes the primary is refused (unless it's `primary`), where
  it was ignored, even a role that isn't one.
