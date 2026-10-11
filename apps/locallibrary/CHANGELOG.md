# Changelog

All notable changes to locallibrary are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). The release workflow takes a version's notes from
its section here, so rename "Unreleased" to the version (`## [0.1.0] - YYYY-MM-DD`) in the
commit that gets tagged.

## [Unreleased]

The first release: a Rust library (`locallibrary`) and a command line tool for the rekordbox
libraries on USB drives, with a terminal player and a JSON-RPC sidecar for desktop apps. Linux and
Windows builds are experimental and untested on hardware (see Platforms in the README).

### Added

- **Drive sync and history.** `drives`, `adopt` and `status`; `compare` says which drive is
  newer and why, from each drive's lineage or from the evidence; `sync`, `mirror` and `push`
  copy only what changed, to several drives at once, crash-safely; `pull` keeps
  content-addressed snapshots in a store that holds each audio file once, and `watch --pull`
  saves drives as they're plugged in (`watch --install`, which `setup` offers, runs it whenever
  you log in with `--sync-group`, so the library's other drives plugged in follow, as a launchd
  agent or systemd user service). Files players and
  rekordbox keep for one drive (`PIONEER/LOG`, `CDJ`, `MPJ`, `RBFLTR.DAT`...) stay with that
  drive. `store prune` removes old snapshots, keeping each library's latest, the state each drive
  was last seen holding and the newest snapshot of every history list.
- **Both rekordbox 7 databases.** Every edit updates `export.pdb` and OneLibrary's
  `exportLibrary.db` (the only one the CDJ-3000X, CDJ-1500X, OPUS-QUAD, OMNIS-DUO, XDJ-AZ and
  XDJ-AN read), and warns and changes `export.pdb` alone when `exportLibrary.db` can't be read;
  `convert` builds either from the other, and drives exported for OneLibrary only work too.
- **Exporting without rekordbox.** `collection export` writes rekordbox's collection to a drive or
  a folder as rekordbox's Export does (tracks, playlists, analysis, cues, artwork and My Tags), so
  a library can be had without a drive rekordbox exported to; tracks Cloud Library Sync keeps in
  Dropbox or Google Drive come along, online-only ones downloaded as they're copied.
- **Editing on the drive.** Playlists and folders (create, rename, move, delete, fill, sort,
  order for mixing, import), track details, ratings and colours, My Tags, hot cues, memory cues
  and loops, replacing duplicates and deleting tracks, with a backup before every change and a
  journal so `edits replay` can redo them after rekordbox exports again. `undo` takes the last
  change back (run again, the one before) and is refused when the drive changed since (a
  rekordbox export, a sync, history players recorded) unless `--force`; putting a backup back
  backs up what it replaces, so an undo can be redone. `backups` and `restore` reach older ones,
  and each drive keeps its newest 10 backups of each kind, so small edits never prune the only
  copy of a deleted track.
- **Plans before changes.** Every command that changes or deletes something (on a drive, in the
  store, in your Atmosphere account or on this computer) shows its plan and asks first, then makes
  exactly the change it showed, or nothing if what it read changed in between (`stale_plan`).
  `pull` (which only adds a snapshot) and `eject` don't ask, and files on this computer are never
  replaced without `--replace`. Archive and retired drives need `--overwrite-archive`. Every
  in-place write goes through one routine that leaves the drive as it was if it fails before
  swapping the new files in; Ctrl-C and SIGTERM stop at a safe point, and nothing else changes a
  drive left mid-operation, or reads a library from it, until that's finished or undone. Exit
  statuses say what happened (3 not applied, 4 problems found, 5 some failed), and `--json`
  output carries the outcome. Over RPC, `*.plan` returns a `plan_id` that `*.apply` applies, and a
  plan refused before anything was written is kept for a retry.
- **Libraries across drives.** `merge` brings another library's tracks, playlists, cues,
  artwork, My Tags and (optionally) history across; `pack` writes a drive with only some
  playlists; rule-based playlists (`smart`) live in the store and are written to every drive.
