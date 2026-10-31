# Dangling tasks

Updated 2026-10-07 from the TODO triage: axed items are gone, decided ones are written as tasks,
and hardware checks are grouped into test days. Tick items off or delete them as they land.
Open work left by the 2026-10-11 and 2026-10-12 sessions was added on 2026-10-12.

## Before the first push and release

- [ ] **Nothing is pushed and there's no remote.** The root commit and `main`'s history hold
      session and artifact links, personal paths and nine `Co-Authored-By` trailers (108c7bd,
      69bc77d, 5542708, 2c06b9e, 4feaf17, 2defa0c, 8d68605, be4c221, e2f1d5a), so a public push
      needs a history rewrite or a fresh first commit. The commits on `integration` past `main`
      are unsigned: re-create them signed on a `bnb/` branch from `main`. Then create
      `github.com/cutenode/localnetwork` (already set as `repository` in the root `Cargo.toml`)
      and push.
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
      don't cover Windows. `bnb/ci-runners` adds more that hasn't run either (CodSpeed benchmarks,
      fuzzing, cargo-mutants, CLI timings, a release dry run, Lighthouse, axe, a Docker smoke run,
      Stryker, npm advisories, workflow linting): fold them into this list once it's merged.
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

## Waiting on you

- [ ] Commit the TUI popup-header fix sitting uncommitted in `localnetwork-tui-popup-header`
      (`bnb/tui-popup-header`, 4 files). Until it lands, `library_walks_keep_the_player_whole`
      fails on `main`, since `.hegel/` replays the saved 80x3 case. Its Bombadil `overlay()`
      change hasn't been run.
- [ ] Delete the merged clone at `~/GitHub/cutenode/localnetwork-collection`.
- [ ] Sign rekordbox out of Dropbox and back in: a session printed the OAuth tokens from
      `rekordbox3.settings` into its transcript, and the file hasn't changed since Oct 8.
- [ ] HappyView keys: the web app reads `HAPPYVIEW_API_KEY` (`apps/web/.env.local` has the local
      dev one); locallibrary must never hold HappyView's admin key, so delete
      `apps/locallibrary/.env` (its `HAPPYVIEW_ADMIN_TOKEN` and `HAPPYVIEW_PUBLIC_URL` are read by
      nothing; locallibrary's HappyView is `LOCALLIBRARY_HAPPYVIEW_URL`, defaulting to production)
      and rotate that token if it's the hosted HappyView's. Give the deployed web app the hosted
      HappyView's token under `HAPPYVIEW_API_KEY`: without it, `requestIndex` can't have HappyView
      read an account again.
- [ ] Run the audit: the one asked for on 2026-10-11 stopped when its area subagents were turned
      down.
- [ ] Delete the Hegel coverage session's 11 GB of logs and lcov exports in
      `/private/tmp/claude-501/-Users-cyren-GitHub-cutenode-localnetwork/ad693dc7-2dd1-40a7-9cb5-b96ac5545ebf`.

## Split-out crates

- [ ] Decide whether and when to publish the eleven crates in `crates/` (loclib-progress,
      loclib-model, loclib-equipment, loclib-mysettings, loclib-tracklist, loclib-mixid,
      loclib-transcode, loclib-analysis, loclib-interop, loclib-waveform, loclib-plugin) to
      crates.io. The first eight's names were free on 2026-10-09, and `cargo publish --dry-run`
      packaged those eight then. Publish in dependency order (loclib-progress, then
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
- [ ] `collection export` to a stick, loaded on both players.
- [ ] `pack --transcode` WAV copies, the FLAC and AIFF format codes, and cue timing after `track
      upgrade` (AAC too).
- [ ] OneLibrary's `createdDate` is now the local date (`onelibrary/mod.rs:480`): compare it with
      what a rekordbox export writes.

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
- [ ] `store serve` on this Mac and `store fetch` from another computer.
- [ ] The GiantSteps key and tempo benchmark: import the datasets into rekordbox, export them to a
      stick, then run `locallibrary analysis bench` on it.

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

## Open bugs and gaps

- [ ] Found while raising coverage, each a decision: a lenient replay that skips
      `DeleteHistoryLists` only says "not found", dropping which list; merging the same drive
      twice in folder mode makes "(2)" copies of every playlist; under `LOCALLIBRARY_VOLUMES`,
      `format::disk_at` still runs read-only `diskutil info`/`lsblk` on the disk holding the test
      folder.
- [ ] Redo: the backup an undo makes doesn't carry `edits.json` or the remembered player state, so
      undo then redo leaves edits that replay won't redo, and restoring an older backup then a
      newer one puts back edits the journal has forgotten.
- [ ] A replay cut off by a crash leaves its backup without `after`: running the replay again
      finishes it, but taking it back needs `undo --force`.
- [ ] KEYCHAIN had an "interrupted operation" marker from 2026-10-04 16:56: run `locallibrary
      status KEYCHAIN` next time it's mounted.
- [ ] Loops rekordbox marks active (PCPT status 4) don't engage on their own: `anlz::cues` takes the
      .EXT list, which doesn't carry the flag (fixing it changes loclib-model's `analysis::Cue`).
- [ ] `d` lists the outputs (cpal asks each device for its configs) and opens the chosen one on the
      UI thread, so a slow Bluetooth or AirPlay device can stall it briefly: move both to a worker.
- [ ] `mix` matches one drive's library; tracks only on another drive (or only in rekordbox) show
      as "not in the library". `collection` could supply rekordbox's tracks.
- [ ] `cue_edit.rs` sums its backup's size itself; it could use `backup::needed_bytes` like pack
      and clean (its sum also counts staged analysis files).
- [ ] A sync or pack onto a stick with no `exportLibrary.db` takes on the source's player name and
      colour, since there's none to keep. For set sticks, clearing it may be better.
- [ ] Type-check `docs/rpc/locallibrary.d.ts` with `tsc` in CI once something consumes the RPC.
- [ ] Apple signing secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
      `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER`) need a paid
      Developer ID; the shell installer and Homebrew run an unsigned CLI fine. Windows binaries are
      unsigned too (dist supports Azure Artifact Signing).
