# Demo samples stay out of backups, so undoing a clean can't restore them

Every change to a drive in place is backed up with every file it deletes, which is what makes
`undo` whole. `clean` is the exception. rekordbox puts its demo samples back with every export and
`refresh` removes them after each one, so backing them up kept another copy of the same audio on
the drive each time, and a drive too full for that copy couldn't be cleaned at all. A clean's
backup holds only the databases, and undoing it brings the tracks and their playlist entries back
without their files. The next export brings the files back, or another `clean` removes the tracks.

`delete_unbacked` is its own edit, so the journal carries it and `edits replay` behaves as `clean`
does. Only plugins locallibrary ships can ask for it (`library:delete-unbacked`), and `edit.plan`
refuses it, since it deletes what no backup can bring back and a capability means little if the
sidecar goes around it. A plan that deletes anything else as well backs up everything it deletes.
Undo says which tracks come back without audio instead of refusing, because the refusal's `--force`
also overrides "the drive changed since", which is the check worth keeping.

A journal holding the edit is version 5, so builds from before it refuse that drive's journal.

## Considered options

- A flag on `delete_tracks`: older builds could still read the journal, but `keep_files` and the
  flag together mean nothing, and every place that deletes tracks would have to say it doesn't
  want it.
- Deciding per operation, in the plugin's plan or its manifest: the journal wouldn't carry it, so
  `edits replay` after an export would pile up the same audio.
- Refusing to undo a clean unless forced: a clean is undone so rarely that the refusal would mostly
  stop people stepping back to a change before it.
- Letting sidecar clients ask for it, as they're ours today: that stops being true once plugins or
  third parties speak RPC.
- Removing the tracks from the database again after undoing: undo would no longer put back exactly
  what the backup holds.