- **Drive health.** `doctor` (duplicates, missing and orphaned files, database drift, files too
  big for FAT32), `check` before a set, `clean` for rekordbox's demo tracks, `verify` and `fsck`,
  `space`, `tidy`, `recordings`, and the history lists players recorded, kept even after
  rekordbox removes them.
- **Drive management.** A registry of every drive you own (`drive ...`): names, roles, a
  logbook, copies, recognising a drive after it was reformatted (by its contents, or its disk's
  serial number, which on macOS is a USB disk's), `provision`, `eject` and `firmware`.
- **Formatting.** `format` writes MBR and FAT32 (or exFAT, HFS+) the way players read it,
  including FAT32 above 32 GB, with an optional capacity check; it has only run on macOS (Linux and
  Windows are experimental and untested on hardware). `disks` shows how each connected drive will
  fare on CDJs and XDJs.
- **Settings.** One "My Settings" profile for every drive's player and mixer settings, with
  per-drive overrides. `settings publish` puts the profile's base on your Atmosphere account, and
  `settings import --from-account` (or `--from-handle`, for someone else's) adds published
  settings to yours; `--replace` makes them the whole base.
- **Cover art** on drives and in audio files (`art auto` given no drive fills in each library
  plugged in, with one question), and rekordbox XML export (`xml`) to take a drive's library back
  into rekordbox.
- **Mixes.** `mix` finds which of a drive's tracks a recorded set plays, when, at what speed and
  in what key (marking the matches it's unsure of), and writes an interactive report. The report
  and `--export` (the same as data, for showing elsewhere) hold only what they show: no file
  paths, drive name or size, and tracks your profile hides read "ID - ID". The player's Mixes view
  lists the drive's recordings next to folders of sets opened from the computer.
- **DJ profile.** `profile` sums up how you play from your sets, collection and settings (with no
  drives named, the mounted drives the store knows), with a rounded public view; `account` signs
  in with an Atmosphere (AT Protocol) account, and `profile publish`, `sets publish` and
  `settings publish` write their records there, asking only to create and update them. Signing
  in and publishing are alpha.
- **Sets and sessions.** A history list a player recorded (or a Beat Link Trigger log, `plays
  import`) is a *session*: raw, private, never counted or published as a set. `plays` lists sessions
  with an id that's the same on every drive and in the store (any list name argument takes one), the
  window each was recorded in (players have no clock: after locallibrary last saw the drive without
  it, before it first saw it with it), and the set made from it. A *set* is what the DJ declares in
  `sets`: `suggest` lists the sessions that look like sets with the evidence (a night's together,
  when their windows are narrow enough to tell), `new` builds one from sessions and/or a recording
  (its timed tracklist wins), `edit` changes its details (kind, venue, city, country, event, who it
  was played with, a link, notes) and tracklist by position, and `export` (text, Mixcloud, CUE,
  YouTube, 1001Tracklists, CSV, M3U8), `played`, `playlist` and `publish` use it. Sets live in the
  store's `sets.json`. Saving a drive that came back with new sessions (`pull`, `sync`, `prep`,
  `watch`) says how many. The profile counts declared sets, with sessions standing in (and never
  published) until there are some, and never names a track a set marks unnamed. `sets publish`
  replaces `plays publish`: a set made from a single list that `plays publish` published takes that
  list's key and first-published time the first time it's published. Sets and the profile are
  `computer.localnetwork.dj.set` and `computer.localnetwork.dj.profile`, record types Local
  Network's tools share (with `computer.localnetwork.dj.defs`), and My Settings stay
  `computer.localnetwork.locallibrary.settings`. The set Lexicon gains `kind`, `location` (an open
  union, a Lexicon Community address for now), `event`, `playedWith`, `links` and `sources` (for
  `source`), and the profile's `basis` can be `sessions`. The night-before job `set` is `prep` (`set` is a spelling of `sets`
  now; over RPC `prep.plan/apply`, and `set.*` still works).
- **Player.** A terminal UI to browse a drive the way players show it and listen to it: search
  and filters, track details, harmonic suggestions, history and mixes. It does the drive work
  too, each change planned and confirmed as on the command line: the Drives view lists each library
  with its drives under it, opens a library at its newest drive, and pulls, checks (alone, or a
  set's drives together), syncs a library's drives, packs a set (from playlists or the
  crate, the CDJ's TAG LIST), brings back playlists from snapshots, replays edits and ejects;
  `u` undoes the last change, on the drive it was made to; after a rekordbox export it offers to
  replay the edits it went over; My Tags show in a column and under Browse, and `T` gives tracks
  one; rule-based playlists come from the search box; `N` and `P` jump to the next and previous
  phrase, quantized like cue jumps; in full screen, `enter` queues a harmonic suggestion to play
  next, and `S` turns autoplay off. Every key is in one table that also draws
  the help (the view's own keys first) and the footer's hints, and `:` opens a command palette
  with every action by name.
- **For GUIs.** Every operation is a serde request and response in the library's `api` module,
  and `locallibrary rpc` serves them as newline-delimited JSON-RPC 2.0, to a trusted parent
  process only (see docs/sidecar.md). Its contract is machine-readable: `rpc.describe` and
  `locallibrary rpc --emit-schema DIR` give every method's kind, pairing, aliases and JSON
  Schemas, and TypeScript types are made from them (`docs/rpc/`). Method names follow `noun.verb`
  with `<op>.plan` / `<op>.apply`; older names (`sync_many.*`, `drive.adopt`, `store.gc`,
  `drive.sets`...) still work. Errors say what a GUI can do about them: `error.data` has a
  `reason` (which decision a `conflict` needs), the `fields` it's about and `actions` ready to
  call, and bad parameters say where they are. The library's messages leave commands and flags to
  the CLI, which prints them as hints. The mix report lives in the library: `mix.identify` can
  answer each play's waveform, cues and phrases and keep the report in the store, and
  `mix.reports.list`, `.get` and `.html` find kept reports and write their pages.
- **The library is the unit.** Each drive holding a library is a clone of it, and its name doesn't
  matter much: a library's name works where a drive does (but in commands about one particular
  drive) and means its lead clone, the newest of its drives plugged in (the primary when they're
  identical). A change to a library (playlist, track, My Tag, cue and artwork edits, `clean`,
  `convert`, `merge`, `smart apply`, `edits replay`, `doctor --fix-orphans`, `undo`...) is one plan,
  made on the lead clone and then copied onto its other drives plugged in ("then copied onto BACKUP
  and SD"; `clones` in the plan's JSON); a drive that's newer, diverged or back from a set is left
  alone with a warning, and archive and retired drives aren't written. Recorded edits travel with
  the library on a sync, so `refresh` replays edits made on any of its drives. Reads (`tracks`,
  `playlist ls`, `doctor`, `tags`...) given nothing show each library plugged in once, and reads of
  what each drive keeps (`drive info`, `drive space`, `settings show`, `plays lists`...) show every
  drive, under its name (with `--json`, a list), and one that can't be read doesn't stop the others.
  `sync` with no drives syncs every library plugged in; it and `check`, `eject`, `pull` and `prep`
  take a library's name for its drives, and `settings apply LIB` writes each of them. `status` and
  `drive ls` show each library with its drives under it (`status` also those not plugged in, and
  which are back from a set), then the drives of no library. `xml`, `mix`, `sets export` and `doctor
  --fix-orphans` take the only mounted drive, or the lead clone of the one library plugged in; when
  that can't be told, the error lists the mounted drives (name, mount path, role, library) and exits
  with status 2. A name that matches no mounted drive lists the ones that are, and says when it's a
  drive you own that isn't plugged in, and when it was last seen. A rule-based playlist limited to a
  drive (`smart add --drive`, which takes a library's name too) is limited to its library, so every
  clone gets it. Tab offers library names wherever one is taken.
- **After an export.** `refresh [DRIVE]` (with none, the drive rekordbox exported over recorded
  edits on) puts back what rekordbox's export replaced, in one plan with each step only when
  needed: demo samples out, recorded edits made again, rule-based playlists filled, the name and
  colour players show (which `drive player` now remembers on the drive) and My Settings; then it
  saves the drive in the store and, with `--sync`, copies it onto its library's other mounted
  drives. `status` flags a drive rekordbox exported over recorded edits, and names it in its hint.
  Over RPC, `refresh.plan` and `refresh.apply`.
- **The night before a set.** `prep [DRIVES...]` saves the drives in the store (history lists
  first), copies the newest library onto the others, checks them against your player models and
  ejects them when everything passed; a check with problems, or history lists it couldn't save,
  leaves them mounted (exit status 4). `check`, `eject` and `pull` with no drives take the mounted
  drives of one library. Over RPC, `prep.plan` and `prep.apply`.
- **First run.** `setup [DRIVES...]` asks which drives are copies of your library and what to call
  it (the library they're in, else "My library"; drives keep their volume names unless `--name`),
  chooses their roles (the first one named is the primary unless `--primary` says), starts your
  library with a first pull (run again, it saves them in the library they're in and keeps their
  roles), imports My Settings from the primary drive, and (if you like) sets the name players show,
  your contact details and the player models you play on, each shown before it's applied. It asks in
  a terminal, and takes flags with `--yes` elsewhere. Commands run before there's a store suggest
  it.
- **Releases.** Prebuilt binaries for macOS (Apple silicon and Intel), Linux (x86_64) and
  Windows (x86_64), with shell and PowerShell installers and a Homebrew formula.
- **Smaller builds.** Cargo features: `account` (the Atmosphere account and publishing, in the
  library and the CLI), `tui` (the player) and `playback` (its audio output, and ALSA on Linux),
  all on by default. `--no-default-features` builds a headless sidecar of about 150 crates
  instead of 430, with no ALSA; without `account`, the account and publishing commands and RPC
  methods aren't there and `initialize` doesn't list the `account` capability.
- **Shell completion and man pages.** `locallibrary completions SHELL` for bash, zsh, fish,
  elvish and PowerShell: the shell asks `locallibrary` on Tab, so mounted drives, their playlist
  paths, My Tags and history lists, the store's libraries, sets and sessions, player models and
  `refresh --skip` steps complete too (`--static` for commands and flags only).
  `locallibrary man -o DIR` writes a man page for every command.
- **Crates for other DJ tools.** Parts of the library that other tools can use without
  locallibrary are crates of their own in `crates/`, each with a README and an example, and
  JSON Schema behind a `schemars` feature in the ones whose types are serialisable. locallibrary re-exports them
  where they used to live (`model`, `harmony`, `progress`, `anlz`, `settings`, `equipment`,
  `format::FsKind`, `import`, `mix`, `interop`), so the CLI, the RPC contract, the files written
  and the error messages are unchanged.
  - `loclib-progress`: progress events and cooperative cancellation (`Ctx`, `CancelToken`,
    `Event`, `Cancelled`).
  - `loclib-model`: tracks, playlists and the diff between two exports, Camelot keys and mix
    costs, a track's decoded analysis (beat grid, cues, waveforms, phrases, the hot cue palette)
    and the path keys tracks are matched by.
  - `loclib-equipment`: what each Pioneer DJ / AlphaTheta player, all-in-one system and mixer
    reads (databases, file systems, audio formats), with each value's source, and the same
    table as `equipment.json` for other languages.
  - `loclib-mysettings`: reading and writing `MYSETTING.DAT`, `MYSETTING2.DAT`,
    `DJMMYSETTING.DAT` and `DEVSETTING.DAT`, with every known setting's schema and edits that
    patch single bytes and recompute the checksum.
  - `loclib-tracklist`: reading M3U, CSV and other tables, rekordbox's text export and XML, and
    plain "Artist - Title" text, and finding the tracks in a library by path, ISRC, artist and
    title, then fuzzily.
  - `loclib-mixid`: which known tracks a recording plays, when, at what speed and with or without
    key lock, from any audio files (decoding, landmark fingerprints, the index and the matcher).
    locallibrary keeps the drive, the fingerprint cache, Audacity projects and the report.
  - `loclib-interop`: writing a library as Traktor's `collection.nml`, Serato crates (with the
    `Serato Markers2` cue tag in copies of the audio) and a new Engine DJ library, a feature per
    format, so a build for NML alone needs no SQLite.
  - `loclib-waveform`: the player's ratatui widgets (overview and detail waveforms, cue lane,
    phrase strip, spectrum, scope, cover art in half blocks), in the colours of a palette the
    caller passes.

### Changed

For anyone who used a development build. Commands and flags that were renamed still work under
their old names.

- Reading a track's analysis from a drive is `anlz::load` and `anlz::load_grid` in the library
  (they were `TrackAnalysis::load` and `TrackAnalysis::load_grid`), now that `TrackAnalysis` is
  loclib-model's.

- "Set" now means a DJ set. The drives of one library that `sync` syncs together are a drive
  *group*: `drive sets` is `drive groups`, `watch --sync-set` is `watch --sync-group`, and over
  RPC `drive.sets` and `sync_set.plan/apply` are `drive.groups` and `sync_group.plan/apply`.
- The `gig` drive role is `set` (`drive role DRIVE set`). `gig` is still accepted, and a registry
  that says `gig` reads as `set`. A sync plan's `after_gig` is `after_set`, with
  `back_from_set` and `left_is_back_from_set`.
- The commands are grouped by what they're for, and `locallibrary --help` lists them under headings
  (Everyday, Drives, Library and store, Editing, Plays and players, Other) with common tasks. Drive
  commands are under `drive` (`drive ls`, `info`, `adopt`, `disks`, `format`, `verify`, `fsck`,
  `space`, `tidy`, `recordings`, `firmware`, `backups`, `restore`), snapshot history under `library`
  (`library log`, `push`, `prune`), history lists and play counts under `plays` (`plays lists`,
  `stats`, `export`, `import`), the playlist tree is `playlist ls`, `sync --from` replaces `mirror`
  and `diff --summary` replaces `compare`. The old names still work and aren't listed. `drive`,
  `library`, `plays`, `playlist`, `track`, `settings`, `art`, `edits` and `smart` with no subcommand
  list or show (`drive REKORDBOX` is `drive info REKORDBOX`).
- Names that clashed are told apart: `playlist delete` deletes a playlist (`playlist rm` before) and
  `playlist remove` takes tracks out of one; `playlist move` moves a playlist (`playlist mv`) and
  `playlist reorder` moves tracks within one (`playlist move --at --to` still does); `playlist
  order` orders one for mixing (`playlist plan`); `playlist sort --sort` (`--by`); `tags untag`
  takes a tag off tracks (`tags remove`) and `tags delete` deletes it; `settings base` is the
  settings profile (`settings profile`), apart from the DJ `profile`. `plays stats --since` takes a
  date as well as a snapshot, like `profile --since`. A mistyped command suggests the current
  name, not an old one, and a group run with a mistyped subcommand says which it meant.
- `tracks` finds tracks the way RPC `tracks.query` does: `--search` is free text in the filter
  (its words each match artist, title, album, genre, label, key, comment or path), and `--json`
  gives `{total, total_duration_secs, tracks}`.
- Messages say what to run: a sync that can't tell which drive is newer names `sync --from` with
  the drives, a drive left mid-operation names the command that finishes it, `doctor` points
  duplicates to `track dedupe`, unfiled tracks to `playlist unfiled` and missing artwork to `art
  auto`, and a freshly formatted drive to `provision`. Every `<DRIVE>` argument and flag says what
  it takes, `--json` mentions `--yes` only on commands that write, and the commands people run
  most have examples in their `--help`.
- Output can be piped: what a command reports (lists, plans, the problems a check finds) goes to
  stdout and only progress, questions and errors to stderr; long listings go through a pager in
  a terminal (`$PAGER`, else `less`; `--no-pager`); output piped into `head` stops quietly.
  `tracks` has a header row, `--columns` and `--format table|tsv|csv`; columns are measured as
  the terminal shows them (wide characters take two). `--all` lifts the "... and N more" caps of
  `doctor`, `drive verify`, `diff` and `plays stats`. `-q` also leaves out tips; `--color
  auto|always|never` and `--ascii` (the default when the locale isn't UTF-8). Progress bars
  count in GB like plans do, and show the time left.
- `--json` has one shape for every command that writes: `{schema, plan, report, applied, outcome}`
  (sync printed `{preview, result}` or `{preview, results}`, `drive verify --repair` `{report,
  repaired}`, publishing `{plan, published}`, and changes made in one step, such as `pull` or `drive
  role`, only their result). Every top-level JSON object has `"schema": 1`, `status --json` is
  always `{mounted, offline, duplicates, libraries}`, `smart apply --json` prints one document for
  every drive, and errors are `{"error": {"kind", "reason", "fields", "actions", "message"},
  "outcome"}` with the sidecar's kinds and detail (`code` is still there). A `provision` that stops
  part way exits with status 5.
- `library log --json` nests the libraries under `libraries`, and `profile document --json`
  carries `schema`. Incomplete or mistyped command lines (`library WORD` included) exit with
  status 2 and kind `usage` (they exited 1 with `invalid`), and under `--json` a command line that
  doesn't parse prints a JSON error too.
- `account client-metadata`, for locallibrary's developers, is left out of `--help`.
- More commands ask first: `smart add/rm`, `drive role/forget/retire/rename`, `settings
  set/unset`, `library rename` and `store label/unlabel` show a one-line plan (`--yes` skips it;
  under `--json` without it nothing changes), and `drive reidentify` shows a plan and needs
  evidence that the drive is a copy (or `--force`). `export`, `xml`, `art get -o`, `plays export`,
  `plays lists -o`, `profile document -o`, `mix --html`/`--export` and `account client-metadata
  -o` refuse to replace a file without `--replace`, and the player asks first. `smart apply` and
  `settings apply` with no drives leave archive and retired drives out.
- `undo` refuses to put a backup back over a drive that changed since its change (a rekordbox
  export, a sync or push, history players recorded) unless `--force`, and its plan lists the
  history lists it drops. Putting a backup back backs up what it replaces first (put that back to
  redo), and `undo` run again steps back to the change before instead of putting the same backup
  back. Backups are numbered in the order they're made on the drive and keep files' modification
  times, and each drive keeps the newest 10 of each kind: backups holding removed tracks' audio
  and backups undos took are counted apart (`drive backups` shows which hold audio).
- Files players and rekordbox keep for one drive (`PIONEER/LOG`, `CDJ`, `MPJ`, `TRASHBOX`,
  `RBFLTR.DAT`, `playlists3*.sync`) are no longer copied, deleted or counted in the library's
  fingerprint; drives recorded before still match. Without lineage, `sync` no longer takes the
  drive with more tracks as newer on that alone, and never the side with fewer tracks: say which
  way to copy. When it can't tell, the refusal says why, with reason `uncertain` (`diverged` is
  for drives that both changed). A sync, push or pack that replaces a drive's library forgets the
  edits recorded for it, and its plan says so. An interrupted operation blocks edits on the drive,
  and `pull`, `merge`, a sync or a pack from it, until it's finished (sync or pack onto it again;
  for a change in place that made no backup, the next change) or taken back (`undo`); `check` says
  which.
- `edits.json` has a format version, and removing and moving tracks and deleting a playlist with
  its tracks are recorded as what they changed, so a replay keeps entries a later export added.
  Builds from before the version read the file as a plain list: they change the drive, then fail
  to record the edit. Update every copy (the CLI and an app's sidecar) before editing a drive this
  version has edited.
- Rules, `playlist fill`, `playlist unfiled` and `playlist import --into` fill the playlist at
  exactly their path, never one of that name in another folder, and a path several playlists
  match ignoring case is refused, naming them. In filters, a word before a colon that names no
  field (`gnere:house`) is refused with a suggestion (a quoted colon, or one in a link, is text),
  and `playlist:` takes a path or a unique name, as `--playlist` does, refusing one that names
  nothing. A bare drive word is a drive's name before a folder in the working directory; one
  that's both is refused.
- `track dedupe` without `--metadata` never replaces a track that only shares artist, title and
  length with another, and with nothing to replace it reports nothing to do. `playlist import`
  offers a title-only entry whose length is far off instead of matching it, keeps a track the set
  played twice, and reports picks made with `--choose` as `chosen`.
- `store prune` applies the plan it showed, refusing if a pull, label or drive change came in
  between (RPC `store.prune.plan` and `store.prune.apply`; `store.prune` still works), and its dry
  run reports exactly what it frees. `store gc` removes only snapshots of no library and files
  nothing uses, and refuses when it can't read a snapshot. `store copy` refuses, without
  `--force`, a copy that was written to with `--store` since the last copy.
- `format <name>` takes only an adopted drive's name (or a path or disk identifier), never a
  volume label or a folder in the working directory (give `./NAME`), and the typed confirmation is
  that name or the disk identifier. `format` and
  `provision --format` refuse archive and retired drives without `--overwrite-archive`, look the
  disk up again just before erasing, and under `sudo` ask for `--store` when the default store
  isn't the invoking user's. `--check-capacity` wipes the partition table before testing. `eject`
  refuses a disk that also holds the store or locallibrary, and names the other volumes it
  unmounts.
- `LOCALLIBRARY_VOLUMES` is a test switch: it also hides drives the store knows outside its
  folders, `drive disks` and `format` refuse while it's set, it lists each volume once without
  the boot volume, and every command warns when it's set.
- The sidecar refuses relative paths to write to (`art.embed.*` files included), `mix.export`
  refuses to replace a file without `replace: true`, `drive.reidentify` is a `.plan`/`.apply` pair
  (the old name still works), a second SIGINT or SIGTERM waits for a database swap under way,
  `settings.import_published` takes only an `https://` `pds` (or `http://` on this machine),
  `drive.fsck` is described as destructive, and a request line over 16 MiB is answered with a
  parse error while serving goes on.
- `mix --export` writes format version 2: no `library` (drive name and size), a cover only when
  it's embedded in the recording, and tracks the profile hides as "ID - ID"; the HTML report
  leaves out the same, and a page written with `mix --html` names the recording by its file name
  alone. Kept reports, pages and exports are named `<title>_<hash>`, and kept reports find their
  drive by its identity. Without drives named, `profile` reads only the mounted drives the store
  knows. Library tracks over 20 minutes aren't fingerprinted for mix matching. Over RPC,
  `mix.index`, `mix.identify` and `mix.export` are immediate (writing) methods.
- Each store folder has its own keychain item, so a session kept in the keychain needs one sign-in
  again, and a `--session-file` keeps each account's secrets apart. Publishing asks only to create
  and update locallibrary's records (`repo:<nsid>?action=create&action=update`, where it asked
  for delete too), so the hosted client document has to be regenerated with those scopes. Sign-in
  stops when that document is online but doesn't fit, instead of quietly using the development
  client, and `account` and the player say when a session is the development client's. `settings
  import --from-handle` and `--from-account` (and `settings.import_published.*`) add the
  published settings to the base; `--replace` (`replace: true`) replaces it. TLS also trusts the
  system's root certificates.
- In the player, `u` undoes the last change on the drive it was made to (after a pack, the drive
  packed onto), in every view, and confirmations say what they do ("Eject DEMO?", "Pack a set onto
  SET?", "Undo the last change on DEMO?") instead of "Write to the drive?".
- ISRCs in `export.pdb` are read and written in rekordbox's form; before, they read as CJK
  characters, and `convert` and `merge` carried those into OneLibrary. Artist and album rows for
  non-ASCII (and very long) names put the name 4-byte aligned, where rekordbox puts it.
- For the library's users: `api::format_plan`, `format_apply` and `list_disks` take the store's
  folder, and refuse the disk holding it whether or not the store opens. Before, only the store
  `set_store_root` named was kept safe, so a program that never called it could format the disk
  its store was on. `set_store_root` is now only the default for finding drives.
- For the library's users: the enums that grow are `#[non_exhaustive]`, so a new variant isn't a
  breaking change: `Error` and `error::Reason`, progress `Event` and `Phase`, the sidecar's
  `Notice` and `JobEvent`, `WatchEvent` and `DriveChange`, doctor's `Problem`, `AudioProblem`,
  `Unplayable`, `CopyReason`, `fleet::Check`, `SpaceKind`, `TidyKind`, `Suggestion`,
  `interop::Software`, `StepKind` and the import and export formats. A `match` on them needs a `_`
  arm.
- `pack` keeps the target's own player name and colour instead of stamping the source's on it, a
  cut-off `sync` or `pack` puts them back when it's finished, and undoing `drive player` no longer
  comes back on `refresh`. `pack` counts its backup in the space check and prints the snapshot it
  saved and its backup id without `--json`.
- Merging from an `export.pdb`-only drive into OneLibrary gives the merged tracks their cues.
- Hot cues D-H are `djmdCue.Kind` 5-9 in rekordbox's `master.db` (4 is a memory loop): before,
  `collection` read memory loops as hot cue D and shifted D-H by one. OneLibrary's `cue.kind` is
  written the same way.
- `mix` no longer reports riding the pitch fader as a loop or skip.
- `status` and the player's replay offer no longer take a player's writes for a rekordbox export
  on drives with both databases, and replaying field edits already in place changes nothing.
- `library log` shows snapshots saved beside the latest (drives back from a set) as `beside`;
  `store info` counts the copies `art embed` keeps; `watch --once` says when the store is in use
  and gives up after a minute; `watch --json` lines carry `schema`; `drive verify` and `drive
  recordings` default to the mounted drive.
- The "untested on this platform" warning for disk commands is part of their plans, so the
  sidecar and the player show it too. On Windows, the drive watcher and the sidecar cache tell
  files apart by volume serial and file index.
- The player quits cleanly on SIGINT, SIGTERM and SIGHUP, and space while a track is opening
  decides whether it starts paused. A refused sign-in says why. The Traktor export uses the Mac's
  real startup disk name.

### Security

- Sign-in acts only on the browser redirect that carries its own `state`: other requests to its
  `127.0.0.1` port get a 400, and request heads are read up to 64 KiB.
- Servers found from a handle or DID (signing in, `settings import --from-handle`) are only
  contacted at public `https://` addresses, redirects only go to public `https://` addresses, and
  a published record is read up to 1 MiB.
- Text from servers and the browser has control characters removed before it reaches the terminal
  or an error.
- The sign-in page, `mix --open` and the player's mix report open only `https://` addresses and
  files that exist, without a shell (on Windows, `rundll32` instead of `cmd /C start`).
- A drive's `exportLibrary.db` with a view or trigger is refused, its schema runs with
  `trusted_schema` off and SQLite's defensive mode, and column names from it are quoted safely.
  Audacity projects open with `trusted_schema` off too, and are refused when they hold a view.
  `exportLibrary.db` and `master.db` are refused when SQLite's `quick_check` finds them damaged,
  `master.db` gets the same defensive mode and `trusted_schema` off, and SQLite checks every b-tree
  cell's size as it reads any of them.
- A page of zeros in an encrypted database no longer passes as a valid empty page: it's reported
  as damage, as SQLCipher does, unless SQLite would never read it (the WAL replaces it, or it's
  the lock-byte page at 1 GiB).
- Crafted files can't make a read or an edit allocate without bound: a OneLibrary or `master.db`
  WAL's pages are authenticated before its commit size is believed, and `export.pdb` free-page
  fields and `exportExt.pdb` table counts are checked. A playlist tree that repeats an id or loops
  reads each playlist once instead of crashing or hanging.
