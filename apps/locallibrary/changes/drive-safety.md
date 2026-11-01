### Fixed

- `refresh` and `prep` plan a step again only when the drive is as the job's own earlier steps
  left it. Anything else that changed it since the plan was confirmed (a rekordbox export
  mid-job) stops the job with the stale plan, as a single apply does, instead of being planned
  over and applied unconfirmed.
- A backup carries the drive's recorded edits and the player name and colour it remembers, and
  putting one back puts them back too: undo then redo, or an older backup then a newer one,
  leaves `edits replay` and `refresh` knowing exactly the edits and name the drive has. The undo
  plan says how many recorded edits go back on the journal. A replay cut off part way notes
  what each step left in its backup, so it's undone without `--force`.
- A file copied, hashed or snapshotted in the second or two after it was written is no longer
  taken as unchanged by its size and modification time alone (FAT32 keeps those to 2 s, so a
  rewrite then can look the same): sync, pack, collection export and the after-a-set changes
  compare it by content, and taking embedded art back out checks each file by content.
- A sync or pack onto a stick with no `exportLibrary.db` leaves it without a player name and
  colour, where it took the source's, and the plan says so.
- A My Tag category numbered past what `exportExt.pdb` holds in a OneLibrary database no longer
  makes an edit that plans cleanly and fails as it's applied: `exportExt.pdb` gets a free id, and
  an id that can't fit is refused when planning.
- `collection export` counts the online-only files it downloads in its space check when their
  cloud folder is on the target's disk, and says which file it's waiting on when one takes more
  than ten seconds to download.
- A replay that skips deleting history lists none of which are on the drive names them.
  Merging the same drive into a folder again adds nothing, where it made a "(2)" copy of every
  playlist. Under `LOCALLIBRARY_VOLUMES`, the disk holding a drive folder isn't looked up with
  `diskutil` or `lsblk`.
