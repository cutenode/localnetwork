### Added

- A deck strip under every view: what's playing, its tempo, key and time left, the hot cues and loop
  over the waveform. Dialogs stay above it.
- The Prepare view: the jobs that get a drive ready for a set, run with `enter`.
- A context line with the drive, the list, the search and how much is marked, and a status line with
  what keys do, the latest job or message, where the selection is, space left and who's signed in.
  Under 30 rows the header carries the context.
- Pane titles have numbers, and digits jump to panes. The pane with focus has a marker in its title
  as well as its border.
- `g` and a letter goes to a view: `g l`, `g p`, `g d`, `g r`, `g u`, with a hint after a moment.
  `g g` is the first row. The digits play hot cues while the pads are on, now in every view.
- The Drives view's History pane: what players recorded on the drive open in the library.

### Changed

- The player has five views, one for each job: Library, Prepare, Drives, Recordings and Publish.
  Mixes is now Recordings, Profile is now Publish, and the store's snapshots are a pane of Drives.
- The deck strip takes the place of the Now playing pane, so the track list has more rows. `f` still
  opens the full deck.
- Sessions are called History in the player.
- Small terminals: stacked panes collapse to their title rows first, then the details pane shows
  only while it has focus, then one pane has the room. Below 60x20 there's only the deck and the
  size it needs.
- A selection in a pane without focus stays, dimmed.
- `esc` closes one thing at a time, and the output list closes before a dialog that came up behind
  it.
