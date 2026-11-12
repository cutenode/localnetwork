### Changed

- `pull` makes a track rekordbox's collection already holds on the store's volume (APFS, Btrfs or
  XFS) a copy-on-write copy of its file instead of storing it again. The object takes no space until
  one side changes, and it's no longer a separate physical copy: keep a `store copy` on another disk
  as a backup.
