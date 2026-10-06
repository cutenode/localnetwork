# Dangling tasks

Updated 2026-10-06 after the opportunity map (tracks A to G) and the October audit's fixes
landed; see CHANGELOG.md "Unreleased" for what changed. Tick items off or delete them as they land.

## Before the first push and release

- [ ] **Nothing is pushed and there's no remote.** The root commit and `main`'s history hold
      session and artifact links and personal paths, so a public push needs a history rewrite or
      a fresh first commit. The commits on `integration` past `main` are unsigned: re-create them
      signed on a `bnb/` branch from `main`. Then create `github.com/cutenode/locallibrary`
      (already set as `repository` in `Cargo.toml`) and push.
- [ ] **CI has never run.** On pull requests and pushes to `main` (Markdown-only changes skip it):
      fmt, clippy, nextest on macOS/Linux/Windows, the msrv job (`cargo +1.89 check`, not yet run
      against the updated lockfile), docs, cargo-deny, and a slim `features` job (clippy on the
      headless CLI without ALSA headers). Weekly and on demand, `scheduled.yml` runs Bombadil, the
      no-default-features tests, the cargo-hack feature powerset and cargo-deny's advisories:
      after the first push, run it once (Actions > Scheduled > Run workflow). The Linux and
      Windows legs have never run, and the Windows-only code (raw-volume FAT32 writer,
      `fsutil::flush` with a write handle, the clippy lints fixed by reading, paths compared
      without the `\\?\` prefix, the mix report's `file:///E:/` links) has never compiled. The
      no-`playback` and no-`account` builds have only been built on macOS, and the feature jobs
      don't cover Windows.
- [ ] On the first Linux run of `scheduled.yml`, check that the AppArmor sysctl lets Playwright's
      headless shell start with its sandbox (otherwise pass Bombadil's `--no-sandbox` in
      `report.sh` on Linux).
- [ ] If branch protection requires `ci.yml`'s checks, Markdown-only PRs (which `paths-ignore`
      skips) will wait on them: don't require them, or drop the `paths-ignore`.
- [ ] After creating the repo, turn on Dependabot alerts and security updates in its settings
      (`dependabot.yml` covers monthly version updates only).
