# loclib-plugin

The protocol [locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary)
plugins speak, the manifest they declare themselves in, and an SDK for writing one in Rust. A
plugin adds commands, operations, track analyzers, exporters and settings; it never ships UI
code, and it never writes to a drive.

## The protocol

A plugin is a program locallibrary runs. They talk JSON-RPC 2.0 over the plugin's stdin and
stdout, one JSON message per line (no newlines inside one, at most 16 MiB). Anything the plugin
writes to stderr is its log. The protocol is versioned on its own, at `0.1.0`
(`PROTOCOL_VERSION`); two versions are compatible by semver's caret rule, so before 1.0 the minor
version must match.

1. The host's first message is `hello`
   (`{"protocol": "0.1.0", "host": {"name", "version"}, "features": []}`). The plugin answers
   `{"protocol", "plugin": {"id", "version"}, "features": []}`, or an error of kind
   `incompatible`. Neither side sends anything else before it, and feature names a side doesn't
   know are ignored.
2. `describe` answers the plugin's manifest as JSON. locallibrary refuses a plugin whose answer
   differs from the manifest it knows.
3. The host calls hooks, one for each kind of contribution, each with a `context`: the drive
   (by id or name), for a command or an operation the playlist and tracks the user is on, and
   the plugin's `settings`.
   - `command/run {command, params, context}`: any JSON for a headless command; for a view
     command a view, `{"kind": "list" | "table" | "kv" | "text", ...}` (`View`), which each
     host draws its own way.
   - `track/analyze {analyzer, track: {id, file_path, audio_path, analyze_path?}, context}` ->
     `{"fields": {name: value}}`, of the types the manifest declares (`FieldValue`).
   - `export/write {exporter, source: {drive, playlists, history}, output, options, dry_run,
     context}` -> `{"files", "warnings"}`. The plugin writes the files; with `dry_run` it only
     says which it would, so the host can ask before overwriting any. An export reads the
     drive's whole library as locallibrary combines it; `playlists` and `history` (lists, by
     name) only narrow which lists go in.
   - `op/plan {operation, params, context}` -> `{"summary", "warnings", "edits", "inputs"}`,
     where each edit is an `Edit` as the sidecar's `edit.plan` takes it (`{"op":
     "create_playlist", ...}`). locallibrary plans them as one `edit.plan` (one confirmation, one
     backup, applied together) and applies them itself. `inputs` (optional) lists the files on
     the computer the plan was worked out from (absolute): it isn't applied once one changes.
4. While answering, a plugin may call back with requests named after the sidecar's read
   methods (`tracks.query`, `playlists.tree`, `drive.info`...), with their params and results
   (see `apps/locallibrary/docs/rpc`). Its manifest's capabilities say which it may call (one
   they don't grant fails with kind `denied`); writes never can be, nor reads of files on the
   computer. Each read takes only some of its params from a plugin: drives by id or name (never
   a path), no files, servers or secrets, and pages of at most 500. A plugin has a few calls
   under way at once (more fail with kind `busy`).
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
is its JSON Schema.

```rust
use loclib_plugin::{Capability, Manifest};

