# Editing your library

Edits name the library (`Home` here). Each is made on its lead clone, the newest of its drives
plugged in, then copied onto the others in the same plan, which ends "then copied onto BACKUP and
SD". A drive that's newer, diverged or back from a set is left alone, with a warning (`sync` first),
and archive and retired drives aren't written. Naming one drive (`REKORDBOX`) works the same way,
from that drive.

## After rekordbox exports

```bash
locallibrary refresh                          # the drive exported over your edits, else your library's newest
locallibrary refresh --dry-run                # the plan only
locallibrary refresh --sync                   # then copy it onto the library's other drives plugged in
locallibrary refresh REKORDBOX --skip clean,settings
```

rekordbox's Export replaces a drive's databases, and with them what you did since the last one:
playlist and track edits, My Tags and cues, rule-based playlists, the name players show. It
brings its demo samples back too. `refresh` puts the drive right in one plan, each step only when
it's needed:

1. remove rekordbox's demo samples (`clean`);
2. make the recorded edits again (`edits replay`), when the export replaced them;
3. fill the rule-based playlists that apply to the drive (`smart apply`);
4. put back the Device Name and colour `drive player` last set, which it remembers on the drive;
5. write My Settings (`settings apply`), when the drive's differ from the profile;
6. save the drive in the store (`pull`);
7. with `--sync`, copy it onto the other mounted drives of its library, when it's the newest of
   them.

The plan lists every step, with the reason for each one that won't run, and asks once. Each step
goes through the same write routine as its own command, backed up, so `undo` takes back the last
one; the steps change only the drive refreshed, and `--sync` copies it onto the others. A step
planned on the drive as it was is planned again on the drive as the step before left it (refused if
another drive is mounted there by then). The first step that fails stops the rest: what was done
stays done, and the exit status is 5 (1 when nothing was done). `--skip STEP` (repeatable, or
comma-separated) leaves steps out.

`status` flags a drive rekordbox exported over recorded edits ("rekordbox exported over 3 recorded
edit(s): run `locallibrary refresh "REKORDBOX"`"): rekordbox exported to it after its last recorded
edit, and making the edits again would change something. On a drive with both databases, an export
is when both were written since: a player writes only the one it reads (history lists, a CDJ-3000X's
playlist edits), so a set isn't taken for one. With no drive, `refresh` takes the one mounted drive
flagged so, else the only mounted drive, or the newest drive of the one library plugged in. My Tag
and cue edits count once the drive was exported to after them, since telling whether they're in
place means making them.

## Editing playlists and tracks

```bash
locallibrary playlist new Home "Sets/2026/Friday" --filter "bpm:122-128 key:8A~"   # folders are created as needed
locallibrary playlist add Home "Sets/2026/Friday" --query "floorplan" --position 1
locallibrary playlist remove Home "Sets/2026/Friday" --at 3,4      # take tracks out of it
locallibrary playlist reorder Home "Sets/2026/Friday" --at 7,8 --to 1
locallibrary playlist sort Home "Sets/2026/Friday" --sort key,bpm
locallibrary playlist order Home "Sets/2026/Friday" --into "Sets/2026/Friday (mix order)"
locallibrary playlist move Home "Sets/2026/Friday" --into Archive --position 1
locallibrary playlist rename Home "Archive/Friday" "Friday, 10 October"
locallibrary playlist delete Home "Sam" --with-tracks   # a guest's merged crate, and the tracks only it used
locallibrary playlist unfiled Home                   # tracks in no playlist, so players can find them
locallibrary playlist import Home spotify.csv --into "From Spotify"
locallibrary track set Home --filter "bpm:>=128 genre:techno" --rating 5 --colour red
locallibrary track rm Home --filter "is:unfiled plays:0" --dry-run
locallibrary track dedupe Home                       # replace Sync Manager copies (--hash-audio: identical audio too)
locallibrary track dedupe Home --by-sound            # ...and a FLAC and an MP3 of one recording
locallibrary track replace Home 812 77               # duplicate 812 -> keep 77, everywhere
locallibrary track relocate Home --look-in ~/Music   # find missing tracks' files, see Missing files below
locallibrary track show Home --query "daft punk - one more time"   # details, playlists, tags, copies, analysis
locallibrary track matches Home 77                   # what mixes into track 77
```

Edits change the drive's databases in place, `export.pdb` and OneLibrary's `exportLibrary.db` alike,
so every player sees them. Each shows its plan first, is backed up (`locallibrary undo` takes it
back), and is checked by reading the edited databases back before they replace the old ones. A name,
or a track's title, comment and mix name together, too long for one `export.pdb` row (about 4,000
characters, half that outside ASCII) is refused in the plan. A playlist or track one database lacks
(they can drift apart) is skipped there with a warning, and My Tags in `exportExt.pdb` follow tracks
that are removed or replaced. A playlist edit is made in each database's own entries (a CDJ-3000X
saves its playlist edits to OneLibrary alone, and the plan warns when they differ): entries the
player added or took out stay that way, its order stays unless the edit reorders the playlist, and
an added entry goes after the one it follows in export.pdb. Of two playlists with one name in a folder (rekordbox allows that),
edits change the one shown first. A playlist and a folder can share a path too: the path names the
playlist, and the path with a `/` after it (`Sets/`) the folder.