- [ ] `dist plan` runs only on PRs that touch a `Cargo.toml`, `Cargo.lock`, `dist-workspace.toml`,
      `rust-toolchain.toml` or `release.yml`: confirm it passes on one (`release.yml` is
      hand-written from dist 0.33.0's template), then try the release pipeline with a prerelease
      tag (`v0.1.0-rc.1`, which skips the Homebrew push) before tagging v0.1.0.
- [ ] Third-party notices for the release archives and the Homebrew formula: generate them at
      release (`cargo about`, say) for the dependencies, plus rekordbox-pdb's MIT notice for the
      vendored code and `crates/locallibrary/src/convert/template.pdb` (shipped in the binary).
- [ ] Create `cutenode/homebrew-tap` and the `HOMEBREW_TAP_TOKEN` secret (a fine-grained token
      with write access to the tap's contents).
- [ ] Add the Apple secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
      `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER`) and check
      the first signed release's log for Apple's verdict. Windows binaries are unsigned
      (SmartScreen warns); dist supports Azure Artifact Signing.
- [ ] Decide whether to rename the CLI package so release archives and installers aren't called
      `locallibrary-cli-*` (only the Homebrew formula is `locallibrary`).
- [ ] Revisit the cargo-deny ignore of RUSTSEC-2023-0071 (rsa via jacquard-oauth's jose-jwk; sign-in
      only uses an ES256 key) when jacquard or rsa change.
- [ ] `cargo audit` flags quick-xml 0.37 (RUSTSEC-2026-0194 and -0195), a dev-dependency that
      rekordcrate 0.3.0 pins; cargo-deny skips dev-dependencies. Move to quick-xml 0.41 or later
      once rekordcrate does, so the tests keep one version.
- [ ] Type-check `docs/rpc/locallibrary.d.ts` with `tsc` in CI.
- [ ] Reinstall the CLI (`cargo install --path crates/locallibrary-cli --locked`) and rebuild the
      GUI's sidecar from this build: builds from before the versioned journal change a drive this
      version has edited, then fail to record the edit.

## Untested on real hardware or platforms

### Players and drives
- [ ] **Every writer on a player.** On a spare drive, run each writer (playlist/track edits, `tags`,
      `cues`, `merge`, `clean`, `art set`, `convert`, `pack`, `refresh`, `drive player`) then
      `undo`, and load the results on an export.pdb player and a OneLibrary player (CDJ-3000X).
      No OneLibrary write has been loaded on a player yet.
- [ ] OneLibrary `cue.kind` for hot cues D-H (assumed 4-8 like A-C are 1-3) and `colorTableIndex`
      for hot cues are inferred; check against a real export with hot cues D-H.
- [ ] Check against the DJM-A9 and DJM-V10 update guides that their firmware needs FAT32 (now
      `drive firmware`, `check` and the equipment table all say FAT32 only).
- [ ] Merging from a source that has no `exportLibrary.db`: `analysedBits` and `contentLink` are
      copied from whatever value most of the target's tracks use (a guess from one export).
- [ ] Whether OneLibrary-only players read My Settings from `*SETTING.DAT`.
- [ ] Whether a player rewrites `exportLibrary.db` (WAL checkpoint) when the drive is plugged in:
      plug in a synced drive and run `locallibrary diff --summary` afterwards.
- [ ] What rekordbox's automatic playlist sync (`playlists3.sync` / `playlists3Plus.sync`) does
      with playlists that `merge` added.
- [ ] Whether players show OneLibrary's `deviceName` (and what colour codes 1-8 look like), where
      `export.pdb` players get the Device Name, and which files each player writes during a set.
      The back-from-a-set rule treats CDJ-3000X playlist edits as real changes.
- [ ] `format` on a real drive (only macOS disk images so far; formatting KEYCHAIN was offered and
      is waiting for your go-ahead). Confirm the macOS USB serial from the IORegistry on real
      sticks, two of one model too (SD cards and Thunderbolt disks have none). A test of
      `format::apply` on a disk image (`LOCALLIBRARY_FORMAT_VIRTUAL`) that only CI runs would
      cover the erase itself.
- [ ] `eject` on a disk with several volumes: refusing when the store is on another of them, and
      naming the volumes it unmounts, have only been unit-tested.
- [ ] Player capability table (`equipment.rs`): check the manua.ls-sourced values (CDJ-1500X,
      XDJ-AZ, XDJ-RX3, XDJ-1000MK2, XDJ-700) against the official PDFs, and the inferred CDJ-2000,
      XDJ-RX2, XDJ-RR and XDJ-RX3 AAC rates against their manuals; whether players really refuse
      files outside the listed rates (e.g. a 32 kHz MP3 on a CDJ-3000); update the CDJ-3000 entry
      if a firmware adds OneLibrary. The `.m4a` AAC/ALAC bitrate rule is a guess; check it against
      an export that holds ALAC.
- [ ] The NXS2, NXS, XDJ-XZ, OPUS-QUAD and OMNIS-DUO manuals say a partition holding a rekordbox
      library takes priority over the first partition; `format::players` and the README don't say so.
- [ ] `check --audio` / `doctor --audio` on real MP3, FLAC and M4A files and on a large USB drive
      (only WAV truncation is tested).
- [ ] `refresh` after a real rekordbox export, and `set` with a real eject.
- [ ] Pack now saves a set's history lists and cues in the store before packing again, and backs
      up on the drive: check the backup size on full sticks.

### Linux and Windows
- [ ] `format`, `fsck`, `eject`, `drive rename --label` and `format --check-capacity` have never
      run on Linux or Windows (the CLI says so when they run there), nor has the Windows erase
      script's check of the disk's size, serial and that it isn't the system disk. Once each has,
      remove `output::untested_on_this_platform` (the warning is CLI-only; RPC callers don't get
      it), and once CI is green there too, the README's "experimental" label.
- [ ] Linux `fsck` now unmounts with `udisksctl`, runs on the block device and mounts again (as
      `$SUDO_USER` under sudo); check that the remount works under polkit and comes back at the
      same path.
- [ ] The sidecar's Windows Ctrl-C path (ctrlc) has never run. Windows has no catchable SIGTERM, so
      a GUI there should send `shutdown {cancel: true}`.
- [ ] On Windows the drive watcher has no device id or inode (size and mtime only).
- [ ] `watch` with real USB drives, and on Linux and Windows; the TUI's hot-plug now goes through
      the library's watcher (tested with messages only, and on macOS with a disk image).
- [ ] Run the opener on Windows (`rundll32 url.dll,FileProtocolHandler`, used by sign-in,
      `mix --open` and the player's mix report) with a report path that has spaces.

### The player and the TUI
- [ ] Nobody has listened: playback at normal volume, loop wraps (no crossfade), quantized jumps,
      gapless MP3 joins, the start-of-track resampling fix, the underline on the current hot cue,
      and a USB or Bluetooth output unplugged mid-track.
- [ ] The TUI's Drives actions (pull, sync group, pack, restore, eject), the `:` palette and the
      footer hints haven't been used on real drives or in a real terminal.
- [ ] The light palette, `COLORFGBG` detection and `NO_COLOR` were checked in tests and a pty only,
      not in real light terminals (rxvt, Konsole, Terminal.app light profile).
- [ ] Bombadil hasn't been re-run since the message log, the keymap, the palette and the crate
      landed (`bombadil/tui.ts` and `tui.sh` were updated but not run; they need `npm ci`). The
      spec's keys leave out `q`, Ctrl-C/Ctrl-Q, `L` (sign-in), `:` and `O` (`q`, `L` and `:` are
      typed only as text in a text field); the report spec doesn't click "Download the data".

### Software and services
- [ ] `locallibrary collection` against real rekordbox 6 and 7 collections. Inferred: `ColorTableIndex`
      as the hot cue colour, `StockDate` as date added, `Subtitle` as mix name, `rb_local_deleted`
      rows skipped. Intelligent playlists aren't evaluated.
- [ ] Traktor: load `collection.nml` with Import another Collection; check locations (the startup
      disk is assumed to be "Macintosh HD"), grid markers, hot cues and the MP3 timing offset.
- [ ] Serato: check that crates whose tracks Serato hasn't seen get picked up, and how
      `--write-tags` cues and colours look. FLAC and M4A Markers2 aren't written. Crate names are
      cut to 200 bytes, so a very deep or long folder path loses its `%%` nesting there (children
      of a cut crate can collide and get ` (2)`).
- [ ] Engine DJ: open an exported library in Engine DJ Desktop 4.x/5.x, and on a player with
      `--paths drive`. Grid, cue and loop blobs aren't written.
- [ ] `store copy` against a real NAS, SMB/AFP share or cloud-synced folder; resume after
      unplugging a share mid-copy.
- [ ] `watch` at login: the launchd plist and systemd unit in the README have never been run, and
      whether macOS asks a LaunchAgent for removable-volume access.

## Decisions waiting on you

- [ ] **What goes public with the repo** (the October audit): whether TODO.md stays in it (it
      names your drives), and the OneLibrary passphrase, which is in the fixtures' `make.sh` (a
      script that needs it literally) as well as `onelibrary/cipher.rs`. The history is under the
      first push.
