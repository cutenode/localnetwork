# Dangling tasks

Updated 2026-10-07 from the TODO triage: axed items are gone, decided ones are written as tasks,
and hardware checks are grouped into test days. Tick items off or delete them as they land.

## Before the first push and release

- [ ] **Nothing is pushed and there's no remote.** The root commit and `main`'s history hold
      session and artifact links and personal paths, so a public push needs a history rewrite or
      a fresh first commit. The commits on `integration` past `main` are unsigned: re-create them
      signed on a `bnb/` branch from `main`. Then create `github.com/cutenode/localnetwork`
      (already set as `repository` in the root `Cargo.toml`) and push.
- [ ] **CI has never run.** On pull requests and pushes to `main` (Markdown-only changes skip it):
      fmt, clippy, nextest on macOS/Linux/Windows, the msrv job (`cargo +1.91 check`, not yet run
      against the updated lockfile), docs, cargo-deny, and a slim `features` job (clippy on the
      headless CLI without ALSA headers). Linux runs the tests under coverage, with thresholds
      (`.cargo/config.toml`) measured in an arm64 Linux container: set them just under what the
      first run measures. Weekly and on demand, `locallibrary-scheduled.yml` runs Bombadil, the
      no-default-features tests, the cargo-hack feature powerset and cargo-deny's advisories:
      after the first push, run it once (Actions > locallibrary scheduled > Run workflow). The
      Linux and Windows legs have never run, and the Windows-only code (raw-volume FAT32 writer,
      `fsutil::flush` with a write handle, the clippy lints fixed by reading, paths compared
      without the `\\?\` prefix, the mix report's `file:///E:/` links) has never compiled. The
      no-`playback` and no-`account` builds have only been built on macOS, and the feature jobs
      don't cover Windows.
- [ ] Rename the packages before v0.1.0, when installer names become permanent: the CLI package
      to `locallibrary` and the library to `locallibrary-core`, keeping `[lib] name =
      "locallibrary"` so no `use` changes. Release tags then become `locallibrary-v0.1.0` (update
      CONTRIBUTING.md's "Releasing"). Until then a `locallibrary-v…` tag picks the library
      package, so releases are tagged `locallibrary-cli-v…`.
- [ ] Drop the pre-release compatibility aliases (the hidden CLI commands in `main.rs`, the `gig`
      role, the RPC method aliases): they only map from builds nobody else ran, and after v0.1.0
      they're public contract. Fold CHANGELOG "Changed" into "Added" at the same time (it still
      lists `mirror` and `push`).
- [ ] `dist plan` runs only on PRs that touch a `Cargo.toml`, `Cargo.lock`, `dist-workspace.toml`,
      `rust-toolchain.toml` or `locallibrary-release.yml`; the rename PR will trigger it. Then try
      the release pipeline with `locallibrary-v0.1.0-rc.1` (which skips the Homebrew push) before
      tagging `locallibrary-v0.1.0`.
- [ ] Third-party notices for the release archives and the Homebrew formula: generate them with
      `cargo about` in `locallibrary-release.yml`, plus rekordbox-pdb's MIT notice for the vendored
      code and `crates/locallibrary/src/convert/template.pdb`. Keep the task here or in
      CONTRIBUTING.md's release checklist, not both.
- [ ] Jobs (`refresh`, `prep`) re-plan later steps on any `StalePlan`, including a rekordbox export
      mid-job, which breaks `op.rs`'s promise that the confirmed plan is what runs. Re-plan only
      drives this job already wrote; refuse otherwise.
- [ ] `clean` backs up only the databases. It deletes only rekordbox's demo samples, which every
      export brings back, yet `refresh` runs it after every export, so audio backups pile up and a
      full drive can't be cleaned at all.

## Split-out crates

- [ ] Decide whether and when to publish the eight crates in `crates/` (loclib-progress,
      loclib-model, loclib-equipment, loclib-mysettings, loclib-tracklist, loclib-mixid,
      loclib-interop, loclib-waveform) to crates.io. The names were free on 2026-10-09, and `cargo
      publish --dry-run` packages all eight. Publish in dependency order (loclib-progress, then
      loclib-model, then the rest); each `test-util` feature would ship as public API.
      locallibrary itself can't be published while it depends on the vendored rekordbox-pdb
      (`publish = false`; crates.io's 0.1.0 lacks the patches), which the next item decides.
- [ ] Upstream rekordbox-pdb PRs, waiting on your go-ahead: 13 branches drafted against
      fragmede/rekordbox-pdb-rs, unsigned (re-create them signed), nothing pushed. Recommendation:
      open the first four bug fixes (`bnb/corrupt-file-guards`, `bnb/slot-cap-new-page`,
      `bnb/isrc`, `bnb/export-ext-reader`) as a probe, since the repo has had no activity since
      July. If there's no answer in three or four weeks, publish the fork under a new crate name
      (MIT, keeping fragmede's copyright line) and depend on that, which also lets locallibrary go
      on crates.io. The branches are in `~/GitHub/fragmede/rekordbox-pdb-rs`, with their titles and
      bodies in `~/GitHub/fragmede/PRS.md`.

## Test days

### 1. Spare drive on players (an export.pdb player and the CDJ-3000X)
- [ ] **Every writer, then `undo`:** playlist/track edits, `tags`, `cues`, `merge`, `clean`,
      `art set`, `convert`, `pack`, `refresh`, `prep`, `drive player`, `settings apply`,
      `drive firmware` and `edits replay`, each loaded on both players. No OneLibrary write has
      been loaded on a player yet. Include a merge from a source with no `exportLibrary.db`
      (`analysedBits` and `contentLink` are guessed from the target's tracks; cues should show).
- [ ] Whether OneLibrary-only players read My Settings from `*SETTING.DAT` (one `settings apply`).
- [ ] Which files each player writes during a set, and whether it checkpoints `exportLibrary.db`:
      snapshot, plug in, play, eject, `locallibrary diff --summary`.
- [ ] Whether players show OneLibrary's `deviceName`, what colour codes 1-8 look like, and where
      `export.pdb` players get the Device Name.
- [ ] The TUI's Drives actions (pull, sync group, pack, restore, eject), the `:` palette and the
      footer hints on real drives in a real terminal.
- [ ] My Tags from `exportExt.pdb` on a real tagged drive.
- [ ] Sessions (sets assume each of these): a track on deck 1's stick played from deck 2 is logged
      on deck 1's stick; plugging a stick in and playing nothing leaves no empty history list (or
      does it?); turning a player off and on with the stick in starts a new list; a b2b with two
      sticks leaves each stick only its own tracks.

### 2. Mac desk
- [ ] Clones on real sticks: with three drives of one library plugged in, a playlist edit, an
      `undo` and a `refresh` after a rekordbox export each end with the drives identical
      (`status` says so), and each loads on a player. Time an edit: every write now also syncs
      the other clones.
- [ ] `watch --install` on a real login: the agent loads (`launchctl print gui/$(id -u)/local.locallibrary.watch`),
      survives a log out and in, pulls a drive plugged in afterwards, and `--uninstall` removes it.
      The systemd user service on Linux has never run.
- [ ] What rekordbox's automatic playlist sync (`playlists3.sync` / `playlists3Plus.sync`) does
      with playlists that `merge` added. If it drops them, merge should warn harder.
- [ ] `format` one real stick, and two sticks of one model to confirm the macOS USB serial from
      the IORegistry. Formatting KEYCHAIN is your call.
- [ ] `collection` against rekordbox 7: compare three tracks (hot cue colour from
      `ColorTableIndex`, `StockDate` as date added, `Subtitle` as mix name).
- [ ] Open an Engine DJ export in Engine DJ Desktop 4.x/5.x.
- [ ] Serato (only if you use it): crates whose tracks Serato hasn't seen, and how `--write-tags`
      cues and colours look.
- [ ] `watch` with a real USB stick, and at login: whether macOS asks the LaunchAgent for
      removable-volume access.
- [ ] Ten minutes of listening: normal volume, loop wraps, quantized jumps, gapless MP3 joins, the
      start-of-track resampling fix, unplugging a USB or Bluetooth output mid-track.
- [ ] Listen along to one recorded set and confirm the `mix` tracklist and in/out points by ear.
- [ ] Time `check --audio` on DNA.
- [ ] Export a track with hot cues D-H from rekordbox 7 to a fresh drive: does OneLibrary's `cue`
      table get rows at all (no export checked so far had any), and is D `kind` 5 as in
      `master.db`? `onelibrary/cues.rs` assumes so.
- [ ] Upload a `mix --export` to a mix through a request on a running Local Network.

### 3. Linux box
- [ ] `format`, `fsck`, `eject`, `drive rename --label` and `format --check-capacity` on a stick.
      Once each has run, drop its "untested on this platform" warning, and once CI is green there
      too, the "experimental" labels in the README and docs/install.md.
- [ ] `fsck`'s udisks unmount and remount under polkit (as `$SUDO_USER` under sudo) comes back at
      the same path.
- [ ] `watch` with a stick, and the systemd unit from docs/store.md.

### 4. Windows box
- [ ] The same disk commands, including the erase script's check of the disk's size, serial and
      that it isn't the system disk.
- [ ] `watch` with a stick.
- [ ] The opener (`rundll32 url.dll,FileProtocolHandler`, used by sign-in, `mix --open` and the
      player's mix report) with a report path that has spaces.

## Tests to add

- [ ] A CI-only `format::apply` test on a disk image per OS (hdiutil, a diskpart vdisk, losetup).
      Linux first needs loop devices treated as removable under `LOCALLIBRARY_FORMAT_VIRTUAL`
      (`format/linux.rs`).
- [ ] `eject` on a two-partition image on the macOS leg of that job: refusing when the store is on
      another volume, and naming the volumes it unmounts.
- [ ] `check --audio` on tiny committed MP3, FLAC and M4A fixtures, truncated in
      `tests/it/check.rs` (only WAV truncation is tested). The same fixtures cover MP3 gapless.
- [ ] The underline on the current hot cue, with a ratatui `TestBackend`.

## Open bugs and gaps

- [ ] Found while raising coverage, each a decision: `drive tidy` exits 0 when it couldn't remove
      some files (`recordings` says Partial); `setup --role` is ignored for the drive that becomes
      the primary, even a bad value; a lenient replay that skips `DeleteHistoryLists` only says
      "not found", dropping which list; merging the same drive twice in folder mode makes "(2)"
      copies of every playlist; under `LOCALLIBRARY_VOLUMES`, `format::disk_at` still runs
      read-only `diskutil info`/`lsblk` on the disk holding the test folder.
- [ ] Redo: the backup an undo makes doesn't carry `edits.json` or the remembered player state, so
      undo then redo leaves edits that replay won't redo, and restoring an older backup then a
      newer one puts back edits the journal has forgotten.
- [ ] A replay cut off by a crash leaves its backup without `after`: running the replay again
      finishes it, but taking it back needs `undo --force`.
- [ ] `prep` syncs one pair at a time; it could use the parallel multi-drive sync when no drive came
      back from a set.
- [ ] Nothing warns when a command writes to a store copy opened with `--store`; the next `store
      copy` refuses instead (without `--force`).
- [ ] `players set --dry-run` and `players clear` still make a store with none there: their plans
      take a `&Store` (`api::players_set_plan`, `players_clear_plan`), though with no store there's
      nothing remembered to compare against. Take an `Option<&Store>`, as the reads now do.
- [ ] Changes that look something up in the store make an empty one and then fail or find nothing
      to do: `drive forget`/`role`/`retire`, `sets delete`, `smart rm`, `profile unpin`/`unhide`,
      `store label`/`unlabel`, and the dry runs of `sets playlist`, `smart apply`, `adopt --as`,
      `store gc`/`prune`/`copy`, `library prune` and `art unembed`. Look up with
      `diff_cmd::store_to_read` or `read_or_empty` and make the store only when applying.
- [ ] The sidecar's `drive.verify` is registered as a `read` but makes a store (`store_opt`), since
      `drive_verify` records the check in it (as `drive verify` and `fsck` do on the command line).
      Register it as what it is, or record only into a store that's there.
- [ ] `drive groups` (and the sidecar's `drive.groups`) answers `[]` with no store, though
      `fleet::sets` groups mounted drives by the library id each one holds and listed them
      (unnamed) against a new store. Decide whether no store should still list those groups.
- [ ] KEYCHAIN had an "interrupted operation" marker from 2026-10-04 16:56: run `locallibrary
      status KEYCHAIN` next time it's mounted.
- [ ] `check.rs` findings name CLI commands and flags; structured actions like errors have would
      let the TUI offer one-key fixes.
- [ ] Loops rekordbox marks active (PCPT status 4) don't engage on their own: `anlz::cues` takes the
      .EXT list, which doesn't carry the flag (fixing it changes loclib-model's `analysis::Cue`).
- [ ] `d` lists the outputs (cpal asks each device for its configs) and opens the chosen one on the
      UI thread, so a slow Bluetooth or AirPlay device can stall it briefly: move both to a worker.
- [ ] `mix`: riding the pitch fader splits a play into chains with slightly different speeds; the
      report shows one averaged speed.
- [ ] `mix --cover FILE`: reports and exports carry a cover only when it's embedded in the
      recording.
- [ ] `mix` matches one drive's library; tracks only on another drive (or only in rekordbox) show
      as "not in the library". `collection` could supply rekordbox's tracks.
- [ ] A file whose container claims huge counts can abort `mix`, `check` or the player on Linux
      without overcommit and on Windows: symphonia-format-isomp4's `stsz.rs` sizes a `Vec` from
      them (`TODO: Apply a limit`). File it upstream.
- [ ] `provision` shows the format plan's warnings twice: in the format plan and again in its own
      `warnings`.
- [ ] `cue_edit.rs` sums its backup's size itself; it could use `backup::needed_bytes` like pack
      and clean (its sum also counts staged analysis files).
- [ ] A sync or pack onto a stick with no `exportLibrary.db` takes on the source's player name and
      colour, since there's none to keep. For set sticks, clearing it may be better.
- [ ] Type-check `docs/rpc/locallibrary.d.ts` with `tsc` in CI once something consumes the RPC.
- [ ] Apple signing secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
      `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER`) need a paid
      Developer ID; the shell installer and Homebrew run an unsigned CLI fine. Windows binaries are
      unsigned too (dist supports Azure Artifact Signing).

## DJ profile and publishing (`locallibrary profile`, `account`)

- [ ] **Nobody has signed in all the way yet.** Run `locallibrary account login <your handle>`,
      approve in the browser, then `account status --check` and `account logout`; try the TUI's
      Profile view (`L`) too. Tests use session files and an in-memory keychain, so the computer's
      keychain hasn't been used anywhere. Also check that a first real publish's `swapRecord:
      null` is taken as "must not exist", and what a real revocation answers.
- [ ] Once a real sign-in and publish have worked, drop the "alpha" labels: the `account` and
      `publish` help, the player's `L`, README, docs/account.md, docs/commands.md, docs/sidecar.md
      and CHANGELOG.
- [ ] **Put `https://locallibrary.localnetwork.computer` online** (served by the Local Network web
      app). Regenerate the hosted client document with `locallibrary account client-metadata -o
      apps/web/src/lib/tools/locallibrary/oauth-client-metadata.json` and deploy it there, so it lists the narrowed publish scopes
      (`repo:<nsid>?action=create&action=update`) before releasing publishing. The copy waiting
      in localnetwork has them now; it only needs deploying. While it's offline, sign-in falls
      back to the development client; online without those scopes, a publish sign-in stops and
      points at `--dev-client`. jacquard sends `application_type: web`
      for a hosted ID while the document says `native`: untested.
- [ ] Publish the Lexicons locallibrary writes as `com.atproto.lexicon.schema` records with the
      repo's lexicon publisher (`pnpm -C apps/web lexicons:publish`), and add a `_lexicon` TXT
      record for each of their two authorities: `locallibrary.localnetwork.computer`
      (`lexicons/computer/localnetwork/locallibrary/`, My Settings) and `dj.localnetwork.computer`
      (`lexicons/computer/localnetwork/dj/`, the profile, set and their shared defs).
- [ ] Switch publishing to a permission set (`include:`) once a real sign-in shows whether the PDS
      expands it in the token response; jacquard only resolves includes with its `scope-check`
      feature.
- [ ] First real run of `settings publish`, `profile publish`, `sets publish` and `settings import
      --from-handle`; check what a real PDS answers to a write without the scope (the refusal
      mapping is a guess). Everything so far ran against a mock PDS only.
- [ ] Unpublishing (`deleteRecord`) needs `action=delete` added to `PUBLISH_SCOPES` and the
      hosted document; until then `sets delete` leaves a published set's record on the account.
      The Lexicon validator counts characters, not graphemes, for `maxGraphemes`.
- [ ] `plays` reads each drive's databases twice (for its lists, then for its sessions). Without
      declared sets the profile finds sessions its own way (history lists, dated by the pull that
      archived them) rather than with `sets::session::gather` and its windows.
- [ ] File the jacquard-oauth issue on tangled (nonbinary.computer/jacquard): 0.12.2 and `main`
      panic when a token response grants a scope `Scopes` can't parse, in `callback_core`
      (`client.rs`) and on every refresh (`session.rs`'s `update_with_tokens`). Once a release
      returns an error, drop `readable_scopes` in `atmosphere/net.rs`.
- [ ] Trait thresholds (60% of transitions harmonic, 80% within 3% tempo, 2 BPM for "builds") are
      first guesses.
