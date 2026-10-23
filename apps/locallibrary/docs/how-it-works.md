# How it works

```text
rekordbox ──Export──▶ USB A ──locallibrary sync──▶ USB B, USB C ...
                        │
                    locallibrary pull              locallibrary library push ──▶ any drive, any snapshot
                        ▼
                 store (history, deduplicated)
```

## The code

```text
crates/loclib-progress/     progress events and cooperative cancellation, shared by every long-running operation
crates/loclib-model/        the library model (tracks, playlists, the differences between libraries), Camelot
                            keys, harmonic matches, mix costs and the set planner, a track's decoded analysis,
                            path keys
crates/loclib-equipment/    what each player, all-in-one system and mixer reads (databases, file systems, audio
                            formats, sample rates, bit depths), with the manual each value comes from;
                            equipment.json is the table for other languages
crates/loclib-mysettings/   My Settings files (MYSETTING.DAT and the rest): every setting's schema, decoding
                            and byte-level patching
crates/loclib-tracklist/    reading outside tracklists (M3U, CSV, rekordbox text and XML, plain text) and
                            matching them to a library: by path, ISRC, artist and title, then fuzzily
crates/loclib-mixid/        which known tracks a recorded mix plays: decoding and resampling, landmark
                            fingerprints, the hash index and its print format, the matcher, and how
                            two tracks line up, refined to a fraction of a sample from their audio
                            (doctor's duplicates by sound, cues carried or moved by the offset)
crates/loclib-transcode/    which copy of a file a set of players needs, and making it: AIFF, WAV or
                            FLAC, resampled with the timing kept to the sample, dithered, with tags
crates/loclib-analysis/     a second opinion on a track's tempo, key and beats (stratum-dsp), its loudness, peaks and
                            clipping (ebur128), half/double tempo checks, and scoring analyzers against
                            the GiantSteps annotations
crates/loclib-interop/      a library for other DJ software: Traktor's collection.nml, Serato's crates and cue
                            tags, an Engine DJ library (engine.sql), rekordbox XML
crates/loclib-waveform/     the player's ratatui widgets: overview and detail waveforms, cue lane, phrase strip,
                            spectrum, scope and cover art, in the colours of a palette the caller passes
crates/loclib-plugin/       the plugin protocol (JSON-RPC over stdio, one message a line), the plugin manifest
                            and its checks, a peer over a pair of pipes and the Rust plugin SDK; the sidecar
                            reads lines and reports errors with it
plugins/coverart/           the coverart plugin: front covers for tracks without artwork (or all, asked to),
                            each album looked up on MusicBrainz and its cover fetched from the Cover Art
                            Archive, as `set_artwork` edits
plugins/interop/            the interop plugin: exporters for Traktor, Serato, Engine DJ and rekordbox XML,
                            writing a drive's library with loclib-interop on this computer
plugins/sound/              the sound plugin: the `tempo-key-loudness` analyzer, a track's tempo, key and
                            loudness measured with loclib-analysis, as doctor's `--analyze` measures them
plugins/tracklist/          the tracklist plugin: a playlist from a tracklist, read and matched with
                            loclib-tracklist, as one `set_entries` edit
vendor/rekordbox-pdb/       export.pdb and exportExt.pdb reader/editor (MIT), with locallibrary's patches, see PATCHES.md
crates/locallibrary/src/    the library, built on the crates above
  lib.rs       the crate's entry point (`model` and `harmony` are loclib-model's, re-exported), and the
               `testing` hooks tests use
  progress.rs  loclib-progress, re-exported, and the guard that keeps a front end quitting from cutting
               a database swap short
  hash.rs      BLAKE3 hashes for stored files, change detection and fingerprints
  fsutil.rs    atomic writes, copies that keep modification times, free space
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
  peers/       fetching snapshots from another computer's store over iroh (`store serve`, `store fetch`; only
               with the `peers` feature)
  merge.rs     semantic merge via export.pdb and OneLibrary editing
  edit.rs      playlist and track edits in both databases, verified against a model; the edit journal
  query.rs     the track filter and sort language, track selectors
  smart.rs     rule-based playlists
  pack.rs      drives for a set, cut down from another drive's library; pack/transcode.rs: copies of
               tracks the set's players can't play, and their record on the drive
  collection_export.rs  rekordbox's collection written to a drive or folder, as rekordbox's Export does
  library_write.rs      writing a whole library to a drive: incremental copies, the database swap, removal
  import.rs    loclib-tracklist, re-exported
  tags.rs      My Tags in exportExt.pdb and OneLibrary
  cue_edit.rs  hot cues, memory cues and loops in analysis files and OneLibrary's cue table
  doctor.rs    duplicates (by file, name, metadata, audio and sound), missing and orphaned files, drift
  relocate.rs  finding missing tracks' audio on the drive and in folders, used only when one file fits
  upgrade.rs   pointing lossy tracks at lossless files of their recording, found by sound, cues and
               grid moved by the offset between them
  plays.rs     history lists on drives and in the store, play statistics
  sets/        the DJ's sets (sets.json): sessions and their windows, suggestions, building a set from
               sessions and recordings, editing, exports, Beat Link Trigger logs
  profile/     what sets, tags, analysis and My Settings say about a DJ; the public document and its preferences
  audio_cache.rs  what's worked out from each audio file (fingerprints, analyses), cached in the store
               by file name, size and modification time, and worked out in parallel when it isn't
  fields.rs    analyzer plugins' fields for each audio file, kept the same way, by plugin, analyzer
               and the analyzer's version
  plugin/      the plugin host: starting the plugins locallibrary ships and checking who they are,
               the reads they may call back for (callbacks.rs), and their commands, operations
               (planned as edit plans), exports, analyzers and settings
  sound/       tempo, key and loudness through loclib-analysis: doctor's analysis section, how sure each
               key is for the set planner, the check's loudness item, and `analysis bench` (bench.rs)
  mix/         which library tracks a recorded set plays, through loclib-mixid: the drive's tracks, the
               store's fingerprints (index.rs), Audacity projects (aup.rs), the report (the
               interactive page, the mix's playable audio, the reports kept in the store: report.rs and
               report.html) and the export for Local Network (export.rs)
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
  settings.rs  My Settings on drives and in rekordbox's folder (the files' codec is loclib-mysettings),
               and the settings profile
  artwork.rs   drive artwork rows/thumbnails, audio tag embedding
  anlz.rs      analysis files (anlz/write.rs writes them): path rewriting, beat grid, cues (read and write),
               waveforms, phrases, decoded into loclib-model's analysis types
  pdb.rs       reading export.pdb into the library model, through rekordbox-pdb
  paths.rs     which files on a drive locallibrary manages (the path keys are loclib-model's)
  onelibrary/  exportLibrary.db: SQLCipher pages, WAL replay, reading and editing via SQLite
  masterdb.rs  rekordbox's own collection (master.db), decrypted from a copy and read; comparing a drive with it
  format/      drive formatting: which layouts players read, diskutil/lsblk/PowerShell backends, FAT32
               writer, fake-capacity probe
  equipment.rs loclib-equipment's table, and the player models remembered in the store (players.json)
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
  api.rs       service layer used by the CLI and RPC, with a file per area in api/ (api/drives.rs: drive management, api/undo.rs:
               undo, api/autopull.rs: `watch`, api/cache.rs and api/reads.rs: parsed libraries
               kept between requests, the library in pieces, api/sidecar.rs: the sidecar's own
               requests and notifications); every *_plan returns an op::Planned
  schema.rs    JSON Schema for the API's types (every request and result derives it), labelled tuples
  error.rs     errors with a stable code, and the reason, fields and actions a front end acts on (its
               messages never name a CLI command or flag: the CLI's hints.rs words those)
apps/locallibrary/          `locallibrary` (clap), the JSON-RPC sidecar (rpc.rs; rpc/methods.rs: every method in
                            one registry, rpc/contract.rs and rpc/typescript.rs: `rpc.describe`, schema.json
                            and the TypeScript types), the plugins it ships (plugins.rs: the sound, tracklist,
                            interop and coverart plugins linked in, each run as `locallibrary plugin serve <id>`;
                            plugin_cmd/: what they add to the command line), and the player (tui/: ratatui +
                            rodio, drawing with loclib-waveform, the `tui` and `playback` features;
                            account_off.rs and the `playback`-less audio.rs stand in for what a build leaves
                            out)
apps/locallibrary/docs/rpc/ the sidecar's contract: schema.json and locallibrary.d.ts (`rpc --emit-schema`)
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
  Syncing A to B hands B A's history. "B's current state appears in A's history, after every
  other state the two share" means B is behind, and "both moved on from a shared state" means
  they diverged. (A drive edited back to what it was holds a state other drives' histories have
  from before; that doesn't put it behind them.)
- **Libraries and clones.** The drives holding one library are its clones (the library a drive
  holds is in its `.locallibrary/state.json` and the store's registry). A command given a
  library's name works on its lead clone: the newest of its clones plugged in, as `sync` finds it
  by lineage (the primary when they're identical, or when they can't be ordered). A change to a
  library is planned on the lead and, once applied, copied onto the other clones plugged in the
  way a group sync copies, refusing a clone that's no longer older; a clone that's newer, diverged
  or back from a set is left out of the plan with a warning. Reading a library reads it once,
  from the lead. A sync carries the drive's recorded edits along with its library, so `refresh` makes
  edits made on any clone again.
- **Store.** Files are stored by BLAKE3 hash. Snapshots are manifests with parent links, so
  history is a DAG and storage is deduplicated across drives and time.
- **OneLibrary.** `exportLibrary.db` is SQLite encrypted with SQLCipher 4's defaults (AES-256-CBC
  pages with HMAC-SHA512, PBKDF2-HMAC-SHA512 key derivation) under one passphrase every rekordbox
  installation shares. It's decrypted (with its WAL replayed) into a temporary working copy,
  edited with SQLite, checked (`PRAGMA integrity_check` and an encrypt/decrypt round trip) and
  encrypted again with the file's own salt. Tracks are matched to `export.pdb`'s by file path.

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
  and `account logout`; `sets new`, `sets edit`, `sets suggest --dismiss` and `plays import`,
  which only change the store's own records; `watch --pull` and `--sync-group`, which you opt
  into; and `export`, `xml`, `art get`, `plays export`, `sets export -o`, `profile document -o`
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
  history players recorded would be lost too. Only the drive a change was made on is backed up:
  copying it onto a library's other drives isn't, and an undo is copied onto them like any change.
  The plan shows the files and databases that change,
  how the playlists and tracks change, the history lists it drops, and the recorded edits that come
  off the drive's journal (the one the backup was made for, and any made since), so `edits replay`
  doesn't make them again; `undo --list` shows what can be undone. Putting a backup back goes
  through the same write routine as every change, and backs up what it replaces first: putting that
  backup back redoes the undo. Each drive keeps the newest 10 backups of each kind, counting those
  holding removed tracks' audio (`drive backups` says which) and those undos took apart, so small
  edits don't prune the only copy of a deleted track. Art `art embed` put in files on this computer
  comes back out with `art unembed`. Repairs (`drive verify --repair`,
  `drive fsck --repair`), `drive tidy`'s deletions, firmware files and collected recordings aren't
  backed up.
- **One write routine.** Every change to a drive in place (but `drive tidy`'s deletions) locks it,
  builds and checks the new databases and files in memory, backs it up (not repairs or update
  files), marks the drive as being written, copies new files
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
  Tracks from a source without OneLibrary get OneLibrary `cue` rows made from their analysis
  files, as `convert` makes them. Hot cue bank lists only merge between OneLibrary databases.
  History playlists merge with `--history`, flattened to the top level. Tracks matched as
  duplicates keep the target's metadata and tags. A track whose details are too long for one of
  the target's `export.pdb` rows (OneLibrary has no such limit) is left out with a warning, and a
  playlist, folder or history list name that long is refused in the plan.
- The computer-side rekordbox database (`master.db`) is only read, never written: interchange
  with rekordbox goes through device exports and rekordbox XML. The reader follows pyrekordbox's
  description of the tables and was checked against databases built to that description (with
  the `sqlcipher` CLI installed, the tests also have SQLCipher write and read them under
  rekordbox's key), not against a real rekordbox 6 or 7 collection: which column holds a hot
  cue's colour (`ColorTableIndex` is taken as the hot cue palette code), the date added
  (`StockDate`) and that `rb_local_deleted` rows are deleted are inferred. Intelligent playlists
  aren't evaluated.
- **Exports for other DJ software** follow those programs' formats as their users and open source
  projects document them; none has been loaded in the program it's for. Traktor's NML follows a
  collection Traktor wrote (from traktor-nml-utils' test files) and is parsed back with an
  independent XML parser in the tests, but whether Traktor places an MP3's cues exactly where
  rekordbox does (the two handle the encoder's delay differently) hasn't been checked, and nothing
  corrects for it. Serato's crate layout follows the mixxx wiki and a crate Serato wrote
  (serato-tools' test file), and the `Serato Markers2` encoder reproduces, byte for byte, a tag
  Serato DJ wrote (serato-tags' test files); whether Serato picks up crates whose tracks it hasn't
  seen before, and how it shows rekordbox's hot cue colours (mapped to its 18 by hue), hasn't been
  checked. The Engine Library follows Engine DJ's schema as libdjinterop documents it and passes
  libdjinterop's schema 3.0.2 checks (tables, columns, indexes, views), but no Engine version has
  opened one, and its performance data (grids, cues) isn't written at all.
- `*SETTING.DAT` layouts are reverse-engineered (thanks to
  [rekordcrate](https://github.com/Holzhaus/rekordcrate)). Unknown byte values are reported as
  `unknown(0x..)` and preserved.
- **Drive management** hasn't been checked on hardware: whether players show OneLibrary's
  `deviceName` (rekordbox writes it there, and leaves it empty until a name is set), where players
  reading `export.pdb` find the name, what the colour codes 1 to 8 look like, and which files each
  player model writes during a set (one OMNIS-DUO report saw only the history tables and analysis
  files change). The back-from-a-set rule stays conservative: playlist edits made on a CDJ-3000X
  count as a real change.
- **Linux and Windows are experimental, untested on hardware** (see
  [Platforms](install.md#platforms)), and `drive format`, `drive fsck`, `eject`, `drive rename
  --label` and `--check-capacity` have never run there. `drive fsck` needs administrator rights on
  Linux and Windows. `--check-capacity` needs them on a real disk and isn't available on Windows.
  `eject` on Windows uses Explorer's Eject, which reports nothing: locallibrary waits for the drive
  to disappear.
