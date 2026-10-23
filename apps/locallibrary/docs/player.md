# The player

`locallibrary` on its own (or `locallibrary tui [<library>]`) opens a terminal UI for browsing and
listening to your library the way the players see it:

- **Library**: mounted drives (hot-plugged ones appear on their own, and an unplugged one is
  marked), the playlist tree with folders, and the track list with BPM, key and rekordbox's track
  colours (`*` adds the ratings, `#` the [My Tags](editing.md#my-tags)). Keys that mix with the
  playing track, or with nothing playing the selected one, show green: the same key, ±1 or relative
  on the Camelot wheel. **Browse**, at the top of the tree, groups the drive like a CDJ's top menu:
  by artist, album, genre, key, BPM (5-BPM bands), rating, colour, month added, My Tag, the sets in
  the store (newest first, each with its tracks on this drive in the order played, and a count of
  the rest), the sessions players recorded (their history lists), and problems.
- **Search and sort**: `/` searches as you type the way `tracks --filter` does (plain words look in
  title, artist, album, genre, label, key, comment, remixer, mix name and file path), and takes the
  whole [filter language](editing.md#filters) (`bpm:120-128 key:8A~ rating:>=4 -genre:house
  tag:Deep`); Tab in the search box widens it to the whole drive. Click a column header, or press
  `b`, to sort by title, artist, album, genre, BPM, key (round the Camelot wheel), rating, time,
  date added or play count; click again to reverse, a third time for no sort.
- **Track details** (`i`): everything rekordbox knows about the selected track (comment, label,
  remixer, mix, date added, plays, format, size), the playlists it's in, plugins' panels (what
  each shows about the track, asked for once the selection settles on it: "loading…" until it
  answers, and what went wrong in the messages when it fails), and the tracks that **mix into**
  it, which you can play or tag from there.
- **Crate** (`t`): a CDJ's TAG LIST, called the crate here so it isn't taken for My Tags. Collect
  tracks while browsing (`a` adds the marked or selected ones; `m` marks), reorder them, play the
  crate, save it to the drive as a playlist, or pack it onto another drive for a set.
- **Commands** (`:`): every action by name, with its key; type a few of its letters (in order:
  `fsp` finds "fill the smart playlists") and enter. It also has the commands with no key of their
  own: pull, check, set prep, sync, pack, restore, replay, export, smart, smart apply, smart new
  and eject.
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
  operation, or a command of a plugin that reads the drive, needs one plugged in, not a folder
  opened by path. What plugins log goes to the messages (`!`). See
  [Plugins](plugins.md).
- **Analyzers**: "analyze the tracks: …" in the palette (or a drive's `e` menu) runs a plugin's
  analyzer over the drive's tracks in the background, those not analysed already. "show the
  sound.bpm column" adds a column of what it worked out (remembered for the next run; `-` where
  it found nothing to show; shown as its analyzer says, in bytes or as a meter, say), sorted by a click on its header like any column, and the search box
  filters by it: `sound.bpm:>120`. "write keys and BPMs to the tracks: …" writes what an
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
  drive's tracks and the sessions it keeps, with how many tracks it couldn't find, confirmed like
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
- **What plugins tell you**: a message, and when a plugin suggests one of its operations (on a
  drive plugged in, say), "review: …" in the palette plans it, confirmed like any edit; what
  `watch` kept while the player was closed is offered as it opens.
- **Problems**: ✖ marks a track whose audio file isn't on the drive, ≈ one the drive holds another
  copy of. Browse › Problems lists them.
- **Playback** of MP3, AAC/ALAC (`.m4a`), FLAC, WAV and AIFF, with a queue built from the list you
  started in, shuffle and repeat. The beat counter, BPM and hot cues come from the track's
  rekordbox analysis, and `c` jumps between cue points. Tracks follow each other without a gap:
  half a minute before one ends, the next is opened and queued behind it, so a continuous mix
  or a live album plays straight through (a next track at another sample rate starts the
  moment the playing one ends, without the wait for its file). An MP3 over 40 MB (a recorded set)
  seeks by its place in the file, which is quick but can land a few seconds off in a VBR file.
- **Play modes and output**: `S` turns autoplay off (single play, as on a CDJ): the player stops
  at the end of each track with the next one ready for `space` (`S` again plays on). `A` turns on auto
  cue: each track starts at its first memory cue, else its first beat, where a CDJ would cue
  it, rather than at the silence before. `d` lists the outputs to play through: the system's
  default, or another such as headphones or an audio interface. The track carries on there
  from where it was, and the player uses that output from then on; while it's unplugged, it
  plays through the default and says so. An output that goes away mid-track (headphones
  unplugged) is let go of, and the track waits where it was: `space` plays on through whatever
  output there is then.
- **Hot cue pads**: in full screen the number keys `1` to `8` play hot cues A to H the way a
  CDJ's pads do: the track jumps to the cue and plays (from pause too: a terminal can't tell when
  a key is let go, so there's no previewing while it's held). `'` turns the pads on in the
  library as well, until it's pressed again (`1` to `5` switch views only while they're off).
  Each hot cue shows its key while the pads are on.
- **Beat jumps and quantize**: `,` and `.` jump a beat back or forward along the track's beat
  grid, `<` and `>` a bar, `{` and `}` eight bars (a phrase); the arrows still seek by time.
  Quantize (`Q`, on until turned off, shown as `Q on` by the clock) keeps jumps on the grid:
  while playing, cue, hot cue, phrase and beat jumps land as far before or after the beat as the
  playhead was, as if they had waited for the nearest beat, so the rhythm carries on; paused,
  beat jumps land on a beat. A track without a beat grid jumps by its BPM.
- **Loops**: `l` loops the loop the playhead is in (a saved one, or the last one exited), or makes
  one of 4 beats from the beat it's in (from the playhead with quantize off); `l` again exits it and
  playback carries on. `(` and `)` halve and double it along the grid, and a hot cue saved as a loop
  plays its loop. Seeking out of a loop leaves it. The loop repeats on the audio thread, to the
  sample: the first time round it's kept in memory and then replayed from there, so going round
  doesn't read the drive. The player shows `LOOP` with its length in beats, and a loop made in the
  player is drawn like a saved one.
- **Visualisers** (`v`): a log-frequency spectrum with peak caps, a mirrored spectrum, an
  oscilloscope, and *deck*, rekordbox's scrolling colour waveform with beat ticks. The whole-track
  overview waveform sits under the player; click it to seek. `f` goes full screen and shows every
  visualiser at once: the deck across the top, spectrum and scope side by side, the mirrored
  spectrum beside the track details, and the cue lane along the bottom. Wide terminals also get a
  details column: tracks on the drive that **mix into** the playing one (compatible key on the
  Camelot wheel, tempo within 6% or half/double time), the playlists it's in, and a summary of its
  analysis. `↑` `↓` pick one of those tracks and `enter` (or a double-click) queues it to play
  next, marked *next* in the list.
- **Phrases**: rekordbox's phrase analysis (Intro, Up, Down, Chorus, Verse, Bridge, Outro) shows
  as the current phrase and bar in the player, with a countdown to the next one, and as a strip
  under the full-screen overview. `N` and `P` jump to the next and previous phrase. Keys show in
  both Camelot and note names, whichever rekordbox stored.
- **Cues** everywhere: under the spectrum and scope, a lane of the 32 seconds around the playhead
  (by default) shows hot cues as lettered chips, memory cues as diamonds and loops as bars, with
  their names and loop lengths in beats (`[` `]` zoom it). Memory cues and loops take the colours
  they were given in rekordbox, and loop lengths are the ones rekordbox stored (½ beat, 4 beats),
  not worked out from the tempo. The deck draws cue lines through the waveform and shades loops. The
  visualiser flashes in a cue's colour as the playhead crosses it, and the player shows the next cue
  with a countdown.
- **Changing the drive** (opt-in): `e`, or right-click, opens actions for the marked or selected
  tracks, the playlist shown and the crate: set the rating or colour, give them a My Tag, add them
  to a playlist (picked in the tree), remove them from the playlist, create, rename or delete a
  playlist or folder, sort a playlist on the drive the way the list is sorted, save the crate, bring
  a playlist back from a snapshot, or replay recorded edits. `T` gives the marked or selected tracks
  a My Tag (one the drive has, or a new `Category/Tag`), or takes off one they all have. My Tags are
  read once as the drive loads, so after changing them elsewhere, `R` reads them again. Every change
  first shows its plan and is written only when you press `y`, the same change the
  [commands](editing.md#editing-playlists-and-tracks) make (copied onto the library's other drives
  plugged in), with a backup first; `u` takes the last one back, after showing what that puts back
  (the playlists and tracks, the files, the recorded edits that come off the journal).
- **After a rekordbox export**: when rekordbox exported to the drive after locallibrary last wrote
  to it, and replaying the [edits recorded on it](editing.md#when-rekordbox-exports-again) would
  change something, a line above the footer says "rekordbox exported over 3 edits on REKORDBOX.
  Replay?": `y` shows the replay to confirm, `esc` puts the offer away for the run.
- **Rule-based playlists**: in the palette, "smart new" makes a
  [rule](editing.md#rule-based-playlists) from the search box's filter and the list's sort (saved in
  the store, then shown filling its playlist on the open drive), "smart apply" fills every rule on
  the drive, and "smart" lists them.
- **Drives** (`2`): each library with its drives under it (how many are plugged in, whether they're
  identical and saved), then the drives of no library. On a library, `enter` opens its newest drive
  in the library view, and `e` syncs, checks or ejects its drives plugged in; the pane beside it
  lists its drives, those not plugged in too. On a drive: identity, library summary, space, store
  status, role, logbook (writes, clean ejects, the last verify and file-system check, problems),
  copies, My Settings files and interrupted operations, and the work: `e` lists what can be done to
  the selected drive, each planned in the background (with progress in the header; `esc` stops it)
  and confirmed like an edit. **Pull** saves it in the store. **Check** runs the checklist of
  `locallibrary check` against the player models remembered for it, and **set prep** checks it with
  the other mounted drives of its library, with the results in a panel. **Sync** copies the newest
  drive of its group over the others plugged in, or says why none is newest. **Pack** puts the
  marked playlists (`m` in the tree), the playlist shown or the crate (saved as a playlist first)
  from the drive open in the library onto it, for a set. **Restore** brings back a playlist or
  folder from a snapshot picked from its library's. **Replay** makes recorded edits again, **undo**
  takes its last change back (`u` undoes the last change made this run, on whichever drive it was
  made to, which may not be the selected one), and **eject** unmounts it and powers it off (playback
  from it stops first). **Snapshots** (`3`): the store's snapshots; `enter` brings a playlist back
  from the selected one onto the drive open in the library. **Profile** (`4`): your [DJ
  profile](sets.md#dj-profile), section by section, with what each would reveal. `i` puts a section
  in the public document or takes it out, `m`/`h`/`H` pin and hide tracks and artists, `D` shows the
  document and `w` writes it. The bar at the top shows your [Atmosphere account](account.md): `L`
  signs in (type your handle, approve in the browser), and signed in, `L` twice signs out.
- **Export**: in the palette, "export" writes the playlist, set, session or all tracks shown as
  M3U8, CSV or text, asking where first. A plugin's format ("export for traktor…", or "export for
  rekordbox…" for rekordbox XML, in a drive's `e` menu too) asks where and for the format's options in a form, then shows what it would write (each
  file, what goes in, what the format can't hold) and writes it on `y`, asking first when that
  overwrites files there already; it takes the marked playlists too. A set's format ("export the
  set for …") writes out the set open in Browse › Sets.
- **Mixes** (`5`): a recorded set and the tracks it plays, like [`locallibrary mix`](sets.md#mixes)
  against the drive open in the library. Pick one of the drive's recordings (XDJ-AZ and OPUS-QUAD
  MASTER REC files), or `O` to type the path of a recording or a folder of them on this computer. A
  folder stays in the list with every recording in it and its subfolders, new ones included, and
  recordings opened are remembered; `del` forgets one, or the folder it was found in. The view draws
  the set's 3-band waveform with each track's lane under it (shaded by how clearly it's heard, so
  blends and EQ swaps show), the tempo played, the keys on the way, and the tracklist with every
  handoff: blend, cut or gap, key move and tempo change. The selected track's own waveform, cues and
  phrases show the stretch that was played. `enter` or `space` plays the set in the player, and the
  timeline and tracklist follow along; `n`/`p` jump between its tracks, a click on the timeline
  seeks, `z` zooms. `o` goes to the selected track in the library, `E` exports the set as data for
  [Local Network](sets.md#mixes) and `w` writes the HTML report and opens it.

Colours are the theme's: the default, or a plugin's chosen in the palette ("Night theme",
"default theme"), kept for the next run in `tui.json`. They're made for a dark terminal. For a
light one, set `LOCALLIBRARY_THEME=light`; terminals
that set `COLORFGBG` (rxvt, Konsole and others) are recognised without it, and
`LOCALLIBRARY_THEME=dark` overrides them. With [`NO_COLOR`](https://no-color.org) set, the player
draws no colours: the selection is in reverse video, the focused pane has a heavy border, keys
that mix are marked `⇄` and underlined, track colours show as their numbers (1 pink to 8 purple)
and cover art is left out. `LOCALLIBRARY_REDUCED_MOTION=1` redraws only a few times a second and
stops the spinning record, the pulsing logo and the cue flashes.

Cover art is drawn in half blocks (24-bit colour where the terminal supports it, the nearest of
256 colours otherwise). The player remembers the drive, list, view, sort, volume, visualiser,
panels and columns, shuffle, repeat, quantize, single play and auto cue, the output, the crate,
and the queue with how far into its track it got, to the last 15 seconds (`space` carries on
from there). It saves them as they change, in the
store once there is one and in a `locallibrary-player` folder beside it until then. Messages show
in the footer: errors stay until `esc`, and `!` lists every message since starting. The footer
hints at the keys for where focus is, as many as fit. `?` lists every key, the view's own first;
all of them come from one table, so a key means one thing in every view (`l` loops, `d` picks the
output, `o` goes to a track, `w` writes a file, `e` opens the Library and Drives actions):

| Keys | |
| --- | --- |
| `enter` | play the selected track (queues the list it's in); open a playlist or folder; in Drives, browse the drive (a library: its newest drive) |
| `space` | pause / resume |
| `←` `→` | seek 5 s (shift: 30 s) |
| `n` `p` | next / previous track |
| `c` `C` | next / previous cue |
| `N` `P` | next / previous phrase |
| `1` … `8` | hot cue A … H, in full screen or with the pads on |
| `'` | pads on / off |
| `,` `.` | beat jump back / forward (`<` `>`: a bar, `{` `}`: 8 bars) |
| `Q` | quantize on / off |
| `l` | loop here / exit the loop (`(` `)`: halve / double it) |
| `+` `-` | volume (`=` `_` too) |
| `d` | the output to play through |
| `:` | commands: every action by name, those with no key, and plugins' commands |
| `s` `r` | shuffle / repeat (off, all, one) |
| `S` | autoplay off (single play: stop after each track) / on |
| `A` | auto cue: start tracks at their first memory cue or beat |
| `x` | stop |
| `o` | go to the playing track |
| `tab` `shift-tab` | next / previous pane: drives, playlists, tracks, open panels |
| `↑` `↓` `j` `k` | move (`pgup` / `pgdn`, `g` / `G` or `home` / `end`) |
| `/` | search or filter; in the box, `tab` switches between the list and the whole drive |
| `b` `B` | sort by the next column / the other way (or click a column header) |
| `*` `#` | show / hide the rating column / the My Tags column |
| `i` | details of the selected track |
| `t` | show / hide the crate |
| `m` `insert` | mark / unmark a track (in the tree: a playlist or folder, to pack) |
| `M` | mark everything shown / clear the marks |
| `a` | add the marked (or selected) tracks to the crate |
| crate: `shift ↑↓` (`K` `J`), `del`, `w`, `X` | move a track, remove it, save the crate as a playlist, clear it |
| `e`, right-click | actions that change the drive (each asks first); in Drives, what can be done to the drive, or to a library's drives |
| `T` | give the marked (or selected) tracks a My Tag, or take one off |
| `u` | undo the last change, on the drive it was made to |
| `y` | replay edits rekordbox overwrote, while it offers to |
| `del` | remove from the playlist shown / delete the selected playlist |
| `esc` | leave full screen; stop a change before it writes, or a plan under way; hide an error; put away the replay offer; clear the search, then the marks (press twice) |
| `v` | spectrum · mirror · scope · deck |
| `[` `]` | zoom the deck and cue lane out / in |
| `f` | full screen |
| `1` … `5` | library · drives · snapshots · profile · mixes (while the pads are off) |
| Profile: `i` `m` `h` `H` `D` `w` `a` `#` `L` | in/out of the document, pin, hide a track / artist, the document, write it, scope, rounding, sign in/out |
| Mixes: `O` `del` `z` `o` `E` `w` | open a recording or a folder of them by path, forget one, zoom, the track in the library, export for Local Network, write the report |
| `R` `F5` | read again: the drive, and what the view shows (drives' status, snapshots, profile, recordings) |
| `!` | messages since starting |
| `?` `q` | help, quit (`ctrl-c` too) (a write to the drive finishes first, also when the terminal closes or a signal stops the player) |

Mouse: click to select (or a tab to switch views), double-click to play, scroll to move, click a
column header to sort, click or drag the waveform to seek, right-click in the library for actions.
