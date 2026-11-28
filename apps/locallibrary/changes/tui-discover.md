### Added

- With no drive open, the library says what to do: plug one in, or open a folder, browse snapshots or
  sign in. Opening a folder holding an export is in `:` too.
- The first library each version opens names `enter`, `e`, `:` and `?` in the footer until a key.
- The palette ranks the commands run lately first, kept between runs and in `settings export`. It
  shows each command's key, or `Prepare` where it has none here, and keeps what can't run now in the
  list, greyed, with the reason.
- `e` opens the actions for the selection in every view, the likeliest first, and enter runs it.
- A job list (`&`) with a cancel for each job that can still stop, and a note once one can't.
- A hint setting in the palette: full, compact or off. It's saved, and goes through `settings export`
  and import. Errors and job progress show whatever it's set to.
- Prepare and the Drives view's actions menu (`e`) have the jobs only the palette had: check (`c`),
  save a snapshot (`s`), sync (`g`), pack (`p`) and eject (`d`). Prepare now lists all twelve of
  those jobs.

### Changed

- The player's own track and playlist edits are made at once and say `u undo`, instead of asking
  first. A plan that removes tracks, files or history lists, has a warning, reaches other clones or
  comes from a plugin still asks, as does anything that can't be undone.
- `?` opens on the pane with focus, narrows as you type, and enter runs the row selected. `esc` closes
  it.
- The help is one column, at every width.

### Fixed

- The first frame no longer says there's no drive before the drive list has been read.
