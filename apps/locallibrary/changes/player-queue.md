### Added

- **A real queue in the player.** The crate is now the queue: what's picked plays in order after
  the playing track, before the rest of the list `enter` was pressed in. `q n` plays the marked or
  selected tracks next and `q a` (or `a`) adds them to the end, from the track list, search results,
  the details' matches and player mode's "mixes into". The queue panel (`q q`, or `t`) shows what's
  playing, the picks (played ones greyed) and "next from" with that list's next few tracks, and
  keeps the crate's keys: move, remove, clear, save as a playlist, and `enter` to play a track now.
  Starting another list, shuffle and repeat leave the picks alone. The queue and what's played in
  it are kept between runs.
- **Fill**, in the palette (off until turned on): when the queue runs out, the best match for the
  last track that hasn't played goes on the end, marked *fill*.

### Changed

- **Quit is `Q` (or ctrl-q, ctrl-c)**, as `q` now starts the queue's keys. Quantize moves from `Q`
  to `Z`.

### Fixed

- **Several tracks picked to play next play in the order picked**, not the last one first.
