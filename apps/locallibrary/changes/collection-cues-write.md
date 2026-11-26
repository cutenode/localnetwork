### Added

- **Cue edits in rekordbox's collection.** `cues set`, `rename`, `colour`, `rm`, `auto` and `copy`
  take `collection` in place of a drive to edit the cues of rekordbox's collection on this
  computer (`master.db`). They're written only once rekordbox and its agent have both quit, over a
  backup kept in the store (the latest 10). Made while either runs, they wait in the store and are
  written together, in order, once both have quit: by the player, which shows them on the deck at
  once and counts them in the status line, or by `collection apply`. `collection undo` (or `u` in
  the player) takes the latest back as a new change, leaving a track whose cues rekordbox changed
  since; one still waiting is dropped. Nothing is written while Cloud Library Sync is on, and the
  refusal says why. A cue the edit leaves as it is keeps its row, colour and loop length too.
- **`collection repair`** rewrites the cue lists rekordbox keeps for each track (`contentCue`) where
  they disagree with its cue rows, as older scripts left them (`--create-missing` gives a list to
  tracks with cues and none).
