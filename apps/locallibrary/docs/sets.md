# Sets, history and mixes

## Before a set

```bash
locallibrary check --players CDJ-3000X,CDJ-2000NXS2  # your library's drives plugged in
locallibrary players set CDJ-3000X,CDJ-2000NXS2       # remember them: `check` uses them from now on
locallibrary players set OPUS-QUAD --library Home     # ...or for one library's drives
locallibrary check SD --verbose                       # list the tracks a player won't load
locallibrary check Home --audio                       # also decode every file's end (slow; finds cut-short copies)
locallibrary players models [CDJ-3000]                # what each model reads, and where that comes from
```

One checklist over every drive you're taking: they hold the same library, `export.pdb` and
OneLibrary agree, no track's audio, analysis or artwork is missing, no audio file is cut short,
nothing is too big for a FAT32 drive in the set, no operation was interrupted, and their history
lists are in the store.

`players set` and `players clear` show what they'd change and ask first (`--dry-run` stops there).
For each player model (`--players`, or else the models `players set` remembered for the drive's
library, or for every library) it checks:

- **The database it reads is there.** The CDJ-3000X, CDJ-1500X, OPUS-QUAD, OMNIS-DUO, XDJ-AZ and
  XDJ-AN read only OneLibrary; every other player reads only `export.pdb`. The DJM-V10 and DJM-A9
  mixers read no library: for them `check` makes sure the drive is FAT32, which they need for My
  Settings and firmware updates. Without any models, a drive with only one of the two databases is
  a warning, since some players can't read it.
- **The partition table and file system:** MBR, and exFAT only where the model reads it (some
  need a firmware update, which `check` mentions).
- **Every track's format, sample rate and bit depth:** "12 FLAC files at 96 kHz won't load on the
  OMNIS-DUO". They come from the database rows; a file's header is read only when its row doesn't
  say (or for an `.m4a` whose bit rate doesn't tell AAC from Apple Lossless). `--verbose` lists the
  tracks, and `--json` gives them to scripts (`unloadable`, grouped by player and reason).

What each model reads comes from its instruction manual (`players models MODEL` names the page) and
AlphaTheta's notice on which units read OneLibrary (March 2026). Players differ more than you'd
think: the OMNIS-DUO, XDJ-AN, CDJ-1500X, XDJ-1000MK2, XDJ-XZ and XDJ-RX3 play lossless files at 44.1
and 48 kHz only, the CDJ-2000NXS generation (and the XDJ-700) plays WAV and AIFF at those rates and
no FLAC or Apple Lossless, the CDJ-3000 generation takes MP3 and AAC at 44.1 and 48 kHz only, and
the XDJ-XZ and XDJ-RX3 manuals list no Apple Lossless. A few older models (CDJ-2000, XDJ-RX2,
XDJ-RR) and one value of the XDJ-RX3 are inferred rather than documented; `check` says "probably"
for those, as a warning. Anything that won't load is a problem.

`check --audio` (and `doctor --audio`) also opens every audio file and decodes its last seconds,
on every core, with progress: a file that doesn't open, stops short of the length the database
recorded (a copy interrupted part way), or doesn't decode at the end is a problem; one whose own
header gives another length is a warning (replaced since the export?). It reads a little of every
file, so it takes a while on a big drive.

`check` exits with status 4 when it finds a problem, so a script can stop there; `doctor` does
too.

## The night before a set

```bash
locallibrary prep                                  # the mounted drives of your library
locallibrary prep REKORDBOX SD --players CDJ-3000X  # just the drives you're taking
locallibrary prep --no-eject --dry-run
```

`prep` gets the drives you're taking ready in one plan:

1. save them in the store, history lists first: the drive with the newest export goes on top of
   the library's latest snapshot, then any drive back from a set beside it (as `watch --pull`
   does), so no history list a player recorded is lost to the sync;
