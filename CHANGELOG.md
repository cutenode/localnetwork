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
  content-addressed snapshots in a store that holds each audio file once. Files players and
  rekordbox keep for one drive (`PIONEER/LOG`, `CDJ`, `MPJ`, `RBFLTR.DAT`...) stay with that
  drive. `store prune` removes old snapshots, keeping each library's latest, the state each drive
  was last seen holding and the newest snapshot of every history list.
- **Both rekordbox 7 databases.** Every edit updates `export.pdb` and OneLibrary's
  `exportLibrary.db` (the only one the CDJ-3000X, CDJ-1500X, OPUS-QUAD, OMNIS-DUO, XDJ-AZ and
  XDJ-AN read), and warns and changes `export.pdb` alone when `exportLibrary.db` can't be read;
  `convert` builds either from the other, and drives exported for OneLibrary only work too.
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
- **Cover art** on drives and in audio files (`art auto` given no drive fills in every mounted
  drive's, with one question), and rekordbox XML export (`xml`) to take a drive's library back into
  rekordbox.
- **Mixes.** `mix` finds which of a drive's tracks a recorded set plays, when, at what speed and
  in what key (marking the matches it's unsure of), and writes an interactive report. The report
  and `--export` (the same as data, for showing elsewhere) hold only what they show: no file
  paths, drive name or size, and tracks your profile hides read "ID - ID". The player's Mixes view
  lists the drive's recordings next to folders of sets opened from the computer.
- **DJ profile.** `profile` sums up how you play from your sets, collection and settings (with no
  drives named, the mounted drives the store knows), with a rounded public view; `account` signs
  in with an Atmosphere (AT Protocol) account, and `profile publish`, `plays publish` and
  `settings publish` write locallibrary's own records there, asking only to create and update
  them.
- **Player.** A terminal UI to browse a drive the way players show it and listen to it: search
  and filters, track details, harmonic suggestions, history and mixes. It does the drive work
  too, each change planned and confirmed as on the command line: the Drives view pulls, checks
  (alone, or a set's drives together), syncs a drive group, packs a set (from playlists or the
  crate, the CDJ's TAG LIST), brings back playlists from snapshots, replays edits and ejects;
  `u` undoes the last change, on the drive it was made to; after a rekordbox export it offers to
  replay the edits it went over; My Tags show in a column and under Browse, and `T` gives tracks
  one; rule-based playlists come from the search box. Every key is in one table that also draws
  the help (the view's own keys first) and the footer's hints, and `:` opens a command palette
  with every action by name.
- **For GUIs.** Every operation is a serde request and response in the library's `api` module,
  and `locallibrary rpc` serves them as newline-delimited JSON-RPC 2.0, to a trusted parent
  process only (see the README). Its contract is machine-readable: `rpc.describe` and
  `locallibrary rpc --emit-schema DIR` give every method's kind, pairing, aliases and JSON
  Schemas, and TypeScript types are made from them (`docs/rpc/`). Method names follow `noun.verb`
  with `<op>.plan` / `<op>.apply`; older names (`sync_many.*`, `drive.adopt`, `store.gc`,
  `drive.sets`...) still work. Errors say what a GUI can do about them: `error.data` has a
  `reason` (which decision a `conflict` needs), the `fields` it's about and `actions` ready to
  call, and bad parameters say where they are. The library's messages leave commands and flags to
  the CLI, which prints them as hints. The mix report lives in the library: `mix.identify` can
  answer each play's waveform, cues and phrases and keep the report in the store, and
  `mix.reports.list`, `.get` and `.html` find kept reports and write their pages.
- **The drive you mean.** Commands that read a drive (`tracks`, `playlist ls`, `drive info`,
  `doctor`, `tags`, `plays lists`...) read every mounted one when given none, each under its name
  (with `--json`, a list, one entry per drive), and a drive that can't be read doesn't stop the
  others. `xml` and `doctor --fix-orphans` take the only mounted drive, or the primary of the one
  library whose drives are mounted; when that can't be told, the error lists the mounted drives
  (name, mount path, role, library) and exits with status 2. A name that matches no mounted drive
  lists the ones that are, and says when it's a drive you own that isn't plugged in, and when it
  was last seen.
- **After an export.** `refresh [DRIVE]` (with none, the drive rekordbox exported over recorded
  edits on) puts back what rekordbox's export replaced, in one plan with each step only when
  needed: demo samples out, recorded edits made again, rule-based playlists filled, the name and
  colour players show (which `drive player` now remembers on the drive) and My Settings; then it
  saves the drive in the store and, with `--sync`, copies it onto its library's other mounted
  drives. `status` flags a drive rekordbox exported over recorded edits, and names it in its hint.
  Over RPC, `refresh.plan` and `refresh.apply`.
- **The night before a set.** `set [DRIVES...]` saves the drives in the store (history lists
  first), copies the newest library onto the others, checks them against your player models and
  ejects them when everything passed; a check with problems, or history lists it couldn't save,
  leaves them mounted (exit status 4). `check`, `eject` and `pull` with no drives take the mounted
  drives of one library. Over RPC, `set.plan` and `set.apply`.
- **First run.** `setup [DRIVES...]` names the drives you pick, chooses their roles (the first one
  named is the primary unless `--primary` says), starts your library with a first pull (run again,
  it saves them in the library they're in and keeps their roles), imports My Settings from the
  primary drive, and (if you like) sets the name players show, your contact details and the player
  models you play on, each shown before it's applied. It asks in a terminal, and takes flags with
  `--yes` elsewhere. Commands run before there's a store suggest it.
- **Releases.** Prebuilt binaries for macOS (Apple silicon and Intel), Linux (x86_64) and
  Windows (x86_64), with shell and PowerShell installers and a Homebrew formula.
- **Smaller builds.** Cargo features: `account` (the Atmosphere account and publishing, in the
  library and the CLI), `tui` (the player) and `playback` (its audio output, and ALSA on Linux),
  all on by default. `--no-default-features` builds a headless sidecar of about 150 crates
  instead of 430, with no ALSA; without `account`, the account and publishing commands and RPC
  methods aren't there and `initialize` doesn't list the `account` capability.
- **Shell completion and man pages.** `locallibrary completions SHELL` for bash, zsh, fish,
  elvish and PowerShell: the shell asks `locallibrary` on Tab, so mounted drives, their playlist
  paths, My Tags and history lists, the store's libraries, player models and `refresh --skip`
  steps complete too (`--static` for commands and flags only).
  `locallibrary man -o DIR` writes a man page for every command.

### Changed

For anyone who used a development build. Commands and flags that were renamed still work under
their old names.

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
  `stats`, `export`, `publish`), the playlist tree is `playlist ls`, `sync --from` replaces `mirror`
  and `diff --summary` replaces `compare`. The old names still work and aren't listed. `drive`,
  `library`, `plays`, `playlist`, `track`, `settings`, `art`, `edits` and `smart` with no subcommand
  list or show (`drive Main` is `drive info Main`).
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
- `--json` has one shape for every command that writes: `{schema, plan, report, applied,
  outcome}` (sync printed `{preview, result}` or `{preview, results}`, `drive verify --repair`
  `{report, repaired}`, publishing `{plan, published}`, and changes made in one step, such as
  `pull` or `drive role`, only their result). Every top-level JSON object has `"schema": 1`,
  `status --json` is always `{mounted, offline, duplicates}`, `smart apply --json` prints one
  document for every drive, and errors are `{"error": {"kind", "reason", "fields", "actions",
  "message"}, "outcome"}` with the sidecar's kinds and detail (`code` is still there). A
  `provision` that stops part way exits with status 5.
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
- Crafted files can't make a read or an edit allocate without bound: a OneLibrary or `master.db`
  WAL's pages are authenticated before its commit size is believed, and `export.pdb` free-page
  fields and `exportExt.pdb` table counts are checked. A playlist tree that repeats an id or loops
  reads each playlist once instead of crashing or hanging.
