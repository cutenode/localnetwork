# Commands

## First run

```bash
locallibrary setup
```

`setup` walks through the first run in a terminal. It finds the mounted rekordbox drives, asks
which are copies of your library (`setup DRIVE...` names them) and what to call the library (the
one they're in already, else "My library"), and marks each drive as a copy under its volume name
(a drive the store knew before it was erased gets its identity back). It asks which one rekordbox
exports to (the primary) and what the others are for (backup, set or archive), and starts your
library: the primary is saved in the store, then the other drives where they hold the same
library. Run again, it saves them in the library they're in rather than starting another, and
keeps their roles. Then, if you like, it imports the primary's My Settings into your profile, sets
the Device Name players show and your contact details on the drives (archive drives are kept as
they are), and remembers the player models you play on for `check` and `prep`. Last, it offers to
save drives into the store as they're plugged in from then on, syncing the other copies plugged in
(`watch --install`, see
[Saving drives as they're plugged in](store.md#saving-drives-as-theyre-plugged-in)). Each change
shows its plan and asks first. It ends with the commands you'll use most.

Without a terminal (or with `--json`) it asks nothing: pass `--yes` and `--library NAME`, and the
rest as flags (a drive is its mount path, volume name or name; `--name DRIVE=NAME` gives a drive
a name other than its volume's; `--watch` installs `watch`). It takes every mounted rekordbox
drive but those of another library in the store (name the drives to choose), and won't move a
primary out of the library it's in:

```bash
locallibrary --yes setup --library Home --primary /Volumes/REKORDBOX --role SD=set \
  --player-name SD="SAM SD" --contact sam@example.com --players CDJ-3000,CDJ-2000NXS2
```

Until there's a store, commands other than `setup`, the player and `rpc` start with a one-line tip
about `setup` on stderr (not with `--json` or `-q`). Commands that only read never make a store:
they answer as if it were empty, and the ones about the store itself (`store info`, `store verify`,
`library log`, `drive log`, `sets show`) say there's none.

## Commands

Your drives are copies of a library, and most commands take the library: give its name (`Home`), and
many need nothing when one library is plugged in. A library's name means its lead clone, the newest
of its drives plugged in (its primary when they're identical, or when which is newer can't be told).
A drive is a mount path, its name (its volume name unless you gave it another) or an id prefix, and
a drive's name wins over a library's; a bare word is a name before it's a folder in the working
directory (one that's both a drive and a folder is refused: give `./NAME`). Commands about one
particular drive take a drive, not a library: the `drive` commands (but `drive restore`), `library
push`'s target, `provision`, `settings show` and `settings set --drive`. Every command accepts
`--json` for machine-readable output (it never prompts; commands that change something need `--yes`
to apply changes), `--yes` to skip confirmations, `--overwrite-archive` to write to an archive or
retired drive (writes to them are refused otherwise), and `--store DIR` (or `LOCALLIBRARY_STORE`) to
choose the store. `locallibrary --help` lists the commands by what they're for, with common tasks;
`locallibrary help COMMAND` (or `COMMAND --help`) says more about one, with examples.

Commands that read a library (`tracks`, `playlist ls`, `doctor`, `tags`, `art missing`, `edits
list`) read each library plugged in once, from its lead clone, when given none, and each rekordbox
drive of no library; those that read what each drive keeps for itself (`drive info`, `drive
backups`, `drive space`, `settings show`, `plays lists`) read every mounted rekordbox drive. Each
comes under its name, and one on its own reads as if it were named. `tracks --format tsv` or `csv`
puts them in one listing with a `drive` column. One that can't be read is left out with a line on
stderr, and the exit status is 5. `xml`, `drive verify`, `drive recordings`, `mix`, `sets export`
and `doctor --fix-orphans` given no drive take the only mounted rekordbox drive, or, when the
mounted drives that belong to a library all belong to one, its lead clone (`refresh` first looks for
the drive rekordbox exported over recorded edits on). When that can't be told, the error lists the
mounted drives (name, mount path, role, library) and the exit status is 2 (with `--json`, the
error's kind is `usage`). A name that matches no mounted drive gets the drives that are mounted in
its error, and, for a drive you own that isn't plugged in, when and where it was last seen. `sync`
with no drives syncs every library plugged in. `check`, `eject`, `pull` and `prep` with no drives
take the mounted drives of one library, and given a library's name (as `sync` is), its mounted
drives (archive and retired drives are left out unless named); `pull --library` names the library a
drive is pulled into, so it doesn't go with a library's name.

Every command that writes shows its plan first and asks, then makes exactly the change it showed: if
the drive changed in between (rekordbox exported again, another command wrote to it), nothing is
written and it says to plan again. A change to a library (an edit to its playlists, tracks, My Tags,
cues or artwork, `clean`, `convert`, `merge` into it, `smart apply`, `edits replay`, `drive restore
--playlist`, `doctor --fix-orphans`, `undo`) is made on its lead clone, then copied onto its other
drives plugged in, all in one plan, which ends "then copied onto BACKUP and SD". A drive that's
newer than the lead, diverged from it or back from a set is left alone, with a warning (`sync` sorts
it out), and archive and retired drives aren't written. Naming one of the library's drives does the
same from that drive. My Settings aren't part of the library: `settings apply Home` writes each of
its drives.

**Everyday**

| | |
| --- | --- |
| `locallibrary setup` | The first run: say which drives hold your library and what to call it, choose their roles, start it, import My Settings, set what players show, remember your player models and save drives as they're plugged in, see [First run](#first-run) |
| `locallibrary refresh [<library> \| <drive>] [--sync] [--skip STEP]` | rekordbox just exported to a drive: its demo samples out, your recorded edits, rule-based playlists, player name and My Settings back, then saved in the store (`--sync`: and copied onto its library's other drives), one plan, see [After rekordbox exports](editing.md#after-rekordbox-exports) |
| `locallibrary prep [<library> \| <drive>...] [--players MODEL,...] [--no-eject]` | The night before a set: save the drives in the store, copy the newest library onto the others, check them for the players, eject them, one plan, see [The night before a set](sets.md#the-night-before-a-set) |
| `locallibrary status [--all]` | Each library with its drives, plugged in or not: whether those plugged in are identical and saved, where each stands against the library's latest snapshot (up to date / behind / has new changes / back from a set / diverged), settings drift, roles, write protection, low space and copies; drives of no library last. `--all` adds the libraries and drives with none plugged in, see [Your library's drives](drives.md#your-librarys-drives) |
| `locallibrary sync [<library> \| <a> <b> ...] [--allow-guess]` | Copy the newest drive over the others, all at once (refuses if any diverged or hold different libraries, except a drive [back from a set](drives.md#back-from-a-set); `--allow-guess` goes with a low-confidence verdict on which is newer); with no drives, every library plugged in, each from its newest drive, with one question |
| `locallibrary sync --from <source> <target>` | Copy one drive onto another, overwriting the target's library whichever is newer |
| `locallibrary pull [<library> \| <drive>] [-l LIB] [-m MSG] [--label L]` | Snapshot a drive into the store (with a library, or no drive, every mounted drive of one library), see [The store](store.md) |
| `locallibrary check [<library> \| <drive>...] [--players MODEL,...] [--audio] [--verbose]` | Check the drives you're taking to a set, down to tracks a player won't load (`--audio`: and audio files cut short; tracks too loud or clipped, once `doctor --analyze` has measured them); exit status 4 on problems, see [Before a set](sets.md#before-a-set) |
| `locallibrary eject [<library> \| <drive>...]` | Unmount drives and power them off (experimental on Linux and Windows), see [Setting up and handing over](drives.md#setting-up-and-handing-over) |
| `locallibrary undo [<library> \| <drive>] [--list]` | Take back the last change locallibrary made to a library or drive (run again, the one before), after a plan of the files, playlists (in both databases), history lists and recorded edits that change; refused if the drive changed since, unless `--force`; `--list` shows what can be undone on every mounted drive |
| `locallibrary` / `locallibrary tui [<library>]` | Interactive player, see [Player](player.md) |

**Drives**

| | |
| --- | --- |
| `locallibrary drive [<drive>]` | With no drive, the mounted drives (`drive ls`); with one, its details (`drive info`) |
| `locallibrary drive ls [--all] [--known [--retired]]` | Mounted rekordbox drives, under their library (`--all`: every volume, for picking a blank target; `--known`: every drive locallibrary has seen, mounted or not) |
| `locallibrary drive info [<drive>]` | Library summary, space, settings files, history, interrupted operations |
| `locallibrary drive adopt <drive> [--name N \| --as KNOWN]` | Give a drive a stable identity, under its volume name unless `--name` (`--as`: give back a known drive's identity) |
| `locallibrary drive rename\|forget\|role\|retire\|log\|recognise\|reidentify\|player\|contact\|groups` | The drives you own, see [Your library's drives](drives.md#your-librarys-drives) |
| `locallibrary drive disks` | External disks, their partition table and file system, and which players can read them |
| `locallibrary drive format <drive> [--fs fat32\|exfat\|hfs+] [--check-capacity]` | Erase a drive and format it for players (experimental on Linux and Windows, see [Platforms](install.md#platforms)), see [Formatting a drive](drives.md#formatting-a-drive) |
| `locallibrary drive verify [<drive>] [--repair]` / `drive fsck <drive> [--repair]` | Re-read every file against a known-good copy and rewrite damaged ones; the OS's file-system check (`fsck`: experimental on Linux and Windows), see [Health](drives.md#health) |
| `locallibrary drive space [<drive>]` / `drive tidy <drive>` / `drive recordings [<drive>]` | What's using a drive and when it fills; remove OS clutter; copy off set recordings, see [Space](drives.md#space) |
| `locallibrary drive firmware <drive> [file.UPD] [--remove]` | Stage a player or mixer firmware update, see [Setting up and handing over](drives.md#setting-up-and-handing-over) |
| `locallibrary drive backups [<drive>]` / `drive restore <library> \| <drive>` | The backups taken before every change to a drive; put any of them back (`--backup ID`; `--dry-run` shows what would change; refuses a backup whose audio has since been removed, unless `--force`). `--playlist P` brings back one playlist or folder from a snapshot or backup, see [Bringing back a playlist](editing.md#bringing-back-a-playlist) |
| `locallibrary provision <target>` | Set up a new drive in one step, see [Setting up and handing over](drives.md#setting-up-and-handing-over) |
| `locallibrary doctor [<library>] [--hash-audio] [--by-sound] [--audio] [--analyze] [--only SECTION,...]` | Health report: duplicates with a suggested keeper (`--by-sound`: also the same recording in another file or encoding), missing or cut-short files, orphaned files, tracks in no playlist, database drift, files over 4 GB, how players read the drive (`--audio`: audio files cut short or that don't decode; `--analyze`: tempos and keys an analysis of the audio disagrees with, and tracks too loud, clipped or far from the library's level); `--fix-orphans` deletes orphans after a backup |
| `locallibrary analysis bench [<library> \| <drive>] [--keys PATH] [--tempos DIR] [--audio DIR]` | Score rekordbox's stored keys and tempos, and the analysis `doctor --analyze` runs, against the GiantSteps datasets (`--keys`, `--tempos` or both), see [Checking the analysis](drives.md#checking-the-analysis) |
| `locallibrary watch [--pull] [--sync-group] [--once] [--install \| --uninstall]` | Pull drives into the store as they're plugged in, so their history lists outlive rekordbox's import, and sync the rest of their group; `--install` runs that whenever you log in (`--no-sync-group`: only pull), see [Saving drives as they're plugged in](store.md#saving-drives-as-theyre-plugged-in) |

**Library and store**

| | |
| --- | --- |
| `locallibrary tracks [<library>] [-s WORDS] [-p PLAYLIST] [-f FILTER] [--sort KEYS] [--columns C,...] [--format table\|tsv\|csv]` | List, search, filter and sort tracks (`-s`: words to find, as free text in the filter), see [Filters](editing.md#filters) and [Output](#output) |
| `locallibrary library [<library>]` | With no library, the libraries in the store (`library ls`); with one, its snapshots (`library log`) |
| `locallibrary library ls\|rename` | The libraries in the store, their latest snapshot and drives; rename one |
| `locallibrary library log [LIB] [-n N]` | Snapshot history, with labels and the snapshots saved beside the latest |
| `locallibrary library push <drive> [--snapshot ID]` | Write the latest (or any) snapshot onto a drive |
| `locallibrary library prune [--keep-last N] ...` | Remove old snapshots (as `store prune`), see [Pruning old snapshots](store.md#pruning-old-snapshots) |
| `locallibrary store [info\|gc\|verify\|label\|unlabel\|prune]` | Store size, garbage collection, integrity check, snapshot labels, removing old snapshots |
| `locallibrary store serve` / `store fetch <code>` | Copy snapshots between computers over the network: one serves its store, the other fetches what it lacks, see [From another computer](store.md#from-another-computer) |
| `locallibrary store copy <dir>` / `store copies` | Copy the store somewhere else (a NAS, another disk, a cloud-synced folder), only what's new; where copies were made, see [Off-site copies](store.md#off-site-copies) |
| `locallibrary diff <a> <b> [--playlist P]` | Playlist entries added, removed and moved between libraries (a library's newest drive plugged in, else its latest snapshot), drives or `snapshot:<id>`s |
| `locallibrary diff <a> <b> --summary` | Which drive is newer, the evidence, and the differences (in both databases) |
| `locallibrary collection [info\|tracks\|playlists\|diff <library>]` | rekordbox's own collection on this computer (`master.db`), read only, and what a drive lacks against it, see [rekordbox's collection](other-software.md#rekordboxs-collection) |
| `locallibrary collection export <drive or folder> [--playlist P]...` | Write the collection to a drive or folder as rekordbox's Export does, Cloud Library Sync tracks included, see [Exporting it without rekordbox](other-software.md#exporting-it-without-rekordbox) |
| `locallibrary export <library> [PLAYLIST] -o FILE` | A playlist, folder, history list or the whole library as M3U8, CSV or text |
| `locallibrary export <library> [PLAYLIST] --format traktor\|serato\|engine -o DIR [--dry-run]` | The library for other DJ software, written on this computer, see [Other DJ software](other-software.md#other-dj-software) |
| `locallibrary export <library> [PLAYLIST] --format FORMAT -o PATH [--option KEY=VALUE]... [--dry-run] [--replace]` | A format a plugin adds; overwriting a file is asked about first unless `--replace`, see [Plugins](plugins.md) |
| `locallibrary analyze [<library>] [--plugin ID] [--analyzer NAME] [--track ID]...` | Run plugins' analyzers on a library's tracks, once each; `track` shows what they worked out as `PLUGIN.FIELD` (`sound.bpm`), see [Plugins](plugins.md) |
| `locallibrary xml [<library>] -o rekordbox.xml [--only PLAYLIST]` | Export as rekordbox XML (`--only`: just those playlists or folders and their tracks) |

**Editing**

| | |
| --- | --- |
| `locallibrary playlist [<library>]` | A library's playlist tree (`playlist ls`) |
| `locallibrary playlist ...` | Create, rename, move, delete, fill, sort, order for mixing and import playlists, see [Editing playlists and tracks](editing.md#editing-playlists-and-tracks) |
| `locallibrary track ...` | Show a track's details and where it's filed (`track <library> --query ...`), rate, colour and edit tracks, delete them, replace duplicates, find missing files (`track relocate`, see [Missing files](editing.md#missing-files)), swap MP3s for lossless files of their recording (`track upgrade`, see [Upgrading to lossless](editing.md#upgrading-to-lossless)), find harmonic matches |
| `locallibrary tags ...` | My Tags: list, `add`/`untag` tracks, `rename`, `delete` a tag, see [My Tags](editing.md#my-tags) |
| `locallibrary cues ...` | Hot cues, memory cues and loops: list, set, rename, recolour, delete, copy, see [Cues](editing.md#cues) |
| `locallibrary art [<library>] ...` | Tracks without artwork (`art missing`), and setting it, see [Cover art](editing.md#cover-art) |
| `locallibrary edits [list\|replay\|clear] [<library>]` | The edits made to a library; make them again after rekordbox exports over them |
| `locallibrary smart ...` | Rule-based playlists kept in the store and written to drives, see [Rule-based playlists](editing.md#rule-based-playlists) |
| `locallibrary merge <source> <target>` | Add the source's missing tracks and playlists to the target library (`--history`: and its history playlists) |
| `locallibrary pack <library> <drive> --playlist P` | A drive for a set holding only some playlists, with `--transcode` as copies its players play, see [Drives for a set](editing.md#drives-for-a-set) |
| `locallibrary convert <library> --to onelibrary\|device-library` | Build OneLibrary from `export.pdb` or the reverse; one the drive has gains what it lacks (`--replace` rebuilds it), see [Convert](editing.md#convert) |
| `locallibrary clean <library>` | Remove useless tracks (rekordbox's bundled demo samples, which pile up with every export) with their files, analysis and playlist entries |
| `locallibrary settings [<drive>] ...` | The settings profile, or a drive's settings, see [Settings](settings.md) |

**Plays and players**

| | |
| --- | --- |
| `locallibrary plays [<drive>]` | The sessions (history lists) players recorded on a drive, with their ids and dates (`plays lists`), see [History and play counts](sets.md#history-and-play-counts) |
| `locallibrary plays lists [<drive> [SESSION]] [--all] [--delete]` | Sessions players recorded (`--all`: every one the store kept); one session's tracks; `--delete` once archived |
| `locallibrary plays export <drive> <SESSION> -o FILE` | Write a session as text, CSV or M3U8 |
| `locallibrary plays import FILE.csv [--drive D] [--name N]` | Keep a log from Beat Link Trigger's Playlist Writer as a session, with the time each track played |
| `locallibrary plays stats <library\|drive\|snapshot:ID> [--since SNAPSHOT\|DATE]` | Most played, never played, and what was played since a snapshot or a date |
| `locallibrary sets [list] [--venue V] [--kind K] [--since D] [--until D]` | Your sets, newest first, see [Sets](sets.md#sets) |
| `locallibrary sets suggest [--dismiss SESSION]` | The sets your sessions suggest, each with the command that declares it |
| `locallibrary sets new\|show\|edit\|delete ...` | Declare a set from sessions or a recording, show it, change its details and tracklist, delete it |
| `locallibrary sets export <SET> [--format text\|mixcloud\|cue\|youtube\|1001\|csv\|m3u8] [-o FILE]` | A set's tracklist for Mixcloud, a CUE sheet, YouTube chapters, 1001Tracklists, a spreadsheet, or a playlist file |
| `locallibrary sets played <QUERY>` / `sets playlist <SET> <library>` | When and where you played a track; a playlist of a set's tracks in a library |
| `locallibrary sets publish <SET>` | Publish a set on your Atmosphere account (alpha), see [Publishing](account.md#publishing) |
| `locallibrary mix <recording>... [--drive LIB] [--html PATH] [--export PATH] [--open]` | Which of a drive's tracks a recorded set plays, when, at what speed and in what key, as a tracklist and an interactive report, see [Mixes](sets.md#mixes) |
| `locallibrary profile [<library>...]` | What your sets, collection and settings say about how you play, and what a public profile would show, see [DJ profile](sets.md#dj-profile) |
| `locallibrary players [show\|set\|clear\|models]` | The player models `check` uses when none are given, and what each model reads, see [Before a set](sets.md#before-a-set) |

**Other**

| | |
| --- | --- |
| `locallibrary account [login\|logout\|status]` | Sign in with your Atmosphere account (the one you use for Bluesky and other AT Protocol apps; alpha), see [Atmosphere account](account.md) |
| `locallibrary plugin [list\|info ID\|settings ID [KEY [VALUE \| --reset]]]` | The plugins locallibrary has, what each may do and adds, and their settings; the commands and operations they add are `locallibrary ID NAME` or where the plugin puts them, see [Plugins](plugins.md) |
| `locallibrary completions SHELL [--static]` | Shell completion for bash, zsh, fish, elvish or PowerShell, see [Shell completion and man pages](install.md#shell-completion-and-man-pages) |
| `locallibrary man [-o DIR]` | Man pages for every command (not in `--help`) |
| `locallibrary rpc` | JSON-RPC server for GUIs (not in `--help`), see [Building a GUI](sidecar.md) |

Commands that moved into a group still work under their old names, which `--help` doesn't list:
`drives` (`drive ls`), `info`, `adopt`, `disks`, `format`, `verify`, `fsck`, `space`, `tidy`,
`recordings`, `firmware`, `backups` and `restore` (each `drive ...`), `log` and `push`
(`library ...`), `history` and `stats` (`plays lists`, `plays stats`), `playlists`
(`playlist ls`), `mirror` (`sync --from`), `compare` (`diff --summary`), `drive list`
(`drive ls --known`) and `drive sets` (`drive groups`). `set` is a spelling of `sets` now: the
night-before job it used to be is `prep`.

### Output

What a command reports (lists, plans, the problems a check finds, results) goes to stdout, so it
can be piped or redirected; progress bars, questions and errors go to stderr. With `--json`,
stdout holds only the JSON. Long listings (`tracks`, `playlist ls`, `doctor`, `diff`, `library
log`, `plays lists`, `plays stats`, `profile`...) go through a pager on macOS and Linux when stdout
is a terminal:
`$PAGER`, else `less` (with `LESS=FRX` unless `LESS` is set, so a listing that fits on the
screen is just printed); `--no-pager`, `PAGER=` or `PAGER=cat` turn it off. Output piped into
`head` stops quietly.

- `tracks` prints a header row; `--columns id,artist,title,bpm,key,rating,time,path...` picks
  the columns and `--format tsv` or `--format csv` writes every value whole for other programs
  (a header row, no count). Names are cut to fit by the columns the terminal shows them in,
  wide characters taking two.
- Lists capped at "... and N more" (`doctor`'s duplicates and files, `drive verify`'s damaged
  files, `diff`'s entries, `plays stats`'s never played) show everything with `--all`.
- `-q`/`--quiet` leaves out progress bars and tips (what to run next, how to undo); warnings and
  results still print.
- `--color auto|always|never`, given before the command (`NO_COLOR` turns colour off too), and
  `--ascii` for `v x * -> ...` instead of ✓ ✗ ★ →, which is the default when the locale
  (`LC_ALL`, `LC_CTYPE`, `LANG`) isn't UTF-8.
- Progress bars count in decimal units (GB, as plans do) and show the time left.

### Exit status

| | |
| --- | --- |
| 0 | Done, nothing to do, or a dry run |
| 1 | An error (`--json` prints `{"error": {"kind", "message"}, "outcome": "error"}`) |
| 2 | Usage: an unknown command or option, a missing argument, or no drive given where several could be meant |
| 3 | Not applied: the question was answered no, or there was no terminal to ask on and no `--yes` |
| 4 | Problems found: `doctor`, `check`, `prep`, `drive verify` (and `drive verify --repair` when files stay wrong), `drive fsck`, `store verify`, `drive firmware` checks |
| 5 | Some failed: some of several drives (`sync`, `settings apply`, `smart apply`, `art auto`, a read of every library or drive mounted), recordings, embedded files, a store copy's snapshots, the tracks `analyze` ran on (1 when every one failed), or a `provision`, `refresh`, `prep` or `pull` (of every drive) that stopped part way |
| 130 | Cancelled with Ctrl-C (`"outcome": "cancelled"`) |

### JSON

With `--json`, every command prints one JSON document on stdout, and every JSON object it prints at
the top level (each line of `watch --json` too) has `"schema": 1`, the version of these shapes
(raised only by a change that would break a script); a list at the top level has none. A command
that writes prints the same envelope whatever it does:

```json
{ "schema": 1, "plan": { ... }, "report": { ... }, "applied": true, "outcome": "applied" }
```

`plan` is what it planned (the same object the sidecar's `*.plan` returns, with `clones` when it's
copied onto other drives of the library: each `{name, root}`, and `left_alone` saying why one isn't;
`{"summary": [...]}` for a change made in one step, such as `drive role`; `null` for one that
doesn't ask, such as `pull` or a new file `export` writes), and `report` what applying it did
(`null` when it wasn't applied; a list, one entry per drive, for `sync` with several drives and
`smart apply`). `outcome` is `applied`, `nothing_to_do`, `dry_run`, `declined`, `not_confirmed`,
`problems` or `partial`. A check (`doctor`, `check`, `drive verify` without `--repair`, `drive
fsck`, `store verify`) prints its report with `outcome` added; `drive verify --repair` prints the
check as the plan and the repair as the report. Reads print what they read: an object (`status` is
always `{mounted, offline, duplicates, libraries}`, `library log` is `{libraries: {<name>:
[snapshots]}}`) or a list. A read given no drive while several libraries or drives are mounted
prints a list, one `{"drive", "root", "result", "error"}` for each (a library under its name, with
its lead clone's `root`; `doctor`'s reports without `outcome`); `error` is `{"kind", "message"}` for
a drive that couldn't be read, and `null` otherwise. An error is `{"schema": 1, "error": {"kind",
"message"}, "outcome": "error"}` (`"cancelled"` after Ctrl-C), with the sidecar's error kinds
(`not_found`, `conflict`, `stale_plan`...; `usage` for a command line that doesn't say enough, such
as which drive, with exit status 2; `invalid` for one asking for something that can't be done);
`code`, the old name of `kind`, is still there. The error also has the sidecar's `reason`, `fields`
and `actions` (see [Building a GUI](sidecar.md)); without `--json`, the actions are printed as
`hint:` lines with the command to type.

`sync` and `library push` accept `--dry-run`, `--verify` (compare audio by content hash rather than
size + modification time), `--keep-extra` (don't delete audio and other files the source lacks;
database files it lacks still go, and the drive still compares as holding the source's library),
`--delete-first` (free space before copying when the target is nearly full) and `--eject` (eject
the drives afterwards).

### Typical workflows

**One library on several drives**

```bash
locallibrary pull -m "after Friday's export"   # keep history (optional but recommended)
locallibrary sync                              # the newest drive is copied onto the others
locallibrary status                            # the library and its drives at a glance
```

**A drive went missing / got corrupted**

```bash
locallibrary library log                       # pick a snapshot
locallibrary library push /Volumes/NEW -l Home --snapshot 3f9a2c
```

**Rekordbox exported to the wrong drive, and both changed**

```bash
locallibrary diff REKORDBOX BACKUP --summary   # "both changed since they were last in sync (2026-09-30 21:04)"
locallibrary merge BACKUP REKORDBOX            # keep both sets of new tracks
locallibrary sync Home                         # then make them identical again
```

**A new drive**

```bash
locallibrary drive format /Volumes/NEW --label SPARE   # MBR + FAT32; type the name to confirm
locallibrary provision /Volumes/SPARE --like Home      # another copy of the library
```

**B2B: bring a friend's crate into your library**

```bash
locallibrary merge "Friend's USB" Home --folder "Sam" --only "Sets/Saturday"
locallibrary undo Home                 # changed your mind
locallibrary playlist delete Home Sam --with-tracks   # after the set: the folder and the tracks only it used
```

**A drive for tonight**

```bash
locallibrary pack Home /Volumes/TONIGHT --playlist "Sets/2026/Friday" --playlist Classics --transcode
locallibrary check REKORDBOX /Volumes/TONIGHT --players CDJ-3000X,CDJ-2000NXS2
```
