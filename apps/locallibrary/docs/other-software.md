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
without a drive it exported to. A folder exported to works like a drive: `locallibrary pull
~/rekordbox-export` saves it in the store, and `library push` writes it to any USB drive later.
Exporting again copies only what changed, and a stopped export finishes when you run it again.

It exports every track and playlist, or the playlists you name (`--playlist`, repeatable, with the
tracks they use), which it remembers for next time. Audio goes in `Contents/<artist>/<album>/`, and
each track gets the analysis files rekordbox made on this computer (beat grid, waveforms, cues,
phrases), with artwork and My Tags. A few things don't come along:

- intelligent playlists, whose rules aren't read;
- history lists from rekordbox's own History;
- My Tags on players that read `export.pdb` (CDJ-3000 and older): `exportExt.pdb` isn't made, so
  only OneLibrary players show them.

With Cloud Library Sync on, tracks kept in Dropbox or Google Drive are read from the folder
rekordbox records for the service, and the plan says how many are online only and download as
they're copied. A track whose audio isn't on this computer is left out, and one whose analysis
Cloud Library Sync hasn't finished bringing over is exported as it is; the plan names both. So on
a second computer signed in to the same rekordbox account, `collection export` gets you the
library with no drive at all.

A drive holding a library that wasn't exported from the collection is only written over with
`--replace`. History lists players recorded on the target are saved in the store first, and its
databases, analysis and artwork are backed up, so `undo` takes the export back.

## Other DJ software

```bash
locallibrary export Home --format traktor -o ~/Desktop/for-traktor --dry-run   # what would be written
locallibrary export Home "Sets/2026" --format traktor -o ~/Desktop/for-traktor  # one folder and its tracks
locallibrary export Home --format traktor -o out --location-root ~/Music/USB-copy
locallibrary export Home --format serato -o ~/Desktop/for-serato                # crates
locallibrary export Home --format serato -o ~/Music/for-serato --write-tags     # and cues, in copies of the audio
locallibrary export Home --format engine -o ~/Desktop/for-engine                # a new Engine Library
locallibrary export Home --format engine -o out --paths drive                   # one to copy onto the drive
```
 `export --format` with another DJ program's name reads the drive and writes that program's files
into a folder on this computer (`-o`); nothing is written to the drive, and an output folder on the
drive is refused. A playlist or folder given after the drive limits the export to it and its tracks.
Tracks point at their audio on the drive where it's mounted now, or, with `--location-root`, in a
folder holding a copy of the drive's `Contents/`. Files the folder already holds are replaced only
with `--replace` (the folder could be the program's own), and only once every new file is written
beside them; `--no-analysis` leaves beat grids and cues out, and `--dry-run` lists the files and
what the format leaves out without writing. RPC: `interop.plan` and `interop.apply` (`drive`,
`software`, `output`, and the options as `location_root`, `only`, `skip_analysis`, `replace`,
`write_tags`, `paths`).

- **Traktor** (`collection.nml`): tracks with their details, tempo and key, the beat grid as one
  grid marker on the first downbeat (locked, so Traktor doesn't analyse it again), hot cues A-H
  on pads 1-8, memory cues, loops, and the playlist tree. In Traktor, right-click Track
  Collection, choose "Import another Collection" and pick the file. Traktor's grid has one
  tempo, so a track that changes tempo keeps its first; cue colours, history lists, My Tags and
  artwork don't come across. Audio outside `/Volumes` is placed on the Mac's startup disk, by
  its name ("Macintosh HD" when exporting from another system; Traktor's Relocate finds it if
  the Mac's is called otherwise).
- **Serato** (`_Serato_/Subcrates/*.crate`): a crate per playlist, in the playlist's order;
  folders become crates holding their crates (`Sets%%Friday.crate`), a name past 200 bytes cut
  to fit a file name. Serato keeps crates in a
  `_Serato_` folder at the root of the volume that holds the music (for the startup disk,
  `~/Music/_Serato_`): copy the `.crate` files into its `Subcrates` folder, then restart Serato.
  Crates only list tracks; Serato reads the files' own tags and analyses them itself, so beat
  grids don't come across. Cue points live in the audio files' tags, which the export leaves
  alone unless `--write-tags`: then it copies the audio into the output folder (it asks first,
  and the drive's files are never changed), writes rekordbox's hot cues, loops and track colour
  into each MP3, AIFF and WAV copy's `Serato Markers2` tag, and the crates list the copies. Hot
  cues keep their pads, hot cue loops their slot and memory loops take the free saved-loop slots;
  memory cues have no equivalent in Serato. FLAC and M4A copies get no cues.
- **Engine DJ** (`Engine Library/Database2/m.db`): a new Engine Library (schema 3.0.2, as Engine DJ
  4.5 and 5.0 write it; newer versions upgrade it when they open it) with the tracks and their
  details, and the playlist tree (folders become playlists holding playlists). Track paths are
  relative to the `Engine Library` folder where it's written, or with `--paths drive` to the drive's
  root: copy that `Engine Library` folder onto the drive, next to `Contents/`, and Engine OS players
  and Engine DJ read it from there. In Engine DJ Desktop, a library elsewhere is added as a source
  and its playlists dragged into the collection. Beat grids, cues and loops aren't written (Engine
  keeps them in its own encoded form), so Engine analyses the tracks when it first reads them;
  artwork, colours and history lists don't come across, and an Engine playlist holds a track once.