- [ ] **Behaviour changes from today's work, worth a look** (all deliberate, all reversible):
  - Track selectors are the intersection of the fields given (they were the union); blank filters,
    queries and names, and filter terms with nothing to look for (`genre:|`), are refused.
  - Every write plans and asks; `--json` needs `--yes`; exit codes are 0 done, 1 error, 2 usage,
    3 not applied, 4 problems found, 5 partial, 130 cancelled. `format` and `provision --format`
    need the disk name (`--erase NAME`). Archive and retired drives are refused by every writer
    without `--overwrite-archive`.
  - The CLI was regrouped (old names are hidden aliases); "drive set" is now "drive group" and the
    `gig` role is `set`; JSON output is one envelope `{schema, plan, report, applied, outcome}`.
  - Pack pulls a drive back from a set into the store before packing again; compare treats a pack
    and its source as different libraries; `--keep-extra` deletes old database files; sync refuses
    when a file of yours sits where a track's audio goes outside `Contents/`; an interrupted sync's
    target compares as the older drive; a cut-off `--delete-first` sync leaves the drive with no
    library (rather than one missing files) until a plain `sync` finishes it.
  - When a playlist and a folder share a path, the bare path means the playlist (trailing `/`
    means the folder); fill, unfiled, smart and import match names exactly; a path that matches
    several playlists only when ignoring case is "not found" (and refused, naming them, when
    filling one).
  - Settings, firmware and recordings writes give an unadopted drive a `state.json` (for the
    pending marker); the player can now create the store (pull, saving a smart rule).
  - Earlier: pinning a hidden profile track is an error; sets of 1-2 tracks get reordered;
    `CON` in a file name becomes `CON_`; `store gc` refuses when any snapshot's manifest can't be
    read.
- [ ] `engine.sql` (Engine DJ export) restates Engine DJ's schema as libdjinterop (LGPL-3.0)
      describes it; no code was copied. Confirm you're comfortable with it.
