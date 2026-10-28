# Building a GUI

All operations live in the library's [`api`](../../../crates/locallibrary/src/api.rs) module as
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
  if (msg.method) return onEvent(msg.params); // `event` notifications (events.subscribe, watch.start)
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
const preview = await call("sync.group.plan", { library: "Home" }).result;
if (!preview.noop && (await confirmDialog(preview.summary, preview.warnings))) {
  // Exactly what the dialog showed: refused with `stale_plan` if a drive changed since.
  const job = call("sync.group.apply", { plan_id: preview.plan_id });
  // cancelButton.onclick = () => call("$/cancel", { id: job.id });
  await job.result;
}
```

- **Trust.** Only a trusted parent process may spawn and drive the sidecar: it acts with your
  rights. Requests name files and folders it reads and writes (`output` for exports,
  `plugins.export.*` and `mix.export`, `to` for `recordings.collect.*`, `path` for `store.copy`,
  images and audio for `art.*`, `file` for `firmware.*`, a plugin's path params (`file` for the
  tracklist plugin's `import`), `db` for `collection.*`, any folder as a drive)
  and what it asks servers for (`client_id` and `scopes` for `account.login.begin`). Never expose it
  on a socket or to another user, and never pass web content (a page's or a renderer's input) into
  its parameters. It refuses relative paths to write to and a line longer than 16 MiB; those checks
  only catch mistakes.
- **The contract.** [`docs/rpc/schema.json`](../docs/rpc/schema.json) describes every method:
  its `kind` (`read`; `plan` and `apply`, paired; `immediate`, a change with no plan step;
  `control`, about the sidecar itself), its `aliases`, whether it's `long_running` (sends
  `progress`, can be cancelled) or `destructive` (can delete or overwrite something), and JSON
  Schemas for its `params` and `result`, with the `progress` and `event` notifications. A `read`
  never makes a store: with none, it answers as if the store were empty, or fails with `not_found`
  when it's about the store itself (`store.info`, `store.history`, `store.verify`, `drive.log`).
  [`docs/rpc/locallibrary.d.ts`](../docs/rpc/locallibrary.d.ts) has the same as TypeScript types
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
  `settings.apply.plan` / `settings.apply` (`settings.write.*`), `drive.sets` (`drive.groups`)
  and `set.*` (`prep.*`).
  `drive.restore.preview` is kept as it was; `drive.restore.plan` is the plan `drive.restore.apply`
  applies (an `undo` plan), as `drive.eject.plan` makes `eject` plans. A few methods are a single
  verb, as the CLI's commands are: `status`, `compare`, `diff`, `check`, `doctor`, `export`;
  `refresh` and `prep` are jobs of several steps.
- Every `*.plan` answers with the plan's own fields plus `plan_id`, `operation` (the method's name
  without `.plan`, except `undo`, `eject` and `set` for `drive.restore.plan`, `drive.eject.plan` and
  `prep.plan`), `summary` (a few lines for a dialog), `warnings` (all of them), `destructive` (it
  deletes or overwrites something), `noop` (applying it changes nothing), `drives` (each `{root,
  name, writes, drive}`), `clones` and `planned_at`. A request's `drive` takes a library's name
  too, meaning its lead clone (the newest of its drives plugged in), except in methods about one
  particular drive: its identity, role, health, space, backups and player name (`drive.info`,
  `drive.rename`, `drive.verify`, `drive.space`, `player.*`...), `push.*`, `provision.*` and
  `settings.show`. A plan that changes a
  library is made on that drive and copied onto the library's other drives plugged in when it's
  applied: `clones` lists them, each `{name, root}`, with `left_alone` saying why one isn't copied
  onto (it's newer, diverged or back from a set; also among the `warnings`), and `drives` includes
  those it's copied onto. The sidecar keeps the plan: `*.apply {"plan_id": "..."}`
  applies exactly that plan, once, and fails with `stale_plan` if anything it touches changed since
  (the drive was written, rekordbox exported again, or it would copy other files now), if it was
  applied already, or if it's gone (the newest 64 plans are kept for 30 minutes, shared by every
  worker). Plan again and ask again. An apply refused before anything was written (`stale_plan`, an
  archive drive's `conflict`, `locked`) keeps the plan, so it can be sent again. `*.apply` with the
  request itself, as before, plans and applies in one go: it isn't bound to what anyone confirmed.
- Writes to archive and retired drives are refused (`conflict`) unless the request (the plan's or
  the apply's) has `"overwrite_archive": true`.
- Requests run concurrently, on two pools of workers: quick reads (`tracks.query`, `track.*`
  but `track.fields`, `art.*` previews and paths, `drive.library`, `playlists.tree`,
  `tracks.get`, `tracks.analysis`, `cues.list`, `tags.list`, `drives.*`, listings) in one,
  everything else (writes, plans, scans, mixes, `track.fields`) in the other, so a long job never
  holds up a GUI's reads. Each runs 4 requests at once
  (`locallibrary rpc --workers N`, or `LOCALLIBRARY_RPC_WORKERS`); a request waiting for a worker
  shows in `jobs.list` with `queued: true`, and `$/cancel` answers it at once.
- The sidecar keeps the parsed libraries of the last 4 drives it read, so paging through tracks,
  showing one track after another, cues, My Tags, artwork and `mix.identify` (its fingerprint
  index too) parse a drive's database once. A library is read again as soon as one of the
  drive's database files has a new size, modification time or file id, after any request that
  may have written a drive (an `*.apply`, with the request or a `plan_id`, and `undo.apply`
  among them), and when `events` says the drive changed.
- Progress byte counts and per-file `item` events are held back and sent about ten times a second,
  separately for each drive and step. An `item` stands for the files since the previous one; `items`
  says how many when that's more than one (count `items ?? 1`). Events about one drive of several
  (`sync.many.*`, `sync.group.*`) carry `target`, the drive's root as in the results.
  `provision.apply` announces each step it runs with a `step` event (ids `format`, `name`,
  `library`, `settings`, `role`, `player`, `verify`, `eject`), and the events during a step carry
  `step: {index, total, id}` (`index` counts from 1).
- Ids are strings or numbers. A request whose id a running request already has is refused with
  `-32600` (`kind: "invalid_request"`); an id is free again once its response has been sent.
  Notifications (no `id`) get no response, and `$/cancel` can't reach them.
- `initialize` returns `{protocol: 1, version, capabilities, methods}`. `capabilities` lists
  features a GUI can check for (`progress`, `progress.scope` for the `target`, `step` and
  `items` fields above, `cancel`, `jobs.list`, `shutdown.cancel`, `events`, `plan_id`,
  `describe` for `rpc.describe`, `error.detail` for `error.data`'s `reason`, `fields` and
  `actions`, and `account` in a build with the Atmosphere account); `methods` lists every name,
  aliases included. `protocol`
  changes only when a change would break existing clients. An unknown method gets `-32601`
  (`kind: "method_not_found"`).
- `$/cancel {id}` stops a request at the next safe point. It then fails with `kind: "cancelled"`.
  `jobs.list` returns the running requests, oldest first, as `{id, method, started}` (`id` is
  `null` for a notification).
- Events: `events.subscribe {kinds?, interval_ms?}` returns `{kinds, drives}` (the drives mounted
  now, as `drives.mounted` gives them) and from then on sends notifications `{"method": "event",
  "params": {"event": "drive.attached", ...}}`: `drive.attached`, `drive.detached` (the drive as
  last seen) and `drive.changed` (`changed`: `databases` and/or `state`, its `.locallibrary`
  identity) with the drive's `root`, `id`, `name` and `has_export`; `store.changed` with `changed`
  (`libraries`, `snapshots`, `labels`, `pruned`, `drives`, `settings`, `smart`, `sets`); and
  `job.started` / `job.finished` (`id`, `method`, `ok`, and the error's `kind`) for every request.
  `plugin.notice` is what a plugin tells the user (see [Events](#events)).
  `kinds` takes event names or groups (`drive`, `store`, `job`, `watch`, `plugin`); empty means all. Drives
  and the store are polled every second (`interval_ms`, at least 100) by looking at file metadata
  only, so watching costs a few `stat` calls per drive. Every mounted volume counts, blank ones
  included (`has_export: false`). `events.unsubscribe` stops them.
- `drives.mounted` lists the mounted volumes as `{root, id, name, has_export}` without reading
  any database, cheap enough to poll; `drives.list` reads every drive's library.
- `watch.start {pull?, sync_group?, settle_ms?, interval_ms?}` does what `locallibrary watch` does
  (see [Saving drives as they're plugged in](store.md#saving-drives-as-theyre-plugged-in)) inside
  the sidecar until `watch.stop {cancel?}`, and sends an event for each thing it does, whether or
  not you subscribed (unless a subscription's `kinds` leave them out): `watch.up_to_date`,
  `watch.pending` and `watch.pulled` (`library`, `history_lists`, `beside`, and the `snapshot`),
  `watch.synced` (`targets`, as `sync.many.apply` returns them), `watch.waiting` and `watch.skipped`
  (`reason`), and `watch.failed` (`kind`, `message`), each with the drive's fields as above. It
  shows in `jobs.list` as `{id: null, method: "watch"}`; a second `watch.start` while it runs is a
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
  way. Windows has no SIGTERM to catch, and a GUI's piped child never gets Ctrl-C, so a Windows
  app should send `shutdown {cancel: true}` or close stdin instead.
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
  both) and `rekordbox_sync` (rekordbox's automatic playlist sync to the drive). `drives.list`
  adds `library`, the store's name for the library `library_id` is.
- `version` lists every method. They mirror the CLI: `status`, `drives.list`, `drive.info`,
  `drive.adopt.plan/apply`, `drive.library`, `drive.backups`, `drive.restore.plan/apply` (`{drive,
  backup?, force?}`), `undo.list`, `undo.plan/apply` (`{drive?, backup?, force?}`; either apply
  takes the other's `plan_id`), `compare`, `sync.plan/apply`, `sync.many.plan/apply`,
  `pull.plan/apply` (refuses a `library` the store doesn't have unless `new_library: true`; `label`
  names the snapshot), `snapshot.label` (`{label, snapshot}`, or `{label, remove: true}`),
  `library.list`, `library.rename`, `store.prune.plan/apply` (`keep_last`, `keep_daily`,
  `keep_weekly`, `library`; says why each kept snapshot stays), `store.copy` (`path`, `mirror`,
  `verify`, `force`, `dry_run`), `store.copies`, `push.plan/apply`, `merge.plan/apply`,
  `convert.plan/apply`, `settings.*`, `art.*` (`art.get` returns base64 JPEG previews), `store.*`,
  `disks.list`, `format.plan/apply` (`format.apply` needs the plan's `identity`, so the disk the
  user confirmed is the one erased).
- Editing: `edit.plan/apply` takes `{"drive": "Home", "edits": [...]}`, a list of edits tagged by
  `op` (`create_playlist`, `rename_playlist`, `move_playlist`, `delete_playlist`, `add_tracks`,
  `remove_tracks`, `move_tracks`, `set_tracks`, `set_entries`, `sort_playlist`, `dedupe`,
  `plan_set`, `edit_tracks`, `delete_tracks`, `replace_track`, `set_path`, `delete_history`,
  `my_tag`, `cues`, `set_artwork`; see `edit::Edit`), applied as one change with one backup and one
  journal entry. `my_tag`, `cues` and `set_artwork` are made first, in their order, then the rest;
  the plan checks the tracks they name are on the drive, and applying stops (`replay_stopped`)
  at one that can't be made: what was made is recorded, and `undo` takes it back (having made
  nothing, the error says it changed nothing and no backup is left). `set_artwork`'s `image` is
  `{"path": ...}` (a file on this computer), `{"base64": ...}` (the image's bytes) or `{"staged":
  hash}` (one the drive keeps, as the journal records it). `edit_tracks` also sets `bpm`,
  `album_artist` (with an album), `isrc` and `release_date` (`YYYY-MM-DD`). Tracks are chosen with selectors (`{"ids": [12]}`,
  `{"filter": "bpm:>=128"}`, `{"paths": ["/Contents/..."]}`, `playlist`, `album`, `query`,
  `history`; a track must fit every field given). Also `edits.list`, `edits.replay.plan/apply`,
  `edits.clear`, `dedupe.plan/apply` (`by_sound` also finds the same
  recording in another file or encoding), `relocate.plan/apply` (`{drive, folders}`, absolute
  paths: finds missing tracks' files, `ambiguous` lists those with several, `unfit` those whose
  only file isn't as long), `upgrade.plan/apply`
  (`{drive, folders, remove_old}`: lossy tracks pointed at lossless files of their recording,
  `not_upgraded` says why the others weren't), `history.delete.plan/apply`,
  `smart.list/set/remove`, `smart.write.plan/apply`, `pack.plan/apply` (`transcode: {players,
  container, decode_lossy}` packs copies the players play; `transcodes` and `unplayable` in the
  plan), `tags.list`,
  `tags.plan/apply`, `cues.list`, `cues.plan/apply`, `playlist.restore.plan/apply` (`{drive,
  playlist}`, with `snapshot` or `backup` to choose the source; the plan lists the `missing` entries
  left out).
- Reading a library a piece at a time: `drive.library` takes `offset` and `limit` for its
  tracks (the reply then has `page: {offset, count, total}`; without them every track comes, as
  before); `playlists.tree {drive}` is the playlist tree without tracks, `{playlists, history}`
  of `{id, name, path, folder, tracks, duration_secs, children}`; `tracks.get {drive, ids}`
  returns `{tracks, missing}` in the order asked. `track.analysis` takes `waveform`
  (`none`, `preview`, `detail` or `all`, the default) and `encoding`: `json` (the default, each
  column an object, 4-5 MB for a scrolling waveform) or `base64`, where each waveform is
  `{encoding: "base64", style, columns_per_sec, columns, back, front}` and a layer is
  `{heights, colors}`: one byte a column of height (0-255 of the full height) and three of red,
  green and blue, in base64 (`front` is `null` when no column has one: scrolling and blue
  waveforms). `tracks.analysis {drive, ids}` answers up to 100 tracks at once as `{tracks:
  [{track, analysis}], missing}` in the order asked, with the same `encoding` and `waveform`
  `none` (the default here) or `preview`; scrolling waveforms come one track at a time. `art.get`
  and `art.path` take `track` (one answer, `null` without artwork) or `tracks` (a list of
  `{track, image}` in the order asked); `art.path` returns `{path, mime}`, an absolute path
  inside the drive with symlinks resolved (never one leading out of it), for a webview to load
  directly.
- The reads above, `track.show`, `track.matches`, `cues.list` and `track.fields` see the library
  players see; with `combined: true` they read the whole library exports are made from, adding
  the tracks, playlists and history only OneLibrary holds on a drive with both databases. Those
  tracks keep OneLibrary's id with the top bit set (2^31 and up), so the id never names an
  `export.pdb` track, even after another export; reads without `combined` and edits refuse it.
- `track.fields {drive, tracks?, plugin?}` answers what analyzer plugins worked out and the store
  keeps: `{tracks: [{track, plugins: {<plugin>: {<analyzer>: {<field>: value}}}}], missing}`,
  for tracks whose audio file hasn't changed since (`missing` lists ids asked for that the
  library lacks), from the plugins locallibrary has, at their analyzers' versions; a value is
  `null` when it was worked out with nothing to show. Without a store
  there are none, and none is made.
- Plugins: see [Plugins](#plugins) below.
- Reading: `tracks.query` (filter, sort, paging), `track.analysis` (beat grid, cues, waveforms,
  phrases), `track.show` (details, playlists, My Tags, copies, analysis summary), `track.matches`
  (harmonic matches), `doctor` (`analyze`: the analysis section, after the analyzers for its
  roles run), `doctor.orphans.plan/apply`,
  `analysis.bench` (scoring rekordbox and the analysis against GiantSteps annotations), `plays.history`, `plays.archive`,
  `plays.stats`, `profile.build` (the report, preferences and document), `profile.document`,
  `profile.prefs`, `profile.prefs.set`, `export`, `diff`, `check`, `players.models` (what each model
  reads), `players.show/set/clear` (`{models, library}`; the models `check` uses without `players`),
  `account.status`, `account.check`, `account.logout`, `account.client_metadata`, `collection.info`,
  `collection.library` and `collection.diff` (rekordbox's `master.db`: optional `db` and `key`, and
  `drive` for the diff). Signing in takes two calls: `account.login.begin` (`handle`, optional
  `client_id`, `dev_client`, `session_file`, `scopes`, and `publish: true` to allow publishing)
  returns the `url` to open in the browser, an `id`, and a `note` when it fell back to the
  development client; `account.login.finish` (`id`, optional `timeout_secs`: 300 by default, an hour
  at most) waits for the browser and returns the account (`$/cancel` stops waiting;
  `account.login.paste` takes a pasted address instead; `account.login.cancel` forgets a sign-in
  that won't be finished). `account.logout` says whether the server revoked the session (`revoked`)
  and what couldn't be removed (`left_behind`). Errors about the account have `kind: "account"`.
  The account and publishing methods are alpha (see [Atmosphere account](account.md)).
  Publishing: `settings.publish.plan/apply`, `profile.publish.plan/apply` (the parameters of
  `profile.build`), `sets.publish.plan/apply` (`set`, optional `hide`), `sets.lookup` (the same
  parameters: runs the analyzers chosen for the MusicBrainz roles on the tracks publishing names
  that have no IDs, keeps what they find in the set and answers `asked`, `found`, `failed` and
  `unmounted`; call it before the plan), and
  `settings.import_published.plan/apply` (`from`: a handle or DID, default the signed-in account;
  `replace: true` to replace the base instead of adding to it). A plan holds the exact
  `record`, what's published in its place now as Local Network's directory has it (`current`),
  `changed`, `notes` to show the DJ, and
  `missing_scope` when the session may not write it: sign in again with `publish: true` before
  applying. Like every plan it has a `plan_id`: `*.apply {"plan_id": ...}` publishes (or imports)
  exactly what it showed, and fails with `stale_plan` if the record was published again in between
  (or, for My Settings, the settings profile changed). Mixes: `mix.index`, `mix.identify` and
  `mix.export` keep fingerprints in the store (and make one), so they're `immediate`. `mix.identify`
  returns the whole `MixReport`, plus `audio`: a file a browser can play for the mix, the recording
  or an export next to an Audacity project, `null` when there's none; `extras: true` adds each
  play's overview waveform, hot cues and phrases in `extras`, in the order of `plays`, so following
  along needs no `track.analysis` per track; `save: true` keeps the report in the store's `reports`
  folder, its id in `saved` (a report that can't be kept still comes back, with a warning); progress
  phases `fingerprint`, `index`, `listen`, `match`. `mix.export` is the same, as the portable export
  (`output` also writes it to a file, refusing to replace one that's there without `replace`).
  `mix.reports.list` (the reports in the store's `reports` folder, newest first: `id`, title,
  artist, recording, drive, tracks, coverage, and which of the report's data `.json`, page `.html`
  and Local Network export `.mix.json` are there), `mix.reports.get {id, extras?}` (a kept report,
  as `mix.identify` answers) and `mix.reports.html {id, inline?}` (writes the report's interactive
  page next to it and answers its `path`, for a webview; the tracks' waveforms, cues and covers come
  from the drive it was matched against, by its identity, when it's mounted; `inline: true` adds the
  page itself as `html`).
- Sets and sessions: `sessions.list {drive?, unassigned?}` (every session on the mounted drives
  and in the store, newest first, with its `id`, `window {after, before, exact}`, tracks and the
  `sets` made from it), `sessions.dismiss {sessions, undo?}`, `sessions.import {file, drive?,
  name?}` (a Beat Link Trigger log), `sets.suggest {drive?}` (`candidates`, and the `sessions`
  they're made of), `sets.list`, `sets.get {set | venue}`, `sets.new`, `sets.edit` (both answer
  the `set` and `notes` to show), `sets.delete`, `sets.export {set, format, output?}` (without
  `output`, the document comes back as `text`), `sets.played {track}` and
  `sets.playlist.plan/apply`. Sets are written to the store at once, so `sets.new`, `sets.edit`,
  `sets.delete`, `sets.export`, `sessions.dismiss` and `sessions.import` are `immediate`.
- Drive management: `fleet.status` (`libraries`: each with its clones plugged in and not, whether
  those plugged in are `identical`, and `unsaved` changes; `mounted` drives carry `back_from_set`),
  `drives.known`, `drive.rename/forget/role/retire/log`,
  `drive.recognise`, `drive.adopt_as.plan/apply`, `drive.reidentify.plan/apply`, `drive.groups`,
  `sync.group.plan/apply` (one library a plan: without `library`, the only one with two or more
  drives plugged in, and with several it's refused as `choose_library`, listing them, where
  `locallibrary sync` syncs each; `watch.start` takes `sync_group`, or `sync_set` as before),
  `drive.eject.plan/apply`, `drive.verify`, `drive.repair` (takes the `problems` a verify returned;
  each is checked again first), `drive.fsck` (destructive: `repair: true` unmounts the drive and may
  truncate or remove damaged files), `drive.space`, `tidy.plan/apply`, `recordings.list`,
  `recordings.collect.plan/apply`, `firmware.plan/apply`, `firmware.remove.plan/apply`,
  `player.show`, `player.set.plan/apply`, `drive.contact.plan/apply`, `provision.plan/apply`
  (`provision.apply` with the request takes the plan's `format.identity` as `format_identity` when
  formatting), and the jobs `refresh.plan/apply` and `prep.plan/apply` (what `locallibrary refresh`
  and `prep` do, as one plan of steps; its plans' `operation` is still `set`). `refresh.plan` takes
  `operations`, plugins' operations to run as its `operations` step (`[{operation:
  "<plugin>.<name>", params}]`); one that can't be planned fails the plan, its error's
  `fields.operation` naming it. `sync.many.apply`
  returns one result per target: a target that fails carries an `error` and doesn't stop the others.
- Writes that had no plan step have one: `drive.adopt.plan`, `drive.adopt_as.plan`,
  `drive.rename.plan`, `drive.contact.plan`, `player.set.plan`, `edits.clear.plan`,
  `firmware.remove.plan`, `recordings.collect.plan`, `settings.write.plan` (`{drives,
  create_missing}`, every mounted drive but archive and retired ones by default; applying it returns
  one result per drive, a drive that fails carrying an `error`), `settings.import.plan`,
  `art.set.plan` (an `edit.plan` of one `set_artwork`), `art.embed.plan` (applying keeps a copy of each file in the store first) and
  `art.unembed.plan` (puts them back), `store.gc.plan`, `players.set.plan`, `players.clear.plan`,
  `pull.plan`, `drive.eject.plan`, `drive.reidentify.plan`, `store.prune.plan`,
  `playlist.restore.plan`. Their `.apply` applies a plan by `plan_id`, or plans and applies a
  request as before (`settings.write.apply` with a request still writes one `drive`, honouring
  `dry_run`).
- Parameter names are the fields of the request structs in `api.rs`, as `docs/rpc` describes
  them. Results that were pairs keep their JSON (`failed: [["Contents/a.mp3", "unreadable"]]`);
  the schema and the TypeScript types name each element (`[path: string, error: string]`).

Native bindings (napi-rs) can wrap the same `api` functions later without changing behaviour.

## Plugins

What plugins add (see [Plugins](plugins.md)) is the `plugins.*` methods. `plugins.list` answers
each plugin's `{id, name, version, capabilities: [{capability, means}], accounts, commands,
destinations, events, operations, panels, analyzers, checks, exporters, importers, providers,
services, sources, themes, settings, running, left_out}`, `means` saying in plain words what a capability lets it do and `left_out` why
a plugin isn't loaded, when it clashes with a command, a method or another plugin (else `null`);
`plugins.describe {plugin}` answers `{manifest, capabilities, settings, secrets, secrets_unknown?,
running}`, the manifest with the JSON Schemas of each command's and operation's `params` and each
exporter's and importer's `options`, to draw forms from, and where to offer each command and
operation (`where`: `track`, `playlist`, `drive`), each panel's title, each theme's colours
(`tokens`) and how each analyzer's fields are shown (`display`). A param's
`x-from` (`selection.tracks`, `selection.playlist`, `selection.drive`) is filled from the
request's `context` when it isn't given; given as `null`, it's left out. `plugins.settings
{plugin}` and `plugins.settings.set {plugin, key, value}` (`value` `null` or left out: back to its
default; `key` `services.<name>` turns a service on or off) answer `{plugin, settings: {key: value},
secrets: {key: set}}`: a secret setting is set
like any other (text), kept in the system's keychain, and only ever answered as set (`true`) or
not, never in `settings`; when the keychain can't say, each is `null` and `secrets_unknown` says
why.

The applies below also take the request, planning and applying in one go. Plugins' plans are kept
apart from the rest, so planning with plugins never pushes out a plan of locallibrary's own. A
plugin starts when it's first used and stops once it's idle a while, and when the sidecar exits. A
plugin's errors come with locallibrary's kinds (`not_found`, `invalid`, `io`...), the plugin named
in the message. Each kind of thing plugins add is below, in a section of its own.

### Accounts

`plugins.accounts {plugin}` answers each of a plugin's accounts, `[{plugin, account, title,
signed_in, expires?, refreshes, scopes}]` (never a token). `plugins.account.sign_in.begin {plugin,
account}` answers `{id, plugin, account, url, redirect_uri}`: open `url` in the browser, then
`plugins.account.sign_in.finish {id, timeout_secs?}` waits for it to come back (300 seconds unless
it says, an hour at most; `$/cancel` stops it) and answers the account, signed in.
`plugins.account.sign_in.paste {id, url}` finishes with the address the browser ended up on, when
it runs on another computer, and `plugins.account.sign_in.cancel {id}` answers `{cancelled}`.
`plugins.account.sign_out {plugin, account}` answers the account with `{was_signed_in,
revoked}`.

### Analysis

`plugins.apply_analysis.plan {plugin, analyzer, drive, parts: ["grid" | "phrases" | "waveform"],
tracks?, overwrite?}` plans writing the beat grid, phrases and waveform an analyzer worked out into
the tracks' analysis files (each track's `set_analysis` edit, a grid with its `edit_tracks` BPM),
applied by `plugins.apply_analysis.apply {plan_id}`; tracks it can't write to are left out with a
warning. An operation's `set_analysis` edit (`{op, track, beats?, phrases?, waveform?: {blob}}`,
the waveform a file in its scratch folder) needs `analysis:plan`.

### Analyzers

`plugins.analyze {plugin, analyzer, drive, tracks?}` runs an analyzer on the drive's tracks (those
it hasn't worked out already), with progress (an `analyze` phase) and `$/cancel`, answering
`{analyzed, kept, failed}`.

### Checks

What plugins' checks find is in `check`'s `items` and `doctor`'s `checks` (the `checks` section),
each named `<plugin>.<check>`, and a plugin's problem counts as one; a check that failed or took
too long is a warning saying so. An item a plugin can fix has `fix: {plugin, operation, params}`:
plan it with `plugins.op.plan {plugin, operation, params, context: {drive}}`, confirmed as any
plan is. Guards (checks run before a plan is applied) add what they say to the `warnings` of a
plan that edits a library (`edit`, a plugin's operation, `cues`, `playlist.restore`, a contact's
playlist...); one the user made blocking that finds a warning or a problem sets the plan's `blocked`
(`<plugin>.<check>`), and applying it fails with reason `blocked`. `plugins.checks` answers
`{checks: [{plugin, name, title, when, on, blocking}], timeout}` (`when`: `check`, `apply`;
`timeout` in seconds), `plugins.checks.set {plugin, check, on?, blocking?, reset?}` changes one
(left out: as it is; `reset`: back to its defaults first) and `plugins.checks.timeout.set
{timeout}` how long a check has (0.001 to 3600; `null`: back to 10), each answering
`plugins.checks`'s.

### Commands and operations

`plugins.run {plugin, command, params, context: {drive, playlist, tracks, set}}` answers a headless
command's JSON, or a view (`{kind: "list" | "table" | "kv" | "text" | "tree" | "chart" | "image"
| "sections", title, ...}`, see loclib-plugin's README for each kind and its plain text form) to
draw, with every row the plugin sent in parts as it worked; an image's `data` is a PNG `data:`
URL, and a table's column is its heading or `{name, kind, unit?}`. `plugins.op.plan {plugin, operation, params,
context}` (`context.drive` is required) answers an edit plan whose `operation` is
`<plugin>.<operation>`, its summary first (an image a plugin's `set_artwork` gives is read as it's
planned, and held by the plan), applied by `plugins.op.apply {plan_id}`; it's stale once the drive
changes, or a file on this computer the plugin worked it out from. A view command or an operation
may ask the user something first: `plugins.run` answers a view `{kind: "choose", title, param,
multi?, items: [{label, detail?, value}]}`, `plugins.op.plan` `{choose: {...}}` (no plan, nothing
kept). Show the items, then call it again with `params[param]` set to the chosen item's `value`
(with `multi`, a list of the values chosen, maybe none); `plugins.op.apply` with a request refuses
one that asks. A question may carry `more`, further questions of the same shape (at most 64 in
all, each about a param of its own): ask them all, then call it again once with every param
set. A tracklist is imported as the tracklist plugin's `import` operation (`{file, into,
playlist?, append?}`, `file` absolute), which asks about the entries that could be several tracks
64 at a time, the first as the question and the rest in its `more`: `param` is the entry's number,
counting from 1, each item's `value` a track id, and `null` leaves the entry out.

### Destinations

`destination` is `<plugin>:<destination>`. `plugins.store.copy {destination, force?, dry_run?}`
copies the store to a store destination, answering `store.copy`'s report (`path` the destination).
`plugins.store.restore {destination, libraries?, dry_run?}` brings back what it has that the store
lacks, answering `store.fetch`'s report (`peer` the destination). `plugins.store.verify
{destination, drive, snapshot?, against?}` answers `drive.verify`'s report, a file that's good
there and nowhere nearer with `repair: {from: "destination", plugin, destination}`, and
`plugins.store.repair {destination, drive, problems, against?}` fetches those into the store
(each checked against its hash) and rewrites them as `drive.repair` does. `plugins.publish.plan
{destination, set}` answers the plugin's dry run as a plan (`<plugin>.publish.<destination>`:
`{plugin, destination, set, recording, tracks, url?, replaces?}`, destructive when it replaces the
set's link), applied by `plugins.publish.apply {plan_id}` (`{set, url?, warnings}`, the set's link
then `url`): always confirm it, as it reaches outside the computer.

### Events

Plugins listening for them are told of drives coming and going, changing and pulled, sets coming
back, requests finishing and plans applied while the sidecar runs, whether or not the client
subscribed (it polls drives for them when a plugin listens). What a plugin tells the user is an
`event` notification, `{"event": "plugin.notice", plugin, message, operation?: {name, params},
action?: {name, params}}` (subscribe to `plugin`), with one of `operation` and `action` at most:
show `message`, and offer to review what it suggests. An `operation` is one of the plugin's own:
plan it with `plugins.op.plan {plugin, operation: name, params}`. An `action` is one of
locallibrary's own flows on the plugin's own contributions (`HostAction`). Its `drive` is one the
sidecar found among the drives and libraries it knows (never a path or a folder; a plugin naming
one it doesn't know has its suggestion left out), named by its id, else its name, and its
`tracks` are those that drive had, each once. Start it as the user would, filled in, and let them
change it before it goes on as it does when they start it.

- `art.fetch` and `track.fetch` `{drive, tracks?, provider?}`: `plugins.provide {kind: "artwork" |
  "metadata", drive, tracks: {ids: tracks}, from}`, `from` being `<plugin>.<provider>`, or
  `<plugin>` without one, then `plugins.provide.plan` with what the user picks.
- `export` `{format, drive?, set?, mix?, options?}`: `plugins.export.plan {plugin, exporter:
  format, drive, set, mix, options, output}`, `output` being where the user picks (never from the
  plugin).
- `analyze` `{analyzer, drive, tracks?}`: `plugins.analyze {plugin, analyzer, drive, tracks}`,
  once the user says so.

Never plan or apply either unless the user chooses to.

### Export formats

`plugins.export.plan {plugin, exporter, drive, playlists?, history?, set?, mix?, output, options}`
(`output` absolute, never on a drive or in the store; an exporter whose manifest's `source` lists
`set` takes `set`, as `sets.get` names one, one listing `mix` takes `mix`, the id of a report kept
in the store, `mix.reports.list`, either with a `drive` or not, and no playlists; it writes out
whichever of a set, a mix or the drive the request names) answers `{plugin, exporter,
output, files: [{path, what, bytes, exists}], counts, notes}` (`counts`: labelled numbers of what
goes in, in order; `notes`: what the format can't hold and what to do with the files; the library's
problems are the plan's `warnings`), destructive when a file `exists` already, applied by
`plugins.export.apply {plan_id}` (`{files: [{path, what, bytes}], counts, notes, warnings}`).

### Import formats

`plugins.import.plan {plugin, importer, file, drive, options}` (`file` absolute) has the plugin
read the file, finds what it read on the drive and answers `{plugin, importer, file, tracks:
{entries, found, ambiguous, missing}, plays: {...}, edits, sessions}`: how many of its tracks (and
its sessions' plays) were found, the edit plan adding to the tracks found what they lack (a key and
a BPM, cues in slots or places they have none in, a track's as one `cues` edit `{"set_many": [...]}`,
My Tags), and the sessions it keeps in the store
(as `sessions.import` keeps Beat Link Trigger's), with what it left out among the `warnings`. Its
`operation` is `<plugin>.import.<importer>`; applied by `plugins.import.apply {plan_id}` (`{edits?,
sessions}`, the edit report and the sessions kept, by id), and stale once the file or the drive
changes.

### Panels

`plugins.panel {plugin, panel, drive, track}` answers the panel's view of the track (by id), as
`plugins.run` answers a view; a panel never asks a question.

### Providers

`plugins.provide {kind, drive, tracks?, from?, replace?}` (`kind`: `artwork` or `metadata`;
`tracks` a selection as `edit.plan`'s take [default: every one]; `from` a plugin, or
`<plugin>.<provider>`) asks the providers of that kind what they find for the tracks without it
(cover art, or some tag; every one chosen, with `replace`), with progress and `$/cancel`, and
answers `{kind, asked, offers: [{track, name, candidates: [{provider, source, label, score,
artwork?, metadata?}], picked}], images, warnings}`: each track's candidates best first (`artwork`
the id of its image in `images`, `metadata` `{title?, artist?, album?, year?, genre?, label?,
isrc?}`), `picked` the one picked for the user, when it alone scores the `auto_pick` setting or
more, `images` each image once, however many tracks it's offered for, by id (its content's hash)
in base64, and in `warnings` a provider that failed, or answered more than is kept (25 candidates
a track, 50,000 in all, images coming to 512 MiB in base64: the rest are left out). Show the rest,
then `plugins.provide.plan {kind, drive, picks: [{track, candidate}], images?, replace?}` (each
candidate as it was offered, with the images picked, or all of `images`) answers an edit plan
(`plugins.provide`) setting the images or writing the tags (only where a track has none, unless
`replace`), applied by `plugins.provide.apply {plan_id}`. `plugins.providers` answers `{providers:
[{plugin, name, title, kind}], auto_pick}`, and `plugins.providers.set {auto_pick}` (0 to 1, more
than 1: never; `null`: back to 0.9) sets it.

### Library sources

`plugins.source.plan {plugin, source, options, grants, drive, playlists?, all?, replace?}`
(`grants`: absolute folders the plugin may read files in, for this build) answers a
`collection.export.plan`-shaped plan (`<plugin>.source.<source>`, with `source`:
`<plugin>:<source>`) building the drive or folder `drive` from what the plugin reads: only audio
files in the folders granted are copied, and what's left out is in its `warnings`. Applied by
`plugins.source.apply {plan_id}`, answering what `collection.export.apply` does.

### Roles

`plugins.roles` answers which analyzer each role (`key`, `bpm`, `loudness`...) is read from,
`[{role, chosen, set, off, offered}]`, and `plugins.roles.set {role, analyzer}` (`<plugin>.<analyzer>`,
or `null`: back to its default) chooses one, or with `none: true` none (`off`). `plugins.apply_fields.plan {plugin, analyzer, drive,
fields, tracks?, overwrite?, min_confidence?}` (`fields`: `key`, `bpm`) answers an edit plan
(`plugins.apply_fields`) writing what the analyzer kept to the tracks, applied by
`plugins.apply_fields.apply {plan_id}`.

### Services

A plugin's services the user turned on (`plugins.settings.set {plugin, key: "services.<name>",
value: true}`) run while the sidecar does, started at once and stopped when turned off; what they
tell the user is a `plugin.notice` event. One that fails five times in a row is left off, with a
`plugin.notice` saying so.