2. copy the newest library onto the others, as `sync` does (a drive back from a set included);
3. check them against your player models (the ones `players set` remembered, or `--players`,
   which takes only models `players models` lists), with the checklist above (`--audio`: and
   decode every file's end);
4. eject them, once the check passed (`--no-eject` keeps them mounted).

If the check finds problems, it stops before ejecting, lists them, and exits with status 4. A
drive whose history lists aren't in the store (one step 1 couldn't save) counts as a problem. As
with `refresh`, a step that fails stops the rest (exit status 5 when some were done). With no
drives, `pull` also saves each mounted drive of the library the way step 1 does.

## History and play counts

Players start a history list every time a drive goes in, and log any track that plays for about a
minute, practice and track checks too: each list is a session. rekordbox imports and deletes them on
its next sync, but every `locallibrary pull` keeps the drive's databases: `locallibrary plays lists
Home --all` lists every session the store ever saw (once, however many drives or snapshots held it),
each with its id, how many tracks and how long, when it was recorded and the set made from it. An
id, or its first 4 or more characters, names a session wherever a list's name does: `locallibrary
plays export SD "HISTORY 012" -o set.txt` (or `plays export SD 1a2b3c4d -o set.txt`, adding
`--all` for a session only the store has) writes one as `01. Artist - Title` (or CSV, M3U8).
`locallibrary plays import friday.csv` keeps a log from Beat Link Trigger's Playlist Writer as a
session, with the time each track played. `locallibrary plays lists SD --delete` clears the lists
from the drive, like a player's ALL DELETE, but only those the store has kept. `locallibrary plays
stats Home --since 3f9a2c` (or `--since 2026-09`) shows the most and never played tracks and what
was played since a snapshot. `locallibrary export` writes playlists and folders the same way, and
`locallibrary diff REKORDBOX BACKUP --playlist Sets/Friday` shows which entries were added, removed
or moved between two drives, libraries or snapshots.

## Sets

A session is what a player logged; a set is what you played, where and when, and only you can say
which sessions were one. A set can span several sessions and drives (a play is logged on the drive
the file came from, whichever deck played it), and a recording of it says more than any of them.

```bash
locallibrary sets suggest                     # sessions that look like sets, with the evidence
locallibrary sets new --from 1a2b3c4d --from 5e6f7a8b --title Friday --venue Warehouse \
  --city Leeds --country GB --date 2026-10-03