- [ ] Jobs (`refresh`, `set`) re-plan a later step on the drive as it is after earlier steps, so a
      step can differ from what was confirmed (they do stop when another drive is mounted where
      one of theirs was). Refuse instead when it differs materially?
- [ ] `log`, `store info` and `store copies` print UTC while `store prune` prints local time
      (its days and weeks are local). Pick one.
- [ ] **`clean` backups:** deleted files are backed up so `undo` works, but the space isn't freed
      until the backup is pruned, and backups holding audio are now kept apart (the newest 10),
      so their space stays used longer. The alternative is backing up only the databases.
- [ ] **`clean` on DNA:** only dry runs so far. DNA still has the 48 rekordbox demo-sample tracks
      (98 MB); they come back on the next export unless they're also removed in rekordbox.
- [ ] Player: is key lock worth a C++ time-stretch dependency? Should playback move into the
      library crate so a desktop app can use it? (Settled: the player writes to drives with a
      confirmation and a backup; hot cue pads are `1`-`8` in full screen or with `'`.)
- [ ] Should `paths.rs` also ignore the old `.olib-tmp-*` prefix (drives written by an
      interrupted pre-rename build)?
- [ ] `vendor/rekordbox-pdb/README.md` still says "stick" 3 times, and its tests say "Gigs"
      (vendored code, left alone).

## Open bugs and gaps

### Drives and copies
- [ ] Merge: running again after a cancelled merge renames tracks whose files the cancelled run
      left ("x (2).mp3"); merge's copies set the modification time before they're flushed.
- [ ] A drive synced with `--keep-extra` isn't recognised by content in `fleet::recognise` (only
      compare, status and push know its alias).
- [ ] A cut-off sync can lose the drive's player name and colour from `exportLibrary.db`
      (`player::preserve` only runs when a run finishes); `--delete-first` widens the window.
- [ ] Pack and sync keep their own resumable copy outside `commit.rs`; decide whether pack should
      stage its databases through the shared routine.
- [ ] The archive guard is enforced in the api/op layer only; library callers of `edit::apply` and
      similar bypass it.
- [ ] Still request-based with their own dry-run or confirmation: `store copy`, `drive verify
      --repair`, `drive fsck`, provision steps.
- [ ] Pack's text output doesn't print the saved snapshot or the backup id (only `--json` does).
- [ ] `undo` of `drive player` doesn't forget the name and colour remembered in `state.json`, so
      `refresh` would put the undone name back.
- [ ] Redo: the backup an undo makes doesn't carry the journal, so restoring an older backup, then
      a newer one, puts edits back on the drive that the journal has already forgotten.
- [ ] A replay cut off by a crash leaves its backup without `after`: running the replay again
      finishes it, but taking it back needs `undo --force`.
- [ ] `drive verify` hashes only library files, so a drive holding player files (`PIONEER/LOG`...)
      can't be matched by its legacy fingerprint to a snapshot taken before those stopped counting
      as library data.
- [ ] Merging into OneLibrary from an export.pdb-only source creates no cue rows
      (`cue_edit::rows_from_analysis` could be reused).
- [ ] Twin playlists (sharing a path) with equal `sort_order` are ordered by id, which differs
      between export.pdb and OneLibrary.
- [ ] `status`'s "rekordbox exported over N recorded edits" uses export.pdb's mtime, so a player
      writing history after a set counts as newer, and a CDJ-3000X playlist edit can make a replay
      look like it changes something. The TUI's replay offer has the same heuristic, and `EditPlan`
      counts every `SetFields` track as changed.
- [ ] `set` syncs one pair at a time; it could use the parallel multi-drive sync when no drive came
      back from a set.
- [ ] `check` and `set` don't remember the last `--players` for each drive: `check` would have to
      write `players.json` (under the store lock the watcher may hold), `PlayersFrom` would gain a
      variant (RPC schema), and `players show`/`clear` would need a per-drive form.
- [ ] `check` scans each drive twice (`Side::from_device`, then `device.scan`); reusing the tree
      needs a `Side` built from a pre-scanned tree without changing the fingerprint.
- [ ] Optional DRIVE for `plays lists`, `plays stats`, `export`, `edits list`, tags/cues,
      `drive recordings` and `drive verify`; RPC methods for the default drive and the drive group.
- [ ] The setup tip repeats on commands that never create the store (`drive ls`, `drive disks`)
      until one exists.
- [ ] Embed backups (`art unembed`) are whole-file copies in the store (last 5); they could be
      tags-only, and `store info` doesn't count them.
