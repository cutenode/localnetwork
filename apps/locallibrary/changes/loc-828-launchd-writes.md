### Changed

- On macOS, commands that write drives, run from a terminal, run as a launchd job from a copy of
  locallibrary that launchd wrote, so macOS adds no `._*` files to FAT and exFAT drives: about 5,700
  fewer files and 180 MB less on a 600-track drive. Output, prompts, the exit status and Ctrl-C work
  as before. The copy has no `com.apple.provenance` attribute by design ([drives](../docs/drives.md)).
  `watch --install` runs the copy too.