- [ ] `collection export`'s space check counts `written.needed_bytes` but not `download_bytes`
      (online-only Dropbox files that download onto the boot disk): both the fit warning
      (`collection_export.rs:276`) and `apply`'s `NoSpace` check (`:473`) miss them. A full export
      to `~/rekordbox-export` stopped at 1.8 of 25 GB.
- [ ] Not investigated: `track show` may show the My Tags of another track that shares its file,
      and `account logout` may forget the handle after a refused status check.
- [ ] Two property tests failed once and passed on reruns, with no saved case:
      `drive_reads_say_what_their_json_says`, and rekordbox-pdb's `edits_match_the_model` (during
      two coverage runs at once).

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
      apps/web/src/lib/tools/locallibrary/oauth-client-metadata.json` and deploy it there, so it asks
      for the publishing permission set (`include:computer.localnetwork.locallibraryPublishing`)
      before releasing publishing. The copy waiting in localnetwork has it now; it only needs
      deploying. While it's offline, sign-in falls
      back to the development client; online without those scopes, a publish sign-in stops and
      points at `--dev-client`. jacquard sends `application_type: web`
      for a hosted ID while the document says `native`: untested.
- [ ] **Publish every lexicon** as `com.atproto.lexicon.schema` records in the root account's
      repo, keyed by NSID, the way upvote.at publishes `at.upvote.*`, with a `_lexicon` TXT record
      (`did=<root DID>`) for each of the 11 authorities (each sub-namespace is its own; upvote.at
      has one, at `_lexicon.upvote.at`). None exist yet. One command does both:
      `ROOT_IDENTIFIER=localnetwork.computer ROOT_PASSWORD=… CLOUDFLARE_API_TOKEN=… pnpm -C
      apps/web lexicons:publish`. Sign-in to publish fails with `invalid_scope` until
      `computer.localnetwork.locallibraryPublishing` resolves, and so does the website's until
      `computer.localnetwork.permissions` and the space types do. Until the set is published with
      its `rpc:computer.localnetwork.directory.requestIndex?aud=*` permission, sessions can't have
      the directory read an account again (plans say so); deploy the web app's `requestIndex` and
      `dj/set.lua` with it.
- [ ] First real run of `settings publish`, `profile publish`, `sets publish` and `settings import
      --from-handle`; check what a real PDS answers to a write without the scope (the refusal
      mapping is a guess). Everything so far ran against a mock PDS only.
- [ ] First real `sets delete` of a published set: check a real PDS grants
      `repo:computer.localnetwork.dj.set` and `repo:computer.localnetwork.dj.recording` (all three
      actions) from the permission set, and
      answers `deleteRecord` of a record that's already gone as done.
- [ ] A PDS without permission-set support: untested. If it drops `include:`, the session gets
      only `atproto`, publishing reports a missing scope, and sign-in loops.
- [ ] File the jacquard-oauth issue on tangled (nonbinary.computer/jacquard): 0.12.2 and `main`
      panic when a token response grants a scope `Scopes` can't parse, in `callback_core`
      (`client.rs`) and on every refresh (`session.rs`'s `update_with_tokens`). Once a release
      returns an error, drop `readable_scopes` in `atmosphere/net.rs`.
- [ ] Trait thresholds (60% of transitions harmonic, 80% within 3% tempo, 2 BPM for "builds") are
      first guesses.

## Features and decisions

- [ ] `store serve` and `store fetch` aren't in the sidecar.
- [ ] Decide: an additive OneLibrary convert brings back a track a player removed when the other
      database still has it (`--replace` is the way out, `docs/editing.md`).

## Lexicons

- [ ] Swap locallibrary's own Lexicon validator (`atmosphere/lexicon.rs`) for jacquard-lexicon's
      `SchemaValidator`, already in the dependency tree: check its error wording, unknown fields
      and the `uri` format first.

## Website (`apps/web`)

- [ ] Dev logs SvelteKit's `ssr_fetch_eager` warning on every page load.
- [ ] A mix can only link a set once the project is published: HappyView 2.16 can't tell a
      backfill from Jetstream, so `dj.set` records are let in only from a published project's
      managers, and the new-project forms have no set picker.
- [ ] Look at `/planner/shows/[rkey]` since the em-dash pass, and give it an axe check like
      `/planner/shows/new` has.
- [ ] `@lhci/cli` 0.15.1 (Lighthouse, on `bnb/ci-runners`) brings 7 advisories into the dev
      dependencies (tmp, uuid, extract-zip, basic-ftp, sprintf-js).

## localtoolbox

- [ ] Move to the monorepo copy: export state from the old extension, load
      `apps/localtoolbox/dist` unpacked, import, then delete
      `~/GitHub/bnb/soundcloud-playlist-to-text`. `src/manifest.json` has no `key`, so every new
      unpacked path gets a new extension ID.
- [ ] About 1,370 em dashes, 348 of them in a 182 KB README. Remove them, trim the README, and
      widen `apps/web/src/writing.test.ts`'s guard to the toolbox.
- [ ] Tests for Stryker's surviving mutants: required fields in `state-transfer.ts`, regex anchors
      in `profiles.ts` and the site adapters, the bulk-probe guard in `content.ts`.
- [ ] Drop the Stryker vitest-runner patch once stryker-js#6214 ships.
- [ ] Decide: zod (already `zod/mini`) adds about 9 KB to the startup chunk; valibot is smaller.

## Dependencies and upstream

- [ ] symphonia 0.6.1 is out and fixes the AIFF SSND bug `loclib-mixid` works around
      (`aiff_frames` in `decode_native`, `audio.rs`). stratum-dsp 1.0.0 still needs symphonia 0.5, so moving means building
      two symphonias until it follows.
- [ ] flacenc's STREAMINFO min-block fix (PR #255) is merged but unreleased (0.5.1): pin the git
      rev, keep the workaround, or ask for a release on #242.
- [ ] File the rubato `output_delay()` off-by-one: the draft, patch and repro are in
      `/private/tmp/claude-501/-Users-cyren-GitHub-cutenode-localnetwork/6f77efa7-5cd0-458f-ba9f-125166b35c1f/scratchpad/upstream/`
      (re-read `rubato-issue.md` first; it changed after it was drafted).

## Performance

- [ ] Compare fat and thin LTO (`lto = "thin"` in the root `Cargo.toml`). On one machine, plugged
      in, with `cargo clean` between: build `cargo build --release -p locallibrary-cli` as is and
      with `CARGO_PROFILE_RELEASE_LTO=fat`, and record for each the build's wall time, the size
      of `target/release/locallibrary` and of `target/`, and the release run time of `convert` on
      a 5,000-track drive, `check` on a drive and the `bnb/perf` criterion benches. Switch only if
      the run times win by more than their run-to-run spread. Needs about 15 GB free.
- [ ] Applying many set-fields ops to the drive's databases still finds each op's tracks by
      building a map of every track (`ids_of` in `edit::apply_op`, and `PdbDb::set_fields`'s
      `self.lib.track`), so a whole-library key and BPM import's apply is O(ops x tracks), as is
      planning a replay of one (its edits name tracks by path, which the plan's per-track model
      in `edit/track_index.rs` doesn't take yet).
- [ ] `merge.rs` finds each new track by scanning the source (`src_lib.track`, five call sites),
      O(new x N). Use `Library::track_lookup` from `bnb/perf` and measure it.
- [ ] Once `bnb/perf` and `bnb/ci-runners` are both in, make the workspace's `criterion`
      `codspeed-criterion-compat`, as `locallibrary-bench.yml` already says it is, and connect
      the repo in CodSpeed after the first push (the free tier needs it public).
- [ ] `a_click_does_what_it_points_at` (`tui/app/props.rs`) draws once more than it needs for
      kinds 9, 12 and 13, about 15% of its renders.

## CI and tooling

- [ ] The shared `.git/hooks/pre-commit` hard-codes `localnetwork-ci`'s lefthook and otherwise
      looks in the worktree's own `node_modules`, so worktrees without one run no hooks. The hooks
      also fail under Node 25, since the workspace is engine-strict on 24.
- [ ] Dependabot doesn't update `apps/locallibrary/bombadil/package-lock.json` (its npm entry only
      watches `/`).
- [ ] A fuzz target for `PdbEditor`'s edits (the `pdb` target only parses).
- [ ] `apps/web/.vscode` does nothing when the root is opened: move it to the root or delete it.
      `apps/web/static/robots.txt` is SvelteKit's stock file, and `**/*.rs.bk` in `.gitignore` is
      left over.
- [ ] Outside this repo: the dotfiles' setup still installs brew `rust` rather than `rustup`
      (`setup/linux.sh:56`, `setup/macos.sh:240`); the PATH fix only helps once rustup is there.
