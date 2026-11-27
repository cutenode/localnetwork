# The player

`locallibrary` on its own (or `locallibrary tui [<library>]`) opens a terminal UI for browsing and
listening to your library the way the players see it. A handful of keys do what you'd do every
session: `enter` plays, `space` pauses, `n` and `p` go to the next and previous track, `c` jumps to
the next cue, `l` loops, `/` searches, `a` adds to the queue and `f` opens player mode. Everything
else is found by name: `:` opens the palette, where a few words find any action ("next phrase",
"shuffle", "headphones"), and `e` opens the actions for what's selected. `?` lists the keys. None
of the keys it comes with is a capital letter, no key waits for another, and you can change any of
them ([Your own keys](#your-own-keys)).

It has five views, one for each job: Library, Prepare, Drives and Recordings for working on your
music, and localnetwork, what goes out to Local Network, its tab apart at the right end of the top
bar. `1` to `5` go to them, and each tab shows its number (in player mode, and in Browse over it,
the digits are hot cues instead). `tab` and `shift-tab` move between a view's panes, and a click focuses one; the pane with
focus has `▸` in its title as well as its border. The deck strip stays under every view, with
what's playing, its waveform, tempo, key, time left, hot cues and loop. Dialogs open above it, and
`esc` closes one thing at a time: a dialog, the search, the pane with focus. Every dialog closes on
`esc` (a result or the messages on `enter` too) and moves with the same keys; the actions menu
(`e`) is the same in every view, each action with its letter, and `/` filters it. The Library view
has two layouts, and `f` switches between them: the track table with the strip, and player mode,
where the deck takes the screen. From any other tab `f` opens player mode too, and closing it (`f`
or `esc`) goes back there. The footer's hints are buttons: click one to do what it says. With
nothing left to close, `esc` says a second quits, and `esc` again does (`ctrl-c` and `ctrl-q` quit
too).

With rekordbox on this computer, the library opens rekordbox's collection (below) unless a drive
was open at the last quit and is plugged in. A drive holding a copy of the whole collection (from
`collection export`) never opens in its place: the collection itself is read as it is now. Without
rekordbox, the drive open last time opens, else a library's lead clone, else the first drive. With
none, the library says what to do: plug one in (it opens as soon as it's here), or
take a next step, at most three of open a folder, browse snapshots and sign in. The first library
each version opens names `enter`, `e`, `:` and `?` in the footer until a key or a click. Nothing says
there's no drive until the first read of the drives has answered.

The top row, beside the tabs, says what the view is about: in the library, which drive and list
you're in, what's searched for and how much is marked (the list's name gives way first as the row
narrows); the drive in Prepare and Drives, the recording in Recordings, who's signed in on
localnetwork. Under the deck the status row says what's under way or the latest message, and where
the selection is and the space left on the drive where that means something. Under it the keys row
says what keys do, `enter` and `e` first, with `:` commands and `?` help always at its end; a chip
leads it when keys mean something else (searching, typing, picking, the pads).
On a smaller terminal, panes stacked in a column collapse to their title rows (from 120x34 down to
100x28), then the details pane shows only while it has focus (down to 80x24), then one pane has the
room and the rest are title rows (down to 60x20). Below that there's only the deck and the size it
needs.

- **Library**: what you can open (with rekordbox on this computer, the pane is called Sources and
  **rekordbox collection** comes first; then the mounted drives, hot-plugged ones appearing on
  their own, and an unplugged one marked), the playlist tree with folders, and the track list with
  BPM, key and rekordbox's track colours (the palette adds the rating and
  [My Tags](editing.md#my-tags) columns). Keys that mix with the playing track, or with nothing
  playing the selected one, show green with how they mix beside them: `=` the same key, `±` a step
  round the Camelot wheel, `↔` its relative. With a track playing, ΔBPM says how far each track's
  tempo is from it: the player has one deck, so the track playing is the master. Tracks heard for
  30 seconds since the player started are dimmed, and "forget what's been played" in the palette
  brings them back. Each row has its cover's two main colours, read as the row comes on screen.
  **Browse**, at the top of the tree, groups the drive like a CDJ's top menu:
  by artist, album, genre, key, BPM (5-BPM bands), rating, colour, month added, My Tag, the sets in
  the store (newest first, each with its tracks on this drive in the order played, and a count of
  the rest), the history lists players recorded, and problems.
- **rekordbox's collection**: choose it in the Sources pane (then `enter`) to browse the collection
  rekordbox keeps on this computer, read from `master.db` as [`collection`](other-software.md#rekordboxs-collection)
  reads it: its playlists, folders and history lists, with each track played from where it is on
  this computer and rekordbox's own waveforms, beat grids, cues, phrases and covers. It's
  read-only: edits, My Tags and drive actions say so, and `collection export` puts it on a drive
  to edit there. Intelligent playlists show empty. While it's open the player watches `master.db`
  (and its WAL): a couple of seconds after rekordbox saves, it reads the collection again and keeps
  your place, and `f5` reads it again at once. A drive holding a copy of the whole collection says
  `copy, <date>` in place of its track count. Choosing a drive in the same pane goes back to the
  drive.
- **Search and sort**: `/` searches as you type the way `tracks --filter` does (plain words look in
  title, artist, album, genre, label, key, comment, remixer, mix name and file path), and takes the
  whole [filter language](editing.md#filters) (`bpm:120-128 key:8A~ rating:>=4 -genre:house
  tag:Deep`); Tab in the search box widens it to the whole drive. Click a column header (or "sort"
  in the palette) to sort by title, artist, album, genre, BPM, key (round the Camelot wheel),
  rating, time, date added or play count; click again to reverse, a third time for no sort.
- **Track details** (`i`): everything rekordbox knows about the selected track (comment, label,
  remixer, mix, date added, plays, format, size), the playlists it's in, plugins' panels (what
  each shows about the track, asked for once the selection settles on it: "loading…" until it
  answers, and what went wrong in the messages when it fails), and the tracks that **mix into**
  it, which you can play or tag from there.
- **Queue** (`t`): what plays next, a CDJ's TAG LIST (in the code and the saved state it's still the
  crate). `q` puts the marked or selected tracks at the front of the queue, to play next, and `a`
  puts them at the end; marked tracks go in together, in the list's order. They work in the track
  list and search results, the details' matches, the queue itself and player mode's "mixes into"
  (where `enter` is `q`). Picks play in the order they're queued, after the playing track and before
  the rest of the list `enter` was pressed in, which the panel shows as "next from" with its next
  few tracks. Starting another list replaces only that; shuffle and repeat are its own, and never
  move a pick. Played picks stay, greyed, so the queue is a record of the set too. In the panel
  `shift ↑↓` moves a track, `del` takes it out and `enter` plays a track now (the others stay to
  play); the actions menu (`e`) saves it to the drive as a playlist (`w`) or clears it (`q`), and
  pack takes it onto another drive. **Fill** (in the palette, off until turned on): when the queue
  and the list both run out, the best match for the last track that hasn't played (as "mixes into"
  ranks them) goes on the end, marked *fill*, and plays. The queue, with what's played, is kept
  between runs.
- **Commands** (`:`): every action by name, with its key, or `Prepare` where it has none here (the
  word it goes by there finds it too: `restore`, `pull`); type
  a few of its letters (in order: `fsp` finds "fill the smart playlists") and enter. The ones run
  lately come first, kept between runs, and what can't run now stays in the list, greyed, with the
  reason. Plugins' commands and what they suggest are in it too. Each command also has words it's
  found by: "duplicates" finds the health report and replacing them, "MP3 to FLAC" upgrading to
  lossless files, "free space" what's using the drive.
- **The command line's jobs**, in the palette by name, on the open drive (the selected one in the
  Drives view): `doctor` (and `doctor --by-sound`), `drive space` and `plays stats` show their
  report in a panel, as a check does; `track dedupe`, `track relocate`, `track upgrade` and
  `refresh` show their plan first, and `u` takes them back. Relocating and upgrading ask for the
  folder to look in. `clean` is the demos plugin's operation, found by that name. What the player
  doesn't do itself (`drive format`, `drive fsck`, `drive firmware`, `provision`, `store gc`,
  `verify`, `prune` and `copy`, `bug`) shows the command to run in a terminal, the drive filled in.
- **Actions** (`e`): what can be done to the selection, the likeliest first, and enter runs it. In
  the library that's the marked or selected tracks, the playlist selected or the queue; in the Drives
  view the selected drive; elsewhere what enter does there, then the pane's own keys.
- **Help** (`?`): the keys that mean something here, the pane with focus first, and only keys: its
  top line says `:` searches everything. Type to narrow it to rows with those words, or to the key
  itself; enter runs the row selected, and `esc` closes it. When no key has the words typed, enter
  searches the palette for them.
- **Jobs** (`&`): what's running, with a cancel for each that can still stop. A write can stop until
  its databases are being written, and from then on it says it can't. A read of a drive just
  finishes, and says so. The status line shows the latest job's progress, and `esc` cancels it.
- **Hints**: "full hints", "compact hints" and "no hints" in the palette set how much the footer
  says. With no hints the footer still says `? help`, faintly; "no ? help with no hints" turns that
  off too. It's kept between runs, and `settings export` carries it with the palette's recent commands.
  Errors and a job's progress show whatever it's set to. An error stays until `esc`, and the messages
  (in the palette) keep it after that.
- **Plugins' commands and operations** are in the palette by title, with the plugin's name beside
  them, and where the plugin offers them: at the end of a drive's `e` menu in the Drives view, and
  of the actions menu (`e`) for those that work on tracks or a playlist. One that takes params
  asks for them in a form first (`↑↓` or `tab` between fields, `←→` or `space` to pick a choice,
  `enter` to run; lists take commas, a secret shows as dots), a param the plugin fills from what's
  selected starting as the marked or selected tracks, the playlist or the drive. It runs on the
  drive open, with the marked or selected tracks and the playlist selected, in the background
  (`esc` stops it). What a command answers shows in a panel (a list's rows as they come, when the
  plugin sends them so), or as a message when it's a line; an operation's
  plan, with what the plugin says about it first, is confirmed and written like any edit, and `u`
  takes it back. A question one asks first (which track an entry is) is a list to pick from
  (`space` marks items when it takes several, `enter` takes them, `esc` stops the run). An
  operation, or a command of a plugin that reads the drive, needs a drive open (a folder opened
  by path too, which the plugin can read only while it runs on it). What plugins log goes to the
  messages. See
  [Plugins](plugins.md).
- **Analyzers**: "analyze the tracks: …" in the palette (or a drive's `e` menu) runs a plugin's
  analyzer over the drive's tracks in the background, those not analysed already. "show the
  sound.bpm column" adds a column of what it worked out (remembered for the next run; `-` where
  it found nothing to show; shown as its analyzer says, in bytes or as a meter, say), sorted by a click on its header like any column, and the search box
  filters by it: `sound.bpm:>120`. Sorted by it, a playlist is sorted on the drive by it too
  (`s` in the actions menu). "write keys and BPMs to the tracks: …" writes what an
  analyzer heard to the tracks (a form asks which, and whether over what they have), confirmed
  like any edit.
- **Checks**: a drive's check (in its `e` menu) shows what plugins' checks found too, and a fix
  one gives is in the palette until the next check ("fix on DRIVE: …"), planned and confirmed
  like any plugin's operation. What a guard says about changes is among the warnings when they're
  confirmed, and a blocking one that found something stops them. "check settings…" in the palette
  says which checks run, which guards stop changes, and how long a check has (as `plugin checks`
  does).
- **Imports**: "import …" in the palette (or a drive's `e` menu) reads a file with a plugin's
  importer (a form asks for the file and the format's options), then shows what it adds to the
  drive's tracks and the histories it keeps, with how many tracks it couldn't find, confirmed like
  any edit.
- **Providers**: "fetch cover art…" and "fetch track details…" in the actions menu (`e`) and the
  palette ask the plugins' providers about the marked or selected tracks (a form says which
  provider, or all, and whether over what they have). A track's candidate scoring high enough
  alone is chosen for you, and for each other track you pick one from a list (or none); then the
  changes are confirmed like any edit. "provider settings…" in the palette sets how high (as
  `plugin providers --auto-pick` does).
- **Library sources**: "build from …" in the palette (or a drive's `e` menu) builds the drive
  plugged in from a plugin's library source: a form asks which folders it may read files in
  (`~/Music` to begin with, several split by commas), whether to replace a library the drive
  holds, and for the source's options; the plan says what it read and left out, and is confirmed
  like any change.
- **Plugins' settings**: "Cover art plugin settings…" in the palette opens a plugin's settings in
  the same form, as they're set now; `enter` saves what changed, and a field left empty goes back
  to its default. A secret is never filled in: typed, it's kept in the system's keychain; left
  empty, it stays as it was. A plugin's services are switches there too: turned on, they run
  while the player does.
- **Accounts**: "sign in to a Google account…" in the palette signs in in the browser, in the
  background (for a service that gives you keys, a form says where to get them and takes them
  unseen), and "sign out of NAME" signs out, removing every plugin's access to it. Signing in
  again to it opens a form: `enter` restores that access, or pick Choose (typing each plugin's
  scopes) or Reset. "access for PLUGIN…" opens a form with the
  account of each type the plugin uses, what it needs it for, and signing in to another; `enter`
  makes the ones chosen its defaults (see [Accounts](plugins.md#accounts)).
- **What plugins tell you**: a message, and when a plugin suggests one of its operations (on a
  drive plugged in, say), "review: …" in the palette plans it, confirmed like any edit; when it
  suggests fetching from its providers, exporting in its format or running its analyzer, "review:
  …" opens that form (or asks first, for an analyzer) filled in, as if you'd started it, saying
  which drive and how many tracks (a mix's export is refused: the player doesn't export mixes).
  What `watch` kept while the player was closed is offered as it opens.
- **Problems**: ✖ marks a track whose audio file isn't on the drive, ≈ one the drive holds another
  copy of. Browse › Problems lists them.
- **Playback** of MP3, AAC/ALAC (`.m4a`), FLAC, WAV and AIFF, with a queue: your picks, then the
  list you started in, with shuffle and repeat. The beat counter, BPM and hot cues come from the
  track's rekordbox analysis, and `c` and `h` jump to the next and previous cue. Tracks follow each
  other without a gap: half a minute before one ends, the next is opened and queued behind it, so a
  continuous mix or a live album plays straight through (a next track at another sample rate starts
  the moment the playing one ends, without the wait for its file). An MP3 over 40 MB (a recorded
  set) seeks by its place in the file, which is quick but can land a few seconds off in a VBR file.
- **Play modes and output**, in the palette: "single play" turns autoplay off (as on a CDJ): the
  player stops at the end of each track with the next one ready for `space` (again plays on). "auto
  cue" starts each track at its first memory cue, else its first beat, where a CDJ would cue it,
  rather than at the silence before. "output" (or "headphones") lists the outputs to play through:
  the system's default, or another such as headphones or an audio interface. The track carries on
  there from where it was, and the player uses that output from then on; while it's unplugged, it
  plays through the default and says so. An output that goes away mid-track (headphones
  unplugged) is let go of, and the track waits where it was: `space` plays on through whatever
  output there is then.
- **Player mode** (`f`, from any tab): the track's waveforms and numbers on the whole screen, as
  on a CDJ's display, and only what a set needs: the file, tags and counts are in the info panel
  (`i`), and the status line names only the modes that are on. `b` opens Browse
  over it, as a CDJ's BROWSE button does: the panes and the strip, until a
  track's loaded or `esc`. `1` to `8` stay hot cues while Browse is open. The layout's kept
  between runs.
  - The zoomed waveform scrolls past a playhead fixed in the middle, under a beat ruler with stronger
    bar starts and bar numbers, sitting on the waveform, each bar's line carried down through it.
    "zoom in" and "zoom out" in the palette zoom it, from 1.5 to 60 seconds across.
    Drag it left or right to scrub: the track moves under the playhead with the pointer, held while
    it moves, and plays on from where it's let go of (on the nearest beat with quantize on). A
    click alone doesn't seek.
  - The overview stays still while its playhead moves, in eighths of a cell, the played part dimmed.
    Click it to seek.
  - The playhead is where the track is heard: the output plays what it's handed a buffer and its
    own latency later (tens of milliseconds through speakers, a few hundred through Bluetooth),
    and the player takes that off, as the output reports it. So a loop made with quantize off, the
    cue flashes and the beat count follow what you hear; on pause the playhead stops where the
    sound does. Jumps that keep the beat go by what's been handed to the output, so the rhythm
    carries on through them. Phrases run under it with rekordbox's labels, as colour alone where a label
    wouldn't fit whole.
  - Hot cues are lettered in their own colours, in a row over each waveform, with a second row where
    two would share a cell or sit side by side, and loops are a tint.
  - The tempo and time left are large under a BPM title, with the key beside them. Under 78 columns
    the time left is a small line by the tempo. A line under the track's names (under the waveforms
    below 34 rows) says playing or paused and how many bars and beats it is to the next cue and
    phrase, as bars.beats; the volume and the modes that are on are at the right of the status
    line. The art, the visualiser and the tempo are the same height. Near the end of a track the time left turns red and
    blinks, and END shows. As the warning starts, the overview washes red twice (in fewer than
    24-bit colours it snaps to red); with `LOCALLIBRARY_REDUCED_MOTION` nothing blinks or washes.
  - `w` (or `waveform colours` in the palette) goes round rekordbox's waveform colours: BLUE, RGB,
    3Band, and as the drive's My Settings say. A drive with no `DEVSETTING.DAT` gets 3Band, and a
    track with no waveform of that kind (older analysis has only blue) the nearest it has. The mode
    drawn is named in the line under the waveforms.
  - From 34 rows, what mixes into the track (compatible key on the Camelot wheel, tempo within 6% or
    half/double time), the playlists it's in and a summary of its analysis are under the waveforms.
    A track with no detail waveform gets a line saying so, and these take its rows at any height.
    The matches take turns by how they mix in (same key, ±1, relative, energy boost), closest tempo
    first, so the first rows show each kind there is. `locallibrary track matches` lists the same
    tracks best first instead: every same-key track, then the rest.
    `↑` `↓` pick one of those tracks and `enter` (or a double-click, or `q`) queues it to play next,
    after any picks queued to play next before it, and `a` puts it at the end of the queue. What
    plays next is marked *next* in the list.
  - With 9 rows or more for the top of the screen and room to spare, "visualiser" in the palette
    picks what's beside the tempo: a log-frequency spectrum with peak caps, a mirrored spectrum, an
    oscilloscope, or nothing.
- **What the deck looks like** (`:`, all kept between runs and in `settings export`):
  - Keys are written in Camelot (`8A`), Open Key (`1m`) or as notes (`Am`), everywhere the player
    writes one.
  - Key colours come from the theme, a colour wheel, or a palette made for protanopia, deuteranopia
    or tritanopia (Mixxx's). The marks beside keys that mix say the same without colour.
  - The warning before a track ends is at 15, 30 (unless you change it), 45 or 60 seconds, or off.
  - Cover art can be turned off.
- **Cover art**: the terminal's own graphics where it has them (kitty, iTerm2 or sixel), in half
  blocks everywhere else, and a record for a track with none. A cover fills its space: a big one
  is shrunk smoothly, and a small one grown in whole pixels, so it stays sharp. There's no art
  with 16 colours or ASCII icons, or inside tmux beyond half blocks. When what's worked out is
  wrong, "cover art in half blocks" in the palette keeps it in half blocks from then on (kept for
  the next run), and `LOCALLIBRARY_ART` names the graphics to use (`kitty`, `iterm2`, `sixel`,
  `halfblocks`) for one run, over the setting.
- **Hot cue pads**: in player mode the number keys `1` to `8` play hot cues A to H the way a
  CDJ's pads do: the track jumps to the cue and plays (from pause too: a terminal can't tell when
  a key is let go, so there's no previewing while it's held). "pads" in the palette turns them on
  in every view, until it's run again (the digits go to views only while they're off).
  Each hot cue shows its key while the pads are on.
- **Beat jumps and quantize**: `,` and `.` jump a beat back or forward along the track's beat
  grid, and the palette jumps a bar or eight bars (a phrase); the arrows still seek by time.
  Quantize (in the palette, on until turned off, shown as `quantize` by the clock) keeps jumps on the
  grid: while playing, cue, hot cue, phrase and beat jumps land as far before or after the beat as
  the playhead was, as if they had waited for the nearest beat, so the rhythm carries on; paused,
  beat jumps land on a beat. A track without a beat grid jumps by its BPM.
- **Loops**: `l` loops the loop the playhead is in (a saved one, or the last one exited), or makes
  a bar of 4 beats from the nearest downbeat, so it's on the bar even pressed a little early or
  late (from the playhead with quantize off); `l` again exits it and playback carries on. "halve
  the loop" and "double the loop" in the palette work along the grid, and a hot cue saved as a loop
  plays its loop. Seeking out of a loop leaves it. The loop repeats on the audio thread, to the
  sample: the first time round it's kept in memory and then replayed from there, so going round
  doesn't read the drive. The player shows `LOOP` with its length in beats, and a loop made in the
  player is drawn like a saved one.
- **Phrases**: rekordbox's phrase analysis (Intro, Up, Down, Chorus, Verse, Bridge, Outro) shows
  as the current phrase and bar in the player, with a countdown to the next one, and as a strip
  under player mode's overview. "next phrase" and "previous phrase" in the palette jump to them.
- **Cues**: memory cues and loops take the colours they were given in rekordbox, and loop lengths
  are the ones rekordbox stored (½ beat, 4 beats), not worked out from the tempo. The visualiser
  flashes in a cue's colour as the playhead crosses it, and the player shows the next cue with a
  countdown.
- **Changing the drive** (opt-in): `e`, or right-click, opens actions for the marked or selected
  tracks, the playlist shown and the queue: set the rating or colour, give them a My Tag, add them
  to a playlist (picked in the tree), remove them from the playlist, create, rename or delete a
  playlist or folder, sort a playlist on the drive the way the list is sorted, save the queue, bring
  a playlist back from a snapshot, or replay recorded edits. A My Tag (`t` in the menu) is one the
  drive has or a new `Category/Tag`, and the same item takes off one the tracks all have. My Tags
  are read once as the drive loads, so after changing them elsewhere, `f5` reads them again. Every
  change first shows its plan and is written only when you press `y`, the same change the
  [commands](editing.md#editing-playlists-and-tracks) make (copied onto the library's other drives
  plugged in), with a backup first; `u` takes the last one back, after showing what that puts back
  (the playlists and tracks, the files, the recorded edits that come off the journal).
- **After a rekordbox export**: when rekordbox exported to the drive after locallibrary last wrote
  to it, and replaying the [edits recorded on it](editing.md#when-rekordbox-exports-again) would
  change something, a line over the status line says "rekordbox exported over 3 edits on REKORDBOX.
  Replay?": `y` shows the replay to confirm, `esc` puts the offer away for the run.
- **Rule-based playlists**: in the palette, "smart new" makes a
  [rule](editing.md#rule-based-playlists) from the search box's filter and the list's sort (saved in
  the store, then shown filling its playlist on the open drive), "smart apply" fills every rule on
  the drive, and "smart" lists them.
- **Prepare** (`2`): the jobs that get a drive ready for a set, each with what it does. `enter`
  runs the one selected, on the drive open in the library, the same as in the palette: check it,
  check every drive of its library together, sync, pack, save a snapshot, fill the rule-based
  playlists, read it again.
- **Drives** (`3`): each library with its drives under it (how many are plugged in, whether they're
  identical and saved), then the drives of no library. On a library, `enter` opens its newest drive
  in the library view, and `e` syncs (`g`), checks (`a`) or ejects (`d`) its drives plugged in; the
  pane beside it lists its drives, those not plugged in too. On a drive: identity, library summary,
  space, store status, role, logbook (writes, clean ejects, the last verify and file-system check,
  problems), copies, My Settings files and interrupted operations, and the work: `e` lists what can
  be done to the selected drive, each planned in the background (with progress in the status line;
  `esc` stops it) and confirmed like an edit. **Pull** saves it in the store. **Check** runs the
  checklist of `locallibrary check` against the player models remembered for it, and **set prep**
  checks it with the other mounted drives of its library, with the results in a panel. **Sync**
  copies the newest drive of its group over the others plugged in, or says why none is newest.
  **Pack** puts the marked playlists (`m` in the tree), the playlist shown or the queue (saved as a
  playlist first) from the drive open in the library onto it, for a set. **Restore** brings back a
  playlist or folder from a snapshot picked from its library's. **Replay** makes recorded edits
  again, **undo** takes its last change back (`u` undoes the last change made this run, on whichever
  drive it was made to, which may not be the selected one), and **eject** unmounts it and powers it
  off (playback from it stops first). Beside the list are **Snapshots**, the store's snapshots,
  where `enter` brings a playlist back from the selected one onto the drive open in the library, and
  **History**, what players recorded on the drive open in the library, where `enter` opens a list
  there.
- **localnetwork** (`5`): your [DJ profile](sets.md#dj-profile): an overview of your collection
  (tracks, genres, artists, release years, and how many sets; rekordbox's own when it's on this
  computer, built again when you come back to the tab after rekordbox saved), then the sections
  that can be published (Sets, Energy, Genres, Artists, Era and Collection), with what each would
  reveal. The actions menu (`e`) and the palette put a section in the public document or take it
  out, show the document and write it, hide an artist and round the numbers. The bar at the top
  shows your [Atmosphere account](account.md): `l` in the actions menu signs in (type your handle,
  approve in the browser), and signed in, signs out after asking.
- **Export**: in the palette, "export" writes the playlist, set, history list or all tracks shown as
  M3U8, CSV or text, asking where first. A plugin's format ("export for traktor…", or "export for
  rekordbox…" for rekordbox XML, in a drive's `e` menu too) asks where and for the format's options in a form, then shows what it would write (each
  file, what goes in, what the format can't hold) and writes it on `y`, asking first when that
  overwrites files there already; it takes the marked playlists too. A set's format ("export the
  set for …") writes out the set open in Browse › Sets.
- **Recordings** (`4`): a recorded set and the tracks it plays, like
  [`locallibrary mix`](sets.md#mixes) against the drive open in the library. Pick one of the drive's
  recordings (XDJ-AZ and OPUS-QUAD MASTER REC files), or `f` in the actions menu (`e`) to type the
  path of a recording or a folder of them on this computer. A folder stays in the list with every
  recording in it and its subfolders, new ones included, and recordings opened are remembered; `del`
  forgets one, or the folder it was found in. The view draws the set's 3-band waveform with each
  track's lane under it (shaded by how clearly it's heard, so blends and EQ swaps show), the tempo
  played, the keys on the way, and the tracklist with every handoff: blend, quick mix or gap, key
  move and tempo change. The selected track's own waveform, cues and phrases show the stretch that
  was played. `enter` or `space` plays the set in the player, and the timeline and tracklist follow
  along; `n`/`p` jump between its tracks, a click on the timeline seeks. The actions menu (`e`)
  exports the set as data for [Local Network](sets.md#mixes) (`x`) and writes the HTML report and
  opens it; zooming the timeline and going to the selected track in the library are in the palette.

Each view drawn in a terminal of two sizes: Library ([80x24](player/library-80x24.svg), [160x45](player/library-160x45.svg)), Prepare ([80x24](player/prepare-80x24.svg), [160x45](player/prepare-160x45.svg)), Drives ([80x24](player/drives-80x24.svg), [160x45](player/drives-160x45.svg)), Recordings ([80x24](player/recordings-80x24.svg), [160x45](player/recordings-160x45.svg)), localnetwork ([80x24](player/localnetwork-80x24.svg), [160x45](player/localnetwork-160x45.svg)).

Colours are the theme's: the default, or a plugin's chosen in the palette ("Night theme",
"default theme"), kept for the next run in `tui.json`.

As it starts, the player asks the terminal what it can draw and what its colours are, and is done
as soon as the terminal has answered, or after 100 ms at most (on a slow SSH connection, say). It
goes by the terminal's real background: a light one gets the light palette, and anything that
wouldn't read on the background, at 4.5:1 for text and 3:1 for waveforms and markers, is blended
towards the foreground until it does. Without an answer, `COLORFGBG` (set by rxvt, Konsole and
others) says which it is, and otherwise it's dark. `LOCALLIBRARY_THEME=light` or `dark` overrides
all of that.

| Colours | Who gets them |
| --- | --- |
| 24-bit | A terminal that says so, `COLORTERM=truecolor`, Terminal.app from macOS 26 on and others known to draw it |
| 256 | `TERM` with `256color` in it |
| 16 | Any other `TERM`: `xterm`, `linux`, `vt100` |

If the terminal says the wrong thing of itself, pick "24-bit colour", "256 colours" or "16 colours"
in the palette ("colours as the terminal says" goes back), or set `LOCALLIBRARY_COLOR=truecolor`,
`256` or `16`, which wins over the palette. 256 colours stay within the cube and greys (16-255), so
a terminal's scheme can't change them. A track that loads and a panel that opens fade in, in 24-bit
and 256 colours. 16 colours are the scheme's own: text is the terminal's foreground, quieter text is
dim, the selection and chips are in reverse video, the focused pane has a heavy border, track
colours show as their numbers, and nothing fades.

Icons come in three sets. "Nerd Font icons", "Unicode icons" and "ASCII icons" in the palette
choose one, kept in `tui.json`, and `LOCALLIBRARY_ICONS=nerd`, `unicode` or `ascii` wins over that.
Without a choice it's Unicode; ASCII on the Linux console, `TERM=vt*` and a locale that isn't
UTF-8, where everything else on screen becomes ASCII too, line drawing and waveforms included; and
Nerd Font icons in the terminals that bring the font along (Ghostty, kitty, WezTerm).

With [`NO_COLOR`](https://no-color.org) set, the player draws no colours and asks the terminal
nothing: the selection is in reverse video, the focused pane has a heavy border, keys that mix are
marked `⇄` and underlined, track colours show as their numbers (1 pink to 8 purple) and cover art
is left out. `LOCALLIBRARY_REDUCED_MOTION=1` redraws only a few times a second and stops the
spinning record, the pulsing logo, the cue flashes and the fades.

Cover art is drawn in half blocks, in as many colours as the terminal has. The player remembers the drive, list, view, sort, volume, visualiser,
panels and columns, colours and icons, shuffle, repeat, quantize, single play, auto cue and fill, the output,
and the queue (its picks, which have played, and the list it plays on with) with how far into its track it got, to the last 15 seconds (`space` carries on
from there). It saves them as they change, in the
store once there is one and in a `locallibrary-player` folder beside it until then. A file it
can't read as it starts (a newer version's, say) is left as it is: that run uses the defaults and
saves nothing. Messages show in the status row: errors stay until `esc`, and "messages" in the
palette lists every message since starting. The keys row hints at the keys for where focus is, as
many as fit. `?` lists every key, the view's own first (`:` has every command);
all of them, dialogs' too, come from one table, so a key means one thing in every view (`l` loops,
`a` adds to the queue, `e` opens the actions).

## Your own keys

Any action can have the key you want, and one without a key can get one. "change the keys" in the
palette (`:`), or its row in the help, opens the Keys screen: every action under the help's
headings, then plugins' commands and operations, then the keys that can't change. Typing narrows
the list by name, what it does, its id or its key. `enter` on an action waits for the new key, then
says what that key does now and what it would do in each place (a view, a pane, player mode, a
dialog) before anything is kept: `enter` keeps it, `esc` goes back. When another action has the key
where this one works, it's offered as a swap, the other action taking this one's keys; a key two
others have is refused. Pressing the key an action came with puts it back as it came, and "all keys
back to their defaults" in the palette puts every key back, after listing what changes.

A key is one character as typed (a capital is shift and the letter), a named key (`space`,
`pageup`, `f5`), `shift-` and a named key (`shift-up`), or `ctrl-` and a letter. `alt-` keys and
keys pressed one after another can't be used. Nor can `esc`, `ctrl-c`, `?` and `:`, or `ctrl-h`,
`ctrl-i` and `ctrl-m`, which terminals send as `backspace`, `tab` and `enter`. The keys the player
comes with stay single lowercase keys. Wherever the player shows a key (the footer, the help, the
palette, the tab numbers, the actions menu's letters) it shows yours; the tables below are the keys
it comes with.

A plugin's commands and operations are on the Keys screen too, with ids like
`plugin.sound.analyze`, and a key set for one works in every view. Keys that earlier versions kept
in plugins' settings (`keys.<name>`) move into `keybinds.json` the first time the player starts,
and are taken out of the plugins' settings.

The keys you set are kept in `keybinds.json`, beside `tui.json`: each changed action by its id with
its keys in order (the nth key of `deck.hot-cue` plays the nth hot cue), then `from` and
`updatedAt`. An empty list means the action has no key, and an id this version doesn't know is kept
but not used. [`keybinds.schema.json`](keybinds.schema.json) describes the file, and
[`settings export`](settings.md#taking-your-settings-to-another-computer) carries it.

```json
{
  "keys": { "playback.next": ["]"], "player.jobs": [], "plugin.sound.analyze": ["f6"] },
  "updatedAt": "2026-11-20T18:00:00Z"
}
```

Signed in to your [Atmosphere account](account.md), the player publishes your keys as a
`computer.localnetwork.locallibrary.keybinds` record a couple of seconds after the last change (a
session from before the player published keys is told to sign in again). Signing in on another
computer loads them. When that computer has keys of its own that differ, it lists each action that
differs (`yours → theirs`) with every row marked instead: `space` unmarks one, and `enter` takes the
marked ones after asking. "import someone's keys…" in the palette shows the same list for anyone's
keys, by handle or DID, read through Local Network's directory, and `from` keeps whose they were.
"keys for an AZERTY keyboard" shows it for the views and hot cues on `& é " ' ( - è _`, which leaves
the jobs without a key. Signed out, your keys stay on this computer.

[//]: # (The key tables are generated from the keymap: see src/tui/docs/tests.rs.)

## Keys in Library

Browsing:

| Keys | |
| --- | --- |
| `/` | search title, artist, album, genre, label, key, comment and path as you type |
|  | or filter: bpm:120-128 key:8A~ rating:>=4 -genre:house tag:Deep |
|  | in the box: ↑↓ and pgup pgdn move the list, enter keeps it, esc clears it, ctrl-u empties it, ctrl-w deletes a word, tab searches the whole drive or the list shown |
| `Browse` | at the top of the tree: by artist, album, genre, key, BPM, rating, colour, date added, My Tags, sets, history, problems |
|  | green keys mix with the playing track, or the selected one (⇄ its key) |
| `i` | the selected track: details, playlists, what mixes into it (esc closes it) |

Marks and the queue:

| Keys | |
| --- | --- |
| `m  insert` | mark / unmark the selected track (in the tree: a playlist or folder, to pack) |
| `a` | add the marked (or selected) tracks to the end of the queue |
| `t` | show / hide the queue (esc closes it) |
| `enter` | in the queue: play that track now (the others stay to play) |
| `shift ↑↓` | move a track up / down the queue |
| `del` | in the queue: take the track out |

Changing the drive:

| Keys | |
| --- | --- |
|  | The drive is backed up before a change is written. A change that u takes back is made at once and says so; any other shows what it will do and asks first. |
| `e` | actions for the marked or selected tracks, the playlist shown and the queue, the likeliest first (enter runs it): rating, colour, add to a playlist, remove, new / rename / delete a playlist or folder, sort a playlist on the drive, plugins' operations |
| `right-click` | the same, for what's under the pointer |
| `y` | after rekordbox exported over edits made here: replay them (it says so over the status line; esc puts it away) |
| `del` | remove from the playlist shown; delete the selected playlist |

By name, in :

| Keys | |
| --- | --- |
| `:` | open a folder holding a rekordbox export, by its path |
| `:` | bring back the tracks dimmed as played since starting |
| `:` | sort by the next column / the other way (or click a header) |
| `:` | show / hide the rating column |
| `:` | show / hide the My Tags column |
| `:` | mark everything shown / clear the marks |
| `:` | write the queue to the drive as a playlist (played tracks too) |
| `:` | clear the queue (the drive isn't touched; the list it plays on with stays) |
| `:` | give the marked (or selected) tracks a My Tag, or take one off (asks first) |

## Keys in Prepare

Prepare:

| Keys | |
| --- | --- |
| `enter` | run the job selected, on the drive open in the library |
| `e` | the same jobs, each with its letter |

## Keys in Drives

Drives:

| Keys | |
| --- | --- |
| `enter` | open the drive in the library (a library: its newest clone) |
| `e` | what can be done to the selected drive: save a snapshot, check, sync, pack, bring back a playlist, replay edits, undo, eject, plugins' commands; to a library's clones: sync, check, eject (each shows its plan and asks first) |

Snapshots:

| Keys | |
| --- | --- |
|  | every library in the store, its snapshots newest first (locallibrary pull adds one) |
| `enter` | bring back a playlist from the selected snapshot onto the drive open in the library |
| `e` | actions for the selected snapshot |

History:

| Keys | |
| --- | --- |
| `enter` | open the list players recorded in the library |
| `e` | actions for the selected list |

## Keys in Recordings

Recordings:

| Keys | |
| --- | --- |
|  | A recorded set, and which of the open drive's tracks it plays (like locallibrary mix). |
| `enter` | on a recording: find its tracks; on a track: play the set from there |
| `e` | actions for the selection, with exporting the set |
| `space` | play the set, or pause it |
| `n  p` | while the set plays: its next / previous track |
| `del` | on a recording from this computer: forget it, or the folder it was found in |
| `click` | on the timeline: play from there, or pick the track under it |

By name, in :

| Keys | |
| --- | --- |
| `:` | open a recording by path, or a folder of them (subfolders too) to keep listed |
| `:` | zoom the timeline: the whole set, 2×, 4×, 8× |
| `:` | go to the selected track in the library |
| `:` | write the HTML report and open it in a browser |

## Keys in localnetwork

localnetwork:

| Keys | |
| --- | --- |
| `e` | these actions, for the section selected, with hiding an artist and rounding |
| `tab  enter` | move between sections and their lists (esc goes back) |

By name, in :

| Keys | |
| --- | --- |
| `:` | put the section in the document, or take it out |
| `:` | show the document a public profile would get |
| `:` | write the document as JSON in the store folder |
| `:` | sign in with your Atmosphere account (the one for Bluesky and other AT Protocol apps; alpha); signed in, sign out (asks first) |

## Keys everywhere

Playback:

| Keys | |
| --- | --- |
| `enter` | play the selected track (queues the list it's in); open a playlist or folder; browse a drive |
| `space` | pause / resume |
| `← →` | seek 5 s (shift: 30 s) |
| `n  p` | next / previous track |
| `c  h` | jump to the next / previous cue |
| `+  -` | volume |
| `q` | play the marked (or selected) tracks next, before the other picks (in player mode, the match picked) |

Deck:

| Keys | |
| --- | --- |
| `1 … 8` | in player mode (browsing over it too), or with the pads on: hot cue A … H (jumps there and plays) |
| `,  .` | beat jump: one beat back / forward |
| `l` | loop: the saved loop the playhead is in, else 4 beats from the nearest bar; again to exit |
|  | A hot cue saved as a loop plays the loop. Seeking out of a loop leaves it. |

Visuals:

| Keys | |
| --- | --- |
| `w` | in player mode: the waveform's colours, BLUE, RGB, 3Band, as the drive says |
| `b` | browse the library over player mode; loading a track or esc comes back |
| `f` | player mode: the playing track on the whole screen, from any tab; again (or esc) to go back |

Moving about:

| Keys | |
| --- | --- |
| `↑ ↓  j k` | move |
| `pgup  pgdn` | a page up / down |
| `home  end` | the first / last row |
| `tab` | next pane (shift-tab: back) |
| `esc` | back one step: leave player mode, stop a change before it writes, hide an error, clear the search, close the details or the queue, then clear the marks (twice); with nothing left, twice quits |

The player:

| Keys | |
| --- | --- |
| `f5` | read again: the drive, the drives' status, the snapshots, the profile, the recordings |
| `1` | Library (in player mode the digits are hot cues) |
| `2` | Prepare |
| `3` | Drives, their snapshots and history |
| `4` | Recordings |
| `5` | localnetwork |
| `:` | commands: every action by name (type a few of its letters), the ones used lately first, plugins' commands and suggestions |
| `?` | this help: type to narrow it, enter runs the row selected |
|  | with no hints the footer still says ? help; "no ? help with no hints" in : turns that off too |
| `&` | jobs: what's running, with a cancel for each that can still stop |
| `ctrl-c` | quit, also esc twice with nothing open, or ctrl-q (a write to the drive finishes first) |

Changing the drive:

| Keys | |
| --- | --- |
| `u` | undo: take back the last change made, on the drive it was made to (before any, the open or selected drive's last), after showing what that puts back |

In a dialog:

| Keys | |
| --- | --- |
| `/` | in the actions menu (e): each action's letter runs it, and / filters it by what's typed |
| `y  n` | in a plan or a question: yes (once it's been up long enough to read) / no |
| `space` | in results or the messages: a page down |
| `tab` | in a form: the next field (shift-tab: back), ←→ or space change a choice |
| `space` | in a list some of which may be chosen: mark one |
| `x` | in the jobs: cancel the one selected |
| `↑ ↓  j k` | move; pgup pgdn a page, home end the ends (in a list you type into, the arrows and those keys only) |
| `esc` | close a dialog (cancel a plan, a prompt, a search) |
| `enter` | do what's selected |

By name, in :

| Keys | |
| --- | --- |
| `:` | jump to the next / previous phrase |
| `:` | shuffle / repeat (off, all, one) |
| `:` | stop |
| `:` | go to the playing track |
| `:` | pads on / off: the digit row plays hot cues instead of going to the views |
| `:` | one bar (4 beats) back / forward |
| `:` | 8 bars (32 beats, a phrase) back / forward |
| `:` | quantize on / off: playing, cue, phrase and beat jumps keep the beat; paused, beat jumps land on one |
| `:` | halve / double the loop |
| `:` | autoplay off (single play): stop after each track, the next one ready for space; again to play on |
| `:` | auto cue: tracks start at their first memory cue, else their first beat |
| `:` | the output: the system's default, or another (headphones, an interface), remembered |
| `:` | spectrum · mirror · scope · none, in player mode where there's room |
| `:` | zoom the detail waveform out / in |
| `:` | messages: everything the status line said since starting |
| `:` | change any key, or give an action one: pick it and press the new key |

[//]: # (End of the key tables.)

Mouse: click to select (or a tab to switch views, or a pane's title to open it), double-click to play, scroll to move, click a
column header to sort, click or drag the overview to seek, drag the zoomed waveform to scrub, right-click in the library for
actions.
