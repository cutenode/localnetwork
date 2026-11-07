# locallibrary diagrams: the sidecar

Its lifecycle, requests, plans, every method and its contract. The rest are listed in the [README](../../README.md#diagrams).

- [How the sidecar is wired](#how-the-sidecar-is-wired)
- [A sidecar's lifecycle](#a-sidecars-lifecycle)
- [A sidecar request](#a-sidecar-request)
- [Plans in the sidecar](#plans-in-the-sidecar)
- [Sidecar notifications](#sidecar-notifications)
- [Sidecar errors](#sidecar-errors)
- [Sidecar methods that read](#sidecar-methods-that-read)
- [Sidecar plans](#sidecar-plans)
- [Sidecar changes made at once](#sidecar-changes-made-at-once)
- [Sidecar contract](#sidecar-contract)

## How the sidecar is wired

`locallibrary rpc` speaks newline-delimited JSON-RPC 2.0 on stdin and stdout, and nothing else. Only a trusted parent should run it.

```mermaid
flowchart TD
    parent["GUI front end"] -->|"stdin, a line a request"| reader["Reader loop"]
    reader -->|"control methods"| control["Answered at once on the reader"]
    reader -->|"quick reads"| reads["Reads pool, 4 workers by default"]
    reader -->|"everything else"| work["Work pool, 4 workers by default"]
    control --> out["stdout, a line a message"]
    reader -->|"job.started"| out
    reads --> api["Core library"]
    work --> api
    work --> plans[("Plans by id: 64 of its own, 64 plugins'")]
    api --> out
    events["Event poller, each second, while subscribed<br/>or a plugin listens"] --> out
    watch["watch.start thread"] --> out
    plugins["Plugin host"] --> out
    out --> parent
```

Source: [`src/rpc.rs`](../../src/rpc.rs), [`src/rpc/methods.rs`](../../src/rpc/methods.rs), [`docs/sidecar.md`](../sidecar.md)

## A sidecar's lifecycle

From being spawned to exiting, and what happens to requests still running.

```mermaid
stateDiagram-v2
    state "Serving" as Serve
    state "Finishing what's running" as Finish
    state "Cancelling what's running" as Cancel
    [*] --> Serve: spawned
    Serve --> Finish: stdin closes, or shutdown
    Serve --> Cancel: shutdown with cancel, or a signal
    Finish --> Cancel: a signal
    Finish --> Exited: watching and serving stop, requests answer
    Cancel --> Exited: requests answer cancelled
    Cancel --> Exited: a second signal, once no swap is under way
    Exited --> [*]
```

Source: [`src/rpc.rs`](../../src/rpc.rs)

## A sidecar request

What a client sees for a long request, from its id to the answer.

```mermaid
sequenceDiagram
    participant C as Client
    participant S as Sidecar
    participant W as Worker
    C->>S: id 7, sync.group.apply
    S-->>C: event job.started, when subscribed
    S->>W: Queue it, listed by jobs.list
    alt Cancelled while queued
        C->>S: $/cancel id 7
        S-->>C: event job.finished
        S-->>C: Error, data.kind cancelled
    else Cancelled while running
        W-->>C: progress, about ten a second
        C->>S: $/cancel id 7
        S->>W: Cancel at the next safe point
        W-->>C: event job.finished
        W-->>C: Error, data.kind cancelled
    else It ends
        W-->>C: progress, about ten a second
        W-->>C: event job.finished
        W-->>C: Its result or error for id 7
    end
```

Source: [`src/rpc.rs`](../../src/rpc.rs), [`crates/locallibrary/src/api/sidecar.rs`](../../../../crates/locallibrary/src/api/sidecar.rs), [`docs/sidecar.md`](../sidecar.md)

## Plans in the sidecar

What happens to a plan the sidecar keeps by `plan_id`.

```mermaid
stateDiagram-v2
    state "Kept" as Kept
    state "Taken out to apply" as Taken
    [*] --> Kept: *.plan, the newest 64 of its kind
    Kept --> Gone: 30 minutes, or pushed out
    Kept --> Taken: *.apply with plan_id
    Taken --> Applied: done
    Taken --> Kept: its check refuses it, stale_plan or conflict, or locked
    Taken --> Gone: any other failure while applying
    Applied --> [*]
    Gone --> [*]: applying it is stale_plan
```

Source: [`src/rpc.rs`](../../src/rpc.rs), [`src/rpc/methods.rs`](../../src/rpc/methods.rs), [`docs/sidecar.md`](../sidecar.md)

## Sidecar notifications

What the sidecar sends with no request id of its own.

```mermaid
flowchart TD
    out["Notifications"] --> progress["progress: a request's id and an event<br/>phase_start, item, bytes, phase_end, warning, step"]
    out --> event["event: after events.subscribe. watch.* comes while<br/>watching, unless a subscription leaves it out"]
    out --> serve["serve: store.serve's id and an event<br/>ready, with the code, connected, refused, done"]
    event --> drive["drive.attached, detached, changed<br/>store.changed"]
    event --> job["job.started, job.finished"]
    event --> watch["watch.up_to_date, pending, pulled,<br/>synced, waiting, skipped, failed"]
    event --> plugin["plugin.notice"]
```

Source: [`src/rpc.rs`](../../src/rpc.rs), [`crates/locallibrary/src/api/sidecar.rs`](../../../../crates/locallibrary/src/api/sidecar.rs), [`docs/rpc/schema.json`](../rpc/schema.json)

## Sidecar errors

The shape of an error, which a GUI can act on without reading its message.

```mermaid
flowchart TD
    error["error: code, message, data"] --> protocol["Protocol: -32700 a bad line, -32600 a bad request,<br/>-32601 no such method, -32602 bad params, -32603 internal"]
    error --> failed["-32000: the request failed"]
    failed --> kind["data.kind: not_found, conflict, stale_plan,<br/>no_space, locked, cancelled..."]
    failed --> reason["data.reason: which decision a conflict needs,<br/>or what would resolve it"]
    failed --> fields["data.fields: what it's about:<br/>path, drive, left, right..."]
    failed --> actions["data.actions: each label, method, params,<br/>a button and the call it makes"]
```

Source: [`src/rpc.rs`](../../src/rpc.rs), [`crates/locallibrary/src/api/sidecar.rs`](../../../../crates/locallibrary/src/api/sidecar.rs), [`crates/locallibrary/src/error.rs`](../../../../crates/locallibrary/src/error.rs), [`docs/sidecar.md`](../sidecar.md)

## Sidecar methods that read

The `read` and `control` methods, by what they're about.

```mermaid
flowchart TD
    hub["Reads and control"]
    hub --> control["The sidecar<br/>initialize, version, rpc.describe, $/cancel, jobs.list, events.subscribe, events.unsubscribe, watch.start, watch.stop, shutdown"]
    hub --> fleet["Drives<br/>status, fleet.status, drives.list, drives.mounted, drives.known, disks.list, drive.info, drive.library, drive.backups, drive.log, drive.groups, drive.recognise, drive.space, drive.verify, drive.restore.preview"]
    hub --> tracks["Library<br/>tracks.get, tracks.query, tracks.analysis, track.analysis, track.fields, track.show, track.matches, playlists.tree, tags.list, cues.list, art.missing, art.get, art.path, compare, diff, check, doctor"]
    hub --> store["Store<br/>store.info, store.history, store.verify, store.copies, store.serve, library.list, snapshot.library, undo.list, edits.list, smart.list"]
    hub --> plays["Plays and sets<br/>plays.history, plays.archive, plays.stats, sessions.list, sets.list, sets.get, sets.played, sets.suggest, mix.reports.list, mix.reports.get, profile.build, profile.document, profile.prefs"]
    hub --> settings["Settings and players<br/>settings.schema, settings.profile, settings.show, settings.export, players.models, players.show, player.show, recordings.list"]
    hub --> other["Others<br/>collection.info, collection.library, collection.diff, analysis.bench, account.status, account.check, account.client_metadata, accounts.list, accounts.access.list"]
    hub --> plugins["Plugins<br/>plugins.list, plugins.describe, plugins.settings, plugins.run, plugins.panel, plugins.checks, plugins.provide, plugins.providers, plugins.roles, plugins.store.verify"]
    control ~~~ plays
    fleet ~~~ settings
    tracks ~~~ other
    store ~~~ plugins
```

Source: [`src/rpc/methods.rs`](../../src/rpc/methods.rs), [`src/rpc/methods/accounts.rs`](../../src/rpc/methods/accounts.rs), [`src/rpc/methods/plugins.rs`](../../src/rpc/methods/plugins.rs), [`docs/rpc/schema.json`](../rpc/schema.json)

## Sidecar plans

Every change that comes as a `.plan` and `.apply` pair.

```mermaid
flowchart TD
    hub["Plan and apply"]
    hub --> copies["Whole libraries<br/>sync, sync.many, sync.group, pull, push, pack, merge, convert, collection.export, provision, format"]
    hub --> edits["Editing<br/>edit, dedupe, relocate, upgrade, tags, cues, art.set, art.auto, art.embed, art.unembed, smart.write, edits.replay, edits.clear, history.delete, playlist.restore, sets.playlist"]
    hub --> drive["A drive<br/>drive.adopt, drive.adopt_as, drive.reidentify, drive.rename, drive.contact, drive.eject, drive.restore, undo, doctor.orphans, tidy, firmware, firmware.remove, recordings.collect, player.set"]
    hub --> store["The store and settings<br/>store.gc, store.prune, settings.write, settings.import, settings.import_file, settings.import_published, players.set, players.clear"]
    hub --> jobs["Jobs<br/>refresh, prep"]
    hub --> publish["Publishing<br/>settings.publish, profile.publish, sets.publish"]
    hub --> plugin["Plugins<br/>plugins.op, plugins.export, plugins.import, plugins.source, plugins.publish, plugins.provide, plugins.apply_fields, plugins.apply_analysis"]
    copies ~~~ jobs
    edits ~~~ publish
    drive ~~~ plugin
```

Source: [`src/rpc/methods.rs`](../../src/rpc/methods.rs), [`src/rpc/methods/accounts.rs`](../../src/rpc/methods/accounts.rs), [`src/rpc/methods/plugins.rs`](../../src/rpc/methods/plugins.rs), [`docs/rpc/schema.json`](../rpc/schema.json)

## Sidecar changes made at once

The `immediate` methods: small changes and long runs with no plan step.

```mermaid
flowchart TD
    hub["At once"]
    hub --> store["Store<br/>store.copy, store.fetch, snapshot.label, library.rename, smart.set, smart.remove, settings.set, settings.unset"]
    hub --> sets["Sets and profile<br/>sets.new, sets.edit, sets.delete, sets.export, sets.lookup, sessions.dismiss, sessions.import, profile.prefs.set"]
    hub --> mix["Mixes and files<br/>mix.index, mix.identify, mix.export, mix.reports.html, export"]
    hub --> drive["Drives<br/>drive.role, drive.retire, drive.forget, drive.repair, drive.fsck"]
    hub --> account["Accounts<br/>account.login.begin, finish, paste, cancel, account.logout, accounts.sign_in.begin, finish, paste, cancel, accounts.sign_out, accounts.access.give, set_default, take_away"]
    hub --> plugins["Plugins<br/>plugins.settings.set, plugins.analyze, plugins.checks.set, plugins.checks.timeout.set, plugins.providers.set, plugins.roles.set, plugins.store.copy, plugins.store.restore, plugins.store.repair"]
    store ~~~ drive
    sets ~~~ account
    mix ~~~ plugins
```

Source: [`src/rpc/methods.rs`](../../src/rpc/methods.rs), [`src/rpc/methods/accounts.rs`](../../src/rpc/methods/accounts.rs), [`src/rpc/methods/plugins.rs`](../../src/rpc/methods/plugins.rs), [`docs/rpc/schema.json`](../rpc/schema.json)

## Sidecar contract

One list of methods answers everything: dispatch, `rpc.describe`, and the files in `docs/rpc`. A build without the `account` feature lacks `account.*`, the `*.publish.*` methods, `sets.lookup` and `settings.import_published.*`. One without `peers` lacks `store.serve` and `store.fetch`.

```mermaid
flowchart TD
    registry["Method registry: name, kind, aliases,<br/>params and result types"] --> dispatch["Dispatch at run time"]
    registry --> describe["rpc.describe"]
    registry --> emit["rpc --emit-schema DIR"]
    emit --> schema["schema.json"]
    emit --> dts["locallibrary.d.ts"]
    registry --> kinds["Kinds: read, plan, apply,<br/>immediate, control"]
```

Source: [`src/rpc/methods.rs`](../../src/rpc/methods.rs), [`src/rpc/contract.rs`](../../src/rpc/contract.rs), [`src/rpc/typescript.rs`](../../src/rpc/typescript.rs), [`docs/rpc/schema.json`](../rpc/schema.json), [`docs/rpc/locallibrary.d.ts`](../rpc/locallibrary.d.ts)