`playlist order` orders a playlist for mixing: compatible keys on the Camelot wheel (with
energy-boost moves), close tempos (half and double time count), and energy from rekordbox's
phrase analysis that holds or builds. Over 2,000 tracks it skips the last tidying pass; Ctrl-C
stops it. `track replace` moves a duplicate's playlist and history
entries and My Tags to the copy you keep, then deletes the duplicate and its files.

`track dedupe` does that for every duplicate `doctor` finds (see [Doctor](drives.md#doctor)),
keeping the copy with the best audio (lossless over lossy, then the higher bitrate, sample rate
and bit depth), then the one with the most cues, playlists and history lists; `doctor` says what
decided it. A copy's hot cues, memory cues and loops never go with it. When the copy it keeps has
no cues, it gets those of the best copy that has some, moved by the offset between their audio:
an MP3 made from a WAV or FLAC starts its audio about 25 ms later (its encoder's delay), so a cue
copied as it is lands that much early. The fingerprints line the two files up, their decoded audio
then gives the offset to a fraction of a sample, and for an MP3 whose timeline in rekordbox starts
a frame before its decoded audio, that frame is allowed for. A copy whose cues the keeper then has
goes. One stays, with a warning saying why, when its cues can't be carried: the two don't sound
like one recording (copies by artist, title and length alone can be different edits), their audio
doesn't line up closely enough to measure the offset, or the keeper has cues of its own that
aren't these. The carried cues are recorded with the replacement, so `edits replay` carries them
again first.

### Missing files

```bash
locallibrary track relocate Home                     # files moved or renamed on the drive
locallibrary track relocate Home --look-in ~/Music --look-in /Volumes/NAS/Music --dry-run
```

`track relocate` looks for the audio of tracks whose file isn't on the drive, by file name (any
letter case or Unicode form): among the drive's audio files no track uses, and with `--look-in`,
in those folders and everything below them. A track changes only when exactly one file fits it.
Of several, those of the size the database recorded are kept, then those of its length (within a
second); one with more than one file left is listed with every file found and left as it is, and
so is one whose only file left isn't as long as the track (another recording of the name) and a
file two tracks could each be. rekordbox's Relocate All takes the first match instead,
which can be another album's `Intro.mp3`.

A file found on the drive stays where it is: the track is pointed at it in `export.pdb`,
OneLibrary and its analysis files, and no audio is copied. A file found elsewhere is copied to the
path the track has (only under `Contents/`), so the databases stay as rekordbox wrote them and its
next export finds the file in place. That export writes the paths rekordbox's collection has, so a
track pointed at a moved file has its old path again after it. Both are backed up first, so
`locallibrary undo` takes back what changed in the databases and analysis files (a copied file
stays on the drive), and neither is recorded for `edits replay`.

`playlist import` reads M3U/M3U8, CSV and other tables (Spotify via Exportify, TuneMyMusic,
Soundiiz, Beatport-style exports, Apple Music's text export, and any table with a title or file
column), rekordbox's playlist text export, rekordbox XML (`--from` picks one of its playlists) and
plain "Artist - Title" lists. Tracks are matched by file path, then ISRC, then artist, title and
length (within 2 s), then artist and title alone, then fuzzily; entries that fit several
tracks are listed instead of guessed (settle them with `--choose ENTRY=TRACK`). It works offline:
export a streaming playlist to CSV first.

### Upgrading to lossless

```bash
locallibrary track upgrade Home --from ~/Music/Lossless --dry-run
locallibrary track upgrade Home --from ~/Music/Lossless --from /Volumes/NAS/Bandcamp --remove-old
```

`track upgrade` finds lossless files (FLAC, WAV, AIFF, Apple Lossless) of the recordings the drive
holds as MP3 or AAC, in the folders given and below them, by their sound, as `doctor --by-sound`
finds duplicates (the store keeps the fingerprints): lengths within 2 seconds, so an extended mix
never stands in for a radio edit, and prints that line up over most of both. Each is copied beside
the track's file and the track is pointed at it, in both databases and its analysis files, with
its format, size, rate, depth and bit rate. The track keeps its row, so its playlists, history
lists, My Tags, rating and play count stay. Its cues and beat grid move by the offset between the
two files' audio (an MP3 starts its audio some 25 ms later than the file it was made from),
measured to a fraction of a sample by cross-correlating the audio. The detail waveforms move with
them, by whole columns (6.7 ms); the rest of the analysis stays as it was. The copy goes beside the
track's file under its own name made safe for FAT, or `Name (2).flac` when that's taken; a copy
already there from an upgrade that was undone is used again. With `--players` (or the ones
`players set` remembered), a lossless file they can't play is no upgrade.

A track is listed with why and left as it is when its offset can't be measured that closely,
another row of the drive uses its file, its file isn't under `Contents/`, its analysis files can't
be rewritten, or its timing can't be told (below).

rekordbox times an MP3 from its first decoded frame: it doesn't trim the encoder's delay the way
gapless players do. Some MP3s (those with a complete LAME tag, and some made by FFmpeg) it times
from one frame earlier, 26 ms at 44.1 kHz, and no tag says which. Measured against the
waveforms rekordbox drew for 1,072 MP3s, 85% start at the first frame and 14% a frame early, so
`track upgrade` tells the two apart for each track by its waveform, and a track without one is
taken to start at the first frame. AAC (M4A) is timed the same way among three starts (the first
frame, and 1,024 or 2,112 samples before it, the encoder delays AAC files carry), and an AAC track
without a waveform is listed rather than guessed. Unlike the MP3 figures, these haven't been
measured against rekordbox's own analysis.

The old file stays unless `--remove-old`. It's backed up first, like everything the upgrade
changes, so `locallibrary undo` points the track back at it; the lossless copy stays on the drive
for the next upgrade. The upgrade is recorded, so after rekordbox's
next export, which writes the old file's row and cues again, `edits replay` points the track at
the lossless file and moves its cues again. Upgrading in rekordbox's own collection too (Relocate)
keeps the two in step.

### Filters

Tracks are chosen with `--track ID`, `--query TEXT`, `--playlist P`, `--album A`, `--history
"HISTORY 003"`, `--path /Contents/...` or `--filter`. Given together, they narrow the choice to
the tracks that fit all of them: `--playlist Friday --filter "bpm:>=128"` is Friday's tracks at
128 BPM or more. An empty filter, search or name is refused rather than taken to mean every
track. `--filter` takes the same filter `tracks --filter` does:

| Filter | Matches |
| --- | --- |
| `daft punk` | words in artist, title, album, genre, label, key, comment, remixer, mix name or path |
| `artist:"daft punk"`, `genre:house\|techno`, `-genre:house` | one field; either value; not |
| `key:8A`, `key:Am`, `key:8A~` | a key in any notation; `~` adds the keys that mix into it |
| `bpm:128`, `bpm:120-128`, `bpm:>=124`, `bpm:~128` | tempo; `~` within 6%, half and double time included |
| `rating:>=4`, `plays:0`, `year:1999`, `time:>6:00`, `bitrate:<320` | numbers, ranges, comparisons |
| `color:aqua`, `color:none`, `added:2026-09`, `added:>=2026-01-01` | colour, date added |
| `playlist:Sets/Friday`, `tag:Genre/Deep`, `is:unfiled`, `is:unplayed`, `has:artwork` | membership, My Tags, state |

`playlist:` takes a path or a unique name, as `--playlist` does, and a folder means everything in
it; one that names no playlist is refused. So is a word before a colon that names no field
(`gnere:house`), which would otherwise match nothing (and negated, everything); in quotes
(`"re:zero"`) it's text. `tag:` reads the drive's My Tags wherever a filter goes, and `--history`
finds lists in both databases.

Sort orders are lists of fields, highest first with a `-`: `key,bpm`, `-rating,title`.

### When rekordbox exports again

rekordbox's next export to a drive replaces its databases, and nothing here writes to rekordbox's
own collection. Two ways to keep edits:

```bash
locallibrary edits list           # every edit made to the library (.locallibrary/edits.json on its drives)
locallibrary edits replay Home   # make them again after an export; edits that no longer apply are skipped
locallibrary xml --only "Sets/2026" -o sets.xml   # or take playlists back into rekordbox
```

`locallibrary refresh` makes the edits again along with the rest an export replaces, see [After
rekordbox exports](#after-rekordbox-exports).

Playlist, track, My Tag and cue edits (including those made in the player and by `smart apply`)
are recorded with tracks named by file, so they replay on a new export where row ids differ: a
selection (`--track`, `--query`, `--filter`...) as the tracks it picked, `playlist order` as
the order it made, playlists by their exact paths, and deleting history lists as the lists it
deleted, each with its entries (a list a player recorded since under the same name stays). Edits
that changed nothing aren't recorded. A replay leaves out tracks the new export no longer has,
with a warning, instead of skipping the whole edit. It makes the My Tag and cue edits first, in
the order they were made, then the others, and backs the drive up once before all of it, so
`undo` takes back the whole replay. Removing and moving entries are recorded as the files removed
or moved, and deleting a playlist with its tracks as the tracks it deleted, so a replay keeps
entries the new export added. rekordbox can export over one database and leave the other, so a
replay makes edits again in whichever lacks them. The recorded edits travel with the library: a sync copies the
source drive's onto the drives it updates, so `refresh` makes edits made on any of the library's
drives again. A push or pack, which gives a drive a library from a snapshot or a selection, forgets
the edits recorded for the one it held (its plan says so). `edits.json` says which version of its
format it is: a locallibrary older than the one that wrote it refuses to read it or add to it
(and says so) rather than misreading it. Builds from before it had a version read it as a plain
list: they change the drive, then fail to record the edit. Update every copy (the CLI and the
app's sidecar) before editing a drive this version has edited.

### Bringing back a playlist

```bash
locallibrary drive restore Home --playlist "Sets/2026/Friday"              # from the newest snapshot that has it
locallibrary drive restore Home --playlist "Sets/2026" --snapshot 3f9a2c   # a whole folder, from that snapshot
locallibrary drive restore REKORDBOX --playlist "Friday" --backup 20261004T165600.123Z --dry-run
```

`drive restore --playlist` takes one playlist, or a folder and everything in it, as a stored
snapshot (`pull` keeps them) or one of the drive's backups has it, without touching the rest of the
library. Without `--snapshot` or `--backup` it uses the newest snapshot of the drive's library that
has the playlist. It's an ordinary playlist edit: shown first, backed up, recorded for `edits
replay`, and taken back by `undo`. A playlist only OneLibrary had (a CDJ-3000X makes its
playlists there alone) is found there too, and comes back into both databases. Missing playlists
and folders are made again next to their old
neighbours; existing ones get the old entries back, in their old order. Entries whose track isn't on
the drive any more are listed and left out, and playlists you've added to the folder since are left
as they are.

### Rule-based playlists

```bash
locallibrary smart add "Smart/Peak time" --filter "bpm:126-132 rating:>=4" --sort -rating --limit 60
locallibrary smart add "Smart/Tokyo" --filter "tag:Situation/Tokyo" --drive Tokyo
locallibrary smart apply             # each library plugged in that a rule is for, but archive and retired drives
```

The rules live in the store. `--drive` limits a rule to a library, or to a drive's library: every
clone of it gets the playlist, so keeping them identical never undoes it. Drives get ordinary
playlists, since players can't evaluate rules (rekordbox exports its Intelligent Playlists the same
way). Run `smart apply` again after an export or a merge to refresh them. A rule fills the playlist at exactly its path, never one of its
name in another folder, so rules for `Smart/Peak` and `Smart/PEAK` are two rules filling two
playlists; with no exact match, a path several playlists match ignoring case is refused, naming
them. `playlist fill`,
`playlist unfiled` and `playlist import --into` find their playlist the same way.

### Drives for a set

```bash
locallibrary pack Home /Volumes/TONIGHT --playlist "Sets/2026/Friday" --playlist Classics
locallibrary pack Home /Volumes/TONIGHT           # later: the same selection, copying only what changed
```

`pack` writes only the chosen playlists and folders, the folders above them and the tracks they
use, with their analysis and artwork, to another drive, and removes files the packed library
doesn't use from it (`--keep-extra` keeps them). The databases are cut down from the source's
own, so cues, grids, My Tags and details come across unchanged; history lists stay behind unless
`--history`. A source whose OneLibrary database can't be read is refused (rebuild it with
`convert --to onelibrary --replace`): the packed drive would have none, and players that read only
OneLibrary couldn't browse it. A packed drive holds a different library from its source, so `sync`
won't copy one over the other, and `check` says whether it's up to date with the drive it was packed from.

A packed drive comes back from a set with history lists and memory cues the players saved on it.
Before packing it again, `pack` saves its state in the store when it holds history lists the new
pack won't have, or changed since it was last packed, or its OneLibrary database can't be read
(so history lists players recorded there can't be checked; it refuses when there's no store), and
backs up the databases, analysis and artwork it replaces on the drive, so `locallibrary undo`
puts them back (it says which snapshot and backup; the plan counts the backup's space). The drive
keeps its own player name and colour. Audio it removed isn't kept (it's on the source): tracks it
removed come back by packing again with the old selection, or with `undo --force` without their
audio. Packing forgets the edits recorded on the drive. Like every command that writes, it won't
write to an archive or retired drive without `--overwrite-archive`.

```bash
locallibrary pack Home /Volumes/TONIGHT --playlist Friday --transcode --players CDJ-2000NXS,OMNIS-DUO
locallibrary pack Home /Volumes/TONIGHT --playlist Friday --transcode --container flac --decode-lossy
```

`--transcode` packs lossless tracks the players can't play as copies they can: a 24/96 FLAC goes
to a CDJ-2000NXS (no FLAC, nothing over 48 kHz) as a 48 kHz, 24-bit AIFF. The players are
`--players`, or the ones `players set` remembered for the source's library. A copy is AIFF unless
`--container wav` or `flac` (if the players don't take that, the next of AIFF, WAV and FLAC they
do), keeps the master's rate and depth when the players take them, and otherwise takes the highest
they do below it in the same family (88.2 kHz becomes 44.1, 96 becomes 48), with dither. It starts
at the master's first sample and is as long, to the sample, so cue points and beat grids, which
rekordbox keeps in milliseconds from the start, still land. The copy keeps the track's row:
playlists, history, My Tags, cues and grid stay, and both databases give its path, format, size,
rate, depth and bit rate, as the analysis files give its path. Its master isn't packed. Tags and
artwork come along.

Lossy files are never encoded lossy again. An MP3 or AAC file a player can't play (an MP3 at
22.05 kHz on a CDJ-3000) stays as it is unless `--decode-lossy` asks for a lossless copy, bigger
but no worse. Its cues and grid move by the decoder's lead the way `track upgrade` times it, so an
AAC without a waveform stays unplayable. So does a copy too long for AIFF or WAV (4 GiB), or one
whose name is another track's file. The plan lists the copies, which players don't play each master and why, the space
they need (the pack is refused when it doesn't fit), and the tracks that stay unplayable. Each
copy is recorded on the drive (`.locallibrary/transcodes.json`) with its master's content hash
and how it was made, so packing again keeps current copies and makes them again when a master was
replaced, even by a file of the same name, or the way copies are made changed. Packing again
without `--playlist` keeps the copies' players and options too.

## My Tags

```bash
locallibrary tags                                         # categories, tags, how many tracks carry each
locallibrary tags --tracks                                # ...and which tracks
locallibrary tags add Home "Genre/Deep House" --playlist "Sets/Saturday"   # or --track, --query, --album
locallibrary tags untag Home "Genre/Deep House" --track 12
locallibrary tags rename Home "Genre/Deep" "Deep House"   # a tag
locallibrary tags rename Home "Untitled Column" "Mood"    # a category
locallibrary tags delete Home "Situation/Morning"
```

My Tags are rekordbox's own track tags: four categories (Genre, Components, Situation and one you
name), each with up to 50 tags. Players filter by them (Track Filter) and use them for Related
Tracks. A drive holds them twice, in `exportExt.pdb` for players that read `export.pdb` and in
OneLibrary's `exportLibrary.db`. `tags` edits both, matching tracks by file path and creating a tag
in whichever database lacks it, and lists where the two disagree. Categories can be renamed but not
deleted (`tags add` makes a missing one only while a database has fewer than four). A new or renamed
tag or category can't contain `/` (it separates the category from the tag; a tag rekordbox named
with one is still found), and a name too long for `exportExt.pdb` (about 4,000 characters, half
that outside ASCII) is refused. A drive
without `exportExt.pdb` (older exports) only has its OneLibrary tags edited. Tag edits are recorded
in the drive's journal, so `edits replay` makes them again after rekordbox exports to the drive.

## Cues

```bash
locallibrary cues Home --track 12                      # hot cues, memory cues and loops
locallibrary cues set Home --track 12 --hot B --at 1:02.5 --loop 8 --name Drop --colour red
locallibrary cues set Home --track 12 --memory --beat 129   # on the beat grid (--snap moves a time to the nearest beat)
locallibrary cues rename Home --track 12 --hot B "Big drop"
locallibrary cues colour Home --track 12 --memory-beat 129 aqua
locallibrary cues rm Home --track 12 --hot B           # or --memory-at 1:00, --all
locallibrary cues copy Home --from 12 --to 40          # e.g. onto a duplicate
```

Players read cues from a track's analysis files (`.DAT` for every player; `.EXT`, with names and
colours, for nexus 2 and later) and, on OneLibrary players, from `exportLibrary.db`. Every cue edit
updates all of them, keeping the bytes of cues it doesn't change and the ids hot cue bank lists
point at. Hot cues take rekordbox's 62-colour palette (red, orange, yellow, green, aqua, blue,
violet, pink... or `#rrggbb` for the nearest) and memory cues its 8 track colours. A loop given in
beats ends on the beat grid. Exporting the track from rekordbox again brings back your
collection's cues; cue edits are recorded in the drive's journal, so `edits replay` makes them
again.

## Cover art

```bash
locallibrary art missing                                             # each library plugged in (or name one)
locallibrary art set Home cover.jpg --album "Framed And Defamed"     # or --track 12, --playlist, --query
locallibrary art set Home ~/Music/track.mp3 --track 12               # use a file's embedded art
locallibrary art auto                                                # fill gaps from embedded tags / cover.jpg
locallibrary art embed cover.jpg ~/Music/Album/*.mp3                 # for your computer's collection
locallibrary art unembed                                             # ...and to put those files back
```

On a drive, artwork is rendered to the 80×80 and 240×240 JPEGs players read and linked in
`export.pdb` (`a<id>.jpg`) and OneLibrary (`b<id>.jpg`). Tracks that shared the old artwork keep
it unless they're included. Audio files on
a drive are deliberately left untouched: rewriting tags there would shift the audio data and break
the seek index rekordbox stored for VBR MP3s. For your computer's collection, `art embed`, then
"Reload Tag" in rekordbox, so every future export carries the art. `art embed` lists the files it
will rewrite and asks first, and keeps a copy of each file as it was in the store
(`embed-backups/`, the last 5 embeds, counted in `store info`): `art unembed` puts them back,
each one only if it hasn't changed since (`--list` shows the embeds kept, `--backup ID` picks
one). `art set` asks first too, and the drive is backed up before it's written. `art auto` given
no drive takes each library plugged in (and each rekordbox drive of none) but archive and retired
drives: it shows each plan and asks once.

## Clean

```bash
locallibrary clean Home --dry-run      # what would go
locallibrary clean Home
```

rekordbox's collection ships with demo samples ("rekordbox - House 1", "Techno 1", "Breaks 1"...),
and they end up on drives, often once per export (`House1.wav`, `House1-1.wav`, `House1-2.wav`...).
`clean` removes every track on its list from `export.pdb` and OneLibrary's `exportLibrary.db`, along
with its playlist and history entries, audio, analysis files and artwork, plus artists, albums,
genres, labels, keys and artwork rows nothing else uses. Files another track (in either database)
still points at are kept. The list lives in [`clean.rs`](../../../crates/locallibrary/src/clean.rs)
(`USELESS_TRACKS`). The next export brings them back unless they're removed from the rekordbox
collection too.

## Convert

```bash
locallibrary convert Home --to onelibrary            # give older drives OneLibrary, for CDJ-3000X, OPUS-QUAD, XDJ-AZ...
locallibrary convert Home --to device-library        # give OneLibrary-only drives export.pdb, for every other player
locallibrary drive info REKORDBOX                    # "export.pdb and OneLibrary differ" when they drift apart...
locallibrary convert Home --to onelibrary            # ...give each what the other has
locallibrary convert Home --to device-library
locallibrary convert Home --to onelibrary --replace  # or rebuild OneLibrary from export.pdb
```

Tracks, artwork (copied to the other database's `a`/`b` file names), playlists and history
playlists are converted; audio and analysis files are shared, and OneLibrary's `cue` rows
(players that read only OneLibrary use them) are made from the analysis files.

Players write only the database they read: a CDJ-3000X's history lists and playlist edits go to
OneLibrary alone, an older player's to `export.pdb`. So converting into a drive that has the
database already keeps everything in it and adds what it lacks: tracks (matched by file), playlists
and the entries they're missing, history lists and My Tags. Its own entries stay, and one a player took
out of a playlist comes back if the other database still has it. With nothing to add, it says so. rekordbox's own "Convert from Device Library" overwrites the drive's OneLibrary
instead, losing what only it had.

`--replace` rebuilds the database from the other one, dropping what only it has: the plan names
those playlists, entries and history lists, and OneLibrary's hot cue bank lists. Rebuilding
OneLibrary keeps the rows it already has for tracks `export.pdb` lists (every column, cues, My
Tags and artwork) and the drive's player name and colour. A
new `export.pdb` starts from a real rekordbox export (rekordbox-pdb's test file) with its rows
removed, so it has rekordbox's colours and browse columns; a new OneLibrary database starts from
rekordbox 7's schema and default menus. My Tags come along:
`exportExt.pdb`'s into a new OneLibrary, and when `export.pdb` is rebuilt its tag links move to
the new track ids (by path) and OneLibrary's tags are added. A drive without `exportExt.pdb` doesn't
get one. A track or name too long for an `export.pdb` row (OneLibrary has no such limit) is refused
in the plan, by name. Undo with
`locallibrary undo`.

`drive info` also says when rekordbox syncs playlists to a drive automatically (its
`playlists3.sync` / `playlists3Plus.sync`); merge, clean, convert and playlist edits warn that
rekordbox doesn't know about their changes.