locallibrary sets new --recording friday.wav --title Friday   # the tracks a recording plays, timed
locallibrary sets edit Friday --drop 1-2 --move 5:2 --insert 4:"Floorplan - Never Grow Old"
locallibrary sets edit Friday --unnamed 7      # an unreleased dubplate: published as ID - ID
locallibrary sets export Friday --format mixcloud   # paste into Mixcloud's tracklist
locallibrary sets playlist Friday Home        # a playlist of it in the library, to start the next one
locallibrary sets played "never grow old"     # when and where you played a track
locallibrary sets publish Friday --dry-run    # see Publishing
```

`sets suggest` lists the sessions in no set that look like sets and says why: how many tracks, a
drive you take to sets (`drive role <drive> set`), a recording that has its tracks, several drives.
Sessions one pull saved, whose windows overlap, or that are still only on drives of one library go
together as one night when their windows span 36 hours or less; wider windows (a drive's first pull,
or a week between pulls) can't tell nights apart, so each session of 3 tracks or more is suggested
on its own. It prints the `sets new` command for each; `--dismiss ID` says a session wasn't one.
`sets new` takes sessions in play order, a recording (identified as `mix` identifies one), both, or
neither (then `sets edit --insert` adds tracks). A recording's tracklist wins: it has start times,
catches tracks played for less than a minute and leaves out what was only previewed in headphones,
and the logged tracks it doesn't hear are pointed out. Sets are kept in the store's `sets.json` with
a copy of each track's details, so they read without the drive.

Players have no clock, so a session's date is a window: after locallibrary last saw the drive
without it, before it first saw it with it (`on 3 Oct 2026` when both fall on one day, `between 29
Sep and 3 Oct 2026` otherwise). `watch` keeps the windows tight by saving drives as they're plugged
in. A set's day is the one you give with `--date`, else the recording's, else the day its sessions'
windows agree on; failing those, it's dated the day locallibrary first saw one of its sessions (or
today), with a note to give the real one (`sets edit --date`). A log from Beat Link Trigger has real
times, so a set made from it is timed too.

`sets edit` positions name the tracklist as it was before the edit, so `--drop 1 --move 5:2` puts
the track that was fifth just before the one that was second. `--insert` looks the track up on
the drive (`--drive`) and otherwise takes it as typed. `sets export` writes text, Mixcloud's
tracklist (its rules checked), a CUE sheet (with the recording attached), YouTube chapters,
1001Tracklists, CSV, or an M3U8 of the tracks on a drive (`-o` picks the format from `.cue`, `.csv`,
`.m3u8` or `.txt`). `sets publish` puts a set on your [Atmosphere account](account.md#publishing) as
a `computer.localnetwork.dj.set` record, a type Local Network's tools share.

## Mixes

```bash
locallibrary mix "Live at the Warehouse.wav" --drive Home --open
```

Give it a recording of a set (WAV, AIFF, MP3, M4A or FLAC, or the Audacity `.aup3`/`.aup4`
project it was edited in) and it finds which of the drive's tracks it plays, when each comes in
and goes out, which part of the track was used, the speed it was played at, whether key lock was
on, and the key it was heard in. It prints the tracklist and, with `--html` or `--open`, writes a
self-contained report: the mix's 3-band waveform with each track's lane underneath (the curve in
each block is how clearly the track is heard, so fades and EQ swaps show), the tempo across the
set, the keys as played on the Camelot wheel, and every track with its rekordbox waveform, hot
cues and phrases, the stretch that was played highlighted. A player plays the recording in sync:
the timeline and tracklist follow along, and a little crossfader shows which track is louder
during a blend. The page `--open` keeps in the store plays the recording where it is; a page
written with `--html` names it by its file alone, so it plays once the recording is next to it.

It works like Shazam for one library. Each track is fingerprinted once (peaks in its spectrum,
hashed in pairs) and cached in the store (made then, if there's none yet), so a few thousand
tracks take a couple of minutes the first time and a second after that (`locallibrary mix --drive
Home` on its own just does this). A file that doesn't decode is reported and remembered, so it
isn't tried again until it changes. Tracks over 20 minutes (recorded sets, not tracks) are left
out, and so is the recording itself when it's on the drive too. The recording is fingerprinted as
it decodes, and every speed within `--tempo-range` (±16% by default) is tried both with and
without key lock. Pioneer recorders' files (`REC046`, recorded on an XDJ-RX2) are recognised as
such, and the title falls back to the folder they were filed in.

Duplicates on the drive and remixes that share a vocal are listed under the track they match
rather than as tracks of their own. Matches under 50% confidence are marked tentative, and
stretches nothing matches (a track that isn't on the drive) are marked as such. Things that hide
a track from fingerprints: heavy effects, very long filtered intros, and speeds outside the range
searched. A transition shorter than about 20 s often shows a small gap, since a track under heavy
filtering isn't heard until it's nearly at full volume. Matching only looks at one drive's library
at a time.

`--export PATH` writes the same report as data instead (`<title>_<hash>.mix.json` inside `PATH`
when it's a folder or there are several recordings; the hash is of the recording's path, so two
sets with one title don't overwrite each other), for showing elsewhere, such as on a Local Network
profile, where it's attached to the mix. It's versioned JSON (`"format": "locallibrary.mix-report"`,
`"version": 2`, camelCase) with the mix, every play and the track it matched (tempo, key, cues,
phrases, a downsampled waveform overview and small cover art), the transitions and the gaps.

The `--html` report and the export are made to be shared, so they hold what they show and no
more: no file paths, no drive name or size, no My Tags, comments or ratings, and a cover only when
it's embedded in the recording (an image found next to it shows in the TUI only). Tracks your
profile hides (`profile hide`) are `ID - ID`.

## DJ profile

```bash
locallibrary profile                          # your mounted drives and the store
locallibrary profile Home --since 2026        # one library; sets played this year
locallibrary profile -s tempo,mixing --sets   # just those sections, and every set
locallibrary profile document -o profile.json # what a public profile would show
```

`profile` works out how you play from what locallibrary already reads: the sets you declared
(`locallibrary sets`), each track's analysis (BPM, key, phrase mood, cues) and tags, and My
Settings. A set's tracks are found by file in every library it reads (the drives, the store's
snapshots and every history list `pull` archived), so their analysis counts; a track none of them
has still counts, from what the set kept. Until you declare a set, the sessions players record stand
in: history lists with 5 tracks or more (`--min-tracks`). Practice lands in those too, so the report
calls them sessions and the public document never counts them. With nothing played to count, the
collection is described, and the same recording on several drives or in several files counts once.
Without drives named, it reads the mounted drives the store knows (ones you've pulled or synced), so
a stick someone else plugged in doesn't count as yours; a store that knows none reads every mounted
drive. Transitions are tracks played one after the other: not two either side of a track since
deleted, nor one track logged twice.

| Section | What it says |
| --- | --- |
| Summary | a headline ("Techno and House · 122–128 BPM · 48 sets") and the traits the numbers support, each with its evidence: mixes in key, smooth tempo changes, builds the tempo through a set, genre-hopping, mostly one genre, plays new releases or older records, rarely repeats tracks, has signature tracks, cues up every track, plays with quantize off, a wide pitch range |
| Sets | how many, how long, when they were played (until you declare sets: sessions, never published) |
| Tempo | the BPM range of the middle 80% of plays, and a histogram |
| Set shape | opening against closing tempo, and whether sets build, hold or wind down |
| Mixing | how many transitions are harmonic on the Camelot wheel (same key, ±1, relative, energy boost), tempo steps, genre changes |
| Keys, Energy, Genres, Labels, Artists, Era | what you play: the Camelot wheel, rekordbox's phrase mood, tags, release years |
| Signature tracks | tracks in two or more sets, moves played again and again, openers and closers |
| Prep, Collection, Player style | how many tracks have cues and loops, the collection's size, My Settings such as quantize and jog mode |

The report stays on this computer. `profile document` is the part meant for a public profile, and
`profile publish` puts it on your [Atmosphere account](account.md#publishing) as a
`computer.localnetwork.dj.profile` record: counts rounded down to round numbers, shares (of genres,
labels and artists) to 5% (shares of one whole still add up to it), BPM to whole numbers, and
no paths, drive or history-list names, ratings or exact dates. Sections that name tracks, artists or
labels, or describe the collection itself, stay out until you include them, and the headline and
traits only say what the included sections say. Your choices are kept in the store (`profile.json`):

```bash
locallibrary profile sections                  # what each section reveals, and which are in the document
locallibrary profile include signature,artists # name your most-played tracks and artists
locallibrary profile pin "everlong"            # list this track first (matched against what you've played)
locallibrary profile hide "ID - ID" --exact    # never name a track; it still counts in the numbers
locallibrary profile hide "Rihanna" --artist   # never name an artist, wherever they're credited
locallibrary profile alias dnb "Drum & Bass"   # spell a genre your way
locallibrary profile headline "Warm-up specialist"
locallibrary profile precision exact           # or rounded, the default
```

A track one of your sets marks unnamed (`sets edit --unnamed`) is never named either, pinned or
not. A profile can't compare you with other DJs (which tracks only you play): that needs their
data.
