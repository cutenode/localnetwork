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
  Players write only the database they read, so neither loses what only it has: converting into
  a drive that has the database adds what it lacks (tracks, playlists and their entries, history
  lists, My Tags) where rekordbox's "Convert from Device Library" overwrites it, `--replace`
  names what a rebuild drops and keeps the player name, a playlist edit is made in
  OneLibrary's own entries, so what a CDJ-3000X added, took out or reordered there stays, and a drive back from a set is judged by what changed in
  both databases, so a sync doesn't take a CDJ-3000X's playlist edits for player-only changes and
  copy over them. `status`, `diff`, sync and undo previews, `drive restore --playlist`, `edits
  replay` and exports for other software look at OneLibrary too, and `pack` keeps a drive whose
  OneLibrary can't be read in the store before packing over it, and refuses a source whose
  OneLibrary can't be read rather than leave the packed drive without one.
- **Between computers.** `store serve` prints a code and `store fetch <code>` on another
  computer copies the snapshots and files its store lacks over the network (iroh, directly or
  through relays, or `--local` for the local network alone), checking each file's hash; libraries
  it lacks are added and ones it has move to the other's newer snapshot.
- **Exporting without rekordbox.** `collection export` writes rekordbox's collection to a drive or
  a folder as rekordbox's Export does (tracks, playlists, analysis, cues, artwork and My Tags), so
  a library can be had without a drive rekordbox exported to; tracks Cloud Library Sync keeps in
  Dropbox or Google Drive come along, online-only ones downloaded as they're copied.
- **Editing on the drive.** Playlists and folders (create, rename, move, delete, fill, sort,
  order for mixing, import), track details (BPM, album artist, ISRC and release date too),
  ratings and colours, My Tags, hot cues, memory cues and loops, cover art, replacing duplicates
  and deleting tracks, with a backup before every change and a journal so `edits replay` can redo
  them after rekordbox exports again. One plan can mix them all, under one backup. `undo` takes the last
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
- **A second opinion on tempo, key and level.** `doctor --analyze` estimates each track's tempo
  and key from its audio (stratum-dsp, through the new loclib-analysis crate, in the sound
  plugin) and its loudness, keeps the results in the store, and lists stored tempos that look
  half, double or 3/2 of the estimate (with which one the library's other tempos favour), stored
  keys heard as a fifth, the relative or the parallel key, and tracks over 0 dBTP, clipped, or
  far from the library's level (a track whose best keys tie has no key estimate, rather than one
  picked at random). `check` warns about those levels from what was measured, and
  `playlist order` trusts keys the analysis questions less. `analyze --apply key,bpm` writes the
  estimates to the tracks, as an edit asked about first. `analysis bench` scores rekordbox's keys
  and tempos, and the analysis, against the GiantSteps datasets (MIREX's weighted key score,
  tempo Accuracy1 and Accuracy2).
- **Duplicates by sound.** `doctor --by-sound` and `track dedupe --by-sound` find the same
  recording in another file or encoding (a FLAC and an MP3 made from it) from the fingerprints
  `mix` keeps in the store, so a library is decoded once; tracks of different lengths, such as an
  extended mix and its radio edit, are never copies. The suggested keeper has the best audio
  (lossless, then bitrate, sample rate and bit depth), then the most cues, playlists and history
  lists, and `doctor` says what decided it. Replacing a copy carries its hot cues, memory cues and
  loops onto a keeper that has none, moved by the offset between their audio (an MP3's encoder
  delay is about 25 ms), measured to a fraction of a sample by cross-correlating the two files'
  audio. A copy whose cues can't be carried, or that has cues the keeper lacks, stays, with a
  warning saying why.
