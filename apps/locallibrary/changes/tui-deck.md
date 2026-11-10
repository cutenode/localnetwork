### Added

- A Deck layout for the Library view: `f` gives the deck the whole screen, like a CDJ's display, and
  `b` opens the panes over it until a track's loaded. The layout's kept between runs.
- A zoomed waveform that scrolls past a fixed playhead, with a beat ruler, bar numbers, and hot cues
  lettered in their own colours, a second row taking those that collide. Loops are a tint.
- The overview's playhead moves in eighths of a cell, its played part dimmed, with rekordbox's phrase
  labels under it in the Deck layout.
- rekordbox's waveform colours, BLUE, RGB and 3Band, from the drive's analysis. They follow the
  drive's My Settings unless you pick one with `w` or in the palette, and a drive without them gets
  3Band. Band boundaries fall on eighths of a cell.
- Keys in Camelot, Open Key or as notes, and key colours from a wheel or a palette for protanopia,
  deuteranopia or tritanopia.
- Keys that mix with the playing track are marked `=`, `±` or `↔` in the track list, as well as green,
  and a ΔBPM column says how far each track's tempo is from it.
- A large tempo and time left, bar countdowns to the next cue and phrase, and a warning before a track
  ends, at 30 seconds unless you change it. Under `LOCALLIBRARY_REDUCED_MOTION` it doesn't blink.
- Cover art in the terminal's own graphics (kitty, iTerm2, sixel) where it has them, and a colour chip
  on each track row from its cover. `LOCALLIBRARY_ART` names the graphics to use.
- Tracks heard this session are dimmed in the track list, and `U` brings them back.
- Short fades as a track loads and a panel opens, with 24-bit colour and without
  `LOCALLIBRARY_REDUCED_MOTION`.
- `settings export` takes the key notation, key colours, waveform colours, warning and cover art.

### Changed

- `f` switches layouts instead of going full screen. The Deck layout has no cue lane or progress bar:
  the waveforms say both.
- `v` picks what shows beside the tempo in the Deck layout, where there's room.
- In the track list, keys that mix have their mark with colours too, not only without them.
- Cover art is resized off the screen's thread, so a new size can't hold up a frame.
- A track's playlists show by their own names, then the folder they're in, so playlists in the same
  top folder no longer look alike. A name with a `/` in it stays whole.
