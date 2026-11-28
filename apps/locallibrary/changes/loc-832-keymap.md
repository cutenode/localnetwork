### Changed

- **The player's keys mean the same thing everywhere.** Every dialog closes on `esc` (a list that
  isn't typed into, on the key that opened it too), never on `q` or any other key, and moves with
  the arrows, `j` `k`, page up and down, home and end. The actions menu (`e`) works the same in
  every view: each action has its letter, and `/` filters it. Each action has one name in every
  hint, and a hint clicked does what it says.
- **Quitting is `esc` twice.** With nothing left to close, `esc` says a second quits, and a second
  within a second does; `ctrl-c` and `ctrl-q` still quit. `Q` no longer quits.
- **Keys that clashed have one meaning.** The drive's jobs (check, snapshot, sync, pack, eject)
  are letters in the actions menu instead of capitals in Prepare and Drives. In localnetwork,
  putting a section in the document, showing it, hiding an artist and rounding are in the actions
  menu and the palette. Exporting a set is in the menu and the palette. `esc` closes the details
  and the queue, and turns the pads off.
- **Fewer keys, and everything else by name.** No key is a capital letter, and none waits for
  another: the `g`, `q` and `z` chords are gone. `1` to `5` go to the views and each tab shows its
  number (in player mode the digits are hot cues); `tab` and `shift-tab` move between panes, which
  no longer have numbers. A key is kept for what a DJ does every session: `space`, `← →`, `n` `p`,
  `c` the next cue and `h` the previous, `l`, `,` `.`, `+` `-`, `/`, `m`, `a` to add to the queue,
  `q` to play next, `t` for the queue, `i`, `del`, `u`, `f`, `f5`, `e`, `:`, `?` and `&`.
  Everything else is in the palette by name (type "next phrase", "shuffle", "headphones"), and
  much of it in the view's actions menu too: in the library `t` gives a My Tag and `w` and `q` save
  or clear the queue; a drive's menu checks all its drives (`a`), syncs them (`g`) and ejects
  (`d`); Recordings opens a recording with `f`; localnetwork signs in with `l`. A click on a column
  header still sorts. A result or the messages close on `enter` too. In player mode `w` goes round
  the waveform's colours again, and `b` browses.
- **Quitting with a job under way asks first.** `esc` twice, `ctrl-c` and the palette's quit ask
  whether to quit anyway; `y` or a second `ctrl-c` says yes.
- **A simpler header and footer.** The top row is the same at every size: the tabs by name, what
  the view is about (in the library, the search and the marks stay as the row narrows), and
  localnetwork at the end. The status row no longer says BROWSE, and shows the drive's space and
  the row selected only where they mean something. The keys row starts with `enter` and `e` and
  always ends with `:` commands and `?` help; a chip leads it only when keys mean something else.

### Fixed

- Clicking the footer's play next hint skipped to the next track.
- Opening a recording in Recordings could ask for its path off screen.
- The Drives view drew an empty `u` row when there was nothing to undo.
