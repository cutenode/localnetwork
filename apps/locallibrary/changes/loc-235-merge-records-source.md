### Fixed

- `sync` goes ahead after a `merge` of two drives that both changed, as the docs say: a merge
  that takes in all of the source now counts the target as newer, so the sync copies the merged
  library over the source. The merge plan lists tracks both drives have whose details, file or
  analysis differ (the target's stay, and the sync copies them over), and says when a merge
  leaves some of the source out (`--only`, `--playlists none`, history lists without
  `--history`), which leaves the drives diverged. Undoing the merge leaves them diverged too.

### Changed

- The merge plan's list of new tracks shows every one with `--all`, as `compare`'s lists do.
