# rekordbox and other DJ software

## rekordbox's collection

```bash
locallibrary collection                         # what rekordbox's collection holds
locallibrary collection tracks -f "bpm:>=128 tag:Situation/Peak" --sort -rating
locallibrary collection playlists
locallibrary collection diff Home               # what Home lacks against the collection
locallibrary collection --db ~/old/master.db info
locallibrary collection export ~/rekordbox-export --dry-run         # the collection as a drive, in a folder
locallibrary collection export REKORDBOX --playlist Sets/Friday     # some playlists, onto a drive
```

rekordbox 6 and 7 keep the collection on the computer in `master.db`
(`~/Library/Pioneer/rekordbox/` on macOS, `%APPDATA%\Pioneer\rekordbox\` on Windows):
SQLite encrypted with SQLCipher 4 under a key every installation shares, which the
[pyrekordbox](https://github.com/dylanljones/pyrekordbox) project publishes. `collection` reads
it, never writes it: the database and its WAL are copied to a temporary folder, decrypted there
(the WAL's committed changes applied) and only that copy is opened, so rekordbox can stay open
and nothing of its is locked. A read while rekordbox saves can miss its latest change.

It reads tracks with their details, playlists and folders, history lists, hot cues, memory cues
and loops, and My Tags; `tracks` takes the same [filters](editing.md#filters) and sort orders as a
drive's. Intelligent playlists show empty: their rules aren't evaluated. `diff` finds the
collection's tracks on the drive by file name and the folders above it, then ISRC, then artist,
title and length, and shows (with `+`) the tracks, playlists and playlist entries the drive
lacks, (with `-`) what only the drive has, and the tracks whose details differ, such as a rating
changed since the last export. `--db` (or `LOCALLIBRARY_MASTERDB`) reads another `master.db`;
should rekordbox change its key, `LOCALLIBRARY_MASTERDB_KEY` gives the new one.

### Exporting it without rekordbox

`collection export` writes the collection to a drive or a folder the way rekordbox's Export does,
so you can get your library onto this computer's store, or onto a stick, without rekordbox and
without a drive it exported to. A folder that isn't there yet is made (not the folders above it, so
an unplugged drive isn't mistaken for one), and a folder exported to works like a drive: `locallibrary pull
~/rekordbox-export` saves it in the store, and `library push` writes it to any USB drive later.
Exporting again copies only what changed, and a stopped export finishes when you run it again.

It exports every track and playlist, or the playlists you name (`--playlist`, repeatable, with the
tracks they use), which it remembers for next time; `--all` goes back to everything. Audio goes in `Contents/<artist>/<album>/`, and
each track gets the analysis files rekordbox made on this computer (beat grid, waveforms, cues,
phrases), with artwork and My Tags. A few things don't come along:

- intelligent playlists, whose rules aren't read;
- history lists from rekordbox's own History;
- My Tags on players that read `export.pdb` (CDJ-3000 and older): `exportExt.pdb` isn't made, so
  only OneLibrary players show them.

With Cloud Library Sync on, tracks kept in Dropbox or Google Drive are read from the folder
rekordbox records for the service, and the plan says how many are online only and download as
they're copied. A track whose audio isn't on this computer is left out, and one whose analysis
Cloud Library Sync hasn't finished bringing over is exported as it is; the plan names both, and
tracks with an analysis file that can't be read, which are exported without it. So on
a second computer signed in to the same rekordbox account, `collection export` gets you the
library with no drive at all.

A drive holding a library that wasn't exported from the collection, or files the export would
remove from a drive with no library, is only written over with `--replace`. What only the target
has is saved in the store first: history lists players recorded, anything that changed since
the last export (cues players saved, edits, an `undo`), and a OneLibrary database that can't be
read (history lists players recorded there can't be checked). Its databases, analysis and
artwork are backed up, so `undo` puts back the library it held, but audio isn't: audio the export
removed stays gone unless it's in the store.

## rekordbox XML

```bash
locallibrary import ~/Desktop/rekordbox.xml USB --dry-run       # what its keys, tempos and cues add
locallibrary collection export ~/stick --from tracklist:rekordbox-xml --grant ~/Music \
  --option file=$HOME/Desktop/rekordbox.xml                     # a drive built from it
```

rekordbox writes its whole collection as XML (File > Export Collection in xml format), as do
other library tools and `locallibrary xml`. The tracklist plugin reads one two ways besides
`playlist import`, which makes a playlist of it (see [Plugins](plugins.md)):

- `import` finds each of its tracks on a drive by file (the last folders of its `Location`), then
  artist, title and length, and adds what the drive's tracks lack: a key (`Tonality`) and a BPM
  (`AverageBpm`) where a track has none, and hot cues, memory cues and loops in slots (or at
  places) it has none in. A loop's length is counted in beats at the tempo where it starts; one
  that isn't 1-512 beats or 1/2 to 1/32 of one comes as a cue, said in a warning.
- `collection export --from tracklist:rekordbox-xml` builds a drive from the collection, as from
  rekordbox's own (above): each track whose audio file is in a folder you grant, with its tags,
  rating, key, tempo, cues and a beat grid from its `TEMPO` markers to its end, and its playlists.
  The file is read once; saved again while the drive is planned, it's refused: build again.

The file can say anything, so values out of range (a time past a day, a tempo outside 0.01-655.35
BPM, a hot cue past H, folders nested more than about 60 deep) are left out and counted in the
plan's warnings, and a beat grid stops at 262,144 beats. Fade-in, fade-out and load marks and
meters past 4 beats to the bar aren't supported, and are counted the same way. Track colours and
`Grouping` aren't read.

## Other DJ software

```bash
locallibrary export Home --format traktor -o ~/Desktop/for-traktor --dry-run   # what would be written
locallibrary export Home "Sets/2026" --format traktor -o ~/Desktop/for-traktor  # one folder and its tracks
locallibrary export Home --format traktor -o out --option location_root=$HOME/Music/USB-copy
locallibrary export Home --format serato -o ~/Desktop/for-serato                # crates
locallibrary export Home --format serato -o ~/Music/for-serato --option write_tags=true   # and cues, in copies of the audio
locallibrary export Home --format engine -o ~/Desktop/for-engine                # a new Engine Library
locallibrary export Home --format engine -o out --option paths=drive            # one to copy onto the drive
```
These formats are the interop plugin's (see [Plugins](plugins.md)). `export --format` with
another DJ program's name reads the drive and writes that program's files into a folder on this
computer (`-o`); nothing is written to the drive, and an output folder on the drive is refused. A
playlist or folder given after the drive limits the export to it and its tracks. On a drive with
both databases, playlists and tracks only OneLibrary has (a CDJ-3000X makes its playlists there
alone) come across too. Tracks point at their audio on the drive where it's mounted now, or, with
`--option location_root=DIR`, in a folder holding a copy of the drive's `Contents/`. Overwriting
files the folder already holds is asked about first unless `--replace` (the folder could be the
program's own), and they're replaced only once every new file is written beside them;
`--option skip_analysis=true` leaves beat grids and cues out (Traktor, and Serato's tags), and
`--dry-run` lists each file with its size, what goes in and what the format leaves out, without
writing. In the player, "export for traktor…" (serato, engine) in the palette does the same,
for the marked playlists or the whole drive (see [the player](player.md)). RPC:
`plugins.export.plan` and `plugins.export.apply` with `plugin: "interop"`, the format as
`exporter` and the options as `options` (see [Building a GUI](sidecar.md)).

- **Traktor** (`collection.nml`): tracks with their details, tempo and key, the beat grid as one
  grid marker on the first downbeat (locked, so Traktor doesn't analyse it again), hot cues A-H
  on pads 1-8, memory cues, loops, and the playlist tree. In Traktor, right-click Track
  Collection, choose "Import another Collection" and pick the file. Traktor's grid has one
  tempo, so a track that changes tempo keeps its first; cue colours, history lists, My Tags and
  artwork don't come across. Audio outside `/Volumes` is placed on the Mac's startup disk, by
  its name ("Macintosh HD" when exporting from another system; Traktor's Relocate finds it if
  the Mac's is called otherwise). Entries are stamped with when they were written, or with
  `SOURCE_DATE_EPOCH` (seconds since 1970) when it's set, so a library exports the same each time;
  the export is refused when it's set to anything else.
- **Serato** (`_Serato_/Subcrates/*.crate`): a crate per playlist, in the playlist's order;
  folders become crates holding their crates (`Sets%%Friday.crate`), a name past 200 bytes cut
  to fit a file name. Serato keeps crates in a
  `_Serato_` folder at the root of the volume that holds the music (for the startup disk,
  `~/Music/_Serato_`): copy the `.crate` files into its `Subcrates` folder, then restart Serato.
  Crates only list tracks; Serato reads the files' own tags and analyses them itself, so beat
  grids don't come across. Cue points live in the audio files' tags, which the export leaves
  alone unless `--option write_tags=true`: then it copies the audio into the output folder (the
  drive's files are never changed; `--dry-run` lists each copy and its size first), writes rekordbox's hot cues, loops and track colour
  into each MP3, AIFF and WAV copy's `Serato Markers2` tag, and the crates list the copies. Hot
  cues keep their pads, hot cue loops their slot and memory loops take the free saved-loop slots;
  memory cues have no equivalent in Serato. FLAC and M4A copies get no cues.
- **Engine DJ** (`Engine Library/Database2/m.db`): a new Engine Library (schema 3.0.2, as Engine DJ
  4.5 and 5.0 write it; newer versions upgrade it when they open it) with the tracks and their
  details, and the playlist tree (folders become playlists holding playlists). Track paths are
  relative to the `Engine Library` folder where it's written, or with `--option paths=drive` to the drive's
  root: copy that `Engine Library` folder onto the drive, next to `Contents/`, and Engine OS players
  and Engine DJ read it from there. In Engine DJ Desktop, a library elsewhere is added as a source
  and its playlists dragged into the collection. Beat grids, cues and loops aren't written (Engine
  keeps them in its own encoded form), so Engine analyses the tracks when it first reads them;
  artwork, colours and history lists don't come across, and an Engine playlist holds a track once.

### Sets and mixes

```bash
locallibrary sets export Friday --format traktor --drive Home -o ~/Desktop/friday-for-traktor
locallibrary sets export Friday --format serato --drive Home -o ~/Music/friday --option write_tags=true
locallibrary mix "Live at the Warehouse.wav" --drive Home --format engine --export ~/Desktop/warehouse
```

A set you played (see [Sets](sets.md)), or the tracklist `mix` found in a recording, goes across
as one playlist of its tracks in the order played, with the same formats: `sets export
--format` and `mix --format` take `traktor`, `serato` and `engine`. The files point at the audio
on a drive, so a set's export needs one (`--drive`; a mix's is the drive `mix` matched it against),
and it's written as that drive's export is, with the same options: the playlist, named for the
day and the title (or the venue), holds the tracks found on the drive by their files, and only
those tracks go in, with their beat grids and cues where the format carries them. A track played
twice is listed twice, but for Engine, whose playlists hold a track once. Tracks not on the drive
(one typed in by hand, or gone since) are left out, each named in a warning; with none on it, the
export is refused.
