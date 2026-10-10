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
  rights. Requests name files and folders it reads and writes (`output` for exports, `interop.*` and
  `mix.export`, `to` for `recordings.collect.*`, `path` for `store.copy`, images and audio for
  `art.*`, `file` for `firmware.*` and `import.*`, `db` for `collection.*`, any folder as a drive)
  and servers it fetches from (`pds` for `settings.import_published.*`, `client_id` and `scopes` for
  `account.login.begin`). Never expose it on a socket or to another user, and never pass web content
  (a page's or a renderer's input) into its parameters. It refuses relative paths to write to, a
  data server that isn't https (or on this machine) and a line longer than 16 MiB; those checks only
  catch mistakes.
- **The contract.** [`docs/rpc/schema.json`](../docs/rpc/schema.json) describes every method:
  its `kind` (`read`; `plan` and `apply`, paired; `immediate`, a change with no plan step;
  `control`, about the sidecar itself), its `aliases`, whether it's `long_running` (sends
  `progress`, can be cancelled) or `destructive` (can delete or overwrite something), and JSON
  Schemas for its `params` and `result`, with the `progress` and `event` notifications.
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
  `kinds` takes event names or groups (`drive`, `store`, `job`, `watch`); empty means all. Drives
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
  both) and `rekordbox_sync` (rekordbox's automatic playlist sync to the drive).
- `version` lists every method. They mirror the CLI: `status`, `drives.list`, `drive.info`,
  `drive.adopt.plan/apply`, `drive.library`, `drive.backups`, `drive.restore.plan/apply` (`{drive,
  backup?, force?}`), `undo.list`, `undo.plan/apply` (`{drive?, backup?, force?}`; either apply
  takes the other's `plan_id`), `compare`, `sync.plan/apply`, `sync.many.plan/apply`,
  `pull.plan/apply` (refuses a `library` the store doesn't have unless `new_library: true`; `label`
  names the snapshot), `snapshot.label` (`{label, snapshot}`, or `{label, remove: true}`),
  `library.list`, `library.rename`, `store.prune.plan/apply` (`keep_last`, `keep_daily`,
  `keep_weekly`, `library`; says why each kept snapshot stays), `store.copy` (`path`, `mirror`,
  `verify`, `force`, `dry_run`), `store.copies`, `push.plan/apply`, `merge.plan/apply`,
  `clean.plan/apply`, `convert.plan/apply`, `settings.*`, `art.*` (`art.get` returns base64 JPEG
  previews), `xml.export`, `interop.plan/apply` (other DJ software), `store.*`, `disks.list`,
  `format.plan/apply` (`format.apply` needs the plan's `identity`, so the disk the user confirmed is
  the one erased).
- Editing: `edit.plan/apply` takes `{"drive": "Home", "edits": [...]}`, a list of edits tagged by
  `op` (`create_playlist`, `rename_playlist`, `move_playlist`, `delete_playlist`, `add_tracks`,
  `remove_tracks`, `move_tracks`, `set_tracks`, `set_entries`, `sort_playlist`, `dedupe`,
  `plan_set`, `edit_tracks`, `delete_tracks`, `replace_track`, `delete_history`; see `edit::Edit`),
  applied as one change with one backup. Tracks are chosen with selectors (`{"ids": [12]}`,
  `{"filter": "bpm:>=128"}`, `{"paths": ["/Contents/..."]}`, `playlist`, `album`, `query`,
  `history`; a track must fit every field given). Also `edits.list`, `edits.replay.plan/apply`,
  `edits.clear`, `import.plan/apply`, `dedupe.plan/apply`, `history.delete.plan/apply`,
  `smart.list/set/remove`, `smart.write.plan/apply`, `pack.plan/apply`, `tags.list`,
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
  waveforms). `art.get` and `art.path` take `track` (one answer, `null` without artwork) or
  `tracks` (a list of `{track, image}` in the order asked); `art.path` returns `{path, mime}`, an
  absolute path inside the drive with symlinks resolved (never one leading out of it), for a
  webview to load directly.
- Reading: `tracks.query` (filter, sort, paging), `track.analysis` (beat grid, cues, waveforms,
  phrases), `track.show` (details, playlists, My Tags, copies, analysis summary), `track.matches`
  (harmonic matches), `doctor`, `doctor.orphans.plan/apply`, `plays.history`, `plays.archive`,
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
  `profile.build`), `sets.publish.plan/apply` (`set`, optional `hide`), and
  `settings.import_published.plan/apply` (`from`: a handle or DID, default the signed-in account;
  `pds`; `replace: true` to replace the base instead of adding to it). A plan holds the exact
  `record`, what's published in its place now (`current`), `changed`, `notes` to show the DJ, and
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
  and `prep` do, as one plan of steps; its plans' `operation` is still `set`). `sync.many.apply`
  returns one result per target: a target that fails carries an `error` and doesn't stop the others.
- Writes that had no plan step have one: `drive.adopt.plan`, `drive.adopt_as.plan`,
  `drive.rename.plan`, `drive.contact.plan`, `player.set.plan`, `edits.clear.plan`,
  `firmware.remove.plan`, `recordings.collect.plan`, `settings.write.plan` (`{drives,
  create_missing}`, every mounted drive but archive and retired ones by default; applying it returns
  one result per drive, a drive that fails carrying an `error`), `settings.import.plan`,
  `art.set.plan`, `art.embed.plan` (applying keeps a copy of each file in the store first) and
  `art.unembed.plan` (puts them back), `store.gc.plan`, `players.set.plan`, `players.clear.plan`,
  `pull.plan`, `drive.eject.plan`, `drive.reidentify.plan`, `store.prune.plan`,
  `playlist.restore.plan`. Their `.apply` applies a plan by `plan_id`, or plans and applies a
  request as before (`settings.write.apply` with a request still writes one `drive`, honouring
  `dry_run`).
- Parameter names are the fields of the request structs in `api.rs`, as `docs/rpc` describes
  them. Results that were pairs keep their JSON (`failed: [["Contents/a.mp3", "unreadable"]]`);
  the schema and the TypeScript types name each element (`[path: string, error: string]`).

Native bindings (napi-rs) can wrap the same `api` functions later without changing behaviour.