- **Missing files.** `track relocate` finds the audio of tracks whose file is missing, on the drive
  and in folders given with `--look-in`, by name, narrowing several by size and then length, and
  changes a track only when exactly one file fits, of the track's length when that's known
  (rekordbox's Relocate All takes the first). A
  file on the drive is used where it is, in both databases and the analysis files; one elsewhere is
  copied to where the track expects it.
- **Player-safe copies.** `pack --transcode` packs lossless tracks the drive's players can't play
  (a 24/96 FLAC on a CDJ-2000NXS, a 96 kHz WAV on an OMNIS-DUO) as copies they can: AIFF by default
  (`--container wav|flac`), at the highest rate and depth they take in the same family (96 kHz
  becomes 48), dithered, starting at the master's first sample so cues and beat grids stay
  where they were. The copy keeps the track's row, playlists, history, My Tags and analysis, with
  its path, format, size, rate, depth and bit rate in both databases. Lossy files are never
  encoded lossy again: an MP3 a player can't play stays as it is unless `--decode-lossy` asks for
  a lossless copy, whose cues move by the decoder's lead as `track upgrade` times it. The players are `--players`, or those `players set` remembered; the plan says
  which tracks are copied, why, the space they need, and which stay unplayable. Each copy is
  recorded on the drive with its master's content hash and its recipe, so packing again keeps
  current copies and makes them again when the master or the recipe changed. `check` points to
  it when lossless tracks won't load.
- **Upgrading to lossless.** `track upgrade <library> --from DIR` finds, by their sound, lossless
  files of the recordings the drive holds as MP3 or AAC, copies each beside the old file and
  points the track at it, keeping its row (playlists, history, My Tags, rating), with its cues,
  beat grid and detail waveforms moved by the offset between the two files' audio, measured to
  the sample. An MP3 is timed as rekordbox times it: from its first decoded frame, or for some a
  frame earlier, told apart by its waveform; an AAC among its likely encoder delays the same way.
  Each track that can't be upgraded is listed with why, and `--players` leaves out files the
  players can't play. The old file stays unless `--remove-old`. It's recorded, so `edits replay`
  does it again after rekordbox exports the old row.
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
  list's key the first time it's published. Sets and the profile are
  `computer.localnetwork.dj.set` and `computer.localnetwork.dj.profile`, record types Local
  Network's tools share (with `computer.localnetwork.dj.defs`), and My Settings stay
  `computer.localnetwork.locallibrary.settings`. The set Lexicon gains `kind`, `location` (an open
  union, a Lexicon Community address for now), `event`, `playedWith`, `links` and `sources` (for
  `source`), holds up to 500 tracks (not 1,000), and its tracks gain `artists`, `releaseName`,
  `releaseMbId` and `transitionIn` (how a track came in, from the recording's mix report), shaped as
  teal.fm's and skaists' records so readers can map them. MusicBrainz IDs are bare UUIDs, as
  teal.fm writes them. locallibrary fills them from files Picard tagged, else from a plugin
  analyzer's `recording`, `release` and `artists` roles when its `isrc` and `credits` say they're
  for the track as the set has it, which `sets publish` runs first on the tracks it names, once
  it's signed in and confirmed, and keeps in the set (`--no-lookup` doesn't; over RPC
  `sets.lookup`). The
  profile's `basis` can be `sessions`. The night-before job `set` is `prep` (`set` is a spelling of `sets`
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
  `mix.reports.list`, `.get` and `.html` find kept reports and write their pages. Library reads
  take `combined: true` for the whole library exports are made from (OneLibrary's own tracks, with
  ids of their own, and playlists too), `tracks.analysis` answers up to 100 tracks' analyses at
  once, and `track.fields` reads what analyzer plugins worked out for each track, kept in the store
  by audio file. The library hosts the plugins locallibrary ships (`plugin::Plugins`), each a
  child process started when it's first used: it checks every read a plugin calls back for
  against the plugin's capabilities and the params each read lets a plugin give, finds the drives
  a plugin names itself, plans an operation's edits as one edit plan (never onto an archive
  drive), asks before an export overwrites a file and never writes one to a drive, keeps
  analyzers' fields and each plugin's settings, and never starts one from an elevated process.
  An edit plan is destructive, and says so, when it leaves a playlist with fewer entries or
  deletes cues or My Tags, too.