- [ ] Pruning keeps each drive's last-seen state, but if a drive back from a set is registered at its
      unsaved set state, the snapshot it last shared with its partner can be pruned; sync then
      refuses as diverged (safe, needs a pull or merge). `store prune` reads every snapshot's
      databases to protect history lists: time it on DNA's store.
- [ ] Nothing warns when a command writes to a store copy opened with `--store`; the next `store
      copy` refuses instead (without `--force`).
- [ ] Snapshots `watch` saves beside the head aren't shown by `log` (`plays lists --all` and prune
      see them). Under `watch --sync-group`, every drive the sync writes is scanned again.
- [ ] `watch --once` keeps waiting while the store stays locked (a locked drive is retried every
      settle period); Ctrl-C stops it.
- [ ] `Device::lock` retries for 500 ms before `Locked` (a lock just released could stay held while
      another thread's child process started).
- [ ] OneLibrary's content hash leaves out `property.deviceName`, `backGroundColorType` and SQLite's
      header write counters; older lineage is matched through the legacy hash. Watch for drives
      that suddenly show as changed.
- [ ] KEYCHAIN had an "interrupted operation" marker from 2026-10-04 16:56, never investigated.
      Quitting the TUI during a write could leave one; that's fixed now. Check `locallibrary
      status` next time it's mounted.

### Sidecar and desktop contract
- [ ] `provision.apply`'s `stopped.step` is prose, not the progress step ids; the CLI's own progress
      bars ignore `target`/`step` (multi-drive sync still relies on the "Name: " path prefix).
- [ ] `cues.list` still decrypts OneLibrary on every call; only the library is cached. Cache
      staleness is possible on file systems with coarse timestamps when another program rewrites
      a database at the same size and file id.
- [ ] `check.rs` findings and the interop notes (`engine.rs`, `serato.rs`) still name CLI commands
      and flags; give them structured actions like errors have.
- [ ] Mix reports written by `mix --open/--html` and the TUI have no `.json` beside them, so
      `mix.reports.list` shows them without metadata.
- [ ] Review the method registry's `long_running` and `destructive` flags (`drive.fsck` and the
      mix methods were fixed in the audit).
- [ ] `watch --json` event lines and read-only array outputs carry no `schema` field.

### The player and the TUI
- [ ] Loops rekordbox marks active (PCPT status 4) don't engage on their own: `anlz::cues` takes the
      .EXT list, which doesn't carry the flag.
- [ ] Quantize, auto cue and single play could default from the drive's My Settings.
- [ ] `d` lists the outputs (cpal asks each device for its configs) and opens the chosen one on the
      UI thread, so a slow Bluetooth or AirPlay device can stall it briefly: move both to a
      worker. Gapless across different sample rates starts the next track as the last ends (not
      sample-exact).
- [ ] MP3 loops longer than 30 s seek accurately on the audio thread each time round: a short
      silence per pass on a slow drive.
- [ ] Long VBR MP3 recordings (over 40 MB) seek coarsely by byte position, so the playhead can be
      a few seconds off in a VBR set.
- [ ] Pressing space while a track is still opening is ignored (noticeable on slow USB drives).
- [ ] My Tags are read once per library load; tag edits made outside the TUI need `R`.
- [ ] SIGINT or SIGTERM sent to the TUI with `kill` goes to the CLI's ctrlc handler: the second one
      exits (after any database swap under way) without restoring the terminal. Ctrl-C typed in
      the TUI is a key and is handled.
- [ ] The TUI still uses `Device` directly for the playing track's analysis, beat grid and artwork,
      the missing-audio check, the drive-presence poll, and a mix's analyses, export and report.
- [ ] No TUI surface for store copy, prune or labels; `refresh` and `set` are in the palette only
      as their steps.
- [ ] "Mixes into" suggestions only cover the drive whose library is open; `harmony::suggestions`
      still clones every suggested track.
- [ ] The master.db reader and the Serato export don't read memory cue colours or stored loop
      lengths yet.

## Mix identification (`locallibrary mix`)

Checked against three recorded sets and DNA's library: 23, 7 and 21 tracks found, every played
tempo agreeing with its neighbours (150, 148 and 140 BPM sets).

- [ ] Nobody has listened along to confirm the tracklists or the in/out points by ear.
- [ ] Riding the pitch fader mid-track splits a play into chains with a slightly different speed
      each; the report shows one averaged speed.
- [ ] Transitions shorter than ~20 s often show as a small gap: fingerprints can't hear a track
      under heavy filtering or effects until it's nearly full volume.
- [ ] Reports and exports carry a cover only when it's embedded in the recording (an image found
      next to it shows in the TUI only); a `mix --cover FILE` option would let you choose one.
- [ ] Matching looks at one drive's library; tracks only on another drive (or only in rekordbox)
      show as "not in the library". `collection` could now supply rekordbox's tracks.
- [ ] Identifying an Audacity project doesn't leave out the WAV exported from it when that WAV is
      in the library, so it can still match itself (a recording on the drive is left out).
- [ ] Index entries stay `u64`; `u32` would halve them for libraries under 65,536 tracks. The
      sidecar keeps up to four drives' identifiers resident (`LibraryCache::new(4)`); consider
      capping that by size.
- [ ] A file whose container claims huge counts can still abort `mix`, `check` or the player on
      Linux without overcommit and on Windows: symphonia's isomp4 sizes a `Vec` from them, and an
      allocation failure can't be caught. Needs an upstream fix or a pre-check of the table sizes.
- [ ] Upload a `mix --export` to a mix through a request on a running Local Network.
- [ ] The Mixes view lists the open drive's MASTER REC recordings and ones opened before; a
      configurable folder of sets would save typing paths.

## DJ profile and publishing (`locallibrary profile`, `account`)

- [ ] **Nobody has signed in all the way yet.** Run `locallibrary account login <your handle>`,
      approve in the browser, then `account status --check` and `account logout`; try the TUI's
      Profile view (`L`) too. Tests use session files and an in-memory keychain, so the computer's
      keychain hasn't been used anywhere. Also check that a first real publish's `swapRecord:
      null` is taken as "must not exist", and what a real revocation answers.
- [ ] **Put `https://locallibrary.localnetwork.computer` online** (served by the Local Network web
      app). Regenerate the hosted client document with `locallibrary account client-metadata -o
      oauth-client-metadata.json` and deploy it there, so it lists the narrowed publish scopes
      (`repo:<nsid>?action=create&action=update`) before releasing publishing. While it's offline,
      sign-in falls back to the development client; online without those scopes, a publish
      sign-in stops and points at `--dev-client`. jacquard sends `application_type: web` for a
      hosted ID while the document says `native`: untested.
