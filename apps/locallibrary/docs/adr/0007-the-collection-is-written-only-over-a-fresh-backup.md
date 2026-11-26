# The collection is written only over a fresh backup

locallibrary may write rekordbox's collection (`master.db` on this computer), and not only read
it, so edits made in the player or from the command line can land in rekordbox itself rather
than only on a drive exported from it. These rules hold for every write:

- **Only while rekordbox and its agent are both closed.** While rekordbox or `rekordboxAgent`
  runs, the collection stays read-only, as it is now: edits made meanwhile are staged in the
  store and shown on top of the collection as read, and applied together once both have quit.
  The agent counts as open: it holds the database open and writes `agentRegistry` (sign-in
  refreshes, notification fetches), and it has outlived rekordbox by days. If its WAL were applied
  to the file we swap in, that file would be damaged.
- **Not while Cloud Library Sync is on.** A write is refused, with a message saying why, while
  the collection is synced: what a synced write has to move so it reaches the cloud and isn't
  overwritten by the cloud's copy hasn't been measured. Sync counts as on if any row has a `usn`,
  any row has `rb_local_synced` set, `cloudAgentRegistry` has rows, `agentRegistry` has a
  `lastUpdateCount` row, or the agent's options or log don't say it runs standalone. When it
  can't tell, it takes sync as on. This holds until a synced write has been measured on a test
  account's library, not a person's own.
- **Only over a fresh backup.** No edit is written until a backup of `master.db` and its WAL has
  been made of the database exactly as it is at that moment, and read back: decrypted, opened and
  checked whole. A backup made before rekordbox last changed the database isn't fresh, however
  recent. The write plan makes this backup as its first step, and if the backup can't be made or
  doesn't read back, nothing is written. A backup from rekordbox itself (`master.backup.db`) or
  from a backup plugin counts only if it's verified the same way and the database hasn't changed
  since.

The write itself works like a drive's: the change is made to a decrypted copy, the copy is checked,
encrypted again, and swapped in as one rename, only if rekordbox and its agent are still closed and
`master.db` and its WAL are still as they were when the backup was made. All of a write's changes
go in one transaction on the copy, so rekordbox never sees half of one. Backups are kept in the
store, the latest several of them.

**Undo moves forward.** Each staged edit keeps what it replaced, so one edit can be undone after
others, and undoing writes the old values back as a new change, with new stamps, as any other edit
is written. `localUpdateCount` never goes back. Restoring a backup whole is only for a file that's
damaged: it undoes whatever rekordbox changed since, and it rolls the counter back below what a
synced cloud may already have seen, so it says so before it does it.

**rekordbox's layout for what it reads, stricter where it's sloppy.** The aim isn't to copy what
rekordbox does but to leave a collection rekordbox reads as its own. So what rekordbox reads is
laid out as rekordbox 7 lays it out, and where rekordbox is loose, a write is stricter:

- Every changed row that rekordbox stamps gets `rb_local_usn` and `updated_at`, and
  `agentRegistry`'s `localUpdateCount` (`int_1`, the one counter for the whole database) ends at
  least as high as every stamp and only goes up. rekordbox skips numbers; only that order matters.
- A cue write changes three tables together, as rekordbox does. In `djmdCue` the old row is
  deleted outright (not marked `rb_local_deleted`) and a new one is inserted with a new `ID` and
  `UUID`, `InFrame` = `InMsec` × 0.15 (150 frames a second), `OutMsec` and `Color` -1 for a hot
  cue, and `ColorTableIndex` empty when it has no colour; rekordbox leaves cue rows unstamped, and
  so does the write. `contentCue` holds the track's whole cue list as JSON (`Cues`, with the same
  fields) and its count (`rb_cue_count`), one row per track keyed by the track's UUID; the write
  makes the row when it's missing and stamps it. `djmdContent.CueUpdated` goes up and the track's
  row is stamped.
- `djmdCue` and `contentCue` always agree after a write, and the copy is checked for that before
  it's swapped in. Tracks where they already disagree, as older scripts left them (cue rows added
  without `contentCue`), can be repaired: `contentCue` is rebuilt from `djmdCue` as one more
  write under the same rules.

## Why

The collection is the DJ's main library, and rekordbox is the only program that reads it back. A
write that leaves it damaged, or in a shape rekordbox rejects, loses work that may exist nowhere
else, and rekordbox's own backup runs on its schedule, not ours. A backup taken right before the
write, and proven to read, means any write can be taken back to exactly what was there.
Writing while rekordbox runs would race its own saves and its in-memory copy of the database,
which it writes back over ours; while rekordbox runs its changes sit in `master.db-wal`, and only
when it quits is the WAL folded in and removed, leaving one file. The agent writes the same
database on its own, so it has to be closed too.

A synced collection is also kept by the cloud, and a write that doesn't move the counters sync
expects could be overwritten by the cloud's copy or reach other computers half done. Until that's
measured, refusing is the only answer that can't lose anything. For the same reason undo moves
forward: rolling `localUpdateCount` back would make the database look older than a copy the cloud
has seen.

rekordbox itself is loose in ways that already show in real libraries: in one library of 676
tracks with cues, 439 had no `contentCue` row and 206 had one whose count disagreed with
`djmdCue`, 193 of them because scripts had added cue rows without it. Matching rekordbox's layout
keeps what it reads correct; checking the copy before the swap keeps us from adding to that.

## Considered options

- **Never write the collection** (what `collection` has done until now): edits reach rekordbox
  only through a drive export and rekordbox's own import, and the player's collection stays
  read-only. Safe, but people edit their collection by hand with scripts and keep their own
  `master.backup.*.db` copies anyway, with none of these checks.
- **Write whenever asked, with rekordbox open:** rekordbox holds the database open and saves its
  own state over changes made underneath it.
- **Count only rekordbox as open, not its agent** (what this record first said): the agent writes
  `master.db` on its own and can run for days after rekordbox quits.
- **Write with sync on, moving `rb_local_usn` alone:** sync also has `usn`, `rb_local_synced` and
  `lastUpdateCount`, and which of them a write must move hasn't been seen.
- **Require any recent backup** (one from the last day, say): a backup from before rekordbox's
  latest save can't put back what that save did, so it isn't a backup of what the write changes.
- **Undo by keeping each edit's inverse only, with no backup:** undoes what locallibrary
  understood about the change, not a write that went wrong in a way it didn't foresee.
- **Undo by restoring the backup:** also undoes rekordbox's own changes since, and rolls the
  counter back under what sync has seen.
- **Write cues to `djmdCue` alone**, as older scripts do: rekordbox reads the track's cues from
  `contentCue` too, and that is what leaves the two disagreeing.

## What was measured

On decrypted copies of a library, with rekordbox 7.2.18, before and after rekordbox set two hot
cues on one track, with a watcher on rekordbox, its agent and `master.db`'s files (LOC-1150):

- `localUpdateCount` rose by 15 for five stamped rows; it equals the highest `rb_local_usn` in any
  table.
- Every time rekordbox starts it writes `master.db` (it restamps the three `djmdSampler` rows, and
  the agent rewrites `notificationNextFetch`), even with no edit.
- The agent quit when rekordbox asked it to in 55 of 65 sessions, and outlived it twice, once for
  about 38 hours and once for a week.
- Sync was never on for that library (`usn` empty on all 15,814 rows, and the agent's log says
  `sync mode: standalone` throughout), so a synced write wasn't seen. That is what's still open,
  and why a write is refused while sync is on.
