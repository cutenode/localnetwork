# locallibrary

A Rust library and CLI (`locallibrary`) for the rekordbox libraries that live on USB drives: keep several
drives in sync, know which one is newest, back them up with full history, merge libraries, edit
playlists, tracks, My Tags and cues right on the drive, manage player/mixer settings across every
drive, and fix cover art. It's built to be the engine behind a desktop app: every operation is
available as JSON over a sidecar process.

```text
rekordbox ──Export──▶ USB A ──locallibrary sync──▶ USB B, USB C ...
                        │
                    locallibrary pull              locallibrary library push ──▶ any drive, any snapshot
                        ▼
                 store (history, deduplicated)
```

## Why

rekordbox's Export writes a self-contained library to a drive (`PIONEER/` + `Contents/`). DJs
usually keep two or three drives, and keeping them identical means exporting to each one
separately and remembering which is current. locallibrary treats a drive export as data:

- **Export once, replicate incrementally.** Copy only what changed to your other drives. A
  re-sync after adding 10 tracks copies those 10 tracks, their analysis files and the database.
- **Know which drive is newer.** Every drive locallibrary touches records the history of
  library states it has held (its *lineage*), so "B is behind A" is a fact, not a guess. Drives it
  has never seen are compared on evidence (track sets, `export.pdb`'s save counter, timestamps),
  and the reasons are shown.
- **History and rollback.** The store keeps content-addressed snapshots. Audio is stored once no
  matter how many drives or snapshots contain it. Any snapshot can be written back to any drive.
- **Merge different libraries** (two DJs, two laptops, an old drive). Tracks come across with
  their beat grids, cues, waveforms and artwork, and playlists are recreated.
- **Edit the drive itself.** Create, rename, move and delete playlists and folders, fill and sort
  them (by key and tempo, or in a mixable order), rate and colour tracks, fix their details, tag
  them, move their cues, replace duplicates and delete tracks, without going back to rekordbox.
  Playlist and track edits are recorded, so they can be made again after rekordbox's next export
  replaces them.
- **Drives for a set, and rule-based playlists.** Put only some playlists on a drive and keep it up
  to date incrementally; keep filters like `bpm:126-132 rating:>=4` in the store and write them to
  every drive as playlists.
- **Keep a drive healthy.** Find duplicates, missing and orphaned files and tracks in no playlist,
  check your drives before a set, and keep every history list players recorded, even after
  rekordbox deletes them from the drive.
- **A DJ profile.** What your sets say about how you play: the tempo range, how often you mix in
  key, whether your sets build, the tracks you keep coming back to. A rounded summary that leaves
  out anything you haven't chosen to show is ready for a public profile.
- **Every player, including the newest.** rekordbox 7 exports two databases: `export.pdb` for most
  players, and OneLibrary's `exportLibrary.db`, the only one the CDJ-3000X, CDJ-1500X, OPUS-QUAD,
  OMNIS-DUO, XDJ-AZ and XDJ-AN read. Every edit updates both (when a drive's `exportLibrary.db`
  can't be read, edits warn and change `export.pdb` alone), drives exported for OneLibrary only
  work too, `drive info` shows when the two disagree, and `convert` builds either one from the
  other.
- **One settings profile for every drive.** Players load "My Settings" (quantize, auto cue, jog
  mode, language, fader curves, waveform colours...) from the drive. Define them once, with
  per-drive overrides (e.g. a Japanese-language drive for a residency in Tokyo).
- **Cover art** on drives (players' thumbnails) and in audio files (for your computer's library).
- **Recover a library from a drive.** Export a drive as rekordbox XML and import it into rekordbox
  with cues and beat grids.
- **Format drives the way players read them.** MBR + FAT32 with sensible clusters on macOS,
  Linux and Windows (including FAT32 above 32 GB, which Windows itself refuses), plus a check of
  how every connected drive will fare on CDJs and XDJs. Linux and Windows are experimental, see
  [Platforms](#platforms).

## Install

Each [GitHub Release](https://github.com/cutenode/locallibrary/releases) has `locallibrary` for
macOS (Apple silicon and Intel), Linux (x86_64) and Windows (x86_64), and installers for it. The
Linux and Windows builds are experimental and untested on hardware (see [Platforms](#platforms)).
There hasn't been a release yet (see [Releasing](#releasing)); until then, build from source.

```bash
# macOS and Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/cutenode/locallibrary/releases/latest/download/locallibrary-cli-installer.sh | sh

# Homebrew
brew install cutenode/tap/locallibrary
```

```powershell
# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/cutenode/locallibrary/releases/latest/download/locallibrary-cli-installer.ps1 | iex"
```

The installers put `locallibrary` in `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows). The
Linux build needs glibc 2.35 or newer (Ubuntu 22.04, Debian 12) and ALSA's library
(`libasound2`, which desktops already have) for the player's audio.

From source, with Rust 1.89 or newer:

```bash
cargo install --path crates/locallibrary-cli --locked
```

This installs `locallibrary`. On Linux, the player's audio output needs the ALSA headers
(`sudo apt install libasound2-dev pkg-config` on Debian/Ubuntu).

Cargo features choose what's built. All three are on unless `--no-default-features` turns them
off:

- `account`: the Atmosphere account and publishing to it (`account`, `profile publish`, `plays
  publish`, `settings publish`, `settings import --from-handle`, the player's sign-in, and the
  sidecar's `account.*` and `*.publish.*` methods). Half the dependencies are for this alone (an
  HTTP client, an async runtime, TLS, DNS, the system keychain): about 220 of the 430 crates the
  command builds. Without it the profile and its document are still made, just not published.
- `tui`: the player (`locallibrary tui`, and `locallibrary` on its own in a terminal).
- `playback`: the player's audio output, and with it ALSA on Linux. Without it the player
  browses, edits and shows waveforms, but has no output to play through.

```bash
# A headless sidecar for a desktop app: no player, no account (about 150 crates)
cargo install --path crates/locallibrary-cli --locked --no-default-features
# The same, with the account
cargo install --path crates/locallibrary-cli --locked --no-default-features --features account
# The player without audio output: no ALSA headers needed on Linux
cargo install --path crates/locallibrary-cli --locked --no-default-features --features tui,account
```

The library (`crates/locallibrary`) has the `account` feature too, on by default.

### Shell completion and man pages
Tab completes commands and flags, and also the drives mounted now, their playlist paths, My Tags and
history lists, the libraries in the store, player models and the steps `refresh --skip` leaves out:
the shell asks `locallibrary` itself as you type. Load the script from your shell's startup file
(rather than saving it once), so it always matches the `locallibrary` you have:

```bash
echo 'source <(locallibrary completions bash)' >> ~/.bashrc
echo 'source <(locallibrary completions zsh)' >> ~/.zshrc
locallibrary completions fish > ~/.config/fish/completions/locallibrary.fish
echo 'eval (locallibrary completions elvish | slurp)' >> ~/.elvish/rc.elv
```

In PowerShell, add `locallibrary completions powershell | Out-String | Invoke-Expression` to
`$PROFILE`. Where running `locallibrary` on Tab isn't wanted, `completions SHELL --static`
prints a script that completes only commands and flags. `locallibrary man -o DIR` writes a man
page for every command (`locallibrary.1`, `locallibrary-drive-ls.1`...) into `DIR`, for a
`man` path such as `/usr/local/share/man/man1`; `locallibrary man` prints the top-level one.

## Platforms

locallibrary is developed and used on macOS. **Linux and Windows are experimental, untested on
hardware**, and CI hasn't passed on them yet. Most commands only read and write files, and are
written to work the same on all three. The commands that drive the operating system's own disk
tools have only ever run on macOS: `drive format` (with `--check-capacity` too, and `provision
--format`), `drive fsck`, `eject` (and `--eject` on `sync` and `provision`, and `set`) and `drive
rename --label`. On Linux and Windows their code paths have never run (the Windows FAT32 writer
hasn't even been compiled), so each prints a warning when it starts, and you should check the
result yourself (`locallibrary drive disks`, `locallibrary drive ls`) before trusting a drive to
it.

## Quick start

```bash
# The first time: name your drives, start your library, import My Settings (it asks as it goes)
locallibrary setup

# What's plugged in?
locallibrary drive ls

# Give your drives names (stored on the drive in .locallibrary/, invisible to players)
locallibrary drive adopt /Volumes/REKORDBOX --name "Main"
locallibrary drive adopt /Volumes/BACKUP --name "Backup"

# After exporting from rekordbox to Main: which is newer, and what changed?
locallibrary diff Main Backup --summary

# Update whichever is older (shows the plan and asks first)
locallibrary sync Main Backup

# After rekordbox exports to Main: your edits, rule-based playlists, player name and My Settings
# back, then saved in the store
locallibrary refresh Main

# The night before a set: save, sync, check and eject every drive of the library
locallibrary set

# Or just browse and listen
locallibrary
```

## First run

```bash
locallibrary setup
```

`setup` walks through the first run in a terminal. It finds the mounted rekordbox drives, asks
which to set up (`setup DRIVE...` names them), names each one (a drive the store knew before it
was erased gets its identity back), asks which one rekordbox exports to (the primary) and what the
others are for (backup, set or archive), and starts your library: the primary is saved in the
store, then the other drives where they hold the same library. Run again, it saves them in the
library they're in rather than starting another, and keeps their roles. Then, if you like, it
imports the primary's My Settings into your profile, sets the Device Name players show and your
contact details on the drives (archive drives are kept as they are), and remembers the player
models you play on for `check` and `set`. Each change shows its plan and asks first. It ends with
the commands you'll use most.

Without a terminal (or with `--json`) it asks nothing: pass `--yes` and `--library NAME`, and the
rest as flags (a drive is its mount path, volume name or name). It takes every mounted rekordbox
drive but those of another library in the store (name the drives to choose), and won't move a
primary out of the library it's in:

```bash
locallibrary --yes setup --library Home --primary /Volumes/REKORDBOX \
  --name /Volumes/REKORDBOX=Main --name BACKUP=Backup --role BACKUP=set \
  --player-name BACKUP="BACKUP B" --contact sam@example.com --players CDJ-3000,CDJ-2000NXS2
```

Until there's a store, every command starts with a one-line reminder of `setup` (on stderr).

## Commands

Drives can be given as a mount path, an adopted name, or an id prefix; a bare word is a drive's name
before it's a folder in the working directory (one that's both is refused: give `./NAME`). Every
command accepts `--json` for machine-readable output (it never prompts; commands that change
something need `--yes` to apply changes), `--yes` to skip confirmations, `--overwrite-archive` to
write to an archive or retired drive (writes to them are refused otherwise), and `--store DIR` (or
`LOCALLIBRARY_STORE`) to choose the store. `locallibrary --help` lists the commands by what they're
for, with common tasks; `locallibrary help COMMAND` (or `COMMAND --help`) says more about one, with
examples.

Commands that read one drive (`drive info`, `tracks`, `playlist ls`, `drive backups`, `doctor`,
`drive space`, `xml`, `settings show`, `art missing`) work without one: they take the only mounted
rekordbox drive, or, when the mounted drives that belong to a library all belong to one, its primary
(`drive role <drive> primary`). When that can't be told, the error lists the mounted drives (name,
mount path, role, library) and the exit status is 2 (with `--json`, the error's kind is `usage`). A
name that matches no mounted drive gets the drives that are mounted in its error, and, for a drive
you own that isn't plugged in, when and where it was last seen. `check`, `eject`, `pull` and `set`
with no drives take the mounted drives of one library, its drive group (archive and retired drives
are left out unless named).

Every command that writes shows its plan first and asks, then makes exactly the change it showed:
if the drive changed in between (rekordbox exported again, another command wrote to it), nothing
is written and it says to plan again.

**Everyday**

| | |
| --- | --- |
| `locallibrary setup` | The first run: name your drives, choose their roles, start your library, import My Settings, set what players show and remember your player models, see [First run](#first-run) |
| `locallibrary refresh [<drive>] [--sync] [--skip STEP]` | rekordbox just exported to a drive: its demo samples out, your recorded edits, rule-based playlists, player name and My Settings back, then saved in the store (`--sync`: and copied onto its library's other drives), one plan, see [After rekordbox exports](#after-rekordbox-exports) |
| `locallibrary set [<drive>...] [--players MODEL,...] [--no-eject]` | The night before a set: save the drives in the store, copy the newest library onto the others, check them for the players, eject them, one plan, see [The night before a set](#the-night-before-a-set) |
| `locallibrary status [--all]` | Every mounted drive against its library's latest snapshot (up to date / behind / has new changes / diverged), settings drift, roles, write protection, low space and copies; `--all` adds the drives that aren't plugged in, see [Drive management](#drive-management) |
| `locallibrary sync [<a> <b> ...]` | Copy the newest drive over the others, all at once (refuses if any diverged or hold different libraries, except a drive [back from a set](#back-from-a-set)); with no drives, the mounted drive group |
| `locallibrary sync --from <source> <target>` | Copy one drive onto another, overwriting the target's library whichever is newer |
| `locallibrary pull [<drive>] [-l LIB] [-m MSG] [--label L]` | Snapshot a drive into the store (with no drive, every mounted drive of one library), see [The store](#the-store) |
| `locallibrary check [<drive>...] [--players MODEL,...] [--audio] [--verbose]` | Check the drives you're taking to a set, down to tracks a player won't load (`--audio`: and audio files cut short; exit status 4 on problems), see [Before a set](#before-a-set) |
| `locallibrary eject [<drive>...]` | Unmount drives and power them off (experimental on Linux and Windows), see [Setting up and handing over](#setting-up-and-handing-over) |
| `locallibrary undo [<drive>] [--list]` | Take back the last change locallibrary made to a drive (run again, the one before), after a plan of the files, playlists, history lists and recorded edits that change; refused if the drive changed since, unless `--force`; `--list` shows what can be undone on every mounted drive |
| `locallibrary` / `locallibrary tui [<drive>]` | Interactive player, see [Player](#player) |

**Drives**

| | |
| --- | --- |
| `locallibrary drive [<drive>]` | With no drive, the mounted drives (`drive ls`); with one, its details (`drive info`) |
| `locallibrary drive ls [--all] [--known [--retired]]` | Mounted rekordbox drives (`--all`: every volume, for picking a blank target; `--known`: every drive locallibrary has seen, mounted or not) |
| `locallibrary drive info [<drive>]` | Library summary, space, settings files, history, interrupted operations |
| `locallibrary drive adopt <drive> [--name N \| --as KNOWN]` | Give a drive a stable identity and name (`--as`: give back a known drive's identity) |
| `locallibrary drive rename\|forget\|role\|retire\|log\|recognise\|reidentify\|player\|contact\|groups` | The drives you own, see [Drive management](#drive-management) |
| `locallibrary drive disks` | External disks, their partition table and file system, and which players can read them |
| `locallibrary drive format <drive> [--fs fat32\|exfat\|hfs+] [--check-capacity]` | Erase a drive and format it for players (experimental on Linux and Windows, see [Platforms](#platforms)), see [Formatting a drive](#formatting-a-drive) |
| `locallibrary drive verify <drive> [--repair]` / `drive fsck <drive> [--repair]` | Re-read every file against a known-good copy and rewrite damaged ones; the OS's file-system check (`fsck`: experimental on Linux and Windows), see [Health](#health) |
| `locallibrary drive space [<drive>]` / `drive tidy <drive>` / `drive recordings <drive>` | What's using a drive and when it fills; remove OS clutter; copy off set recordings, see [Space](#space) |
| `locallibrary drive firmware <drive> <file.UPD>` | Stage a player or mixer firmware update, see [Setting up and handing over](#setting-up-and-handing-over) |
| `locallibrary drive backups [<drive>]` / `drive restore <drive>` | The backups taken before every change to a drive; put any of them back (`--backup ID`; `--dry-run` shows what would change; refuses a backup whose audio has since been removed, unless `--force`). `--playlist P` brings back one playlist or folder from a snapshot or backup, see [Bringing back a playlist](#bringing-back-a-playlist) |
| `locallibrary provision <target>` | Set up a new drive in one step, see [Setting up and handing over](#setting-up-and-handing-over) |
| `locallibrary doctor [<drive>] [--hash-audio] [--audio] [--only SECTION,...]` | Health report: duplicates with a suggested keeper, missing or wrong-size files, orphaned files, tracks in no playlist, database drift, files over 4 GB, how players read the drive (`--audio`: audio files cut short or that don't decode); `--fix-orphans` deletes orphans after a backup |
| `locallibrary watch [--pull] [--sync-group] [--once]` | Pull drives into the store as they're plugged in, so their history lists outlive rekordbox's import, and sync the rest of their group, see [Saving drives as they're plugged in](#saving-drives-as-theyre-plugged-in) |

**Library and store**

| | |
| --- | --- |
| `locallibrary tracks [<drive>] [-s WORDS] [-p PLAYLIST] [-f FILTER] [--sort KEYS] [--columns C,...] [--format table\|tsv\|csv]` | List, search, filter and sort tracks (`-s`: words to find, as free text in the filter), see [Filters](#filters) and [Output](#output) |
| `locallibrary library [<library>]` | With no library, the libraries in the store (`library ls`); with one, its snapshots (`library log`) |
| `locallibrary library ls\|rename` | The libraries in the store, their latest snapshot and drives; rename one |
| `locallibrary library log [LIB] [-n N]` | Snapshot history, with labels |
| `locallibrary library push <drive> [--snapshot ID]` | Write the latest (or any) snapshot onto a drive |
| `locallibrary library prune [--keep-last N] ...` | Remove old snapshots (as `store prune`), see [Pruning old snapshots](#pruning-old-snapshots) |
| `locallibrary store [info\|gc\|verify\|label\|unlabel\|prune]` | Store size, garbage collection, integrity check, snapshot labels, removing old snapshots |
| `locallibrary store copy <dir>` / `store copies` | Copy the store somewhere else (a NAS, another disk, a cloud-synced folder), only what's new; where copies were made, see [Off-site copies](#off-site-copies) |
| `locallibrary diff <a> <b> [--playlist P]` | Playlist entries added, removed and moved between drives, `snapshot:<id>`s or libraries |
| `locallibrary diff <a> <b> --summary` | Which drive is newer, the evidence, and the differences |
| `locallibrary collection [info\|tracks\|playlists\|diff <drive>]` | rekordbox's own collection on this computer (`master.db`), read only, and what a drive lacks against it, see [rekordbox's collection](#rekordboxs-collection) |
| `locallibrary export <drive> [PLAYLIST] -o FILE` | A playlist, folder, history list or the whole library as M3U8, CSV or text |
| `locallibrary export <drive> [PLAYLIST] --format traktor\|serato\|engine -o DIR [--dry-run]` | The library for other DJ software, written on this computer, see [Other DJ software](#other-dj-software) |
| `locallibrary xml [<drive>] -o rekordbox.xml [--only PLAYLIST]` | Export as rekordbox XML (`--only`: just those playlists or folders and their tracks) |

**Editing**

| | |
| --- | --- |
| `locallibrary playlist [<drive>]` | A drive's playlist tree (`playlist ls`) |
| `locallibrary playlist ...` | Create, rename, move, delete, fill, sort, order for mixing and import playlists, see [Editing playlists and tracks](#editing-playlists-and-tracks) |
| `locallibrary track ...` | Show a track's details and where it's filed (`track <drive> --query ...`), rate, colour and edit tracks, delete them, replace duplicates, find harmonic matches |
| `locallibrary tags ...` | My Tags: list, `add`/`untag` tracks, `rename`, `delete` a tag, see [My Tags](#my-tags) |
| `locallibrary cues ...` | Hot cues, memory cues and loops: list, set, rename, recolour, delete, copy, see [Cues](#cues) |
| `locallibrary art [<drive>] ...` | Tracks without artwork (`art missing`), and setting it, see [Cover art](#cover-art) |
| `locallibrary edits [list\|replay\|clear] <drive>` | The edits made to a drive; make them again after rekordbox exports over them |
| `locallibrary smart ...` | Rule-based playlists kept in the store and written to drives, see [Rule-based playlists](#rule-based-playlists) |
| `locallibrary merge <source> <target>` | Add the source's missing tracks and playlists to the target (`--history`: and its history playlists) |
| `locallibrary pack <source> <target> --playlist P` | A drive for a set holding only some playlists, see [Drives for a set](#drives-for-a-set) |
| `locallibrary convert <drive> --to onelibrary\|device-library` | Build OneLibrary from `export.pdb` or the reverse (`--replace` rebuilds an existing one), see [Convert](#convert) |
| `locallibrary clean <drive>` | Remove useless tracks (rekordbox's bundled demo samples, which pile up with every export) with their files, analysis and playlist entries |
| `locallibrary settings [<drive>] ...` | The settings profile, or a drive's settings, see [Settings](#settings) |

**Plays and players**

| | |
| --- | --- |
| `locallibrary plays [<drive>]` | History lists players recorded on a drive (`plays lists`) |
| `locallibrary plays lists <drive> [LIST] [--all] [--delete]` | History lists players recorded (`--all`: every one the store kept); one list's tracks; `--delete` once archived |
| `locallibrary plays export <drive> <LIST> -o FILE` | Write a history list as text, CSV or M3U8 |
| `locallibrary plays stats <drive\|library> [--since SNAPSHOT\|DATE]` | Most played, never played, and what was played since a snapshot or a date |
| `locallibrary plays publish <drive> <LIST>` | Publish a history list as a set on your Atmosphere account, see [Publishing](#publishing) |
| `locallibrary mix <recording>... [--drive D] [--html PATH] [--export PATH] [--open]` | Which of a drive's tracks a recorded set plays, when, at what speed and in what key, as a tracklist and an interactive report, see [Mixes](#mixes) |
| `locallibrary profile [<drive>...]` | What your sets, collection and settings say about how you play, and what a public profile would show, see [DJ profile](#dj-profile) |
| `locallibrary players [set\|clear\|models]` | The player models `check` uses when none are given, and what each model reads, see [Before a set](#before-a-set) |

**Other**

| | |
| --- | --- |
| `locallibrary account [login\|logout\|status]` | Sign in with your Atmosphere account (the one you use for Bluesky and other AT Protocol apps), see [Atmosphere account](#atmosphere-account) |
| `locallibrary completions SHELL [--static]` | Shell completion for bash, zsh, fish, elvish or PowerShell, see [Shell completion and man pages](#shell-completion-and-man-pages) |
| `locallibrary man [-o DIR]` | Man pages for every command (not in `--help`) |
| `locallibrary rpc` | JSON-RPC server for GUIs (not in `--help`), see [Building a GUI](#building-a-gui-electron) |

Commands that moved into a group still work under their old names, which `--help` doesn't list:
`drives` (`drive ls`), `info`, `adopt`, `disks`, `format`, `verify`, `fsck`, `space`, `tidy`,
`recordings`, `firmware`, `backups` and `restore` (each `drive ...`), `log` and `push`
(`library ...`), `history` and `stats` (`plays lists`, `plays stats`), `playlists`
(`playlist ls`), `mirror` (`sync --from`), `compare` (`diff --summary`), `drive list`
(`drive ls --known`) and `drive sets` (`drive groups`).

### Output

What a command reports (lists, plans, the problems a check finds, results) goes to stdout, so it
can be piped or redirected; progress bars, questions and errors go to stderr. With `--json`,
stdout holds only the JSON. Long listings (`tracks`, `playlist ls`, `doctor`, `diff`, `library
log`, `plays lists`, `plays stats`, `profile`...) go through a pager when stdout is a terminal:
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
| 4 | Problems found: `doctor`, `check`, `drive verify` (and `drive verify --repair` when files stay wrong), `drive fsck`, `store verify`, `drive firmware` checks |
| 5 | Some failed: some of several drives (`sync`, `settings apply`, `smart apply`), recordings, embedded files, a store copy's snapshots, or a `provision` that stopped part way |
| 130 | Cancelled with Ctrl-C (`"outcome": "cancelled"`) |

### JSON

With `--json`, every command prints one JSON document on stdout, and every JSON object it prints
at the top level has `"schema": 1`, the version of these shapes (raised only by a change that
would break a script). A command that writes prints the same envelope whatever it does:

```json
{ "schema": 1, "plan": { ... }, "report": { ... }, "applied": true, "outcome": "applied" }
```

`plan` is what it planned (the same object the sidecar's `*.plan` returns; `{"summary": [...]}`
for a change made in one step, such as `drive role`; `null` for one that doesn't ask, such as
`pull` or a new file `export` writes), and `report` what applying it did (`null` when
it wasn't applied; a list, one entry per drive, for `sync` with several drives and `smart
apply`). `outcome` is `applied`, `nothing_to_do`, `dry_run`, `declined`, `not_confirmed`,
`problems` or `partial`. A check (`doctor`, `check`, `drive verify` without `--repair`) prints
its report with `outcome` added; `drive verify --repair` prints the check as the plan and the
repair as the report. Reads print what they read: an object (`status` is always `{mounted,
offline, duplicates}`, `library log` is `{libraries: {<name>: [snapshots]}}`) or a list. An error
is `{"schema": 1, "error": {"kind", "message"}, "outcome": "error"}` (`"cancelled"` after
Ctrl-C), with the sidecar's error kinds (`not_found`, `conflict`, `stale_plan`...; `usage` for a
command line that doesn't say enough, such as which drive, with exit status 2; `invalid` for one
asking for something that can't be done); `code`, the old name of `kind`, is still there. The
error also has the sidecar's `reason`, `fields` and `actions` (see [Building a
GUI](#building-a-gui-electron)); without `--json`, the actions are printed as `hint:` lines with
the command to type.

`sync` and `library push` accept `--dry-run`, `--verify` (compare audio by content hash rather than
size + modification time), `--keep-extra` (don't delete audio and other files the source lacks;
database files it lacks still go, and the drive still compares as holding the source's library),
`--delete-first` (free space before copying when the target is nearly full) and `--eject` (eject
the drives afterwards).

### Typical workflows

**Several drives, one library**

```bash
locallibrary pull Main -m "after Friday's export"   # keep history (optional but recommended)
locallibrary sync Main Backup "Club drive"          # whichever is newest is copied to the others
locallibrary status                                 # every drive at a glance
```

**A drive went missing / got corrupted**

```bash
locallibrary library log                       # pick a snapshot
locallibrary library push "New drive" --snapshot 3f9a2c
```

**Rekordbox exported to the wrong drive, and both changed**

```bash
locallibrary diff Main Backup --summary   # "both changed since they were last in sync (2026-09-30 21:04)"
locallibrary merge Backup Main         # keep both sets of new tracks
locallibrary sync Main Backup          # then make them identical again
```

**A new drive**

```bash
locallibrary drive format /Volumes/NEW --label BACKUP   # MBR + FAT32; type the name to confirm
locallibrary sync Main /Volumes/BACKUP
```

**B2B: bring a friend's crate onto your drive**

```bash
locallibrary merge "Friend's USB" Main --folder "Sam" --only "Sets/Saturday"
locallibrary undo Main                 # changed your mind
locallibrary playlist delete Main Sam --with-tracks   # after the set: the folder and the tracks only it used
```

**A drive for tonight**

```bash
locallibrary pack Main /Volumes/TONIGHT --playlist "Sets/2026/Friday" --playlist Classics
locallibrary check Main /Volumes/TONIGHT --players CDJ-3000X,CDJ-2000NXS2
```

## Player

`locallibrary` on its own (or `locallibrary tui [<drive>]`) opens a terminal UI for browsing and
listening to a drive the way the players see it:

- **Library**: mounted drives (hot-plugged ones appear on their own, and an unplugged one is
  marked), the playlist tree with folders, and the track list with BPM, key and rekordbox's track
  colours (`*` adds the ratings, `#` the [My Tags](#my-tags)). Keys that mix with the playing
  track, or with nothing playing the selected one, show green: the same key, ±1 or relative on the
  Camelot wheel. **Browse**, at the top of the tree, groups the drive like a CDJ's top menu: by
  artist, album, genre, key, BPM (5-BPM bands), rating, colour, month added, My Tag, the history
  lists players recorded, and problems.
- **Search and sort**: `/` searches as you type the way `tracks --filter` does (plain words look in
  title, artist, album, genre, label, key, comment and file path), and takes the whole
  [filter language](#filters) (`bpm:120-128 key:8A~ rating:>=4 -genre:house tag:Deep`); Tab in the
  search box widens it to the whole drive. Click a column header, or press `b`, to sort by title,
  artist, album, genre, BPM, key (round the Camelot wheel), rating, time, date added or play
  count; click again to reverse.
- **Track details** (`i`): everything rekordbox knows about the selected track (comment, label,
  remixer, mix, date added, plays, format, size), the playlists it's in, and the tracks that
  **mix into** it, which you can play or tag from there.
- **Crate** (`t`): a CDJ's TAG LIST, called the crate here so it isn't taken for My Tags. Collect
  tracks while browsing (`a` adds the marked or selected ones; `m` marks), reorder them, play the
  crate, save it to the drive as a playlist, or pack it onto another drive for a set.
- **Commands** (`:`): every action by name, with its key; type a few of its letters (in order:
  `fsp` finds "fill the smart playlists") and enter. It also has the commands with no key of their
  own: pull, check, set prep, sync, pack, restore, replay, undo, export, xml, smart and eject.
- **Problems**: ✖ marks a track whose audio file isn't on the drive, ≈ one the drive holds another
  copy of. Browse › Problems lists them.
- **Playback** of MP3, AAC/ALAC (`.m4a`), FLAC, WAV and AIFF, with a queue built from the list you
  started in, shuffle and repeat. The beat counter, BPM and hot cues come from the track's
  rekordbox analysis, and `c` jumps between cue points. Tracks follow each other without a gap:
  half a minute before one ends, the next is opened and queued behind it, so a continuous mix
  or a live album plays straight through (a next track at another sample rate starts the
  moment the playing one ends, without the wait for its file).
- **Play modes and output**: `S` switches to single play, as on a CDJ: the player stops at the
  end of each track with the next one ready for `space` (`S` again plays on). `A` turns on auto
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
  while playing, cue, hot cue and beat jumps land as far before or after the beat as the
  playhead was, as if they had waited for the nearest beat, so the rhythm carries on; paused,
  beat jumps land on a beat. A track without a beat grid jumps by its BPM.
- **Loops**: `l` loops the saved loop the playhead is in, or makes one of 4 beats from the beat
  it's in (from the playhead with quantize off); `l` again exits it and playback carries on.
  `(` and `)` halve and double it along the grid, and a hot cue saved as a loop plays its loop.
  Seeking out of a loop leaves it. The loop repeats on the audio thread, to the sample: the
  first time round it's kept in memory and then replayed from there, so going round doesn't
  read the drive. The player shows `LOOP` with its length in beats, and a loop made in the
  player is drawn like a saved one.
- **Visualisers** (`v`): a log-frequency spectrum with peak caps, a mirrored spectrum, an
  oscilloscope, and *deck*, rekordbox's scrolling colour waveform with beat ticks. The whole-track
  overview waveform sits under the player; click it to seek. `f` goes full screen and shows every
  visualiser at once: the deck across the top, spectrum and scope side by side, the mirrored
  spectrum beside the track details, and the cue lane along the bottom. Wide terminals also get a
  details column: tracks on the drive that **mix into** the playing one (compatible key on the
  Camelot wheel, tempo within 6% or half/double time), the playlists it's in, and a summary of its
  analysis.
- **Phrases**: rekordbox's phrase analysis (Intro, Up, Down, Chorus, Verse, Bridge, Outro) shows
  as the current phrase and bar in the player, with a countdown to the next one, and as a strip
  under the full-screen overview. Keys show in both Camelot and note names, whichever rekordbox
  stored.
- **Cues** everywhere: under the spectrum and scope, a lane of the 32 seconds around the playhead shows
  hot cues as lettered chips, memory cues as diamonds and loops as bars, with their names and loop
  lengths in beats (`[` `]` zoom it). Memory cues and loops take the colours they were given in
  rekordbox, and loop lengths are the ones rekordbox stored (½ beat, 4 beats), not worked out
  from the tempo. The deck draws cue lines through the waveform and shades loops. The
  visualiser flashes in a cue's colour as the playhead crosses it, and the player shows the next
  cue with a countdown.
- **Changing the drive** (opt-in): `e`, or right-click, opens actions for the marked or selected
  tracks, the playlist shown and the crate: set the rating or colour, give them a My Tag, add them
  to a playlist (picked in the tree), remove them from the playlist, create, rename or delete a
  playlist or folder, sort a playlist on the drive the way the list is sorted, save the crate,
  bring a playlist back from a snapshot, or replay recorded edits. `T` gives the marked or
  selected tracks a My Tag (one the drive has, or a new `Category/Tag`), or takes off one they all
  have. Every change first shows its plan and is written only when you press `y`, the same change
  the [commands](#editing-playlists-and-tracks) make, with a backup first; `u` takes the last one
  back, after showing what that puts back (the playlists and tracks, the files, the recorded edits
  that come off the journal).
- **After a rekordbox export**: when the drive's databases were written after locallibrary last
  wrote to it, and replaying the [edits recorded on it](#when-rekordbox-exports-again) would change
  something, a line above the footer says "rekordbox exported over 3 edits on Main. Replay?":
  `y` shows the replay to confirm, `esc` puts the offer away for the run.
- **Rule-based playlists**: in the palette, "smart new" makes a [rule](#rule-based-playlists) from
  the search box's filter and the list's sort (saved in the store, then shown filling its playlist
  on the open drive), "smart apply" fills every rule on the drive, and "smart" lists them.
- **Drives** (`2`): identity, library summary, space, store status, role, logbook (writes, clean
  ejects, the last verify and file-system check, problems), copies, My Settings files and
  interrupted operations for each drive, and the work: `e` lists what can be done to the selected
  drive, each planned in the background (with progress in the header; `esc` stops it) and
  confirmed like an edit. **Pull** saves it in the store. **Check** runs the checklist of
  `locallibrary check` against the player models remembered for it, and **set prep**
  checks it with the other mounted drives of its library, with the results in a panel. **Sync**
  copies the newest drive of its group over the others plugged in, or says why none is newest.
  **Pack** puts the marked playlists (`m` in the tree), the playlist shown or the crate (saved as
  a playlist first) from the drive open in the library onto it, for a set. **Restore** brings
  back a playlist or folder from a snapshot picked from its library's. **Replay** makes recorded
  edits again, **undo** takes its last change back (`u` undoes the last change made, on the drive
  the panel names), and **eject** unmounts it and powers it off (playback from it stops first).
  **Snapshots** (`3`): the store's snapshots; `enter` brings a playlist back from the selected one
  onto the drive open in the library. **Profile** (`4`): your [DJ profile](#dj-profile), section
  by section, with what each would reveal. `i` puts a section in the public document or takes it
  out, `m`/`h`/`H` pin and hide tracks and artists, `D` shows the document and `w` writes it. The
  bar at the top shows your [Atmosphere account](#atmosphere-account): `L` signs in (type your
  handle, approve in the browser), and signed in, `L` twice signs out.
- **Export**: in the palette, "export" writes the playlist, history list or all tracks shown as
  M3U8, CSV or text, and "xml" writes rekordbox XML of the drive (just the marked playlists when
  some are), asking where first.
- **Mixes** (`5`): a recorded set and the tracks it plays, like [`locallibrary mix`](#mixes)
  against the drive open in the library. Pick one of the drive's recordings (XDJ-AZ and OPUS-QUAD
  MASTER REC files), or `O` to type the path of a recording or a folder of them (opened ones are
  remembered). The view draws the set's 3-band waveform with each track's lane under it (shaded by
  how clearly it's heard, so blends and EQ swaps show), the tempo played, the keys on the way, and
  the tracklist with every handoff: blend, cut or gap, key move and tempo change. The selected
  track's own waveform, cues and phrases show the stretch that was played. `enter` or `space`
  plays the set in the player, and the timeline and tracklist follow along; `n`/`p` jump between
  its tracks, a click on the timeline seeks, `z` zooms. `o` goes to the selected track in the
  library, `E` exports the set as data for [Local Network](#mixes) and `w` writes the HTML report
  and opens it.

Colours are made for a dark terminal. For a light one, set `LOCALLIBRARY_THEME=light`; terminals
that set `COLORFGBG` (rxvt, Konsole and others) are recognised without it, and
`LOCALLIBRARY_THEME=dark` overrides them. With [`NO_COLOR`](https://no-color.org) set, the player
draws no colours: the selection is in reverse video, the focused pane has a heavy border, keys
that mix are marked `⇄` and underlined, track colours show as their numbers (1 pink to 8 purple)
and cover art is left out. `LOCALLIBRARY_REDUCED_MOTION=1` redraws only a few times a second and
stops the spinning record, the pulsing logo and the cue flashes.

Cover art is drawn in half blocks (24-bit colour where the terminal supports it, the nearest of
256 colours otherwise). The player remembers the drive, list, view, sort, volume, visualiser,
panels and columns, shuffle, repeat and quantize, the output, the crate, and the queue with how
far into its track it got (`space` carries on from there). It saves them as they change, in the
store once there is one and in a `locallibrary-player` folder beside it until then. Messages show
in the footer: errors stay until `esc`, and `!` lists every message since starting. The footer
hints at the keys for where focus is, as many as fit. `?` lists every key, the view's own first;
all of them come from one table, so a key means one thing in every view (`l` loops, `d` picks the
output, `o` goes to a track, `w` writes a file, `e` opens the Library and Drives actions):

| Keys | |
| --- | --- |
| `enter` | play the selected track (queues the list it's in); open a playlist or folder |
| `space` | pause / resume |
| `←` `→` | seek 5 s (shift: 30 s) |
| `n` `p` | next / previous track |
| `c` `C` | next / previous cue |
| `1` … `8` | hot cue A … H, in full screen or with the pads on |
| `'` | pads on / off |
| `,` `.` | beat jump back / forward (`<` `>`: a bar, `{` `}`: 8 bars) |
| `Q` | quantize on / off |
| `l` | loop here / exit the loop (`(` `)`: halve / double it) |
| `+` `-` | volume |
| `d` | the output to play through |
| `:` | commands: every action by name, and those with no key |
| `s` `r` | shuffle / repeat (off, all, one) |
| `S` | single play (stop after each track) / continuous |
| `A` | auto cue: start tracks at their first memory cue or beat |
| `x` | stop |
| `o` | go to the playing track |
| `tab` | next pane: drives, playlists, tracks, open panels |
| `↑` `↓` `j` `k` | move (`pgup` / `pgdn`, `g` / `G`) |
| `/` | search or filter; in the box, `tab` switches between the list and the whole drive |
| `b` `B` | sort by the next column / the other way (or click a column header) |
| `*` `#` | show / hide the rating column / the My Tags column |
| `i` | details of the selected track |
| `t` | show / hide the crate |
| `m` `insert` | mark / unmark a track (in the tree: a playlist or folder, to pack) |
| `M` | mark everything shown / clear the marks |
| `a` | add the marked (or selected) tracks to the crate |
| crate: `shift ↑↓` (`K` `J`), `del`, `w`, `X` | move a track, remove it, save the crate as a playlist, clear it |
| `e`, right-click | actions that change the drive (each asks first); in Drives, what can be done to the drive |
| `T` | give the marked (or selected) tracks a My Tag, or take one off |
| `u` | undo the last change, on the drive it was made to |
| `y` | replay the edits rekordbox exported over, while it offers to |
| `del` | remove from the playlist shown / delete the selected playlist |
| `esc` | stop a change before it writes; hide an error; clear the search, then the marks (press twice) |
| `v` | spectrum · mirror · scope · deck |
| `[` `]` | zoom the deck and cue lane out / in |
| `f` | full screen |
| `1` … `5` | library · drives · snapshots · profile · mixes (while the pads are off) |
| Profile: `i` `m` `h` `H` `D` `w` `a` `#` `L` | in/out of the document, pin, hide a track / artist, the document, write it, scope, rounding, sign in/out |
| Mixes: `O` `z` `o` `E` `w` | open a recording by path, zoom, the track in the library, export for Local Network, write the report |
| `R` `F5` | read the drive again |
| `!` | messages since starting |
| `?` `q` | help, quit (a write to the drive finishes first, also when the terminal closes) |

Mouse: click to select, double-click to play, click a column header to sort, click or drag the
waveform to seek, right-click for actions.

## The store

`pull` saves a drive's library in the store as a snapshot: every file, kept once however many
snapshots and drives share it. Snapshots belong to a library (one per group of drives that hold
the same music); a drive pulls into its own library unless `--library` names another.

```bash
locallibrary pull Main --label "before NYE"    # name a snapshot you may want back
locallibrary library log                       # every library's snapshots, labels shown
locallibrary library push Spare --snapshot "before NYE"
locallibrary library ls                        # libraries, their latest snapshot and drives
locallibrary library rename Main "Home"
locallibrary store label 3f9a2c "Friday warmup"
locallibrary store unlabel "Friday warmup"
```

A label works wherever a snapshot id does (`library push --snapshot`, `drive restore --snapshot`,
`diff snapshot:...`) and is unique in the store, ignoring case. `pull --library NAME` only pulls
into a library that exists: a name the store doesn't know is asked about in a terminal and refused
otherwise (also with `--yes`), so a typo doesn't start a new library; `--new-library` starts one.

### Saving drives as they're plugged in

rekordbox deletes the history lists players recorded on a drive when it imports them, so a pulled
snapshot is the only copy that lasts. `watch` pulls drives for you as they're plugged in:

```bash
locallibrary watch                    # say which drives hold changes the store lacks
locallibrary watch --pull             # and pull them
locallibrary watch --pull --sync-group  # then sync the rest of the drive's group from it
locallibrary watch --pull --once      # look at what's plugged in now, then stop
```

It runs in the foreground (Ctrl-C stops it) and logs a line for each drive plugged in or
unplugged and for what it did (`--json`: one JSON object per line). A drive is looked at when it's
plugged in, when it's there as `watch` starts, and when its databases change (rekordbox
exporting to it), once they have gone unchanged for 5 seconds (`--settle SECS`). Drives ready
at the same time are saved as `set` saves them: the newest export first, then drives back from a
set beside it.

- Only drives of a library the store has are pulled: a drive that was never pulled, or whose
  library isn't in the store, is left alone and never starts a library (pull it once yourself).
  Archive and retired drives are left alone too.
- A drive is pulled when it holds a newer state of its library than the newest snapshot (new history
  lists, an export). One that came back from a set after the main drive's newer export was pulled,
  with only player changes since (history lists, cues), is saved beside the newest snapshot, which
  stays the newest (`library push` still writes the newer export; `plays lists --all` lists its
  history lists, `library log` doesn't show it). One with other changes since is left alone: merge
  or sync it yourself.
- With `--sync-group`, after a drive is pulled (or found saved), the other mounted drives of its
  group (`drive groups`) are synced from it when it's the newest of them with confidence, as `sync`
  with no drives does. When another drive is newer, or which is newer isn't certain, nothing is
  synced and the log says why.

To run it whenever you log in, on macOS save this as
`~/Library/LaunchAgents/local.locallibrary.watch.plist` (with your home folder and the path
`which locallibrary` prints), then `launchctl bootstrap gui/$(id -u)
~/Library/LaunchAgents/local.locallibrary.watch.plist` (`launchctl bootout
gui/$(id -u)/local.locallibrary.watch` stops it):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>local.locallibrary.watch</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/you/.cargo/bin/locallibrary</string>
    <string>watch</string>
    <string>--pull</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>/Users/you/Library/Logs/locallibrary-watch.log</string>
  <key>StandardErrorPath</key>
  <string>/Users/you/Library/Logs/locallibrary-watch.log</string>
</dict>
</plist>
```

macOS may ask to let `locallibrary` read removable volumes; if drives are never looked at, check
System Settings > Privacy & Security > Files and Folders. On Linux, save this as
`~/.config/systemd/user/locallibrary-watch.service`, then `systemctl --user enable --now
locallibrary-watch` (`journalctl --user -u locallibrary-watch` shows the log):

```ini
[Unit]
Description=Save rekordbox drives into the locallibrary store as they're plugged in

[Service]
ExecStart=%h/.cargo/bin/locallibrary watch --pull
Restart=on-failure

[Install]
WantedBy=default.target
```

On Windows, a Task Scheduler task that runs `locallibrary watch --pull` at log on does the same.
Stopping it in the middle of a pull leaves no snapshot half made: a snapshot is recorded once all
its files are in the store.

### Pruning old snapshots

Every pull adds a snapshot, so the store only grows. `store prune` removes the old ones and then
frees the files nothing else uses (as `store gc` does; `gc` never removes a snapshot of a library,
saved beside its latest or not):

```bash
locallibrary store prune --keep-last 10 --keep-daily 7 --keep-weekly 8 --dry-run
locallibrary store prune --keep-last 20 --library Main
```

`--keep-last N` keeps each library's newest N snapshots; `--keep-daily N` and `--keep-weekly N`
keep the newest snapshot of each of the last N days or weeks that have one (in local time). Some
snapshots are always kept:

- each library's latest, and labelled ones (take a label off with `store unlabel` to let it go);
- the state each drive was last seen holding (retired drives aside): `library push` relies on it to
  know a drive's changes are saved, `drive ls --known` counts how far behind a drive is from it, and
  a drive back from a set is told apart from a new export by it;
- the newest snapshot holding each history list, so `plays lists --all` and the DJ profile keep
  every set the store has seen; a snapshot whose databases can't be read is kept too.

Which of two drives is newer comes from the lineage each drive keeps, so pruning doesn't change it.
The plan is shown first, and exactly that is removed (a pull or label in between makes it plan
again); history still runs through what was removed (`library log` skips it), and a removed
snapshot's id says when it was pruned.

### Off-site copies

```bash
locallibrary store copy /Volumes/NAS/locallibrary        # the first copy takes a while; later ones copy what's new
locallibrary store copy ~/Dropbox/locallibrary --verify  # also re-read what the copy has
locallibrary store copy /Volumes/NAS/locallibrary --mirror   # and remove what the store no longer has
locallibrary store copies                                # where copies were made, when, and whether they're current
locallibrary --store /Volumes/NAS/locallibrary library log   # a copy reads like the store
```

A copy has the store's layout, so `--store DIR` (or `LOCALLIBRARY_STORE`) works with it as it
does with the store: after losing this computer, `library push` a snapshot from it onto a new drive.
Stored files are named by their content, so only ones the copy lacks are copied, each checked
against its name as it's copied. Snapshots are written once all their files are there, and the
list of each library's latest snapshot last, so a copy that's stopped (or loses its network
share) still holds the store as it was at the last copy that finished; run `store copy` again to
carry on. The copy's folder is made, but not the folders above it, so an unmounted share isn't
mistaken for an empty folder.

A copy keeps snapshots the store has since pruned unless `--mirror`. It takes the store's
libraries, labels, drive registry, settings profile, rule-based playlists, remembered player
models and profile preferences, but not your Atmosphere sign-in. A snapshot whose files are damaged in the store is
left out and reported, and `store copy` exits with
status 5 (`store verify` says more). A copy that was
written to since the last copy (pulled into, labelled or given player models with `--store`)
isn't copied over unless `--force`.

## Settings

`locallibrary settings schema` lists all 46 settings with their values. They fall into three groups:
player (quantize, sync, auto cue, jog mode, tempo range, hot cue auto load, beat jump, vinyl
speed adjust...), display (language, LCD/jog/pad brightness, time mode, waveform colour and
position, key format...), and mixer (channel and cross fader curves, headphones, talk over, MIDI,
beat FX quantize...).

```bash
locallibrary settings import                        # start from rekordbox's own My Settings on this computer
locallibrary settings import Main                   # ...or from a drive
locallibrary settings set quantize=off jog_mode=cdj crossfader_curve=constant-power
locallibrary settings set --drive "Tokyo drive" language=japanese
locallibrary settings show "Tokyo drive"            # current values and drift from the profile
locallibrary settings apply                         # every mounted drive but archive and retired ones
locallibrary settings publish                       # put the profile on your Atmosphere account...
locallibrary settings import --from-account         # ...and bring it back on another computer
locallibrary settings import --from-handle dj.example.com   # or start from someone else's
```

`settings publish` writes the profile's base (not the per-drive overrides) to your
[Atmosphere account](#publishing), where any computer can import it without signing in.
Importing sets the values it brings and keeps the rest of the base, and the per-drive overrides;
with `--from-handle` or `--from-account`, `--replace` makes the published settings the whole base.

`apply`, `import` and `publish` show what they'd change and ask first (`--dry-run` stops there).
`apply` backs each drive's settings files up before writing them, so `locallibrary undo` takes
them back.
The profile only enforces the keys it contains, so a partial profile leaves everything else
alone. `apply` patches individual bytes in the existing `*SETTING.DAT` files and recomputes their
checksums, so bytes this tool doesn't understand (including anything newer firmware adds) are
preserved. Library sync never touches settings files, so per-drive overrides survive a sync.
`settings show` lists such bytes: rekordbox 7.2 writes one more display setting
(`DEVSETTING.DAT` data byte 15) and five more mixer settings (`DJMMYSETTING.DAT` bytes 25-29)
than rekordbox 6. Profiles don't manage them until their meaning is known.

## Clean

```bash
locallibrary clean Main --dry-run      # what would go
locallibrary clean Main
```

rekordbox's collection ships with demo samples ("rekordbox - House 1", "Techno 1", "Breaks 1"...),
and they end up on drives, often once per export (`House1.wav`, `House1-1.wav`, `House1-2.wav`...).
`clean` removes every track on its list from `export.pdb` and OneLibrary's `exportLibrary.db`,
along with its playlist and history entries, audio, analysis files and artwork, plus artists,
albums and genres nothing else uses. Files another track (in either database) still points at
are kept. The list lives in
[`clean.rs`](crates/locallibrary/src/clean.rs) (`USELESS_TRACKS`).

## After rekordbox exports

```bash
locallibrary refresh                     # the drive exported over your edits, or your primary
locallibrary refresh Main --dry-run      # the plan only
locallibrary refresh Main --sync         # then copy it onto its library's other mounted drives
locallibrary refresh Main --skip clean,settings
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
one. A step planned on the drive as it was is planned again on the drive as the step before left
it (refused if another drive is mounted there by then). The first step that fails stops the rest:
what was done stays done, and the exit status is 5 (1 when nothing was done). `--skip STEP`
(repeatable, or comma-separated) leaves steps out.

`status` flags a drive rekordbox exported over recorded edits ("rekordbox exported over 3
recorded edit(s): run `locallibrary refresh "Main"`"): its databases were written after its last
recorded edit, and making the edits again would change something. With no drive, `refresh` takes
the one mounted drive flagged so, else the only mounted drive or the primary. My Tag and cue edits
count once the drive was exported to after them, since telling whether they're in place means
making them.

## Editing playlists and tracks

```bash
locallibrary playlist new Main "Sets/2026/Friday" --filter "bpm:122-128 key:8A~"   # folders are created as needed
locallibrary playlist add Main "Sets/2026/Friday" --query "floorplan" --position 1
locallibrary playlist remove Main "Sets/2026/Friday" --at 3,4      # take tracks out of it
locallibrary playlist reorder Main "Sets/2026/Friday" --at 7,8 --to 1
locallibrary playlist sort Main "Sets/2026/Friday" --sort key,bpm
locallibrary playlist order Main "Sets/2026/Friday" --into "Sets/2026/Friday (mix order)"
locallibrary playlist move Main "Sets/2026/Friday" --into Archive --position 1
locallibrary playlist rename Main "Archive/Friday" "Friday, 10 October"
locallibrary playlist delete Main "Sam" --with-tracks   # a guest's merged crate, and the tracks only it used
locallibrary playlist unfiled Main                   # tracks in no playlist, so players can find them
locallibrary playlist import Main spotify.csv --into "From Spotify"
locallibrary track set Main --filter "bpm:>=128 genre:techno" --rating 5 --colour red
locallibrary track rm Main --filter "is:unfiled plays:0" --dry-run
locallibrary track dedupe Main                       # replace Sync Manager copies (--hash-audio: identical audio too)
locallibrary track replace Main 812 77               # duplicate 812 -> keep 77, everywhere
locallibrary track show Main --query "daft punk - one more time"   # details, playlists, tags, copies, analysis
locallibrary track matches Main 77                   # what mixes into track 77
```

Edits change the drive's databases in place, `export.pdb` and OneLibrary's `exportLibrary.db`
alike, so every player sees them. Each shows its plan first, is backed up (`locallibrary undo`
takes it back), and is checked by reading the edited databases back before they replace the old ones.
A name, or a track's title, comment and mix name together, too long for one `export.pdb` row
(about 4,000 characters, half that outside ASCII) is refused in the plan. A playlist or track one
database lacks (they can drift apart) is skipped there with a warning, and My Tags in
`exportExt.pdb` follow tracks that are removed or replaced. Of two playlists with one name in a
folder (rekordbox allows that), edits change the one shown first. A playlist and a folder can
share a path too: the path names the playlist, and the path with a `/` after it (`Sets/`) the
folder.

`playlist order` orders a playlist for mixing: compatible keys on the Camelot wheel (with
energy-boost moves), close tempos (half and double time count), and energy from rekordbox's
phrase analysis that holds or builds. Over 2,000 tracks it skips the last tidying pass; Ctrl-C
stops it. `track replace` moves a duplicate's playlist and history
entries and My Tags to the copy you keep, then deletes the duplicate and its files.

`playlist import` reads M3U/M3U8, CSV (Spotify via Exportify, TIDAL, Beatport, TuneMyMusic,
Soundiiz exports and any CSV with title and artist columns), rekordbox's playlist text export,
rekordbox XML (`--from` picks one of its playlists) and plain "Artist - Title" lists. Tracks are
matched by file path, then ISRC, artist, title and length, then fuzzily; entries that fit several
tracks are listed instead of guessed (settle them with `--choose ENTRY=TRACK`). It works offline:
export a streaming playlist to CSV first.

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
locallibrary edits list Main       # every edit made to the drive (.locallibrary/edits.json)
locallibrary edits replay Main     # make them again after an export; edits that no longer apply are skipped
locallibrary xml Main --only "Sets/2026" -o sets.xml   # or take playlists back into rekordbox
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
entries the new export added. A sync, push or pack that gives a drive a whole library forgets the
edits recorded for the one it held (its plan says so). `edits.json` says which version of its
format it is: a locallibrary older than the one that wrote it refuses to read it or add to it
(and says so) rather than misreading it. Builds from before it had a version read it as a plain
list: they change the drive, then fail to record the edit. Update every copy (the CLI and the
app's sidecar) before editing a drive this version has edited.

### Bringing back a playlist

```bash
locallibrary drive restore Main --playlist "Sets/2026/Friday"              # from the newest snapshot that has it
locallibrary drive restore Main --playlist "Sets/2026" --snapshot 3f9a2c   # a whole folder, from that snapshot
locallibrary drive restore Main --playlist "Friday" --backup 20261004T165600.123Z --dry-run
```

`drive restore --playlist` takes one playlist, or a folder and everything in it, as a stored
snapshot (`pull` keeps them) or one of the drive's backups has it, without touching the rest of the
library. Without `--snapshot` or `--backup` it uses the newest snapshot of the drive's library that
has the playlist. It's an ordinary playlist edit: shown first, backed up, recorded for `edits
replay`, and taken back by `undo`. Missing playlists and folders are made again next to their old
neighbours; existing ones get the old entries back, in their old order. Entries whose track isn't on
the drive any more are listed and left out, and playlists you've added to the folder since are left
as they are.

### Rule-based playlists

```bash
locallibrary smart add "Smart/Peak time" --filter "bpm:126-132 rating:>=4" --sort -rating --limit 60
locallibrary smart add "Smart/Tokyo" --filter "tag:Situation/Tokyo" --drive "Tokyo drive"
locallibrary smart apply             # every mounted drive but archive and retired ones
```

The rules live in the store. Drives get ordinary playlists, since players can't evaluate rules
(rekordbox exports its Intelligent Playlists the same way). Run `smart apply` again after an
export or a merge to refresh them. A rule fills the playlist at exactly its path, never one of its
name in another folder, so rules for `Smart/Peak` and `Smart/PEAK` are two rules filling two
playlists; a path several playlists match ignoring case is refused, naming them. `playlist fill`,
`playlist unfiled` and `playlist import --into` find their playlist the same way.

### Drives for a set

```bash
locallibrary pack Main /Volumes/TONIGHT --playlist "Sets/2026/Friday" --playlist Classics
locallibrary pack Main /Volumes/TONIGHT           # later: the same selection, copying only what changed
```

`pack` writes only the chosen playlists and folders, the folders above them and the tracks they
use, with their analysis and artwork, to another drive, and removes files the packed library
doesn't use from it (`--keep-extra` keeps them). The databases are cut down from the source's
own, so cues, grids, My Tags and details come across unchanged; history lists stay behind unless
`--history`. A packed drive holds a different library from its source, so `sync` won't copy one
over the other, and `check` says whether it's up to date with the drive it was packed from.

A packed drive comes back from a set with history lists and memory cues the players saved on it.
Before packing it again, `pack` saves its state in the store when it holds history lists the new
pack won't have, or changed since it was last packed (it refuses when there's no store), and
backs up the databases, analysis and artwork it replaces on the drive, so `locallibrary undo`
puts them back. Audio it removed isn't kept (it's on the source): tracks it removed come back by
packing again with the old selection, or with `undo --force` without their audio. Packing forgets
the edits recorded on the drive. Like every command that writes, it won't write to an archive or
retired drive without `--overwrite-archive`.

## My Tags

```bash
locallibrary tags Main                                    # categories, tags, how many tracks carry each
locallibrary tags Main --tracks                           # ...and which tracks
locallibrary tags add Main "Genre/Deep House" --playlist "Sets/Saturday"   # or --track, --query, --album
locallibrary tags untag Main "Genre/Deep House" --track 12
locallibrary tags rename Main "Genre/Deep" "Deep House"   # a tag
locallibrary tags rename Main "Untitled Column" "Mood"    # a category
locallibrary tags delete Main "Situation/Morning"
```

My Tags are rekordbox's own track tags: four categories (Genre, Components, Situation and one you
name), each with up to 50 tags. Players filter by them (Track Filter) and use them for Related
Tracks. A drive holds them twice, in `exportExt.pdb` for players that read `export.pdb` and in
OneLibrary's `exportLibrary.db`. `tags` edits both, matching tracks by file path and creating a
tag in whichever database lacks it, and lists where the two disagree. Categories can be renamed,
not added or deleted, and a name too long for `exportExt.pdb` (about 4,000 characters, half that
outside ASCII) is refused. A drive without `exportExt.pdb` (older exports) only has its OneLibrary
tags edited. Tag edits are recorded in the drive's journal, so `edits replay` makes them again
after rekordbox exports to the drive.

## Cues

```bash
locallibrary cues Main --track 12                      # hot cues, memory cues and loops
locallibrary cues set Main --track 12 --hot B --at 1:02.5 --loop 8 --name Drop --colour red
locallibrary cues set Main --track 12 --memory --beat 129   # on the beat grid (--snap moves a time to the nearest beat)
locallibrary cues rename Main --track 12 --hot B "Big drop"
locallibrary cues colour Main --track 12 --memory-beat 129 aqua
locallibrary cues rm Main --track 12 --hot B           # or --memory-at 1:00, --all
locallibrary cues copy Main --from 12 --to 40          # e.g. onto a duplicate
```

Players read cues from a track's analysis files (`.DAT` for every player; `.EXT`, with names and
colours, for nexus 2 and later) and, on OneLibrary players, from `exportLibrary.db`. Every cue edit
updates all of them, keeping the bytes of cues it doesn't change and the ids hot cue bank lists
point at. Hot cues take rekordbox's 62-colour palette (red, orange, yellow, green, aqua, blue,
violet, pink... or `#rrggbb` for the nearest) and memory cues its 8 track colours. A loop given in
beats ends on the beat grid. Exporting the track from rekordbox again brings back your
collection's cues; cue edits are recorded in the drive's journal, so `edits replay` makes them
again.

## Doctor

`locallibrary doctor Main` reports problems and changes nothing (`--hash-audio` keeps the hashes
it makes in the drive's `.locallibrary/` file index): duplicates (several rows for one
file, Sync Manager's `Name-1.mp3` copies, the same artist/title/length, and with `--hash-audio`
identical audio) with a suggested keeper, missing or wrong-size audio, missing analysis and
artwork, orphaned files, tracks in no playlist, drift between the two databases, files over 4 GB,
and how players read the drive. `track dedupe` replaces the duplicates; `--fix-orphans` deletes
audio under `Contents/`, analysis under `PIONEER/USBANLZ/` and images under `PIONEER/Artwork/`
that no track in either database uses, after backing them up. Files other than audio under
`Contents/` are never touched. `--audio` adds audio files that don't open, are cut short or don't
decode at the end (see [Before a set](#before-a-set)).

## History and play counts

Players record a history list per session. rekordbox imports and deletes them on its next sync, but
every `locallibrary pull` keeps the drive's databases: `locallibrary plays lists Main --all` lists
every list the store ever saw (once, however many drives or snapshots held it), and `locallibrary
plays export Main "HISTORY 012" -o set.txt` writes one as `01. Artist - Title` (or CSV, M3U8).
`locallibrary plays lists Main --delete` clears the lists from the drive, like a player's ALL
DELETE, but only those the store has kept. `locallibrary plays publish Main "HISTORY 012" --title
"Friday warm-up"` publishes one as a set on your [Atmosphere account](#publishing). `locallibrary
plays stats Main --since 3f9a2c` (or `--since 2026-09`) shows the most and never played tracks and
what was played since a snapshot. `locallibrary export` writes playlists and folders the same way,
and `locallibrary diff Main Backup --playlist Sets/Friday` shows which entries were added, removed
or moved between two drives or snapshots.

## DJ profile

```bash
locallibrary profile                          # your mounted drives and the store
locallibrary profile Main --since 2026        # one drive; sets first seen this year
locallibrary profile -s tempo,mixing --sets   # just those sections, and every set
locallibrary profile document -o profile.json # what a public profile would show
```

`profile` works out how you play from what locallibrary already reads: the history lists players
record (on the drives, plus every list `pull` archived, so lists rekordbox has since deleted still
count), each track's analysis (BPM, key, phrase mood, cues) and tags, and My Settings. A history
list with fewer than 5 tracks (`--min-tracks`) counts as a sound check, not a set. A library
without sets is described from its collection, and the same recording on several drives or in
several files counts once. Without drives named, it reads the mounted drives the store knows (ones
you've pulled or synced), so a stick someone else plugged in doesn't count as your sets; a store
that knows none reads every mounted drive. Transitions are tracks played one after the other:
not two either side of a track since deleted, nor one track logged twice.

| Section | What it says |
| --- | --- |
| Summary | a headline ("Techno and House · 122–128 BPM · 48 sets") and the traits the numbers support, each with its evidence: mixes in key, smooth tempo changes, builds the tempo through a set, genre-hopping, plays new releases or older records, rarely repeats tracks, has signature tracks, cues up every track, plays with quantize off |
| Sets | how many, how long, when they were first archived |
| Tempo | the BPM range of the middle 80% of plays, and a histogram |
| Set shape | opening against closing tempo, and whether sets build, hold or wind down |
| Mixing | how many transitions are harmonic on the Camelot wheel (same key, ±1, relative, energy boost), tempo steps, genre changes |
| Keys, Energy, Genres, Labels, Artists, Era | what you play: the Camelot wheel, rekordbox's phrase mood, tags, release years |
| Signature tracks | tracks in two or more sets, moves played again and again, openers and closers |
| Prep, Collection, Player style | how many tracks have cues and loops, the collection's size, My Settings such as quantize and jog mode |

The report stays on this computer. `profile document` is the part meant for a public profile,
and `profile publish` puts it on your [Atmosphere account](#publishing): counts rounded down to
round numbers, shares to 5% (shares of one whole still add up to it), BPM to whole numbers, and
no paths, drive or history-list names, ratings or exact dates. Sections that name tracks, artists
or labels, or describe the collection itself, stay out until you include them, and the headline
and traits only say what the included sections say. Your choices are kept in the store
(`profile.json`):

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

A profile can't compare you with other DJs (which tracks only you play): that needs their data.

## Atmosphere account

```bash
locallibrary account login dj.example.com    # sign in in the browser
locallibrary account                         # who's signed in, and until when
locallibrary account status --check          # ask the account's server (refreshes the session)
locallibrary account logout                  # revoke the session and remove it from this computer
```

Your Atmosphere account is the one you use for Bluesky and other AT Protocol apps, hosted by
Bluesky or any other server. Signing in happens in your browser, on your account's server:
locallibrary never sees your password. Give your handle, a DID, or your server
(`locallibrary account login https://bsky.social`, which also lets you pick an account or make
one). Signing in only asks to know who you are: locallibrary can't post or change anything.
[Publishing](#publishing) asks for more the first time, by signing in again: permission to create
and replace locallibrary's own records
(`repo:computer.localnetwork.locallibrary.settings?action=create&action=update`, and the same for
`.profile` and `.set`), not to delete them, and nothing else in the account. `account login
--publish` asks for it up front.

- **The browser comes back to locallibrary** on a one-off `127.0.0.1` port, and only the address
  that carries this sign-in's `state` counts. When the browser runs on another computer (over
  SSH), `--paste` takes the address it ends up on instead; `--no-browser` only prints the link.
  Only `https://` sign-in pages are opened.
- **The session is kept in the system keychain** (macOS Keychain, Windows Credential Manager, the
  Secret Service on Linux), one item per store folder: the key every token is tied to and the
  refresh token. The rest is in `atmosphere.json` in the store folder. Without a keychain,
  `--session-file` (or `LOCALLIBRARY_SESSION_STORE=file`, which the player and the sidecar go by)
  keeps it all in files only you can read. Signing out revokes both tokens at the server and
  removes them here; if the keychain doesn't answer, it says what's left and the next
  `account logout` tries again.
- **Servers found from a handle or DID are only reached at public `https://` addresses**, never
  an address on this computer or its local network, and redirects too (a name that leads there
  fails the certificate check before any request is sent); certificates are checked against the
  system's trusted roots as well as the bundled ones.
- **Sessions last up to two weeks** on Bluesky's servers, however often they're refreshed; then
  sign in again. `account` shows the date. One account is signed in at a time.
- **Servers know locallibrary as `locallibrary.localnetwork.computer`**: they fetch its client
  document from `https://locallibrary.localnetwork.computer/oauth-client-metadata.json`, show
  that name when you approve, and remember the approval. The Local Network web app (a separate
  project) serves that host: the document (in its `src/lib/server/locallibrary/`; `locallibrary
  account client-metadata -o …` regenerates it) and a short page about locallibrary. Until it's
  online, sign-in says so and falls back to the `http://localhost` development client, which
  servers show as "a local app" and ask about every time (`account` says when a session is the
  development client's). The document lists every scope a sign-in may ask for, publishing's
  included, and servers refuse a sign-in that asks for more: a release that asks for new scopes
  needs the hosted document updated first. Until it allows them, sign-in stops and says so.
  `--dev-client` chooses that client on purpose; `--client-id` (or
  `LOCALLIBRARY_OAUTH_CLIENT_ID`) another document.

The OAuth itself (finding your server, the authorization request, PKCE, DPoP-bound tokens, the
checks on the server's answer, refresh and revocation) is the
[jacquard-oauth](https://crates.io/crates/jacquard-oauth) client's.

### Publishing

```bash
locallibrary settings publish                 # your My Settings
locallibrary profile publish --dry-run        # your DJ profile: look first
locallibrary plays publish Main "HISTORY 012" --title "Friday warm-up" --date 2026-10-03
```

Publishing writes a record to your account's repository, where anyone can read it and other
apps can show it. Each command shows exactly what it will publish (`--dry-run` stops there,
and works signed out; `--json` prints the record) and asks first, then publishes exactly that,
to the account it named: if the record was published from somewhere else in between, or
another account signed in, nothing is replaced. Like every write, it exits with status 3 when
it wasn't confirmed (see `locallibrary --help`). The first time, it asks you to sign in again to
allow publishing (`--paste`, `--no-browser` and `--timeout` work as for `account login`), then
shows the plan again for the account you approved. Publishing again replaces the record.

| Command | Record | Holds |
| --- | --- | --- |
| `settings publish` | `computer.localnetwork.locallibrary.settings`, key `self` | the settings profile's base values |
| `profile publish` | `computer.localnetwork.locallibrary.profile`, key `self` | the [profile document](#dj-profile), in whole numbers: shares as percents, BPM whole |
| `plays publish <drive> <list>` | `computer.localnetwork.locallibrary.set`, a key per set | the day it was played, an optional `--title` and `--venue`, and the tracks in order: artist, title, ISRC, BPM, Camelot key |

A set leaves out the history list's name and the files' paths. Tracks your profile hides
(`profile hide`) and the positions you give with `--hide 3,7` are published as unidentified:
only their place in the set. Without `--date`, a set is dated as it was published before, else
the day the store first saw the list, else today, and the preview says which. `--all` also looks
among the lists the store archived after rekordbox took them off the drive. Publishing the same
list again replaces its set.

The record types are Lexicons in [`lexicons/`](lexicons/computer/localnetwork/locallibrary/),
and each record is checked against its Lexicon before it's written. Everything published is
public and copied around the network: deleting a record later doesn't delete the copies.

## Before a set

```bash
locallibrary check Main Backup --players CDJ-3000X,CDJ-2000NXS2
locallibrary players set CDJ-3000X,CDJ-2000NXS2       # remember them: `check Main Backup` uses them from now on
locallibrary players set OPUS-QUAD --library Home     # ...or for one library's drives
locallibrary check Main --verbose                     # list the tracks a player won't load
locallibrary check Main Backup --audio                # also decode every file's end (slow; finds cut-short copies)
locallibrary players models [CDJ-3000]                # what each model reads, and where that comes from
```

One checklist over every drive you're taking: they hold the same library, `export.pdb` and
OneLibrary agree, no track's audio, analysis or artwork is missing, nothing is too big for a FAT32
drive in the set, no operation was interrupted, and their history lists are in the store.

`players set` and `players clear` show what they'd change and ask first (`--dry-run` stops there).
For each player model (`--players`, or else the models `players set` remembered for the drive's
library, or for every library) it checks:

- **The database it reads is there.** The CDJ-3000X, CDJ-1500X, OPUS-QUAD, OMNIS-DUO, XDJ-AZ and
  XDJ-AN read only OneLibrary; every other model reads only `export.pdb`. Without any models, a
  drive with only one of the two is a warning, since some players can't read it.
- **The partition table and file system:** MBR, and exFAT only where the model reads it (some
  need a firmware update, which `check` mentions).
- **Every track's format, sample rate and bit depth:** "12 FLAC files at 96 kHz won't load on the
  OMNIS-DUO". They come from the database rows; a file's header is read only when its row doesn't
  say (or for an `.m4a` whose bit rate doesn't tell AAC from Apple Lossless). `--verbose` lists the
  tracks, and `--json` gives them to scripts (`unloadable`, grouped by player and reason).

What each model reads comes from its instruction manual (`players models MODEL` names the page)
and AlphaTheta's notice on which units read OneLibrary (March 2026). Players differ more than you'd
think: the OMNIS-DUO, XDJ-AN, CDJ-1500X, XDJ-XZ and XDJ-RX3 play lossless files at 44.1 and
48 kHz only, the CDJ-3000 generation takes MP3 and AAC at 44.1 and 48 kHz only, and the XDJ-XZ and
XDJ-RX3 manuals list no Apple Lossless. A few older models (CDJ-2000, XDJ-RX2, XDJ-RR) and one
value of the XDJ-RX3 are inferred rather than documented; `check` says "probably" for those, as a
warning. Anything that won't load is a problem.

`check --audio` (and `doctor --audio`) also opens every audio file and decodes its last seconds,
on every core, with progress: a file that doesn't open, stops short of the length the database
recorded (a copy interrupted part way), or doesn't decode at the end is a problem; one whose own
header gives another length is a warning (replaced since the export?). It reads a little of every
file, so it takes a while on a big drive.

`check` exits with status 4 when it finds a problem, so a script can stop there; `doctor` does
too.

## The night before a set

```bash
locallibrary set                                   # the mounted drives of your library
locallibrary set Main "Club drive" --players CDJ-3000X
locallibrary set --no-eject --dry-run
```

`set` gets the drives you're taking ready in one plan:

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

## Mixes

```bash
locallibrary mix "Live at the Warehouse.wav" --drive Main --open
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
Main` on its own just does this). A file that doesn't decode is reported and remembered, so it
isn't tried again until it changes. Tracks over 20 minutes (recorded sets, not tracks) are left
out, and so is the recording itself when it's on the drive too. The recording is fingerprinted as
it decodes, and every speed within `--tempo-range` (±16% by default) is tried both with and
without key lock. Pioneer recorders' files (`REC046`, recorded on an XDJ-RX2) are recognised as
such, and the title falls back to the folder they were filed in.

Duplicates on the drive and remixes that share a vocal are listed under the track they match
rather than as tracks of their own. Matches under 50% confidence are marked tentative, and
stretches nothing matches (a track that isn't on the drive) are marked as such. Things that hide
a track from fingerprints: heavy effects, very long filtered intros, and speeds outside the range
searched. Matching only looks at one drive's library at a time.

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

## rekordbox's collection

```bash
locallibrary collection                         # what rekordbox's collection holds
locallibrary collection tracks -f "bpm:>=128 tag:Situation/Peak" --sort -rating
locallibrary collection playlists
locallibrary collection diff Main               # what Main lacks against the collection
locallibrary collection --db ~/old/master.db info
```

rekordbox 6 and 7 keep the collection on the computer in `master.db`
(`~/Library/Pioneer/rekordbox/` on macOS, `%APPDATA%\Pioneer\rekordbox\` on Windows):
SQLite encrypted with SQLCipher 4 under a key every installation shares, which the
[pyrekordbox](https://github.com/dylanljones/pyrekordbox) project publishes. `collection` reads
it, never writes it: the database and its WAL are copied to a temporary folder, decrypted there
(the WAL's committed changes applied) and only that copy is opened, so rekordbox can stay open
and nothing of its is locked. A read while rekordbox saves can miss its latest change.

It reads tracks with their details, playlists and folders, history lists, hot cues, memory cues
and loops, and My Tags; `tracks` takes the same [filters](#filters) and sort orders as a
drive's. Intelligent playlists show empty: their rules aren't evaluated. `diff` finds the
collection's tracks on the drive by file name and the folders above it, then ISRC, then artist,
title and length, and shows (with `+`) the tracks, playlists and playlist entries the drive
lacks, (with `-`) what only the drive has, and the tracks whose details differ, such as a rating
changed since the last export. `--db` (or `LOCALLIBRARY_MASTERDB`) reads another `master.db`;
should rekordbox change its key, `LOCALLIBRARY_MASTERDB_KEY` gives the new one.

## Other DJ software

```bash
locallibrary export Main --format traktor -o ~/Desktop/for-traktor --dry-run   # what would be written
locallibrary export Main "Sets/2026" --format traktor -o ~/Desktop/for-traktor  # one folder and its tracks
locallibrary export Main --format traktor -o out --location-root ~/Music/USB-copy
locallibrary export Main --format serato -o ~/Desktop/for-serato                # crates
locallibrary export Main --format serato -o ~/Music/for-serato --write-tags     # and cues, in copies of the audio
locallibrary export Main --format engine -o ~/Desktop/for-engine                # a new Engine Library
locallibrary export Main --format engine -o out --paths drive                   # one to copy onto the drive
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
  artwork don't come across. Audio outside `/Volumes` is placed on a startup disk called
  "Macintosh HD"; Traktor's Relocate finds it if yours is called otherwise.
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
- **Engine DJ** (`Engine Library/Database2/m.db`): a new Engine Library (schema 3.0.2, as Engine
  DJ 4.5 and later write it; newer versions upgrade it) with the tracks and their details, and
  the playlist tree (folders become playlists holding playlists). Track paths are relative to
  the `Engine Library` folder where it's written, or with `--paths drive` to the drive's root:
  copy that `Engine Library` folder onto the drive, next to `Contents/`, and Engine OS players
  and Engine DJ read it from there. In Engine DJ Desktop, a library elsewhere is added as a
  source and its playlists dragged into the collection. Beat grids, cues and loops aren't
  written (Engine keeps them in its own encoded form), so Engine analyses the tracks when it
  first reads them; artwork, colours and history lists don't come across, and an Engine
  playlist holds a track once.

## Convert

```bash
locallibrary convert Main --to onelibrary            # give an older drive OneLibrary, for CDJ-3000X, OPUS-QUAD, XDJ-AZ...
locallibrary convert Main --to device-library        # give a OneLibrary-only drive export.pdb, for every other player
locallibrary drive info Main                         # "export.pdb and OneLibrary differ" when they drift apart...
locallibrary convert Main --to onelibrary --replace  # ...rebuild OneLibrary from export.pdb
```

Tracks, artwork (copied to the other database's `a`/`b` file names), playlists and history
playlists are converted; audio and analysis files are shared, and OneLibrary's `cue` rows
(players that read only OneLibrary use them) are made from the analysis files. Rebuilding
OneLibrary keeps the rows it already has for tracks `export.pdb` lists (every column, cues, My
Tags and artwork). A
new `export.pdb` starts from a real rekordbox export (rekordbox-pdb's test file) with its rows
removed, so it has rekordbox's colours and browse columns; a new OneLibrary database starts from
rekordbox 7's schema and default menus. My Tags come along:
`exportExt.pdb`'s into a new OneLibrary, and when `export.pdb` is rebuilt its tag links move to
the new track ids (by path) and OneLibrary's tags are added. A drive without `exportExt.pdb` doesn't
get one. A track or name too long for an `export.pdb` row (OneLibrary has no such limit) is refused
in the plan, by name. Converting a 3,900-track library takes about a second either way. Undo with
`locallibrary undo`.

`drive info` also says when rekordbox syncs playlists to a drive automatically (its
`playlists3.sync` / `playlists3Plus.sync`); merge, clean and convert warn that rekordbox doesn't
know about their changes.

## Cover art

```bash
locallibrary art missing Main
locallibrary art set Main cover.jpg --album "Framed And Defamed"     # or --track 12, --playlist, --query
locallibrary art set Main ~/Music/track.mp3 --track 12               # use a file's embedded art
locallibrary art auto Main                                           # fill gaps from embedded tags / cover.jpg
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
(`embed-backups/`, the last 5 embeds): `art unembed` puts them back, each one only if it hasn't
changed since (`--list` shows the embeds kept, `--backup ID` picks one). `art set` asks first too,
and the drive is backed up before it's written.

## Formatting a drive

```bash
locallibrary drive disks                          # what's connected, and how players will get on with it
locallibrary drive format "Backup" --dry-run      # the plan: what gets erased, what gets created
locallibrary drive format "Backup"                # ...then type the drive's name to confirm
locallibrary drive format disk4 --fs exfat        # a disk with no mounted volume: disk4 / /dev/sdb / PhysicalDrive2
locallibrary drive format disk4 --yes --erase disk4       # in a script: the name is given, not typed
```

A drive to format is named by its path (`./NAME` for a folder here), the disk's identifier, or
the name of a drive locallibrary adopted; never by a volume label, which an unrelated disk could
carry while the drive you meant isn't plugged in. The confirmation asks for that adopted name, or
the disk's identifier when the disk holds no adopted drive. `--yes` doesn't skip typing it (for
`drive format` and `provision --format`): without a terminal, `--erase NAME` gives it, and a name
that doesn't match erases nothing.

The whole disk is repartitioned: one MBR partition from 1 MiB to the end, holding FAT32 with the
cluster size Windows and macOS would choose (32 KB above 32 GB). That's what AlphaTheta's own
documentation points to (checked October 2026 against the operating manuals, the AlphaTheta
Help Center and the rekordbox 7 manual), and how AlphaTheta's SanDisk DJ drives ship:

| | FAT32 (default) | exFAT (`--fs exfat`) | HFS+ (`--fs hfs+`) |
| --- | --- | --- | --- |
| CDJ / XDJ / OPUS / OMNIS | every model | CDJ-3000 (firmware 1.20+), CDJ-3000X, CDJ-1500X, OPUS-QUAD, OMNIS-DUO, XDJ-AZ, XDJ-AN, XDJ-XZ (1.23+), XDJ-RX3 (1.11+) | every model |
| Not readable on | | CDJ-2000NXS2 and earlier, CDJ-900NXS, XDJ-1000MK2, XDJ-700, XDJ-RX2, XDJ-RR | |
| DJM-V10 / DJM-A9 (settings, firmware) | yes | no | no |
| Player firmware updates | yes | CDJ-3000X only | no |
| rekordbox can export to it from | Windows and Mac | Windows and Mac (rekordbox 6.6.2+) | Mac only |
| Largest file | 4 GB | no limit | no limit |

- **MBR, not GPT.** The Help Center pages for the CDJ-3000, CDJ-3000X, CDJ-2000NXS2, XDJ-XZ,
  XDJ-RX3, XDJ-AZ, OPUS-QUAD and OMNIS-DUO all say the GUID partition map isn't supported
  (macOS Disk Utility uses it by default). Players only read the first partition, so the drive
  gets exactly one. MBR tops out at 2 TiB, so larger drives are refused.
- **Cluster size** has no official guidance and no measured effect on loading or browsing. It
  decides how much space small files take: rekordbox writes three analysis files per track, so
  exFAT's default 128 KB clusters use about 0.5 MB per track where `--cluster-size 32K` uses
  about 0.1 MB. FAT32 clusters are never larger than 32 KB, which many FAT implementations can't
  handle.
- **HFS+** is created non-journaled and case-insensitive (case-sensitive HFS+ may not be
  recognised). Users report players mounting it more slowly than FAT32.
- **Players show the Device Name set in rekordbox**, not the volume label.

How each OS does it (only macOS's has run; Linux and Windows are experimental, see
[Platforms](#platforms)):

- **macOS**: `diskutil eraseDisk ... MBR` (no administrator rights needed). A non-default
  `--cluster-size` re-creates the file system with `newfs_msdos`/`newfs_exfat`, which needs `sudo`.
- **Linux** (never run): `sfdisk` and `mkfs.vfat` (dosfstools), `mkfs.exfat` (exfatprogs) or
  `mkfs.hfsplus` (hfsprogs), as root. The new volume isn't mounted: reconnect the drive.
- **Windows** (never run, nor compiled): the Storage cmdlets (`Clear-Disk`, `Initialize-Disk
  -PartitionStyle MBR`, `New-Partition`, `Format-Volume`) from an elevated terminal. Windows won't
  create FAT32 above 32 GB, so locallibrary writes the FAT32 structures itself (the writer is
  checked in tests against an independent FAT implementation and macOS's `fsck_msdos`). HFS+ isn't
  available.

Only external or removable, physical, writable disks can be formatted; the system disk, the disk
holding the store (the one `--store` names, even when it can't be opened) and the one holding
locallibrary itself are refused, and so is an archive or retired drive unless
`--overwrite-archive` is given. Under `sudo`, the default store is root's on most systems, so
`format` and `provision` ask for `--store` unless the default one is yours. The plan lists every
volume that will be erased, the drive and role it was, and whether a rekordbox library on it is
safely in the store (`locallibrary pull` it first if not). Just before erasing, the disk is looked
up again and must still be the one planned. A drive locallibrary knew keeps its name and id, so
`locallibrary sync Main Backup` works right after.

## Drive management

### Every drive you own

`status` covers the drives that are plugged in. `status --all` adds every drive locallibrary has
seen, with how far behind it was when last seen:

```text
Not mounted
Backup [backup]  Main  last seen 2026-09-28  2 snapshot(s) behind (held 3f9a2c41b0e2)
```

- `drive rename <drive> <name> [--label]` (`--label` changes the volume label too; experimental on
  Linux and Windows, see [Platforms](#platforms)), `drive forget <drive>`,
  `drive retire <drive> [--undo]`, `drive ls --known` (every registered drive).
- **Roles:** `drive role <drive> primary|backup|set|archive` (`gig`, the old name for `set`, works
  too). A library has one primary. Archive and retired drives are never written to (by a sync, an
  edit, a format or anything else) unless `--overwrite-archive` is given (`--force` also does for
  push); locallibrary still keeps its own notes in their `.locallibrary/` (the file index, what it
  last saw). `locallibrary sync` with no drives syncs the mounted drives of one library's group
  (`drive groups`); it, `smart apply` and `settings apply` leave archive and retired drives out.
- **Logbook:** `drive log <drive>`: data written by sync, push and repair, clean ejects, the last
  check of its files and of its file system, and problems found.
- **A drive that lost its identity** (erased elsewhere, or cleaned of `.locallibrary/`) is matched
  to the drive it was by its contents or its disk's serial number (on macOS, a USB disk's, from the
  IORegistry): `drive recognise <drive>`,
  then `drive adopt <drive> --as <name>` gives it back its id, name, library and history.
- **Copies:** copying a drive in Finder or with a cloning tool copies its identity file, so two
  drives claim one identity. `status` says so, and which is the original when the disk serial
  number recorded with the identity tells. `drive reidentify <path>` gives the copy its own, named
  `<name> (copy)` unless `--name`, after showing the plan and asking. It's refused when nothing
  shows the drive is a copy (no other drive mounted has its identity, and it's on the disk
  recorded with it) unless `--force`; a role stays with the original.

### Health

```bash
locallibrary drive verify Main                    # re-read every file and compare it with a known-good copy
locallibrary drive verify Main --repair           # rewrite what's damaged or missing (from the store, or --against another drive)
locallibrary drive fsck Main [--repair]           # the operating system's own file-system check
locallibrary drive format disk4 --check-capacity  # catch a fake-capacity stick before using it
```

- `drive verify` reads every byte under `PIONEER/` and `Contents/`, bypassing the hash cache (which
  would vouch for a damaged file by its size and modification time), and compares each file with the
  best reference there is: a stored snapshot of exactly this state, the drive's own hash records
  when the file's size and modification time haven't changed, or an older snapshot holding the same
  version of the file. A file a player or rekordbox rewrote has a new modification time and counts
  as unverified, not damaged. Unreadable files are reported too, with the tracks and playlists
  affected. `--repair` checks each file again, copies a good copy with every byte verified (keeping
  the modification time), and reads it back. A file that is still wrong after that means the flash
  itself is failing: replace the drive.
- `drive fsck` runs `diskutil verifyVolume`/`repairVolume` on macOS (no administrator rights; the
  drive is unmounted while it runs), `fsck.vfat`/`fsck.exfat` on Linux as root (the drive is
  unmounted with `udisksctl` first and mounted again afterwards; if it can't be unmounted, nothing
  runs), and `chkdsk` on Windows from an elevated prompt (`--repair` dismounts the drive while it
  runs). FAT's "dirty" flag isn't used: macOS clears the clean bit as soon as it mounts a volume,
  and repairs a dirty volume before mounting it. Only the macOS path has run; on Linux and Windows
  `fsck` is experimental (see [Platforms](#platforms)).
- `--check-capacity` writes a 1 MiB pattern to up to 256 regions across the disk, highest first,
  then reads them back from the device (f3probe's approach). A fake stick wraps writes past its
  real size onto what it already holds, so regions come back holding other regions' data, and the
  format stops. The first megabyte (partition table and boot sector) is wiped before the test
  writes anything, so a test that stops part way leaves a disk that no longer mounts and has to
  be formatted. It only runs as part of a format (it overwrites data), needs administrator rights
  on a real disk, and isn't available on Windows yet. On Linux it's experimental: it has only run
  on macOS disk images.
- `status` flags write-protected drives (players can't save history or cues to them) and drives
  with less than 200 MB free, and sync plans warn before leaving less.

### Space

```bash
locallibrary drive space Main                  # what's using the drive, its growth, whether it fits on the others
locallibrary drive tidy Main [--empty-trash]   # remove ._*, .DS_Store, Spotlight's index...
locallibrary drive recordings Main --copy-to ~/Music/Sets --tracklist latest [--delete]
```

- `drive space` sorts every file on the volume: audio, analysis, artwork, databases, settings,
  locallibrary's undo backups, recordings made on the player, OS clutter, trash, and audio outside
  the library (players list it when browsing folders, with no analysis). From the library's
  snapshots it fits a growth rate ("grows about 2.1 GB a month; full around March 2027"), and it
  checks whether the library still fits on the other drives of its library.
- `drive tidy` removes AppleDouble `._*` files (only real ones, by their magic number; other devices
  list them as broken tracks), `.DS_Store`, `Thumbs.db`, `.Spotlight-V100`, `.fseventsd`, and the
  `.locallibrary-tmp-*` files a cut-off write left (up to a whole track each).
  `--empty-trash` also deletes what's in the Finder or Explorer trash, for good. `System Volume
  Information` and locallibrary's own folder are left alone.
- `drive recordings` lists the WAV files MASTER REC writes on the XDJ-AZ (`ALPHATHETA REC`) and
  OPUS-QUAD (`PIONEER REC`), with their length. `--copy-to` copies them (verified), names them by
  when they started, writes a tracklist from a history list next to them (`--tracklist latest`, or
  a list's name), and with `--delete` frees the drive.

### Setting up and handing over

```bash
locallibrary provision /Volumes/NEW --like Main --name "Backup 2" --role backup --player-name "BACKUP 2" --eject
locallibrary provision disk4 --format --check-capacity --name Spare   # erase and test it, then the latest snapshot
locallibrary eject Main Backup                                        # or: locallibrary sync ... --eject
locallibrary drive firmware Main ~/Downloads/CDJ3000Xv150.UPD [--replace]  # afterwards: --remove
```

- `provision` is one plan: format, name, copy the library from a drive (or write the latest
  snapshot), write the settings profile, set the role and player name, read everything back,
  eject. It stops at the first step that fails, and says which.
- `eject` unmounts a drive and powers it off (`diskutil eject`, `udisksctl`, Explorer's Eject),
  which takes the disk's other volumes too: it names them first. It's refused while a
  locallibrary operation is writing to the drive, and for a disk that also holds the store or
  locallibrary itself. Only `diskutil` has run; on Linux and Windows it's experimental (see
  [Platforms](#platforms)).
- `drive firmware` checks an update file's name against the model it's for (CDJ-3000X, CDJ-3000,
  XDJ-AZ, OPUS-QUAD, DJM-A9, DJM-V10) and the drive's partition scheme and file system against
  that model's rules (the DJM-A9 takes MBR with FAT32 and one update file only; the CDJ-3000X
  takes exFAT too), then copies it to the drive's root. It never downloads firmware.

### What players show

```bash
locallibrary drive player Backup                                # the name and colour players show
locallibrary drive player Backup --name "BACKUP B" --colour 6
locallibrary drive contact Main "sam@example.com" --playlist
```

- Players show the Device Name set in rekordbox, not the volume label (a CDJ-3000 shows "NO
  DEVICE NAME" until one is set). rekordbox keeps it, with a background colour code, in
  OneLibrary's `property` table and in its own record of the device
  (`PIONEER/DeviceLibBackup/rbDevLibBaInfo_*.json`, never copied between drives). `drive player`
  sets OneLibrary's, which the newer players read; where players reading `export.pdb` find the
  name isn't documented. Sync and push keep each drive's own name and colour, and OneLibrary's
  fingerprint leaves them out, so drives with different names still compare as identical.
  rekordbox's next export writes rekordbox's values again; `drive player` remembers its own on the
  drive (in `.locallibrary/`), and `locallibrary refresh` puts them back.
- `drive contact` writes `IF FOUND.txt` in the drive's root and, with `--playlist`, a top-level
  playlist named after the contact, the first thing a player shows. The playlist is a recorded
  edit, so `edits replay` puts it back after rekordbox's next export.

### Back from a set

Players write to the drive: a history list every session, memory cues, and on the CDJ-3000X
playlist edits and TAG LIST playlists. If the main drive got a new export in the meantime, both
drives changed since they were last in sync, and a plain sync has to refuse. `sync` looks at what
each drive changed since the last state both held, which must be in the store (pull before
sets). When one drive's changes are only things players do (new history lists, rewritten analysis
files of tracks that were already there, player state files, TAG LIST playlists), it saves that
drive as a snapshot, so its history lists and cues stay in the store, copies the newer library
over it, and saves the result as the newest snapshot. Any other change still counts, and the sync
refuses as before.

## Safety

- **Nothing outside `PIONEER/` and `Contents/` is ever written or deleted** (plus locallibrary's own
  `.locallibrary/`), except by the commands that exist for it and list what they touch: `drive tidy`
  (OS clutter), `drive firmware` (an update file in the root), `drive contact` (`IF FOUND.txt`) and
  `drive recordings --delete`. Your other files on the drive are left alone, even where a library
  keeps audio outside `Contents/`: `sync` and `library push` refuse rather than overwrite a file
  there that the target's own library doesn't use, and `merge` puts the track beside it. Sync
  ignores OS clutter (`._*`, `.DS_Store`, `.Spotlight-V100`...).
- **Per-drive files stay put.** rekordbox's device identity (`PIONEER/DeviceLibBackup/`),
  `PIONEER/extracted/` (`gcred.dat`, written when rekordbox first sets a drive up, which looks
  like an account credential), what players keep for themselves (`PIONEER/LOG`, `CDJ`, `MPJ`,
  `TRASHBOX` and the track filter `PIONEER/rekordbox/RBFLTR.DAT`) and rekordbox's records of the
  playlists it syncs to the drive (`playlists3*.sync`) are never copied to another drive or into
  the store, never deleted, and don't count as a change to the library.
- **Crash-safe ordering.** Every file is written to a temp file and renamed into place. Audio,
  analysis and artwork are copied first and flushed to the device, and only then given their
  source's modification time, so a copy whose data never reached the drive can't pass for a finished
  one. The database files are then staged and swapped in back to back, and stale files are deleted
  only after that. If a sync is interrupted (unplugged drive, Ctrl-C), the database on the drive
  still finds every file it references; a file the sync changed may already hold its new contents. A
  marker is left that `locallibrary drive info` reports, and re-running finishes the job: `diff
  --summary` and `sync` take the interrupted drive as the older one, and the next sync copies again
  any file changed since the interrupted one started, unless its content is known to match. `pack`
  works the same way. Until an interrupted operation is finished, nothing else changes the drive or
  reads a library from it (an edit, `pull`, `merge`, or a sync or pack from it): `drive info` and
  `check` say how to finish it (sync or pack onto it again, or `undo` an interrupted change in
  place, whose backup it made). A change in place that made no backup (an update file, a repair)
  can't be taken back, so the next change finishes it. Once a change is on the drive, Ctrl-C no
  longer stops what records it.
- **Untrusted databases.** Paths read from a drive's databases are validated: nothing that
  climbs out of the drive (`..`), points into `PIONEER/` as audio or into `.locallibrary/` (in any
  letter case), or resolves outside the drive through a symlink is read, written or deleted. A
  playlist tree that repeats an id or loops (a damaged or crafted file) is read with each
  playlist once, the folder closing a loop (or nested past 64 deep) moved to the top level. So
  edits and merges never nest a playlist past 64 deep.
  OneLibrary's key is public, so its schema isn't trusted either: one with a view or trigger
  (rekordbox makes neither) isn't read, and tracks aren't removed while a table rekordbox 7
  doesn't have refers to them.
- **OneLibrary's WAL stays with its database.** rekordbox leaves `exportLibrary.db` in WAL mode,
  often with its latest changes only in `exportLibrary.db-wal`, and SQLite replays a WAL into
  whatever database sits next to it. Reads replay the WAL's committed transactions; syncs,
  edits and restores remove the old WAL (and `-shm`) before a new database goes in, then write
  the matching WAL if there is one. Edited databases are written checkpointed, with no WAL.
- **One writer per drive.** Operations that modify a drive take a lock on it, so a GUI and a
  terminal (or two GUI actions) can't interleave writes.
- **Plans before changes.** Every command that changes or deletes something (on a drive, in the
  store, or files on this computer) shows exactly what it will copy, change and delete and asks
  first (`--dry-run` stops at the plan; `--json` needs `--yes`). Some don't ask: `pull`, which only
  adds a snapshot to the store; `eject`, which changes nothing on the drive; `profile`'s choices
  (`include`, `pin`, `hide`...; `profile reset`, which forgets them all, asks), `account login`
  and `account logout`; and `export`, `xml`, `art get`, `plays export`, `profile document -o`
  and `mix --html`/`--export`, which write new files on this computer, never replacing one that's
  there without `--replace` (the player asks first; `mix --open` alone refreshes the store's own
  copy of a report). Then it makes the change it showed and
  no other: the plan remembers the state of every drive and file it touches, and if anything changed
  in between (rekordbox exported again, another command wrote to the drive, a sync would now copy
  other files) nothing is written and it says to plan again (`stale_plan`). Archive and retired
  drives aren't written to without `--overwrite-archive`.
- **No silent overwrites.** `library push` refuses to overwrite a drive holding a library state the
  store has never seen (pull it first, or `--force`), `sync` refuses when it can't tell which side
  is newer (without lineage, holding more tracks isn't enough on its own, and a side with fewer
  tracks is never taken for the newer one), `undo` refuses to put a backup back over a drive that
  changed since its change (unless `--force`), and `pack` saves the history lists and cues players
  left on a packed drive in the store before packing it again.
- **Formatting is confirmed by typing the drive's name** (`--yes` doesn't skip it; a script
  passes `--erase NAME`), and the confirmed plan is pinned to that exact disk: if a different
  disk turns up under the same identifier, nothing happens.
- **Undo** for every change made to a drive in place (playlist and track edits, My Tags, cues,
  merge, clean, convert, artwork, settings, the player name, the contact file, orphan removal) and
  packs: `locallibrary undo` puts back the backup of the last change the drive still holds, and run
  again, the one before (`drive restore --backup ID` puts back any). Backups include every file a
  change overwrote or deleted (artwork images, analysis files, settings files, removed tracks'
  files), with their modification times. Each notes what its change left, and putting it back is
  refused, unless `--force`, when the drive changed since: a rekordbox export, a sync or push, or
  history players recorded would be lost too. The plan shows the files and databases that change,
  how the playlists and tracks change, the history lists it drops, and the recorded edits that come
  off the drive's journal (the one the backup was made for, and any made since), so `edits replay`
  doesn't make them again; `undo --list` shows what can be undone. Putting a backup back goes
  through the same write routine as every change, and backs up what it replaces first: putting that
  backup back redoes the undo. Each drive keeps the newest 10 backups of each kind, counting those
  holding removed tracks' audio (`drive backups` says which) and those undos took apart, so small
  edits don't prune the only copy of a deleted track. Art `art embed` put in files on this computer
  comes back out with `art unembed`. Repairs, firmware files and collected recordings aren't backed
  up.
- **One write routine.** Every change to a drive in place locks it, builds and checks the new
  databases and files in memory, backs it up, marks the drive as being written, copies new files
  next to where they go (flushed, then stamped), and swaps everything in back to back. A write
  that fails or is cancelled before that swap leaves the drive byte-identical: its backup, temp
  files and marker go again.
- **Verification.** Merged and edited databases are re-read before being written; edits must
  read back exactly as planned. Database files are
  re-hashed on every scan; other files use a (size, mtime) hash cache that ignores entries
  recorded within FAT's 2-second timestamp window. `--verify` compares audio by content, and
  `locallibrary store verify` re-hashes the store.
- Built for FAT32/exFAT/HFS+ drives: matching ignores letter case and Unicode normalisation,
  mtime comparisons allow FAT's 2-second granularity, and a cached hash index keeps rescans fast.

## Building a GUI (Electron)

All operations live in the library's [`api`](crates/locallibrary/src/api.rs) module as
`request → response` functions over serde types. Operations that write come as `*.plan` /
`*.apply` pairs, so a UI can show a confirmation dialog and then make exactly the change it
showed. The simplest integration is the sidecar: spawn `locallibrary rpc` and speak
newline-delimited JSON-RPC 2.0 over its stdin/stdout.

```ts
import { spawn } from "node:child_process";
import * as readline from "node:readline";

const locallibrary = spawn("locallibrary", ["rpc"]);
const pending = new Map<number, { resolve: (v: any) => void; reject: (e: any) => void }>();
let nextId = 1;

readline.createInterface({ input: locallibrary.stdout }).on("line", (line) => {
  const msg = JSON.parse(line);
  if (msg.method === "progress") {
    // msg.params = { id, event: { event: "phase_start" | "item" | "bytes" | "phase_end" | "warning" | "step", ... } }
    return onProgress(msg.params.id, msg.params.event);
  }
  const p = pending.get(msg.id)!;
  pending.delete(msg.id);
  msg.error ? p.reject(msg.error) : p.resolve(msg.result);
});

function call(method: string, params: object = {}) {
  const id = nextId++;
  locallibrary.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  return { id, result: new Promise((resolve, reject) => pending.set(id, { resolve, reject })) };
}

const drives = await call("drives.list").result;
const preview = await call("sync.plan", { left: "Main", right: "Backup" }).result;
if (!preview.noop && (await confirmDialog(preview.summary, preview.warnings))) {
  // Exactly what the dialog showed: refused with `stale_plan` if either drive changed since.
  const job = call("sync.apply", { plan_id: preview.plan_id });
  // cancelButton.onclick = () => call("$/cancel", { id: job.id });
  await job.result;
}
```

- **Trust.** Only a trusted parent process may spawn and drive the sidecar: it acts with your
  rights. Requests name files and folders it reads and writes (`output` for exports, `interop.*`
  and `mix.export`, `to` for `recordings.collect.*`, `path` for `store.copy`, images and audio for
  `art.*`, `file` for `firmware.*` and `import.*`, `db` for `collection.*`, any folder as a
  drive) and servers it fetches from (`pds` for `settings.import_published.*`, `client_id` for
  `account.login.begin`). Never expose it on a socket or to another user, and never pass web
  content (a page's or a renderer's input) into its parameters. It refuses relative paths to write
  to, a data server that isn't https (or on this machine) and a line longer than 16 MiB; those
  checks only catch mistakes.
- **The contract.** [`docs/rpc/schema.json`](docs/rpc/schema.json) describes every method:
  its `kind` (`read`; `plan` and `apply`, paired; `immediate`, a change with no plan step;
  `control`, about the sidecar itself), its `aliases`, whether it's `long_running` (sends
  `progress`, can be cancelled) or `destructive` (can delete or overwrite something), and JSON
  Schemas for its `params` and `result`, with the `progress` and `event` notifications.
  [`docs/rpc/locallibrary.d.ts`](docs/rpc/locallibrary.d.ts) has the same as TypeScript types
  (`Methods["sync.apply"]["params"]`, `Params<"sync.apply">`, `Result<"sync.apply">`).
  `locallibrary rpc --emit-schema DIR` writes both for the version you run, and `rpc.describe`
  answers the same at run time (`{"method": "sync.apply"}` for one method). Both come from the
  one list of methods the sidecar dispatches from, and a test fails when the files in `docs/rpc`
  are out of date.
- **Names** are `noun.verb` (`drive.info`, `tracks.query`); a change is `<op>.plan` and
  `<op>.apply`. Older names still work as aliases: `drive.adopt` (now `drive.adopt.apply`, and
  likewise `drive.adopt_as`, `drive.reidentify`, `drive.rename`, `drive.contact`, `drive.restore`,
  `edits.clear`, `firmware.remove`, `recordings.collect`, `settings.import`, `art.set`, `art.embed`,
  `art.unembed`, `store.gc`, `store.prune`, `players.set`, `players.clear`, `player.set`,
  `playlist.restore`, `drive.eject`, `pull`), `sync_many.*` (`sync.many.*`), `sync_group.*` and
  `sync_set.*` (`sync.group.*`), `smart.apply.plan` / `smart.apply` (`smart.write.*`),
  `settings.apply.plan` / `settings.apply` (`settings.write.*`) and `drive.sets` (`drive.groups`).
  `drive.restore.preview` is kept as it was; `drive.restore.plan` is the plan `drive.restore.apply`
  applies (an `undo` plan), as `drive.eject.plan` makes `eject` plans. A few methods are a single
  verb, as the CLI's commands are: `status`, `compare`, `diff`, `check`, `doctor`, `export`;
  `refresh` and `set` are jobs of several steps.
- Every `*.plan` answers with the plan's own fields plus `plan_id`, `operation` (the method's
  name without `.plan`), `summary` (a few lines for a dialog), `warnings` (all of them),
  `destructive` (it deletes or overwrites something), `noop` (applying it changes nothing),
  `drives` (each `{root, name, writes, drive}`) and `planned_at`. The sidecar keeps the plan:
  `*.apply {"plan_id": "..."}` applies exactly that plan, once, and fails with `stale_plan` if
  anything it touches changed since (the drive was written, rekordbox exported again, or it
  would copy other files now), if it was applied already, or if it's gone (the newest 64 plans
  are kept for 30 minutes, shared by every worker). Plan again and ask again. An apply refused
  before anything was written (`stale_plan`, an archive drive's `conflict`, `locked`) keeps the
  plan, so it can be sent again. `*.apply` with the request itself, as before, plans and applies
  in one go: it isn't bound to what anyone confirmed.
- Writes to archive and retired drives are refused (`conflict`) unless the request (the plan's or
  the apply's) has `"overwrite_archive": true`.
- Requests run concurrently, on two pools of workers: quick reads (`tracks.query`, `track.*`,
  `art.*` previews and paths, `drive.library`, `playlists.tree`, `tracks.get`, `cues.list`,
  `tags.list`, `drives.*`, listings) in one, everything else (writes, plans, scans, mixes) in the
  other, so a long job never holds up a GUI's reads. Each runs 4 requests at once
  (`locallibrary rpc --workers N`, or `LOCALLIBRARY_RPC_WORKERS`); a request waiting for a worker
  shows in `jobs.list` with `queued: true`, and `$/cancel` answers it at once.
- The sidecar keeps the parsed libraries of the last 4 drives it read, so paging through tracks,
  showing one track after another, cues, My Tags, artwork and `mix.identify` (its fingerprint
  index too) parse a drive's database once. A library is read again as soon as one of the
  drive's database files has a new size, modification time or file id, after any request that
  may have written a drive (an `*.apply`, with the request or a `plan_id`, and `undo.apply`
  among them), and when `events` says the drive changed.
- Progress byte counts and per-file `item` events are coalesced to about 10 lines a second per
  request. An `item` stands for the files since the previous one; `items` says how many when
  that's more than one (count `items ?? 1`). Events about one drive of several (`sync.many.*`,
  `sync.group.*`) carry `target`, the drive's root as in the results. `provision.apply` announces
  each step it runs with a `step` event (ids `format`, `name`, `library`, `settings`, `role`,
  `player`, `verify`, `eject`), and the events during a step carry
  `step: {index, total, id}` (`index` counts from 1).
- Ids are strings or numbers. A request whose id a running request already has is refused with
  `-32600` (`kind: "invalid_request"`); an id is free again once its response has been sent.
  Notifications (no `id`) get no response, and `$/cancel` can't reach them.
- `initialize` returns `{protocol: 1, version, capabilities, methods}`. `capabilities` lists
  features a GUI can check for (`progress`, `progress.scope` for the `target`, `step` and
  `items` fields above, `cancel`, `jobs.list`, `shutdown.cancel`, `events`, `plan_id`,
  `describe` for `rpc.describe`); `methods` lists every name, aliases included. `protocol`
  changes only when a change would break existing clients. An unknown method gets `-32601`
  (`kind: "method_not_found"`).
- `$/cancel {id}` stops a request at the next safe point. It then fails with `kind: "cancelled"`.
  `jobs.list` returns the running requests, oldest first, as `{id, method, started}` (`id` is
  `null` for a notification).
- Events: `events.subscribe {kinds?, interval_ms?}` returns `{kinds, drives}` (the drives mounted
  now, as `drives.mounted` gives them) and from then on sends notifications
  `{"method": "event", "params": {"event": "drive.attached", ...}}`: `drive.attached`,
  `drive.detached` (the drive as last seen) and `drive.changed` (`changed`: `databases` and/or
  `state`, its `.locallibrary` identity) with the drive's `root`, `id`, `name` and `has_export`;
  `store.changed` with `changed` (`libraries`, `snapshots`, `labels`, `pruned`, `drives`,
  `settings`, `smart`); and `job.started` / `job.finished` (`id`, `method`, `ok`, and the error's
  `kind`) for every request. `kinds` takes event names or groups (`drive`, `store`, `job`);
  empty means all. Drives and the store are polled every second (`interval_ms`, at least 100)
  by looking at file metadata only, so watching costs a few `stat` calls per drive. Every mounted
  volume counts, blank ones included (`has_export: false`). `events.unsubscribe` stops them.
- `drives.mounted` lists the mounted volumes as `{root, id, name, has_export}` without reading
  any database, cheap enough to poll; `drives.list` reads every drive's library.
- `watch.start {pull?, sync_group?, settle_ms?, interval_ms?}` does what `locallibrary watch` does
  (see [Saving drives as they're plugged in](#saving-drives-as-theyre-plugged-in)) inside the
  sidecar until `watch.stop {cancel?}`, and sends an event for each thing it does, whether or not
  you subscribed (unless a subscription's `kinds` leave them out): `watch.up_to_date`,
  `watch.pending` and `watch.pulled` (`library`, `history_lists`, `beside`, and the `snapshot`),
  `watch.synced` (`targets`, as `sync.many.apply` returns them), `watch.skipped` (`reason`) and
  `watch.failed` (`kind`, `message`), each with the drive's fields as above. It shows in
  `jobs.list` as `{id: null, method: "watch"}`; a second `watch.start` while it runs is a
  `conflict`. `watch.stop` lets what it's doing finish (`cancel: true` stops that too), as do
  `shutdown` and closing stdin.
- `shutdown` stops reading requests; the process exits once the running ones have answered.
  `shutdown {cancel: true}` cancels them first, so a pending `account.login.finish` doesn't hold
  the exit for its 5 minutes.
- A line that isn't JSON (or isn't UTF-8) gets a `-32700` parse error with `id: null`; running
  requests carry on. Closing stdin lets running requests finish before the process exits.
- SIGINT or SIGTERM (Ctrl-C on Windows) cancels every running request at its next safe point
  (each fails with `kind: "cancelled"`) and refuses new ones; the process exits with 128 + the
  signal's number once they have answered. A second signal exits once no database swap is under
  way.
- Errors carry `error.data.kind`: `not_found`, `not_an_export`, `conflict` (needs a user decision
  such as a direction or `force`), `stale_plan` (the drive changed since the plan was made: plan
  again and ask again), `no_space`, `locked` (another operation is writing that drive or the store),
  `cancelled`, `invalid`, `io`, `parse`, `corrupt`, `pdb`, `onelibrary` (`exportLibrary.db` can't
  be read or written), `image`, `tags`, `external` (a system tool such as `diskutil` failed),
  `permission` (needs administrator rights: re-run elevated), `account` (signing in or
  publishing, see below), and for the protocol itself
  `parse_error`, `invalid_request`, `method_not_found`, `invalid_params` (`message` and
  `fields.path` say where in the request: `edits[0].op`) and `internal`.
- `error.data` also has `reason`, `fields` and `actions` (`ErrorData` in the contract), so a GUI
  can act on an error without reading its message. `reason` says which decision a `conflict`
  needs (`uncertain` or `diverged`: choose a direction or merge; `unrelated` libraries;
  `back_from_set`; `not_saved`: pull first, or force; `retired`, `archive`, `exists`,
  `missing_audio`, `label_taken`...) or what else would resolve an error (`unknown_library`,
  `choose_library`, `needs_admin`, `not_signed_in`, `session_ended`, `missing_scope`...).
  `fields` holds what it's about (`path`, `needed` and `available` for `no_space`, `drive`,
  `left` and `right`, `libraries` to choose from). Each action is `{label, method, params}`: a
  button's text and the call it makes, ready to send (`{"method": "pull", "params": {"drive":
  "/Volumes/DNA"}}`); one that repeats the request that failed carries its parameters with the
  new ones added (`force: true`, `overwrite_archive: true`, `new_library: true`), and `elevate:
  true` asks for the sidecar to run with administrator rights. A parameter set to `null` is one
  the user has to give (a library's name). An `*.apply {"plan_id"}` refused before writing offers
  the same plan again (`overwrite_archive: true` with its `plan_id`); one that failed while
  writing offers no repeat: plan again. The CLI shows the same actions as commands to type
  (`hint: locallibrary pull DNA`).
- `DeviceInfo` has `has_export` (either database), `has_device_library` (`export.pdb`) and
  `has_library_plus` (`exportLibrary.db`), plus `drift` (how the two differ, on a drive with
  both) and `rekordbox_sync` (rekordbox's automatic playlist sync to the drive).
- `version` lists every method. They mirror the CLI: `status`, `drives.list`, `drive.info`,
  `drive.adopt.plan/apply`, `drive.library`, `drive.restore.plan/apply` (`{drive, backup?, force?}`),
  `undo.list`, `undo.plan/apply` (`{drive?, backup?, force?}`; either apply takes the other's
  `plan_id`), `compare`, `sync.plan/apply`, `sync.many.plan/apply`,
  `pull.plan/apply` (refuses a `library` the store doesn't have unless `new_library: true`; `label` names
  the snapshot), `snapshot.label` (`{label, snapshot}`, or `{label, remove: true}`),
  `library.list`, `library.rename`, `store.prune.plan/apply` (`keep_last`, `keep_daily`,
  `keep_weekly`, `library`; says why each kept snapshot stays), `store.copy` (`path`, `mirror`,
  `verify`, `force`, `dry_run`), `store.copies`, `push.plan/apply`,
  `merge.plan/apply`, `clean.plan/apply`, `convert.plan/apply`, `settings.*`, `art.*` (`art.get` returns base64 JPEG previews),
  `xml.export`, `store.*`, `disks.list`, `format.plan/apply` (`format.apply` needs the plan's
  `identity`, so the disk the user confirmed is the one erased).
- Editing: `edit.plan/apply` takes `{"drive": "Main", "edits": [...]}`, a list of edits tagged by
  `op` (`create_playlist`, `rename_playlist`, `move_playlist`, `delete_playlist`, `add_tracks`,
  `remove_tracks`, `move_tracks`, `set_tracks`, `set_entries`, `sort_playlist`, `dedupe`, `plan_set`,
  `edit_tracks`, `delete_tracks`, `replace_track`, `delete_history`; see `edit::Edit`), applied as
  one change with one backup. Tracks are chosen with selectors (`{"ids": [12]}`,
  `{"filter": "bpm:>=128"}`, `{"paths": ["/Contents/..."]}`, `playlist`, `album`, `query`,
  `history`; a track must fit every field given). Also `edits.list`, `edits.replay.plan/apply`, `edits.clear`, `import.plan/apply`,
  `dedupe.plan/apply`, `history.delete.plan/apply`, `smart.list/set/remove`,
  `smart.write.plan/apply`, `pack.plan/apply`, `tags.list`, `tags.plan/apply`, `cues.list`,
  `cues.plan/apply`, `playlist.restore.plan/apply` (`{drive, playlist}`, with
  `snapshot` or `backup` to choose the source; the plan lists the `missing` entries left out).
- Reading a library a piece at a time: `drive.library` takes `offset` and `limit` for its
  tracks (the reply then has `page: {offset, count, total}`; without them every track comes, as
  before); `playlists.tree {drive}` is the playlist tree without tracks, `{playlists, history}`
  of `{id, name, path, folder, tracks, duration_secs, children}`; `tracks.get {drive, ids}`
  returns `{tracks, missing}` in the order asked. `track.analysis` takes `waveform`
  (`none`, `preview`, `detail` or `all`, the default) and `encoding`: `json` (the default, each
  column an object, 4-5 MB for a scrolling waveform) or `base64`, where each waveform is
  `{encoding: "base64", style, columns_per_sec, columns, back, front}` and a layer is
  `{heights, colors}`: one byte a column of height (0-255 of the full height) and three of red,
  green and blue, in base64 (`front` is `null` when no column has one, as for scrolling
  waveforms). `art.get` and `art.path` take `track` (one answer, `null` without artwork) or
  `tracks` (a list of `{track, image}` in the order asked); `art.path` returns `{path, mime}`, an
  absolute path inside the drive with symlinks resolved (never one leading out of it), for a
  webview to load directly.
- Reading: `tracks.query` (filter, sort, paging), `track.analysis` (beat grid, cues, waveforms,
  phrases), `track.show` (details, playlists, My Tags, copies, analysis summary), `track.matches` (harmonic matches), `doctor`, `doctor.orphans.plan/apply`,
  `plays.history`, `plays.archive`, `plays.stats`, `profile.build` (the report, preferences and
  document), `profile.document`, `profile.prefs`, `profile.prefs.set`, `export`, `diff`, `check`,
  `players.models` (what each model reads), `players.show/set/clear` (`{models, library}`; the models
  `check` uses without `players`),
  `account.status`, `account.check`, `account.logout`, `account.client_metadata`,
  `collection.info`, `collection.library` and `collection.diff` (rekordbox's `master.db`: optional
  `db` and `key`, and `drive` for the diff). Signing in takes
  two calls: `account.login.begin` (`handle`, optional `client_id`, `dev_client`, `session_file`,
  `scopes`, and `publish: true` to allow publishing) returns the `url` to open in the browser, an
  `id`, and a `note` when it fell back to the
  development client; `account.login.finish` (`id`, optional `timeout_secs`: 300 by default, an
  hour at most) waits for the browser
  and returns the account (`$/cancel` stops waiting; `account.login.paste` takes a pasted
  address instead; `account.login.cancel` forgets a sign-in that won't be finished).
  `account.logout` says whether the server revoked the session (`revoked`) and what couldn't be
  removed (`left_behind`). Errors about the account have `kind: "account"`. Publishing:
  `settings.publish.plan/apply`, `profile.publish.plan/apply` (the parameters of
  `profile.build`), `history.publish.plan/apply` (`drive`, `list`, optional `archived`, `title`,
  `date`, `venue`, `hide`), and `settings.import_published.plan/apply` (`from`: a handle or
  DID, default the signed-in account; `pds`; `replace: true` to replace the base instead of
  adding to it). A plan holds the exact `record`, what's published
  in its place now (`current`), `changed`, `notes` to show the DJ, and `missing_scope` when
  the session may not write it: sign in again with `publish: true` before applying. Like every
  plan it has a `plan_id`: `*.apply {"plan_id": ...}` publishes (or imports) exactly what it
  showed, and fails with `stale_plan` if the record was published again in between (or, for My
  Settings, the settings profile changed).
  Mixes: `mix.index`, `mix.identify` and `mix.export` keep fingerprints in the store (and make
  one), so they're `immediate`. `mix.identify` returns the whole `MixReport`, plus `audio`: a file
  a browser can play for the mix, the recording or an export next to an Audacity project, `null`
  when there's none; `extras: true` adds each play's overview waveform, hot cues and phrases in
  `extras`, in the order of `plays`, so following along needs no `track.analysis` per track;
  `save: true` keeps the report in the store's `reports` folder, its id in `saved` (a report that
  can't be kept still comes back, with a warning); progress phases `fingerprint`, `index`,
  `listen`, `match`. `mix.export` is the same, as the portable export (`output` also writes it to a
  file, refusing to replace one that's there without `replace`). `mix.reports.list` (the reports in
  the store's `reports` folder, newest first: `id`, title, artist, recording, drive, tracks,
  coverage, and which of the report's data `.json`, page `.html` and Local Network export
  `.mix.json` are there), `mix.reports.get {id, extras?}` (a kept report, as `mix.identify` answers)
  and `mix.reports.html {id, inline?}` (writes the report's interactive page next to it and answers
  its `path`, for a webview; the tracks' waveforms, cues and covers come from the drive it was
  matched against, by its identity, when it's mounted; `inline: true` adds the page itself as
  `html`).
- Drive management: `fleet.status`, `drives.known`, `drive.rename/forget/role/retire/log`,
  `drive.recognise`, `drive.adopt_as.plan/apply`, `drive.reidentify.plan/apply`, `drive.groups`,
  `sync.group.plan/apply` (`watch.start` takes `sync_group`, or `sync_set` as before),
  `drive.eject.plan/apply`, `drive.verify`, `drive.repair` (takes the `problems` a verify returned; each is
  checked again first), `drive.fsck` (destructive: `repair: true` unmounts the drive and may
  truncate or remove damaged files), `drive.space`, `tidy.plan/apply`,
  `recordings.list`, `recordings.collect.plan/apply`, `firmware.plan/apply`,
  `firmware.remove.plan/apply`, `player.show`, `player.set.plan/apply`, `drive.contact.plan/apply`,
  `provision.plan/apply` (`provision.apply` with the request takes the plan's `format.identity`
  as `format_identity` when formatting), and the jobs `refresh.plan/apply` and `set.plan/apply`
  (what `locallibrary refresh` and `set` do, as one plan of steps). `sync.many.apply` returns one
  result per target: a target that fails carries an `error` and doesn't stop the others.
- Writes that had no plan step have one: `drive.adopt.plan`, `drive.adopt_as.plan`,
  `drive.rename.plan`, `drive.contact.plan`, `player.set.plan`, `edits.clear.plan`,
  `firmware.remove.plan`, `recordings.collect.plan`, `settings.write.plan` (`{drives,
  create_missing}`, every mounted drive but archive and retired ones by default; applying it
  returns one result per drive, a drive that fails carrying an `error`), `settings.import.plan`,
  `art.set.plan`, `art.embed.plan` (applying keeps a copy of each file in the store first) and
  `art.unembed.plan` (puts them back), `store.gc.plan`, `players.set.plan`, `players.clear.plan`,
  `pull.plan`, `drive.eject.plan`, `drive.reidentify.plan`.
  Their `.apply` applies a plan by `plan_id`, or plans and applies a request as before (`settings.write.apply` with a request
  still writes one `drive`, honouring `dry_run`).
- Parameter names are the fields of the request structs in `api.rs`, as `docs/rpc` describes
  them. Results that were pairs keep their JSON (`failed: [["Contents/a.mp3", "unreadable"]]`);
  the schema and the TypeScript types name each element (`[path: string, error: string]`).

Native bindings (napi-rs) can wrap the same `api` functions later without changing behaviour.

## How it works

```text
crates/locallibrary/        the library
  device.rs    a drive: identity, lineage, scanning, fingerprint
  tree.rs      file scanning + persistent (size, mtime) -> hash cache
  sync.rs      plan/apply mirroring; pull/push between drives and the store
  compare.rs   lineage + evidence -> which side is newer
  op.rs        planned operations: a plan with its id, summary and the state of what it touches,
               applied only as planned (`stale_plan` otherwise)
  commit.rs    the one routine every in-place write goes through: lock, back up, stage, swap,
               record; byte-identical when it fails before the swap
  backup.rs    the backups on a drive, and putting one back (`undo`)
  store.rs     content-addressed objects, snapshot DAG, libraries, labels, drive registry
  retention.rs which snapshots `store prune` keeps
  offsite.rs   copies of the store elsewhere (`store copy`)
  merge.rs     semantic merge via export.pdb and OneLibrary editing
  edit.rs      playlist and track edits in both databases, verified against a model; the edit journal
  query.rs     the track filter and sort language, track selectors
  harmony.rs   Camelot keys, harmonic matches, mix costs and the set planner
  smart.rs     rule-based playlists
  pack.rs      drives for a set, cut down from another drive's library
  import.rs    reading outside tracklists and matching them to a library
  tags.rs      My Tags in exportExt.pdb and OneLibrary
  cue_edit.rs  hot cues, memory cues and loops in analysis files and OneLibrary's cue table
  doctor.rs    duplicates, missing and orphaned files, drift
  plays.rs     history lists on drives and in the store, play statistics
  profile/     what sets, tags, analysis and My Settings say about a DJ; the public document and its preferences
  mix/         which library tracks a recorded set plays (fingerprints, index, matcher); its report: the
               interactive page, the mix's playable audio, the reports kept in the store (report.rs and
               report.html), and the export for Local Network
  atmosphere/  Atmosphere account sign-in around jacquard-oauth: browser callback, keychain storage, lock,
               the network's limits (with publish.rs and api/account.rs, api/publish.rs: only with the
               `account` feature)
  opener.rs    handing an https address to the browser, or a file to its app
  export.rs    playlists as M3U8, CSV and text
  diff.rs      playlist differences between drives and snapshots
  check.rs     the check before a set, down to the tracks each player won't load
  probe.rs     reading audio files through symphonia: their headers, and whether their ends decode
  convert.rs   building export.pdb from OneLibrary or the reverse
  clean.rs     removing unwanted tracks (rekordbox's demo samples) and their files
  settings.rs  schema of every My Settings value, byte-level DAT patching, profiles
  artwork.rs   drive artwork rows/thumbnails, audio tag embedding
  anlz/        analysis files: path rewriting, beat grid, cues (read and write), waveforms, phrases
  pdb.rs       reading export.pdb into the library model, through rekordbox-pdb
  paths.rs     drive-relative path keys, and which files on a drive locallibrary manages
  onelibrary/  exportLibrary.db: SQLCipher pages, WAL replay, reading and editing via SQLite
  xml.rs       rekordbox XML export
  masterdb.rs  rekordbox's own collection (master.db), decrypted from a copy and read; comparing a drive with it
  interop/     the library for other DJ software: Traktor's collection.nml, Serato's crates and cue tags,
               an Engine DJ library
  format/      drive formatting: player compatibility, diskutil/lsblk/PowerShell backends, FAT32 writer,
               fake-capacity probe
  equipment.rs what each player, all-in-one system and mixer reads (databases, file systems, audio formats,
               sample rates, bit depths), with the manual each value comes from
  fleet.rs     every drive you own: registry views, roles, logbook, recognising and re-identifying drives
  scrub.rs     re-reading a drive against known-good copies, and repairing it
  fscheck.rs   the OS's file-system check
  space.rs     what uses a drive's space, growth forecasts
  tidy.rs      removing OS clutter
  recordings.rs  set recordings made on the player
  firmware.rs  staging firmware updates
  player.rs    the Device Name and colour players show, "if found" contacts
  after_set.rs telling changes players made during a set apart from exports
  eject.rs     unmounting and powering off drives
  watch/       noticing drives plugged in, unplugged and changed, by polling file metadata
  api.rs       service layer used by the CLI and RPC (api/drives.rs: drive management, api/undo.rs:
               undo, api/autopull.rs: `watch`, api/cache.rs and api/reads.rs: parsed libraries
               kept between requests, the library in pieces, api/sidecar.rs: the sidecar's own
               requests and notifications); every *_plan returns an op::Planned
  schema.rs    JSON Schema for the API's types (every request and result derives it), labelled tuples
  error.rs     errors with a stable code, and the reason, fields and actions a front end acts on (its
               messages never name a CLI command or flag: the CLI's hints.rs words those)
crates/locallibrary-cli/    `locallibrary` (clap), the JSON-RPC sidecar (rpc.rs; rpc/methods.rs: every method in
                            one registry, rpc/contract.rs and rpc/typescript.rs: `rpc.describe`, schema.json
                            and the TypeScript types), and the player (tui/: ratatui + rodio, the `tui` and
                            `playback` features; account_off.rs and the `playback`-less audio.rs stand in
                            for what a build leaves out)
vendor/rekordbox-pdb/       export.pdb and exportExt.pdb reader/editor (MIT), with locallibrary's patches, see PATCHES.md
docs/rpc/                   the sidecar's contract: schema.json and locallibrary.d.ts (`rpc --emit-schema`)
```

- **Fingerprint.** A drive's library state is identified by hashing every library file's path and
  size, plus the content of everything except audio (database, analysis, artwork). Two drives
  holding the same export get the same fingerprint regardless of timestamps, and fingerprinting
  never reads audio. OneLibrary counts by its decrypted content, not its bytes: a checkpoint
  (which a player opening the database may do) rewrites every byte with fresh IVs but changes
  nothing. It also leaves out what belongs to the drive rather than the library: the name and
  background colour players show (`property.deviceName`, `backGroundColorType`) and the header
  counters SQLite bumps on every write. Lineage and snapshots recorded before that are still
  matched through the earlier hash, so nothing shows as changed.
- **Lineage.** Each drive's `.locallibrary/state.json` keeps the fingerprints it has held.
  Syncing A to B hands B A's history. "B's current state appears in A's history" means B is
  behind, and "both moved on from a shared state" means they diverged.
- **Store.** Files are stored by BLAKE3 hash. Snapshots are manifests with parent links, so
  history is a DAG and storage is deduplicated across drives and time.
- **OneLibrary.** `exportLibrary.db` is SQLite encrypted with SQLCipher 4's defaults (AES-256-CBC
  pages with HMAC-SHA512, PBKDF2-HMAC-SHA512 key derivation) under one passphrase every rekordbox
  installation shares. It's decrypted (with its WAL replayed) into a temporary working copy,
  edited with SQLite, checked (`PRAGMA integrity_check` and an encrypt/decrypt round trip) and
  encrypted again with the file's own salt. Tracks are matched to `export.pdb`'s by file path.

## Limitations

- **OneLibrary** rows that `export.pdb` has no equivalent for (`analysedBits`, `contentLink`,
  `masterDbId`...) are copied from the source's own OneLibrary database when merging. A source
  without one (and `convert` from `export.pdb`) gets the values most of the target's tracks have,
  or rekordbox 7's for an analysed track; this hasn't been checked on every player model.
- What rekordbox's automatic playlist sync does with playlists it didn't create, or tracks
  removed from playlists it syncs, hasn't been tested; check the drive after rekordbox next
  syncs it.
- Whether OneLibrary-only players read My Settings from `*SETTING.DAT` hasn't been checked on
  hardware; `exportLibrary.db` itself holds no settings.
- **Edits on a drive last until rekordbox's next export to it**, which replaces its databases;
  `edits replay` makes them again. Nothing is written to rekordbox's own collection.
- My Tags in `exportExt.pdb`, edited cues and edited playlists follow the formats as documented
  (crate-digger, rekordcrate, pyrekordbox) and as rekordbox writes them in the fixtures, but
  haven't been loaded on every player model. Players don't use `export.pdb`'s index pages as far
  as is known; locallibrary doesn't maintain them, like rekordbox-pdb before it.
- Deleted rows' space in `export.pdb` isn't reused (rekordbox doesn't compact pages either), so a
  database grows a little with every rename or text edit until the next export.
- Merged tracks bring their My Tags into both databases, from whichever of the source's
  databases has them; tags beyond rekordbox's four categories of 50 are skipped with a warning.
  Hot cue bank lists only merge between OneLibrary databases. History playlists merge with
  `--history`, flattened to the top level. Tracks matched as duplicates keep the target's
  metadata and tags. A track whose details are too long for one of the target's `export.pdb`
  rows (OneLibrary has no such limit) is left out with a warning, and a playlist, folder or
  history list name that long is refused in the plan.
- The computer-side rekordbox database (`master.db`) is only read, never written: interchange
  with rekordbox goes through device exports and rekordbox XML. The reader follows pyrekordbox's
  description of the tables and was checked against databases built to that description (with
  the `sqlcipher` CLI installed, the tests also have SQLCipher write and read them under
  rekordbox's key), not against a real rekordbox 6 or 7 collection: which column holds a hot
  cue's colour (`ColorTableIndex` is taken as the hot cue palette code), the date added
  (`StockDate`) and that `rb_local_deleted` rows are deleted are inferred. Intelligent playlists
  aren't evaluated.
- **Exports for other DJ software** follow those programs' formats as their users and open
  source projects document them; none has been loaded in the program it's for. Traktor's NML
  follows a collection Traktor wrote (from traktor-nml-utils' test files) and is parsed back with
  an independent XML parser in the tests, but whether Traktor places an MP3's cues exactly where rekordbox
  does (the two handle the encoder's delay differently) hasn't been checked, and nothing
  corrects for it. Serato's crate layout follows the mixxx wiki and a crate Serato wrote
  (serato-tools' test file), and the `Serato Markers2` encoder reproduces, byte for byte, a tag
  Serato DJ wrote (serato-tags' test files); whether Serato picks up crates whose tracks it hasn't
  seen before, and how it shows rekordbox's hot cue colours (mapped to its 18 by hue), hasn't been
  checked. The Engine Library follows Engine DJ's schema as libdjinterop documents it and passes
  libdjinterop's schema 3.0.2 checks (tables, columns, indexes, views), but no Engine version has
  opened one, and its performance data (grids, cues) isn't written at all.
- `*SETTING.DAT` layouts are reverse-engineered (thanks to [rekordcrate](https://github.com/Holzhaus/rekordcrate)).
  Unknown byte values are reported as `unknown(0x..)` and preserved.
- **Drive management** hasn't been checked on hardware: whether players show OneLibrary's
  `deviceName` (rekordbox writes it there, and leaves it empty until a name is set), where players
  reading `export.pdb` find the name, what the colour codes 1 to 8 look like, and which files each
  player model writes during a set (one OMNIS-DUO report saw only the history tables and analysis
  files change). The back-from-a-set rule stays conservative: playlist edits made on a CDJ-3000X
  count as a real change.
- **Linux and Windows are experimental, untested on hardware** (see [Platforms](#platforms)),
  and `drive format`, `drive fsck`, `eject`, `drive rename --label` and `--check-capacity` have
  never run there. `drive fsck` needs administrator rights on Linux and Windows.
  `--check-capacity` needs them on a real disk and isn't available on Windows. `eject` on Windows
  uses Explorer's Eject, which reports nothing: locallibrary waits for the drive to disappear.

## Development

```bash
cargo test --workspace --locked     # unit + integration tests against real rekordbox export fixtures
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
```

CI expects all four to pass, and the builds without the default features to build and pass
their tests too (see [Install](#install) for the features; CI tests them weekly):

```bash
cargo test -p locallibrary --no-default-features --locked
cargo test -p locallibrary-cli --no-default-features --locked
cargo test -p locallibrary-cli --no-default-features --features account --locked
cargo clippy -p locallibrary-cli --no-default-features --features tui,account --all-targets --locked -- -D warnings
```

Code that needs a feature is behind `#[cfg(feature = "...")]`; tests that need one say so the
same way (`#![cfg(feature = "account")]` for a whole file). The RPC contract in `docs/rpc/`
describes the default build, so the test that checks it only runs there.

**Toolchain:** `rust-toolchain.toml` pins the Rust that CI and
release builds use (1.96.1, with clippy and rustfmt), and rustup installs it the first time
cargo runs in the checkout. A Rust that doesn't come from rustup (Homebrew's, say) ignores the
file, and its clippy may disagree with CI's. The oldest Rust locallibrary supports (its MSRV) is
`rust-version` in Cargo.toml, 1.89; check it with `cargo +1.89 check --workspace --locked`.
Change either on purpose: bump the pin in a commit of its own, fixing what the newer clippy
finds, and raise `rust-version` together with the msrv job in `ci.yml`.

**CI** (`.github/workflows/ci.yml`) runs the quick checks on pull requests and pushes to `main`
(not when only Markdown changed):

- format, clippy and the tests on macOS, Linux and Windows. Tests run under
  [cargo-nextest](https://nexte.st) (`cargo nextest run --workspace` locally), doctests with
  `cargo test --doc`. On macOS and Linux the tests have the `sqlcipher` CLI, and must find it
  (see the fixtures below);
- clippy on the CLI without its default features, on Linux without the ALSA headers;
- the MSRV check, and the docs build with warnings denied;
- [cargo-deny](https://embarkstudios.github.io/cargo-deny/) with `deny.toml`: RustSec advisories
  (each ignored one says why), banned crates (OpenSSL: TLS goes through rustls) and crate
  sources, and that every shipped dependency's licence is one `deny.toml` allows.

`.github/workflows/scheduled.yml` runs the slow suites every Monday, and on any branch from the
Actions tab (Scheduled, Run workflow):

- the builds without the default features (on Linux, without the ALSA headers): the tests of the
  library and the CLI with none and of the CLI with only `account`, and clippy over every
  combination of features with [cargo-hack](https://github.com/taiki-e/cargo-hack);
- the Bombadil explorations below;
- cargo-deny's advisory check, for advisories published since the last push.

Actions are pinned to commits, and Dependabot proposes updates to them and to `Cargo.lock` once a
month.

The library's integration tests are one test binary, `crates/locallibrary/tests/it/` (a module
for each area: `edit`, `sync_compare`, `props_pdb`...), so the library is linked once and every
test runs at once. Tests that need the mounted volumes, or the store drive lookups consult, name
them for their own thread (`locallibrary::testing::volumes_on_this_thread` and
`store_root_on_this_thread`), not with `$LOCALLIBRARY_VOLUMES` or `api::set_store_root`, which hold
for the process. The CLI's are one binary too, `crates/locallibrary-cli/tests/it/`, running
the `locallibrary` cargo built for them with a store of their own and `LOCALLIBRARY_VOLUMES` set,
so they never see the machine's drives.

`LOCALLIBRARY_VOLUMES` (a path list, like `PATH`) is a test switch, set together with
`LOCALLIBRARY_STORE`: drives are looked for only in the folders it names (set empty, nowhere),
never among the machine's mount points or where the store last saw them outside those folders,
and `drive disks` and `drive format` refuse to run. Every command warns when it's set. Tests sharing
a process name their folders for their own thread
(`locallibrary::testing::volumes_on_this_thread`).

```bash
cargo test -p locallibrary --test it                        # the library's integration tests
cargo test -p locallibrary --test it sync_compare::         # one module's
cargo test -p locallibrary-cli --test it rpc::              # the sidecar's, in the CLI's
```

The fixtures in `crates/locallibrary/tests/fixtures/` are real rekordbox files from
rekordcrate (MPL-2.0), including a 3,886-track export database (see the README there); the
tests also use the vendored rekordbox-pdb's exports (MIT, `vendor/rekordbox-pdb/tests/data/`).
The tests cross-check what locallibrary writes against rekordcrate's independent parser. The
OneLibrary fixtures (`onelibrary/`, rekordbox's schema with synthetic tracks) are made by the
`sqlcipher` CLI (`make.sh`), so reading is checked against an independent SQLCipher. With
`sqlcipher` installed, the tests that merge, clean, convert or edit a OneLibrary database also
have it check the result (`tests/support/sqlcipher.rs`); without it they skip that check, unless
`LOCALLIBRARY_REQUIRE_SQLCIPHER=1` is set, as in CI, which makes them fail.

Publishing is tested against a pretend data server on 127.0.0.1
(`crates/locallibrary/tests/support/mock_pds.rs`, shared with the CLI's tests): it keeps records,
checks each write's DPoP proof and hands out a DPoP nonce, and refuses on cue. The tests write the
session as files, or to an in-memory keychain, so none of them signs in to a real account,
touches the computer's keychain or goes on the network.

### Property-based tests

The `props_*` modules in `crates/locallibrary/tests/it/` (and a few unit tests in the CLI) use
[Hegel](https://github.com/hegeldev/hegel-rust): each test states something that must hold for
every input, Hegel generates hundreds of inputs and, when one fails, shrinks it to the smallest
example. They run with `cargo test` like any other test. The bigger ones are model tests: random
sequences of edits to `export.pdb`, `exportExt.pdb`, OneLibrary cue rows, analysis files, whole
drives (plan, apply, undo) and the store, checked after every step against an in-memory model and,
where it can, rekordcrate's independent parser. `props_sync` does the same for sync, push, pack
and merge, with the DJ's own files around the library and runs cancelled, or cut off as by a pulled
plug, at any progress checkpoint.

```bash
cargo test -p locallibrary --test it props_pdb::            # one module
HEGEL_SEED=7 cargo test ...                                 # repeat a run
```

To search harder, raise `test_cases` in the test's `#[hegel::test(...)]` for the run. Most tests
set one, and it wins over `HEGEL_TEST_CASES`, which only reaches the tests that don't.

A failure prints the shrunk example and a `#[hegel::reproduce_failure("…")]` line to replay it;
locally, failing examples are also kept in `.hegel/` in the crate's folder (ignored by git), under
the test's path (`it::props_pdb::...`), and tried first next time.
On CI runs are derandomised.

### Exploring the TUI and the mix report

[Bombadil](https://github.com/antithesishq/bombadil) drives the TUI in a terminal and the mix
report in headless Chrome with random keys, clicks and resizes, checking properties over time
(`bombadil/tui.ts`, `bombadil/report.ts`): the header and footer are always drawn, digits switch
views, help opens and closes, a typed line keeps its cursor in view, edits finish, the report's
counts agree and its player stays consistent. macOS and Linux only, with Node 22:

```bash
cd bombadil && npm ci
./tui.sh                    # 2 minutes; TIME_LIMIT=30m ./tui.sh for longer
./report.sh                 # needs Chrome; npx playwright install --only-shell chromium
```

`tui.sh` runs the TUI on a throwaway copy of a fixture drive with `LOCALLIBRARY_VOLUMES` pointing
at the folder that holds only that copy, an empty store, `open` stubbed out and a home and
working folder of its own, so files it writes stay in the run's folder; it lets the run write
edits to that copy and checks afterwards that the drive still reads. `report.sh` writes
synthetic audio for a fixture drive's tracks, records a "set" from four of them
(`make-mix.mjs`), has `locallibrary mix` identify it and explores the report it writes. Both
print where the trace went; add Bombadil options after the script name (`./tui.sh --seed 7`).

### Releasing

[dist](https://axodotdev.github.io/cargo-dist/) builds releases (`dist-workspace.toml`,
`.github/workflows/release.yml`) when a version tag is pushed:

1. In `CHANGELOG.md`, rename "Unreleased" to the version and date (`## [0.2.0] - 2026-11-02`)
   and start a new "Unreleased" above it. The GitHub Release takes its notes from that section.
2. Bump `version` in `[workspace.package]` in `Cargo.toml`, run `cargo check` so `Cargo.lock`
   follows, and commit.
3. Tag the commit and push the tag: `git tag v0.2.0 && git push origin v0.2.0`. A version with a
   suffix (`v0.2.0-beta.1`) is a prerelease, which leaves the Homebrew formula alone.

The workflow builds an archive for each of macOS (Apple silicon, Intel), Linux (x86_64 glibc) and
Windows (x86_64), plus `locallibrary-cli-installer.sh`, `locallibrary-cli-installer.ps1` and a
Homebrew formula, creates the GitHub Release with all of them and pushes the formula to the tap.
On pull requests that change a `Cargo.toml`, `Cargo.lock`, `dist-workspace.toml`,
`rust-toolchain.toml` or the workflow itself, it only runs `dist plan`, which checks the
configuration. `release.yml` is dist 0.33.0's generated workflow with hand-written additions
listed at its top, so `dist-workspace.toml` sets `allow-dirty = ["ci"]`. After changing the dist
configuration or upgrading dist, regenerate the file (`dist generate --mode=ci`, with
`allow-dirty` removed for the run) and carry the additions over.

Still to set up before the first release (the workflow runs without them, and says what it
skipped):

- **Third-party notices.** dist puts `LICENSE` in the archives and the Homebrew formula, but the
  binary also holds code under other licences (the dependencies, and the vendored rekordbox-pdb
  with its `template.pdb`), whose notices have to ship with it: generate a notices file at
  release (with `cargo about`, say) and include it.
- **The Homebrew tap.** Create the `cutenode/homebrew-tap` repository, and save a fine-grained
  token that can write to its contents as the `HOMEBREW_TAP_TOKEN` secret. Until then the
  formula is only attached to the release.
- **macOS signing and notarization**, with an Apple Developer Program membership. Without these
  secrets the macOS binaries ship unsigned, and macOS refuses one downloaded with a browser
  until its quarantine flag is removed (the installers and Homebrew don't set it). Signing
  (dist runs `codesign` with the hardened runtime before archiving) needs:
  - `APPLE_CERTIFICATE`: a Developer ID Application certificate and its private key, exported
    from Keychain Access as a `.p12` and base64-encoded (`base64 -i cert.p12 | pbcopy`);
  - `APPLE_CERTIFICATE_PASSWORD`: the password the `.p12` was exported with;
  - `APPLE_SIGNING_IDENTITY`: the certificate's name, `Developer ID Application: Name (TEAMID)`.

  Notarization (of signed binaries) needs an App Store Connect API key with the Developer role:
  - `APPLE_API_KEY`: the contents of its `AuthKey_<key id>.p8` file;
  - `APPLE_API_KEY_ID`: the key's ID;
  - `APPLE_API_ISSUER`: the issuer ID shown above the keys list.

  Setting only some of a group fails the release instead of shipping it half done. None of this
  has run yet: check the first signed release's log for Apple's verdict.
- Windows binaries aren't signed (SmartScreen warns about them); dist can sign them through
  Azure Artifact Signing if that's ever wanted.

## Credits

- [rekordbox-pdb-rs](https://github.com/fragmede/rekordbox-pdb-rs) (MIT): `export.pdb` reading and editing, vendored.
- [rekordcrate](https://github.com/Holzhaus/rekordcrate) and [Deep Symmetry's crate-digger](https://github.com/Deep-Symmetry/crate-digger):
  the reverse-engineered formats this builds on.
- [pyrekordbox](https://github.com/dylanljones/pyrekordbox): the OneLibrary database key and schema, and
  `master.db`'s key and tables.
- [jacquard](https://tangled.org/nonbinary.computer/jacquard) (MPL-2.0): AT Protocol OAuth, identity
  resolution and XRPC for Atmosphere account sign-in.
- [baken](https://github.com/M-Igashi/baken) (MIT): byte-level cue sections from a real rekordbox 7
  export, used to test the cue writer.
- Other DJ software's formats as their communities document them:
  [traktor-nml-utils](https://github.com/wolkenarchitekt/traktor-nml-utils) (a collection Traktor wrote),
  [serato-tags](https://github.com/Holzhaus/serato-tags) (MIT; its `Serato Markers2` test data checks the encoder),
  [serato-tools](https://github.com/bvandrc/serato-tools) and the
  [mixxx wiki](https://github.com/mixxxdj/mixxx/wiki/Serato-Database-Format) (crates), and
  [libdjinterop](https://github.com/xsco/libdjinterop) (Engine DJ's library schema).

## Licence

MIT: see [LICENSE](LICENSE). The vendored rekordbox-pdb is MIT as well
([its licence](vendor/rekordbox-pdb/LICENSE)), and the test fixtures carry their own
([fixtures README](crates/locallibrary/tests/fixtures/README.md)).
