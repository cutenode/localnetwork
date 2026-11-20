# The collection is written only over a fresh backup

locallibrary may write rekordbox's collection (`master.db` on this computer), and not only read
it, so edits made in the player or from the command line can land in rekordbox itself rather
than only on a drive exported from it. Two rules hold for every write:

- **Only while rekordbox is closed.** While rekordbox runs, the collection stays read-only, as it
  is now: edits made meanwhile are staged in the store and shown on top of the collection as read,
  and applied together once rekordbox has quit. rekordbox's agent (`rekordboxAgent`) runs all the
  time and doesn't count as rekordbox being open.
- **Only over a fresh backup.** No edit is written until a backup of `master.db` and its WAL has
  been made of the database exactly as it is at that moment, and read back: decrypted, opened and
  checked whole. A backup made before rekordbox last changed the database isn't fresh, however
  recent. The write plan makes this backup as its first step, and if the backup can't be made or
  doesn't read back, nothing is written. A backup from rekordbox itself (`master.backup.db`) or
  from a backup plugin counts only if it's verified the same way and the database hasn't changed
  since.

The write itself works like a drive's: the change is made to a decrypted copy, the copy is checked,
encrypted again, and swapped in as one rename, only if rekordbox is still closed and `master.db`
and its WAL are still as they were when the backup was made. Backups are kept in the store, the
latest several of them. Each staged edit keeps what it replaced, so one edit can be undone after
others; restoring a backup whole is the last resort, since it also undoes whatever rekordbox
changed since.

## Why

The collection is the DJ's main library, and rekordbox is the only program that reads it back. A
write that leaves it damaged, or in a shape rekordbox rejects, loses work that may exist nowhere
else, and rekordbox's own backup runs on its schedule, not ours. A backup taken right before the
write, and proven to read, means any write can be taken back to exactly what was there.
Writing while rekordbox runs would race its own saves and its in-memory copy of the database,
which it writes back over ours.

## Considered options

- **Never write the collection** (what `collection` has done until now): edits reach rekordbox
  only through a drive export and rekordbox's own import, and the player's collection stays
  read-only. Safe, but people edit their collection by hand with scripts and keep their own
  `master.backup.*.db` copies anyway, with none of these checks.
- **Write whenever asked, with rekordbox open:** rekordbox holds the database open and saves its
  own state over changes made underneath it.
- **Require any recent backup** (one from the last day, say): a backup from before rekordbox's
  latest save can't put back what that save did, so it isn't a backup of what the write changes.
- **Undo by keeping each edit's inverse only, with no backup:** undoes what locallibrary
  understood about the change, not a write that went wrong in a way it didn't foresee.

## Open questions

Cloud Library Sync tracks changes with rekordbox's update counters (`rb_local_usn` on each row,
and the agent's own). Which ones a write has to move, so the change syncs to other computers and
isn't overwritten by the cloud's copy, and whether `rekordboxAgent` writes `master.db` while
rekordbox is closed, are to be found out on a copy of a library before any write ships.