let manifest = Manifest::parse(r#"
    id = "tracklist"
    name = "Tracklist import"
    version = "0.1.0"
    protocol = "^0.1"
    capabilities = ["library:read", "library:plan", "net:musicbrainz.org"]

    [[contributes.commands]]
    name = "import"
    title = "Import a tracklist"
    mode = "headless"
    params = { type = "object", properties = { file = { type = "string" } } }
    cli = "import"

    [[contributes.analyzers]]
    name = "tempo"
    title = "Tempo"
    fields = { bpm = "number", confident = "boolean" }
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
  `artwork:plan`, and `net:<host>` (shown to the user; a native plugin can't be held to it).
  Any other is refused, and each is granted once.
- `[[contributes.commands]]`: `name`, `title`, `mode` (`headless` or `view`), `params` (a JSON
  Schema for an object) and `cli`.
- `[[contributes.operations]]`: `name`, `title`, `long_running`, `params` and `cli`. Whether one
  is destructive, the host works out from the edits it plans.
- `cli` (first-party plugins only) is a command's or operation's own place on the command line:
  one name (`import`) or two, the first a group of the host's commands (`playlist import`).
  Otherwise it's `locallibrary <id> <name>`. A params property's `x-cli` says how the command
  line gives it (`CliHints`): `flag` (`--from` rather than `--playlist`), `one_based = true` (an
  object whose keys are positions, `--choose 1=42` for key `0`) and `positional = true` (a
  string given without a flag, after the drive: `playlist import DRIVE FILE`; one param at most).
- A params or options property marked `x-path = true` (a string, or a list of them) is a file or
  folder on the computer: every host refuses a relative one, and the command line makes it
  absolute.
- `[[contributes.analyzers]]`: `name`, `title`, `fields` (each `number`, `integer`, `string`,
  `boolean` or `string[]`). Fields become columns, so no two analyzers have one of a name.
- `[[contributes.exporters]]`: `format`, `title`, `extension` (lowercase letters and digits,
  without the dot) or `folder = true`, `options`.
- `[[contributes.settings]]`: `key`, `title`, `type` (`string`, `number`, `boolean`, `enum`,
  `file`, `directory` or `secret`), `default` and, for an enum, `choices`.

Names (`id`, contributions' `name`, `format` and `key`, fields) are lowercase letters, digits,
`-` and `_`, starting with a letter, each used once; titles aren't empty; `params` and `options`
must be JSON Schemas for an object that compile.

## Writing a plugin in Rust

Implement the `Plugin` hooks the manifest needs and `serve` it over stdin and stdout. `serve`
answers `hello` and `describe`, refuses everything before `hello`, runs each hook on a thread of
its own and returns when the host closes stdin, once the running hooks (cancelled) have
answered. A hook's `Host` calls back to the host, and its `ctx()` is a loclib-progress context
that sends progress (coalesced as the sidecar does) and notices the host cancelling; a callback
under way when the host cancels fails with `cancelled` at once.

`Peer` is the connection itself, for a host or a plugin: either end sends requests and
notifications from any thread, and its reader thread hands the other end's to its owner (a
host's hook sent with `send_request_reporting` gets its own `progress`, in order before its
answer). What the other end gets wrong goes to stderr, or to a host's log (`Peer::with_log`). `check_params` checks a hook's params or options against the contribution's schema.
Here it plays the host:

```rust
use loclib_plugin::{Host, Incoming, Manifest, Peer, Plugin, RpcError, RunCommand, View, method, serve};
use serde::Deserialize;
use serde_json::{Value, json};

/// `tracks.query`'s result, as far as this plugin reads it.
#[derive(Deserialize)]
struct TracksPage {
    total: u64,
}

struct Count;

impl Plugin for Count {
    fn run_command(&self, req: RunCommand, host: &Host) -> Result<Value, RpcError> {
        let drive = req.context.drive.unwrap_or_default();
        let page: TracksPage = host.call("tracks.query", &json!({ "drive": drive, "limit": 0 }))?;
        host.ctx().check()?;
        let view = View::Text { title: Some(drive), text: format!("{} tracks", page.total) };
        Ok(serde_json::to_value(view).expect("a view is JSON"))
    }
}

let manifest = Manifest::parse(r#"
    id = "count"
    name = "Count"
    version = "0.1.0"
    protocol = "^0.1"
    capabilities = ["library:read"]

    [[contributes.commands]]
    name = "count"
    title = "Count the tracks"
    mode = "view"
"#)?;

// A plugin's main: serve(&manifest, Count, std::io::stdin(), std::io::stdout()).
let (plugin_in, to_plugin) = std::io::pipe()?;
let (from_plugin, plugin_out) = std::io::pipe()?;
let served = std::thread::spawn(move || serve(&manifest, Count, plugin_in, plugin_out));
let (host, incoming) = Peer::new(from_plugin, to_plugin);

let hello = json!({ "protocol": "0.1.0", "host": { "name": "locallibrary", "version": "0.1.0" } });
host.request::<_, Value>(method::HELLO, &hello)?;
let run = json!({ "command": "count", "context": { "drive": "USB" } });
let counting = host.send_request(method::RUN_COMMAND, &run)?;
let Ok(Incoming::Request(callback)) = incoming.recv() else { panic!("a callback") };
assert_eq!(callback.method, "tracks.query");
let page = json!({ "total": 412, "total_duration_secs": 0, "tracks": [] });
host.respond(&callback.id, Ok(page));
let view: View = counting.wait()?;
assert_eq!(view, View::Text { title: Some("USB".into()), text: "412 tracks".into() });

host.close();
served.join().expect("the plugin stops");
# Ok::<(), Box<dyn std::error::Error>>(())
```

`examples/echo` is a whole plugin, with one of each kind of contribution.

## Licence

MIT.
