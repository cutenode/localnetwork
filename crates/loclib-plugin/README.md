# loclib-plugin

The protocol [locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary)
plugins speak, the manifest they declare themselves in, and an SDK for writing one in Rust. A
plugin adds the kinds of things its families of hooks contribute (see [Hooks](#hooks)), and
settings; it never ships UI code, and it never writes to a drive.

## The protocol

A plugin is a program locallibrary runs, in a folder of its own (`<store>/plugins/<id>/work/`).
They talk JSON-RPC 2.0 over the plugin's stdin and stdout, one JSON message per line (no newlines
inside one, at most 16 MiB). Anything the plugin writes to stderr is its log. The protocol is
versioned on its own, at `0.2.0` (`PROTOCOL_VERSION`); two versions are compatible by semver's
caret rule, so before 1.0 the minor version must match. A new kind of hook doesn't change it: each
family of hooks is an optional feature named after it (`FEATURES`, see [Hooks](#hooks)), and so
are the SDK's own, which every plugin written with it has: `blobs` (scratch folders and partial
results), `net` (fetching through the host) and `choose` (asking the user first).

1. The host's first message is `hello`
   (`{"protocol": "0.2.0", "host": {"name", "version"}, "features": [...]}`, the features it
   has). The plugin answers `{"protocol", "plugin": {"id", "version"}, "features": [...]}`, those
   it implements, or an error of kind `incompatible`. Neither side sends anything else before it,
   and feature names a side doesn't know are ignored: the host calls only the hooks of features
   both have, and answers only what a feature both have lets a plugin send.
2. `describe` answers the plugin's manifest as JSON. locallibrary refuses a plugin whose answer
   differs from the manifest it knows, or that contributes what a feature it doesn't have runs
   (commands without `commands`, `[[net]]` limits without `net`).
3. The host calls hooks, one for each kind of contribution (see [Hooks](#hooks)), each with a
   `context`: the drive (by id or name), for a command or an operation the playlist, tracks and set
   (by id) the user is on, the plugin's `settings` (never a secret one) and, with `blobs`,
   `scratch`: an absolute, empty folder for this request alone. A value too big for a message (an
   image) is a file the plugin leaves there, given as `{"blob": "cover.jpg"}` (its name in the
   folder): the host reads it itself, refuses a name that leads out of the folder (or is a link, or
   a file with another name) and a folder holding more than it takes (512 MiB), and removes the
   folder once it has.
   - With `choose`, a view command may answer a question for the user instead, `{"kind":
     "choose", "title", "param", "multi", "items": [{"label", "detail", "value"}]}` (`Choice`),
     and an operation `{"choose": {...}}`: the host asks it, and runs the hook again with
     `params[param]` set to the value chosen (with `multi`, a list of any of them). The param is
     one the hook takes but doesn't require (the host checks params first), and each item's
     value one it takes: a question that isn't is refused.
4. While answering, a plugin may call back with requests named after the sidecar's read
   methods (`tracks.query`, `playlists.tree`, `drive.info`...), with their params and results
   (see `apps/locallibrary/docs/rpc`). Its manifest's capabilities say which it may call (one
   they don't grant fails with kind `denied`); writes never can be, nor reads of files on the
   computer. Each read takes only some of its params from a plugin: drives by id or name (never
   a path), no files, servers or secrets, and pages of at most 500, and what it answers says
   nothing of where things are on the computer: no `root` (where a drive is mounted), nor where
   the store's copies, a set's or a mix's recording or rekordbox's collection are, and any other path said
   from its drive, `<store>` or `~`, as errors are. A plugin has a few calls under way at once (more fail with
   kind `busy`).
   With `net`, a plugin goes online through the host: `net/fetch {url, method, headers, body?,
   body_blob?, into_blob?}` -> `{status, headers, body? | blob?}`. The host sends it only to a
   host the manifest's `net:` capabilities grant, at a public https address, following redirects
   itself and each only there, at most five; as `locallibrary/<version> (+<contact>)`, the
   plugin's `contact` setting when it has one; waiting its turn when the manifest limits that
   host (`[[net]]`). `{secret:<key>}` in a header's value, a query value or `body` is the
   plugin's secret setting `key`, filled in once the request is checked, left out of the
   plugin's log and the errors it's told, and never sent on to another server a redirect leads
   to: the headers it was filled into (and `Authorization`, `Cookie`) are dropped there, and a
   body with it isn't sent on. An answer is given as text up to 8 MiB, or written to
   `into_blob` (a new file in the request's scratch folder, absolute) as far as the folder has
   room; `body_blob` sends such a file. Errors have kind `denied` (not a host it may reach),
   `invalid`, `too_big`, `external` (it couldn't be reached) or `cancelled`. Nothing fetched is
   kept.
5. `progress {id, event}` notifications (from the plugin; `id` is the host's request) have the
   sidecar's `progress` shape: a loclib-progress event (or `{"event": "step"}`) with the
   `target` drive, the `step` and, for a coalesced `item`, how many `items` it stands for. A host
   skips an event it doesn't know (a newer plugin's). `$/cancel {id}` (from the host) asks for a
   request to stop; it then fails with kind `cancelled`. Stopping is up to the plugin, so a host
   kills a plugin that doesn't answer a cancelled request in time.

A response is never answered, not even a bad one. Errors use the RPC sidecar's codes: -32700
(parse error), -32600 (invalid request), -32601 (method not found), -32602 (invalid params),
-32603 (internal), and -32000 with `data.kind` saying which failure (`cancelled`, `not_found`,
`incompatible`...). locallibrary shows a hook's error as one of its own of that kind (`invalid`,
`not_found`, `conflict`, `stale_plan`, `permission`, and `io` or `parse` about the file
`data.fields.path` names), with the plugin named; any other kind is the plugin failing.

## The manifest

`locallibrary-plugin.toml` says who the plugin is, what it may call back for and what it adds.
`Manifest::parse` reads it and `Manifest::validate` checks it; `schemars::schema_for!(Manifest)`
is its JSON Schema. A key it doesn't know is left out (`Manifest::unknown` lists them), so an
older locallibrary reads a newer plugin's manifest; in a plugin locallibrary ships it's a mistake,
refused.

```rust
use loclib_plugin::{Capability, Manifest};

let manifest = Manifest::parse(r#"
    id = "tracklist"
    name = "Tracklist import"
    version = "0.1.0"
    protocol = "^0.2"
    capabilities = ["library:read", "library:plan", "net:musicbrainz.org"]

    [[contributes.commands]]
    name = "import"
    title = "Import a tracklist"
    mode = "headless"
    where = ["playlist", "drive"]
    params = { type = "object", properties = { file = { type = "string" } } }
    cli = "import"

    [[contributes.analyzers]]
    name = "tempo"
    title = "Tempo"
    version = "1"
    fields = { bpm = "number", confident = "boolean" }
    roles = { bpm = "bpm" }
"#)?;
assert_eq!(manifest.capabilities[2], Capability::Net("musicbrainz.org".into()));
// Only plugins locallibrary ships may give commands places of their own on the command line.
assert!(manifest.validate(true).is_ok());
assert!(manifest.validate(false).is_err());
# Ok::<(), loclib_plugin::ManifestError>(())
```

- `capabilities`: `library:read`, `analysis:read`, `artwork:read`, `drive:info`, `health:read`,
  `store:read`, `settings:read`, `plays:read`, `sets:read`, `account:status`,
  `collection:read`, `library:plan`, `library:delete`, `tags:plan`, `cues:plan`,
  `artwork:plan`, and `net:<host>`, or `net:*.<domain>` for any host below the domain: what
  `net/fetch` may reach (shown to the user; a native plugin can't be held to it otherwise). Any
  other is refused, and each is granted once.
- `[[net]]`: `host` (one of its `net:` capabilities, as written) and `per_second`, how many
  `net/fetch` requests a second the host sends there, at least 1/3600: the limit the server asks
  for. Each host is limited once.
- `where` is which menus offer a command or an operation: `track` (the tracks selected),
  `playlist` and `drive`, `["drive"]` unless it says. A palette has every one.
- `cli` (first-party plugins only) is a command's or operation's own place on the command line:
  one name (`import`) or two, the first a group of the host's commands (`playlist import`).
  Otherwise it's `locallibrary <id> <name>`. A params property's `x-cli` says how the command
  line gives it (`CliHints`): `flag` (`--from` rather than `--playlist`) and `positional = true`
  (a string given without a flag, after the drive: `playlist import DRIVE FILE`; one param at
  most).
- A params or options property marked `x-path = true` (a string, or a list of them) is a file or
  folder on the computer: every host refuses a relative one, and the command line makes it
  absolute.
- A param's `x-from` (`Selection`) is what fills it in when it isn't given: `selection.tracks`
  (a list of integers, the tracks' ids), `selection.playlist` (a string, its path) or
  `selection.drive` (a string, its id or name), from where the user runs it. A form shows it
  filled in. Given as `null` (cleared in a form), it's left out instead.
- `[[contributes.settings]]`: `key`, `title`, `type` (`string`, `number`, `boolean`, `enum`,
  `file`, `directory` or `secret`), `default` and, for an enum, `choices`. A secret (text, and
  no default) is kept in the system's keychain and never given to the plugin: it's only filled
  into its `net/fetch` requests.

Names (`id`, contributions' `name`, `format` and `key`, fields) are lowercase letters, digits,
`-` and `_`, starting with a letter, each used once; titles aren't empty; `params` and `options`
must be JSON Schemas for an object that compile.

## Hooks

Each family of hooks is a feature of its own, with the methods the host calls for it and what a
manifest contributes for it. A plugin has a family's feature once it implements one of its hooks.

### analyzers

`track/analyze {analyzer, track: {id, file_path, audio_path, analyze_path?}, context}` ->
`{"fields": {name: value}}`, of the types the manifest declares (`FieldValue`), or `null` for one
worked out with nothing to show. A field left out wasn't worked out.

- `[[contributes.analyzers]]`: `name`, `title`, `version` (its own: locallibrary keeps its
  values by it, so bump it when they'd change), `fields` (each `number`, `integer`, `string`,
  `boolean` or `string[]`) and `roles`. Fields become columns, so no two analyzers have one of a
  name.
- `roles` says which of an analyzer's fields locallibrary reads as what (`Role`), a role to a
  field: `key` (a Camelot code, `8A`: a string), `bpm`, `key_confidence` and `bpm_confidence`
  (0 to 1), `loudness` (integrated LUFS), `true_peak` (dBTP), each a number, `clipped_runs` (an
  integer) and `identity` (a string, equal for one recording). A field of another type than its
  role's is refused. Doctor's tempos, keys and levels, the check before a set, ordering a set and
  writing keys and BPMs to tracks read a role from the analyzer the user chose for it, else the
  only one, else the first. Roles need no feature of their own: a host that doesn't know them
  leaves them out.

### commands

`command/run {command, params, context}`: any JSON for a headless command; for a view command a
view, `{"kind": "list" | "table" | "kv" | "text", ...}` (`View`), which each host draws its own way.
While it works, a view command may send its rows (or items) in parts, `partial {id, rows | items}`
notifications (`id`: the host's request), for a host to show as they come; the view it answers still
has them all, first.

- `[[contributes.commands]]`: `name`, `title`, `mode` (`headless` or `view`), `where`, `params`
  (a JSON Schema for an object) and `cli`.

### exporters

`export/write {exporter, source: {drive, playlists, history, set?, mix?}, output, options, dry_run,
context}` ->
`{"files": [{path, what, bytes}], "counts", "notes", "warnings"}`. The plugin writes the files; with
`dry_run` it only says which it would (`what` each holds, its size as far as it's known), so the
host can ask before overwriting any. `counts` (optional) are labelled numbers of what goes in, in
the order to show them (`{"tracks": 12, "cues": 40}`); `notes` (optional) say what the format can't
hold and what to do with the files, `warnings` what's wrong with this library. An export reads the
drive's whole library as locallibrary combines it; `playlists` and `history` (lists, by name) only
narrow which lists go in. A set's exporter is given `set` instead (the set's id, which `sets.get`
reads) and a mix's `mix` (the id of the report kept of it, which `mix.reports.get` reads), each
with the `drive` its tracks are on when the user names one (else `""`).

- `[[contributes.exporters]]`: `format`, `title`, `extension` (lowercase letters and digits,
  without the dot) or `folder = true`, `options`, and `source`: what it writes out, `drive` (a
  drive's library, unless it says), `set` or `mix`; a set's or a mix's reads it, which takes
  `sets:read`.

### importers

`import/read {importer, file, options, context}` -> `{"tracks": [{match: {path?, isrc?, artist?,
title?, duration?}, key?, bpm?, cues?: [{hot_cue?, start_ms, loop_beats?, name?, color?}],
my_tags?}], "sessions"?: [{name?, started?, plays: [{artist, title, started_at?, path?}]}],
"warnings"?}`: what another program's file (absolute, to read only) says about tracks, and the
plays it logged (times in RFC 3339; a play without one starts with its session). An answer too big
for a message is a blob holding it as JSON (`{"blob": "read.json"}`). The host finds each track on
the drive as a tracklist's entries are found (by file, ISRC, then artist, title and length), says
how many it couldn't, and plans the edits itself, adding only what a track lacks: a key (in any
notation rekordbox shows) and a BPM where it has none, cues in hot cue slots (or at places) it has
none in (a cue it can't set, say a colour or a loop length it doesn't take, is left out with a
warning), and the My Tags (`Category/Tag`) it lacks. It keeps the sessions in its store, as it keeps
Beat Link Trigger's logs. An importer plans none, and needs no capability to.

- `[[contributes.importers]]`: `format`, `title`, `extension` (the files it reads usually end in:
  lowercase letters and digits, without the dot), `options`, and `provides`: what of it the host
  takes, `fields` (keys and BPMs), `cues`, `tags` and `sessions`, each once. An answer with
  anything else is refused.

### operations

`op/plan {operation, params, context}` -> `{"summary", "warnings", "edits", "inputs"}`, where each
edit is an `Edit` as the sidecar's `edit.plan` takes it (`{"op": "create_playlist", ...}`).
locallibrary plans them as one `edit.plan` (one confirmation, one backup, applied together) and
applies them itself. `inputs` (optional) lists the files on the computer the plan was worked out
from (absolute): it isn't applied once one changes. A `set_artwork` edit gives its image as a blob
(`{"blob": "cover.jpg"}`), never by path or in base64.

- `[[contributes.operations]]`: `name`, `title`, `long_running`, `where`, `params` and `cli`.
  Whether one is destructive, the host works out from the edits it plans.

### providers

`provide/candidates {provider, kind, tracks: [{id, artist, title, album, album_artist, isrc,
duration}], context}` -> `{"candidates": [{track, score, source, label, artwork?, metadata?}],
"warnings"}`: what a provider found for the tracks, of its kind, any number for each. `score` (0
to 1) is how sure it is; an artwork provider's `artwork` is a blob, a metadata provider's
`metadata` the tags it has (`title`, `artist`, `album`, `year`, `genre`, `label`, `isrc`).
`warnings` say what it found nothing for. The host asks every provider of a kind, offers the user
what they found, best first, picking one alone that scores high enough, and plans the edits
itself: a provider plans none. It keeps 25 candidates a track and 50,000 in all, and images coming
to 512 MiB (in base64), leaving out the rest with a warning.

- `[[contributes.providers]]`: `name`, `title` and `kind`, `artwork` or `metadata`: what it finds
  for tracks (`provide/candidates`). It's given their tags, so it needs `library:read`, and
  never a `:plan` capability for what it finds: the host plans that.

## Writing a plugin in Rust

Register a function for each kind of hook the manifest needs in `Hooks` (`.on(run_command)`, for
a `fn(RunCommand, &Host) -> Result<Value, RpcError>`), and `serve` them, with the manifest, over
stdin and stdout. `serve` answers `hello` (with the features of the hooks registered) and
`describe`, refuses everything before `hello`, runs each hook on a thread of its own, only for a
contribution the manifest declares, and returns when the host closes stdin, once the running
hooks (cancelled) have answered. A hook's `Host` calls back to the host and has the manifest,
and its `ctx()` is a loclib-progress context that sends progress (coalesced as the sidecar does)
and notices the host cancelling; a callback under way when the host cancels fails with
`cancelled` at once. `host.drive(drive)` reads a drive through it, typed (`DriveReads`): its
library, tracks by id or by query, playlists, the tracks' analysis and artwork, and the drive's
info, each whole however the host pages and batches it (`host.call` makes any other read), as
loclib-model's types (`Track`, `Library`, `TrackAnalysis`), which `loclib_plugin::loclib_model`
re-exports. `host.fetch(&Fetch::get(url))` goes online through the host (`net/fetch`), and
`host.partial_rows(rows)` (`partial_items`) sends a view's rows as they're ready; files the
answer names go in `req.context.scratch`. A hook answers its answer or what makes one: an
operation an `OpPlan`, or an `OpAnswer` when it may ask first (`Choice`; `View::Choose` for a
command).
`RpcError::failure` gives a library's error the kind the host shows it as: `cancelled` once the
hook is, `io` for a file that couldn't be read or written, else the kind it's given. A new kind of
hook is a family of its own in this crate (`src/hooks/`): its request types implementing `Hook`
and its row in `FEATURES`.

`Peer` is the connection itself, for a host or a plugin: either end sends requests and
notifications from any thread, and its reader thread hands the other end's to its owner (a
host's hook sent with `send_request_reporting` gets its own `progress`, in order before its
answer). What the other end gets wrong goes to stderr, or to a host's log (`Peer::with_log`).
`check_params` checks a hook's params or options against the contribution's schema. Here it plays
the host:

```rust
use loclib_plugin::{Hooks, Host, Incoming, Peer, RpcError, RunCommand, View, method, serve};
use serde_json::{Value, json};

fn count(req: RunCommand, host: &Host) -> Result<Value, RpcError> {
    let drive = req.context.drive.unwrap_or_default();
    let tree = host.drive(&drive).playlists()?;
    host.ctx().check()?;
    let view = View::Text { title: Some(drive), text: format!("{} playlists", tree.playlists.len()) };
    Ok(serde_json::to_value(view).expect("a view is JSON"))
}

const MANIFEST: &str = r#"
    id = "count"
    name = "Count"
    version = "0.1.0"
    protocol = "^0.2"
    capabilities = ["library:read"]

    [[contributes.commands]]
    name = "count"
    title = "Count the playlists"
    mode = "view"
"#;

// A plugin's main: serve(MANIFEST, Hooks::new().on(count), std::io::stdin(), std::io::stdout()).
let (plugin_in, to_plugin) = std::io::pipe()?;
let (from_plugin, plugin_out) = std::io::pipe()?;
let served = std::thread::spawn(move || serve(MANIFEST, Hooks::new().on(count), plugin_in, plugin_out));
let (host, incoming) = Peer::new(from_plugin, to_plugin);

let hello = json!({ "protocol": "0.2.0", "host": { "name": "locallibrary", "version": "0.1.0" } });
let reply: Value = host.request(method::HELLO, &hello)?;
assert_eq!(reply["features"], json!(["commands", "blobs", "net", "choose"]));
let run = json!({ "command": "count", "context": { "drive": "USB" } });
let counting = host.send_request(method::RUN_COMMAND, &run)?;
let Ok(Incoming::Request(callback)) = incoming.recv() else { panic!("a callback") };
assert_eq!((callback.method.as_str(), callback.params), ("playlists.tree", json!({ "drive": "USB" })));
let set = |id, name| json!({ "id": id, "name": name, "path": name, "folder": false, "tracks": 0, "duration_secs": 0 });
host.respond(&callback.id, Ok(json!({ "playlists": [set(1, "Friday"), set(2, "Warm up")], "history": [] })));
let view: View = counting.wait()?;
assert_eq!(view, View::Text { title: Some("USB".into()), text: "2 playlists".into() });

host.close();
served.join().expect("the plugin stops");
# Ok::<(), Box<dyn std::error::Error>>(())
```

`examples/echo` is a whole plugin, with one of each kind of contribution.

## Licence

MIT.