- **Plugins.** The plugins built into locallibrary (only built-in ones run) add commands and
  operations to the command line (`locallibrary ID NAME`, or where the plugin puts them, like
  `playlist import`), with flags made from their params' JSON Schemas; an operation's plan is
  shown and asked about like any edit. `analyze` runs their analyzers on a library's tracks and
  `track` shows what they worked out (`sound.bpm`); `export --format` takes their formats, showing
  each file, what goes in and what the format can't hold, and asking before overwriting a file;
  `plugin` lists them and what each may do, and `plugin settings` keeps their settings (a secret
  one, read from stdin with `--secret`, in the system's keychain, only ever shown as set). The
  sidecar has all of it as `plugins.*` methods, their plans kept apart from locallibrary's own,
  and `track.fields` reads the installed analyzers' versions (each analyzer has its own, apart
  from its plugin's; a value can be null, worked out with nothing to show). A plugin whose id or
  commands clash with locallibrary's or another plugin's is left out, with a warning saying why,
  and the others work; a plugin's errors keep their kinds (`not_found`, `io`...). Each plugin runs
  in a folder of its own in the store, and goes online only through locallibrary, to the servers
  its manifest names, at public https addresses, as often as each allows, with its secrets filled
  in where a request names them (never sent on to another server) and left out of what it logs;
  what it reads never says where things are on the computer, and images come back as files
  locallibrary reads itself, in a folder for each request that `store gc` removes if a crash
  left it. A view command's
  rows show as they come. A command or an operation may ask you something first (which track a
  tracklist's entry is), or several things in one round (a `Choice`'s `more`, up to 64): picked
  from a list each in a terminal and in the player, or answered with
  `--choose PARAM=VALUE`; over RPC the question is the answer, to call again with. Providers find
  cover art (`art fetch`) or track details (`track fetch`) for a library's tracks: every provider of
  the kind is asked, what they find is offered best first (tags only when they'd change a track,
  each image held and sent once, and what's kept bounded), one alone scoring `plugin providers
  --auto-pick` (0.9) or more chosen for you, and locallibrary plans the edits itself, confirmed
  as any are (the player's actions menu has both, and its palette the auto-pick; the sidecar
  `plugins.provide`, `plugins.provide.plan/apply`, `plugins.providers` and `plugins.providers.set`).
  Checks (`[[contributes.checks]]`, `check/run`, the `checks` feature) find what's wrong with a
  drive: what they find is in `check` and `doctor` (`--only checks`), named `PLUGIN.CHECK`, a
  problem counting as one, with the command line of the plugin's operation that fixes it when
  there is one; a check that fails or takes longer than 10 seconds is a warning. Guards (`when =
  ["apply"]`, `plan/check`) say what they make of a plan that edits a library (edits, cues, a
  playlist restored, a contact's playlist) among its warnings before it's confirmed, and one
  made blocking stops a plan it finds a warning or a problem about. `plugin
  checks` turns each on or off, makes guards blocking and sets how long a check has (the sidecar
  `plugins.checks`, `plugins.checks.set` and `plugins.checks.timeout.set`; the player a "check
  settings…" form, a check's findings in its results and their fixes in the palette).
  An analyzer's fields may have roles (a track's key, tempo, loudness, MusicBrainz IDs...), which
  doctor, `check`, `playlist order` and `sets publish` read from the analyzer `plugin roles`
  chooses (the sound plugin's and the musicbrainz plugin's unless another has them), and `analyze
  --apply key,bpm` writes to the tracks; the sidecar has `plugins.roles`, `plugins.roles.set` and
  `plugins.apply_fields.plan/apply`; `plugin roles ROLE --none` reads one from no analyzer. Plain
  `analyze` leaves out the analyzers of plugins that go online. Plugins limiting one server take
  its turns between them. Plugins may listen for drives
  plugged in, unplugged, changed, pulled and back from a set, the sidecar's requests finishing and
  plans applied (capabilities `events:drives`, `events:jobs`, `events:plans`), told by the player,
  the sidecar, `watch` and any command applying a plan, through a bounded queue for each plugin
  that never holds them up; and run services, turned on with `plugin settings ID services.NAME on`
  and run by the player, the sidecar and `watch` (never a command run once), started again when
  they fail and left off after five failures in a row. What a plugin tells you back is a message
  in the player (what it suggests reviewed from the palette or dismissed, and kept until it is:
  one of its operations, planned and confirmed as any, or one of locallibrary's own flows on its
  own contributions, fetching from its providers, exporting in its format or running its
  analyzer, opened filled in for you to confirm), a `plugin.notice` event in the sidecar and a
  line of `watch`, which keeps the suggestion for the player. In the player, plugins' commands,
  operations and export formats are in the palette, and commands and operations in the menus their
  manifests name (`where`: the actions menu for the tracks selected or a playlist, a drive's menu),
  asking for their params or options in a form drawn from their schemas, a param from the selection
  (`x-from`) filled in with it (cleared, it stays so); a command answers in a panel, an operation's
  plan is confirmed, written and undone like any edit, an export shows what it would write and asks
  before overwriting, analyzers run from the palette and their fields are columns to sort and search
  by (and an analyzer's keys and BPMs are written to the tracks from there), plugins' settings are a
  form in the palette, and what plugins log goes to its messages (`!`). `tracks --filter`, `tracks
  --sort` and the sidecar's `tracks.query` take analyzers' fields (`sound.bpm:>120`, `-sound.bpm`).
  Importers read another program's file (`import FILE [LIBRARY] --format FORMAT`): locallibrary
  finds its tracks on the drive as it finds a tracklist's, says how many it couldn't, and adds what
  they lack (a key and a BPM where a track has none, cues in slots it has none in, My Tags) as one
  edit, confirmed and undone as any is, and keeps the plays the file logged as sessions (the
  player's drive menu has it, the sidecar `plugins.import.plan/apply`). An exporter may write out
  a set or a mix instead of a drive's library, or any of them with one format (its `source` a
  list): `sets export --format` and `mix --format FORMAT --export PATH` take those formats, the
  player offers a set's while the set is open, and the sidecar's `plugins.export.plan` takes `set`
  and `mix`.
  A view is a list, a table (its columns tracks, durations, keys or numbers with a unit, or plain),
  labelled values, text, a tree, a chart, a picture or sections of those, each drawn in the
  player and as plain text on the command line. Panels show what a plugin has to say about a
  track under its details, in the player (asked for once the selection settles, never another
  track's), under `track show`'s details and from the sidecar's `plugins.panel`. A plugin's theme
  (colours as data) is chosen in the player's palette and kept in `tui.json`, the default drawing
  as before; an analyzer says how its fields are shown (a unit, places, a key, a duration,
  decibels, a percentage, a meter, a column's width) in the player's columns and `track show`,
  and a mix's HTML report shows those fields of each track.
  See docs/plugins.md. The ones it ships:
  - `tracklist`: `playlist import` makes a playlist from a tracklist (M3U, CSV exports of
    streaming playlists, rekordbox text or XML, "Artist - Title" lines), asking which track an
    entry is when it could be several. It reads rekordbox XML collections whole too: `import
    rekordbox.xml LIBRARY` adds their keys, tempos, hot cues, memory cues and loops to the
    drive's tracks, and `collection export DRIVE --from tracklist:rekordbox-xml --grant FOLDER
    --option file=rekordbox.xml` builds a drive from one, with its tags, cues, beat grids and
    playlists. Values out of range in the file are left out, counted in the plan's warnings.
  - `interop`: the library for Traktor (`collection.nml`), Serato (crates, and cue points in
    tags of copies of the audio), Engine DJ (a new Engine Library) and rekordbox (XML), with
    `export --format`; and a set or a recorded mix as one playlist of the tracks played, in order,
    from a drive, for all but rekordbox (`sets export SET --format traktor --drive DRIVE -o DIR`,
    `mix RECORDING --drive DRIVE --format serato --export DIR`); and Traktor's `collection.nml`
    read back, `import` adding the keys, BPMs and cues it has for a drive's tracks, and
    `collection export --from interop:traktor` building a drive from it (tracks, cues, beat grids
    and playlists).
  - `sound`: `analyze` measures each track's tempo, key and loudness from its audio, kept as
    `sound.bpm`, `sound.key`, `sound.integrated_lufs`... (shown to a tenth, in LUFS, dBTP and
    percentages), which is what `doctor --analyze` runs. It also works out a three-band waveform,
    drawn where a drive has none and written with `analyze --apply waveform`.
  - `coverart`: finds front covers on MusicBrainz and the Cover Art Archive for `art fetch`, an
    album that could be several release groups offered as each of them, and from the same
    MusicBrainz lookups track details for `track fetch`: the album's title, year and genre, its
    label when it was found by name, and a track's title and artist when its ISRC found it (a
    track without an album only from a single of its title).
  - `demos`: `clean` removes rekordbox's demo tracks with their files, and its check lists them
    in `check` and doctor, with `clean` as the fix.
  - `musicbrainz`: looks up a track's MusicBrainz recording, release and artist IDs by its ISRC
    for `sets publish` to publish: the recording only when it's the track's (its length, or
    without one its title; of several, both), the earliest release named as the album, and the
    artists when each name the track credits is one of the recording's, answered with the ISRC
    and credits they're for (the `isrc` and `credits` roles). It and `coverart` go online, a
    request a second to MusicBrainz between them, only for `art fetch`, `track fetch`, `analyze
    --plugin musicbrainz` (plain `analyze` leaves it out) and a `sets publish` about to publish
    (signed in and confirmed), unless `--no-lookup` or `plugin roles recording --none`.
  - `plyr`: a set destination, `sets publish SET --to plyr:track`, uploading the set's recording
    to plyr.fm as a track in the parts plyr.fm asks for (each sent again a few times when it
    doesn't arrive), titled after the set, its tracklist as the description and tagged `dj mix`,
    and keeping the track's page as the set's link and its `fm.plyr.track` record as the set's
    recording. A recording plyr.fm wouldn't take (another format, or over 1536 MiB) is refused
    before anything goes. It signs in with a plyr.fm developer token, kept as a secret setting, and
    goes online only to publish.
  - `archive`: a set destination, `sets publish SET --to archive:item`, uploading the set's
    recording to the Internet Archive in one PUT as an item in Community Audio, at an identifier
    no item has (`CREATOR-DATE-TITLE`, with `-2` and on when it's taken), titled after the set,
    by the `creator` setting, its tracklist as the description and tagged `dj mix`, sent again a
    few times when it doesn't arrive or the Archive says to slow down, and keeping the item's page
    as the set's link. It signs in with the account's S3 keys, kept as secret settings, goes
    online only to publish, and never waits for the Archive to process the item.
  - `youtube`: a set destination, `sets publish SET --to youtube:video`, publishing the set to
    YouTube as a video: FFmpeg makes it from a still picture (the `picture` setting, else a black
    frame) over the recording, and it goes up through YouTube's resumable upload, titled after the
    set, unlisted unless the `privacy` setting says, with the tracklist as chapters in the
    description, keeping the video's page as the set's link. It signs in to Google through Local
    Network's token exchange, asking only to upload videos.
  - `mixcloud`: a set destination, `sets publish SET --to mixcloud:show`, uploading the set's
    recording to Mixcloud as a show in one form, titled after the set, with the tracklist as the
    show's sections (artist, title and start time), the `tags` setting's tags (five at most), the
    `picture` setting's picture and, with `unlisted`, kept private, and keeping the show's page as
    the set's link. A recording or picture Mixcloud wouldn't take is refused first; an upload that
    fails on the way, or over Mixcloud's rate limit, is sent again whole. It signs in to Mixcloud
    through Local Network's token exchange, with the token in the query as Mixcloud takes it.
  - `soundcloud`: a set destination, `sets publish SET --to soundcloud:track`, uploading the set's
    recording to SoundCloud as a track in one form, titled after the set, its tracklist as the
    description, tagged `dj mix`, not downloadable and private unless the `sharing` setting says,
    with the `genre` and `picture` settings as its genre and artwork, and keeping the track's page
    as the set's link. A recording or artwork SoundCloud wouldn't take is refused before anything
    goes. An upload that fails on the way is sent again whole a few times, once the plugin has
    checked SoundCloud didn't make the track anyway. It signs in to SoundCloud through Local
    Network's token exchange.
- **Beat grids, phrases and waveforms written to drives.** locallibrary writes rekordbox's beat
  grid (`PQTZ`), phrases (`PSSI`, masked as rekordbox 6 writes them) and waveforms (`PWAV`,
  `PWV2`, `PWV3`, `PWV5`, `PWV6`, `PWV7`) into a track's analysis files as a journaled, undoable
  edit (`set_analysis`), each read back before it's written, rewriting rekordbox's own files byte
  for byte where the data is the same, and reads 3-band waveforms (`PWV6`, `PWV7`) for a track
  with no others. An analyzer plugin may work out a track's grid, phrases and waveform (fields of
  type `beats`, `phrases`, `waveform`, feature `analysis`): kept beside its columns, drawn in the
  player where the drive has none, and written with `analyze --apply grid,phrases,waveform` (the
  sidecar's `plugins.apply_analysis.plan/apply`), a grid setting the track's BPM; operations plan
  the same edit with `analysis:plan`. Checked against rekordbox's files, not yet on players.
- **Drives built from a plugin's library source.** `collection export DRIVE --from PLUGIN:SOURCE
  --grant FOLDER` builds a drive from a library a plugin reads (feature `sources`, capability
  `fs:read`), as rekordbox's collection is written: the plugin is given only the folders granted,
  and only audio files players play in them (links resolved) are copied, each read and hashed by
  locallibrary itself and checked as it's copied; the rest is left out with a warning, as is what
  the plugin says it couldn't read, and a read past 64 million beats and cues is stopped. Cues,
  beat grids and waveforms a track brings go into analysis files made for it, read back first.
  The player has "build from …" in the palette and a drive's menu, the sidecar
  `plugins.source.plan/apply`. Not yet tried on players.
- **Plugins' accounts and destinations.** A plugin may have accounts on other services, which
  locallibrary signs you in to in the browser (`plugin account PLUGIN ACCOUNT sign-in`, or the
  player's palette), keeping the tokens in the keychain and refreshing them itself: the plugin
  never sees one. A service that wants a client secret for its tokens (Mixcloud, SoundCloud,
  Google) is signed in to through Local Network's token exchange, which holds the secret, so
  none ships with locallibrary. An account says how its service takes the token: after `Bearer`
  in the `Authorization` header unless it names another scheme (SoundCloud's `OAuth`) or a query
  parameter (Mixcloud's `access_token`). It may name the port the browser comes back to, for a
  service registered with an exact redirect address (SoundCloud). A plugin may keep a copy of the store (`store copy --to PLUGIN:DESTINATION`,
  `store restore --from`, `drive verify --from`), every file it gives back checked against its
  hash, or publish a set with its recording (`sets publish SET --to`), after a dry run you confirm,
  the link it answers kept in the set. The plugin uploads the recording through locallibrary
  (`net/fetch`'s `body_blob`, perhaps a `body_range` of it, or a `multipart` form's file), which
  reads it as it sends it rather than all at once, shows how much has gone (phase `upload`) and
  lets it send nothing else but files in its request's scratch folder. The sidecar has both as
  `plugins.account*`, `plugins.store.*` and `plugins.publish.*`.
- **Where a set's recording plays.** A set destination may name the link it answers (plyr's
  `plyr.fm`, archive's `Internet Archive`, youtube's `YouTube`, mixcloud's `Mixcloud`) and give the
  Atmosphere record holding the recording (plyr's track, as a `com.atproto.repo.strongRef`). The
  set keeps both, and its record names the link and points to that record as `recording`, a field
  the set Lexicon gains. Signed in, `sets publish SET --to` offers to publish the set's record next, in the same
  run (`--dry-run`, `--yes` and `--json` cover both).
- **A set's recording on your own account.** `sets publish SET --upload` uploads the set's
  recording to your Atmosphere account and publishes it as a `computer.localnetwork.dj.recording`
  record, a new Lexicon, then the set's record pointing to it, after one question. ffmpeg encodes a
  recording browsers can't play, or one too big for the data server, as AAC in an M4A file (128
  kbit/s, or down to 64 to fit 300 MiB, or `--max-mib`). The file is read as it's sent, and a server
  that refuses its size says so. Publishing's sign-in now also asks for `blob:audio/*`, and its
  permission set covers the new record type.
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
  drives. `--then 'PLUGIN.OPERATION KEY=VALUE...'` (repeatable) runs plugins' operations in the
  same plan, before the drive is saved (`operations` over RPC). `status` flags a drive rekordbox exported over recorded edits, and names it in its hint.
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
- **Reporting a bug.** `bug` posts a report from your Atmosphere account to Local Network's space on
  userinput.app (asking first, and signing in again to allow only that), or opens GitHub's bug
  report form, with the version, the system and how it was installed filled in either way.
- **Shell completion and man pages.** `locallibrary completions SHELL` for bash, zsh, fish,
  elvish and PowerShell: the shell asks `locallibrary` on Tab, so mounted drives, their playlist
  paths, My Tags and history lists, the store's libraries, sets and sessions, player models and
  `refresh --skip` steps complete too (`--static` for commands and flags only).
  `locallibrary man -o DIR` writes a man page for every command.
- **Crates for other DJ tools.** Parts of the library that other tools can use without
  locallibrary are crates of their own in `crates/`, each with a README and an example, and
  JSON Schema behind a `schemars` feature in the ones whose types are serialisable. locallibrary re-exports them
  where they used to live (`model`, `harmony`, `progress`, `anlz`, `settings`, `equipment`,
  `format::FsKind`, `import`, `mix`), so the CLI, the RPC contract, the files written
  and the error messages are unchanged.
  - `loclib-progress`: progress events and cooperative cancellation (`Ctx`, `CancelToken`,
    `Event`, `Cancelled`).
  - `loclib-model`: tracks, playlists and the diff between two exports, Camelot keys and mix
    costs, a track's decoded analysis (beat grid, cues, waveforms, phrases, the hot cue palette)
    and the path keys tracks are matched by, and a theme: every colour DJ data and a player are
    drawn in, the shared ones by default, set by name.
  - `loclib-equipment`: what each Pioneer DJ / AlphaTheta player, all-in-one system and mixer
    reads (databases, file systems, audio formats), with each value's source, and the same
    table as `equipment.json` for other languages.
  - `loclib-mysettings`: reading and writing `MYSETTING.DAT`, `MYSETTING2.DAT`,
    `DJMMYSETTING.DAT` and `DEVSETTING.DAT`, with every known setting's schema and edits that
    patch single bytes and recompute the checksum.
  - `loclib-tracklist`: reading M3U, CSV and other tables, rekordbox's text export and XML, and
    plain "Artist - Title" text, and finding the tracks in a library by path, ISRC, artist and
    title, then fuzzily; and reading a rekordbox XML collection as a library, with each track's
    tags, key, tempo, beat grid and cues.
  - `loclib-mixid`: which known tracks a recording plays, when, at what speed and with or without
    key lock, from any audio files (decoding, landmark fingerprints, the index and the matcher),
    and how two files of one recording line up, to a fraction of a sample (`refine`).
    locallibrary keeps the drive, the fingerprint cache, Audacity projects and the report.
  - `loclib-transcode`: which copy a set of players needs of a file (as it is, a lossless copy at
    a rate and depth they all take, or none), and making it: AIFF, WAV or FLAC, resampled with
    the timing kept to the sample, TPDF-dithered, with tags and artwork; recipes to store with a
    master's hash, and size estimates.
  - `loclib-interop`: writing a library as Traktor's `collection.nml`, Serato crates (with the
    `Serato Markers2` cue tag in copies of the audio), a new Engine DJ library and rekordbox XML,
    a feature per format, so a build for NML alone needs no SQLite; and reading a `collection.nml`
    back (tracks, markers, playlists, and a beat grid laid out from its grid markers).
  - `loclib-waveform`: the player's ratatui widgets (overview and detail waveforms, cue lane,
    phrase strip, spectrum, scope, cover art in half blocks), in the colours of a palette the
    caller passes, a theme's (`Palette::of`).
  - `loclib-plugin`: the plugin protocol (versioned on its own, at 0.2.0; each family of hooks
    an optional feature named in its handshake), the `locallibrary-plugin.toml` manifest (read
    tolerantly, keys a newer locallibrary adds left out) and its checks, a JSON-RPC peer over a
    pair of pipes and an SDK for writing plugins in Rust, a hook per request type, with typed
    reads of a drive, `net/fetch` through the host, files in a request's scratch folder for big
    answers, a view's rows sent in parts, questions for the user (`Choice`), providers
    (`provide/candidates`), panels (`panel/render`), views of every kind (the `views` feature),
    themes and how analyzers' fields are shown. The RPC sidecar reads lines and reports errors
    with it.

### Changed

For anyone who used a development build. Commands and flags that were renamed still work under
their old names.

- Audio files are told by one list of the extensions players play (the players' manuals': `.fla`
  is FLAC, and `.ogg` or `.alac` isn't counted as loose audio or an orphan under `Contents/`, or
  offered by `relocate` and `upgrade`, any more), kept in loclib-model, and files operating
  systems leave behind (`$Recycle.Bin`, `Thumbs.db`, `desktop.ini`...) are told in any letter
  case.
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
  auto` (`check` and `doctor` to `art fetch` too, while a plugin finds cover art), and a freshly
  formatted drive to `provision`. Every `<DRIVE>` argument and flag says what
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
  played twice.
- `clean` is the demos plugin's now: its plan is an edit plan (journaled, so `edits replay` makes it
  again after an export), a demo track only OneLibrary lists goes too, and one sharing its audio
  file with another track stays, said so. `refresh`'s first step runs it. Over RPC, `clean.plan`
  and `clean.apply` are gone: use `plugins.op.plan {plugin: "demos", operation: "clean", context:
  {drive}}` and `plugins.op.apply`. A `delete_tracks` edit removes every row of a track's audio
  file, and may name a track only the drive's other database lists.
- `playlist import` is the tracklist plugin's now. The entries that could be several tracks are
  asked about 64 at a time (all at once for most lists), so the list and the drive's library are
  read once a round rather than once an entry (in a terminal; otherwise it stops, exit status 2,
  saying how to answer that round's): `--choose 3=812` answers for entry 3, numbered from 1 as
  the warnings number them, and `--choose 3=null` leaves it out. A track chosen that isn't on the
  drive (removed since it was asked about) is warned of and left out. `--json` gives the plan
  alone. Over RPC, `import.plan` and `import.apply` are gone: plan
  with `plugins.op.plan {plugin: "tracklist", operation: "import", params: {file, into, ...},
  context: {drive}}`, which may answer `{"choose": ...}` first, and apply with `plugins.op.apply`.
- `export --format traktor|serato|engine` is the interop plugin's now. Its options are
  `--option`s: `--location-root DIR` is `--option location_root=DIR`, `--no-analysis` is `--option
  skip_analysis=true`, `--write-tags` is `--option write_tags=true` and Engine's `--paths drive`
  is `--option paths=drive`. Overwriting files already there is asked about first (exit status 3
  without a terminal or `--yes`) unless `--replace`. Serato's `write_tags` no longer asks before
  copying the audio: `--dry-run` lists each copy and its size first. Over RPC, `interop.plan`
  and `interop.apply` are gone: use `plugins.export.plan {plugin: "interop", exporter, drive,
  playlists?, output, options}` and `plugins.export.apply`.
- rekordbox XML is the interop plugin's too, as `export --format rekordbox`; `xml` runs it, with
  `--only` as the playlists, `--location-root` and `--no-analysis` as its options (a relative
  location root is taken from where you are). Overwriting a file there is asked about first
  (exit status 3 without a terminal or `--yes`) unless `--replace`, a warning names tracks whose
  audio isn't where the file says, and `--json` gives the plugin export's plan and report. As
  with every plugin export, the drive has to be mounted: a copy of one in a folder isn't read. In the player, "export for rekordbox…" replaces "xml".
  Over RPC, `xml.export` is gone: use `plugins.export.plan {plugin: "interop", exporter:
  "rekordbox", ...}` and `plugins.export.apply`.
- `store prune` applies the plan it showed, refusing if a pull, label or drive change came in
  between (RPC `store.prune.plan` and `store.prune.apply`; `store.prune` still works), and its dry
  run reports exactly what it frees. `store gc` removes only snapshots of no library, files
  nothing uses and caches no version reads any more, and refuses when it can't read a snapshot. `store copy` refuses, without
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
  the boot volume, and every command warns when it's set. `LOCALLIBRARY_AUDIO=silent`, another,
  has the player play to no output at all, in real time, so a harness can play tracks unheard.
- The sidecar refuses relative paths to write to (`art.embed.*` files included), `mix.export`
  refuses to replace a file without `replace: true`, `drive.reidentify` is a `.plan`/`.apply` pair
  (the old name still works), a second SIGINT or SIGTERM waits for a database swap under way,
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
  for delete too), through the permission set `computer.localnetwork.locallibraryPublishing`
  (`include:`), so servers show its description when you approve. The set has to be published
  (`pnpm -C apps/web lexicons:publish`) before a sign-in that allows publishing works. Sign-in
  stops when that document is online but doesn't fit, instead of quietly using the development
  client, and `account` and the player say when a session is the development client's. `settings
  import --from-handle` and `--from-account` (and `settings.import_published.*`) add the
  published settings to the base; `--replace` (`replace: true`) replaces it. TLS also trusts the
  system's root certificates.
- What locallibrary reads from the network (what a publish replaces, My Settings to import, the
  space bug reports go to) comes from Local Network's directory, its HappyView AppView, instead of
  the account's data server, and a publish replaces only the version the directory showed. The
  first publish or import from an account on a computer has the directory read what the account
  published before it kept locallibrary's records, through Local Network's website
  (`computer.localnetwork.directory.requestIndex`, which publishing's permission set now allows).
  `settings import --pds` and `settings.import_published`'s `pds` are gone. A handle is resolved
  only from its DNS record or `/.well-known/atproto-did`, never through Bluesky's AppView. A set
  taking the key `plays publish` gave its list no longer copies the old record's first-published
  time.
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
  `StepKind` and the import and export formats. A `match` on them needs a `_`
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
  real startup disk name, and stamps its entries with `SOURCE_DATE_EPOCH` when it's set (refusing
  a value that isn't seconds since 1970).
- `check`'s items (with `--json`, and over RPC) are findings, the shape plugins' checks report
  in: what was found fine is `info` where it was `ok`, and an item may name the tracks, playlist
  and files it's about (`tracks`, `playlist`, `paths`).

### Fixed

- Tracks your profile hides (`profile hide`) are no longer named when a set is published at a
  plugin's set destination (`sets publish --to`) or written out in a format for publishing (`sets
  export` text, Mixcloud, CUE, YouTube, 1001Tracklists). `sets publish --to` takes `--hide` too,
  and its dry run says which tracks go unnamed and why.
- A release date given to an edit has to be written out in full (`2019-05-01`): `2019-05-1`
  was taken, and the Traktor export left it out.
- On Windows, an edit, sync or other write no longer fails ("Access is denied") when the drive
  watcher of the TUI, the sidecar or `watch` looks at the drive at the same moment.
- A plugin setting or param typed as a whole number (`3`) reads back as one, not as `3.0`.
- A plugin's log keeps a backslash that follows a window title ended by BEL.
- Two making the store at once (the sidecar's workers, or a CLI beside them) make one store:
  neither is refused as "not empty", and both get the same store id.
- `profile`'s cue statistics count hot cues D to H on tracks that weren't played in a set too.
- Matching two tracks by their sound hears the last few seconds of a copy a few milliseconds
  late too, so a copy of a short track isn't missed as the same recording.
- A drive edited back to a state it held before (a rating set, then put back) is newer than a
  drive synced from it in between, where `compare` and `sync` called them diverged. A drive that
  changed on its own from either state has diverged from it, where `sync` would have copied that
  drive over the edit and `pull` saved it on top.
- A store that can't be written (a read-only disk, or folders this user can't write to) is what
  the error names. `pull` named a folder inside it, which depended on the file it was saving. An
  error making a temp file names its folder, without the random name it tried.
- When two mounted drives go by one name (a drive and a copy of it), `setup`'s questions say
  which folder each is about.
- What other code writes to stderr while the player runs goes to its messages (`!`) instead of
  over the screen. On Linux with no sound card, pressing space printed ALSA's errors over the
  player before it said there's no audio output.
- The player's outputs list (`d`) opens at once and lists the devices in the background, where
  the player froze until every one was found (a while with ALSA). Devices that can't be listed
  say why in the list, which used to show only the system's default.
- A plugin's response that isn't JSON fails the request it answers even when it ends on a
  backslash inside a string, where that request was left waiting.
- A drive holding a state the library's latest snapshot went through, though no snapshot holds
  it (synced from the main drive before its next change was pulled), is behind the library.
  `pull` refused it as diverged and offered to force it, which would have rolled the library
  back; it's now refused as older, offering `library push`, which takes it (the
  `older_than_store` error leaves out `fields.snapshot` when no snapshot holds the state).
  `status`, `drive ls --known` and the player say such a drive that isn't plugged in is
  behind, where they said it had changes not in the store.
- When the track queued after the playing one is short enough to play out before the player next
  looks, the player goes on past it at once. It showed it as playing until then, and `n` pressed
  in that moment skipped the track after it.

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
- A path with `..` in it is judged by where it leads. A plugin's export onto a drive or into the
  store by way of a folder not made yet (`~/new/../../../Volumes/USB/x.m3u`) was let through. A
  `..` after a symlink went back to the link's folder instead of out of its target, so a format
  could miss a store reached that way, an offsite copy could go inside the store, and the player
  could save its state on the drive.