- [ ] Publish the three Lexicons (`lexicons/computer/localnetwork/locallibrary/`) as
      `com.atproto.lexicon.schema` records and add the `_lexicon.locallibrary.localnetwork.computer`
      TXT record.
- [ ] First real run of `settings publish`, `profile publish`, `plays publish` and `settings import
      --from-handle`; check what a real PDS answers to a write without the scope (the refusal
      mapping is a guess). Everything so far ran against a mock PDS only.
- [ ] Publish sets from `mix` reports. Unpublishing (`deleteRecord`) needs `action=delete` added to
      `PUBLISH_SCOPES` and the hosted document. The Lexicon validator counts characters, not
      graphemes, for `maxGraphemes`.
- [ ] File the jacquard-oauth issue: `OAuthClient::callback` (0.12.2, `callback_core` in
      `client.rs`, and the same `expect` in `request.rs`'s `par`) panics when a token response
      grants a scope `Scopes` can't parse (`Scopes::new("atproto foo:bar")` fails with
      `UnknownPrefix`). Once a release returns an error, drop `readable_scopes` in
      `atmosphere/net.rs`.
- [ ] Copies of one store share its keychain item (`atmosphere:<did>@<store id>`); give the item a
      per-folder identity if copies are ever used as stores on one computer.
- [ ] jacquard's error text sometimes leaves out the cause (such as the address refused inside
      identity resolution), so a refused sign-in can read only "signing in: ...".
- [ ] Trait thresholds (60% of transitions harmonic, 80% within 3% tempo, 2 BPM for "builds") are
      first guesses. Sets found by `mix` don't count yet. DNA's 4 sets are only on the drive:
      `locallibrary pull DNA` keeps them.

## Roadmap items still open

- Next Features: `diff` against stored snapshots exists; still open are phrase and 3-band
  waveform data in the library crate, and My Tags from `exportExt.pdb` checked against a real
  tagged drive.
- Player Feature Map: done are hot cue pads, beat jump and quantize, loops, gapless, output
  device, single play, auto cue, sortable columns, remembering the session, and playable mix
  suggestions. Open: using the drive's own display settings, key lock, tempo fader, phrase jumps,
  3-band waveforms, headphone pre-listen, media keys, level matching.
