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
results), `net` (fetching through the host), `choose` (asking the user first), `accounts` (naming
an account in a request, see [accounts](#accounts)) and `views` (trees, charts, images, sections
and typed table columns, see [views](#views)).

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
     one the hook takes but doesn't require (the host checks params first), not one it was
     given (it would be asked forever), and each item's value one it takes: a question that
     isn't is refused. `more` (a list of questions of the same shape, without a `more` of their
     own) asks those too in the same round, and the host runs the hook again once with every
     one's param set. Each is checked as the first is, no two ask about the same param, and a
     round asks at most 64 (`Choice::MAX_QUESTIONS`): every entry of a long set's tracklist,
     while the user doesn't sit through more before the hook hears their answers. A caller may
     answer only some of them (`--choose`, an RPC caller, a host older than `more`, which asks
     the first alone), so a hook asks again only about the params it still lacks: one asking
     about a param it was given is refused.
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
   body_blob?, body_range?, multipart?, into_blob?}` -> `{status, headers, body? | blob?}`. The
   host sends it only to a host the manifest's `net:` capabilities grant, at a public https
   address, following redirects itself and each only there, at most five; as
   `locallibrary/<version> (+<contact>)`, the plugin's `contact` setting when it has one;
   waiting its turn when the manifest limits that host (`[[net]]`). `{secret:<key>}` in a
   header's value, a query value, `body` or a form part's `value` is the plugin's secret setting
   `key`, filled in once the request is checked, left out of the plugin's log and the errors
   it's told, and never sent on to another server a redirect leads to: the headers it was filled
   into (and `Authorization`, `Cookie`) are dropped there, and a body with it isn't sent on.
   `{account:<id>}` is the same for one of its accounts' access tokens (see
   [accounts](#accounts)), as the whole value of the `Authorization` header or of the query
   parameter the account names, and refused anywhere else in a request.
   An answer is given as text up to 8 MiB, or written to `into_blob` (a new file in the
   request's scratch folder, absolute) as far as the folder has room. `body_blob` sends a file
   instead (absolute): one in the scratch folder, or the recording `set/publish` gives (not in a
   dry run), perhaps only `body_range {offset, length?}` of it, for an upload in parts.
   `multipart` sends a `multipart/form-data` form instead, its parts `{name, value}` or `{name,
   file, range?, filename?, content_type?}`. The host reads a file as it sends it (again for a
   redirect that asks for the body again), says how long the body is and what type a form is,
   and reports the bytes it sends as the hook's progress: `bytes` events of phase `upload`, so a
   plugin starts that phase with how much it'll send, and ends it. Errors have kind `denied`
   (not a host it may reach), `invalid`, `too_big`, `external` (it couldn't be reached) or
   `cancelled`. Nothing fetched is kept.
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
  `artwork:plan`, `analysis:plan`, `events:drives`, `events:jobs`, `events:plans`, `fs:read`, and `net:<host>`, or `net:*.<domain>` for any host below the domain: what
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

### accounts

The host signs the user in to a plugin's accounts in the browser (OAuth 2's authorization code
flow with PKCE, the browser coming back to `127.0.0.1`), keeps the tokens in the system's keychain
and refreshes them a minute before they expire. The plugin never sees a token: a `net/fetch`
request whose `Authorization` header is `{account:<id>}` is sent with `Bearer` and the access
token, once it's checked, and never on to another server a redirect leads to. A request for an
account that isn't signed in fails with kind `invalid`, saying how to sign in. The feature has no
methods: it's the SDK's own.

A service that wants the token some other way says so with the account's `token`:
`token = { scheme = "OAuth" }` sends `Authorization: OAuth <token>` (SoundCloud), and
`token = { query = "access_token" }` sends it as that query parameter instead (Mixcloud). The
request names the account there, `?access_token={account:<id>}`, and the `Authorization` header
can't. The host never carries a token in the query to another server a redirect leads to, and
where the answer's headers or text say the address back (a next page's link, say), the token is
given back as `{account:<id>}`, so the link can be fetched as it is. An answer written to
`into_blob` is kept as the server sent it.

- `[[contributes.accounts]]`: `id`, `title`, `authorize_url`, `token_url` or `token_exchange`
  (one of them), `scopes`, `client_id` (a public client, without a secret), `pkce = true` (the
  default; nothing else is taken), `revoke_url` (RFC 7009, optional), where the tokens are
  revoked at signing out, `token` (optional, above) and `redirect_port` (optional). Each endpoint
  is an https address without a user name or password. A plugin with an account needs a `net:`
  capability, for the requests its token goes into.
- `redirect_port`, 1024 or more, is for a service that compares the redirect URI with the one its
  client was registered with, port and all (SoundCloud). The browser then comes back to
  `http://127.0.0.1:<port>/callback`, which is what to register; without it, any free port.
- `token_exchange`, for a service whose token endpoint wants a client secret (SoundCloud, Mixcloud,
  Google), is Local Network's token exchange for it:
  `https://localnetwork.computer/api/token-exchange/<service>`, nowhere else, since it's sent the
  user's codes and refresh tokens. The host posts it what it would have sent the token endpoint
  (`grant_type`, `code`, `code_verifier` and `redirect_uri`, or `refresh_token`) as a JSON object,
  and the website adds its client id and secret for the service, sends the request on the way the
  service takes it and answers what the service answered. `client_id` is then Local Network's
  client at the service. The services it knows are in
  [`apps/web/src/lib/server/token-exchange/services.json`](../../apps/web/src/lib/server/token-exchange/services.json);
  one it doesn't know, or isn't set up for yet, fails the sign-in with what the website said.

### analysis

No method of its own: an analyzer's fields may also be a beat grid (`beats`: `[{beat_number,
bpm, time_ms}]`), phrases (`phrases`: `{mood, phrases: [{kind, start_beat, end_beat,
fill_beat?}]}`) or a waveform (`waveform`: `{blob, columns_per_sec}`, the file holding three bytes
a column, its lows, mids and highs, 0-255, `Bands` packed, no more than a day of audio), one
of each at most, or `null`. `FieldValue::beats`, `FieldValue::phrases` and `FieldValue::waveform`
make them and `Output` reads them. Every plugin the SDK serves has the feature, and a host without it refuses a manifest
declaring such a field. They're not columns: the host keeps them apart, draws them where a drive
has none and writes them into analysis files only when the user asks. An operation's
`set_analysis` edit needs `analysis:plan`.

### analyzers

`track/analyze {analyzer, track: {id, file_path, audio_path, analyze_path?}, context}` ->
`{"fields": {name: value}}`, of the types the manifest declares (`FieldValue`), or `null` for one
worked out with nothing to show. A field left out wasn't worked out.

- `[[contributes.analyzers]]`: `name`, `title`, `version` (its own: locallibrary keeps its
  values by it, so bump it when they'd change), `fields` (each `number`, `integer`, `string`,
  `boolean` or `string[]`, or one of [analysis](#analysis)'s) and `roles`. Fields become columns, so no two analyzers have one of a
  name.
- `display` says how hosts show some of its fields (`FieldDisplay`), by field, each part
  optional: `unit` (said after a value), `precision` (a number's places, 0 to 6), `format` (`key`
  for a string; for a number `duration` in seconds as `3:07`, `db` signed with `dB`, `percent` of
  a fraction, `meter` a bar of a fraction) and `width` (its column in the player, 1 to 40 cells; a
  meter's length). Without them a number shows to two places at most. The player's columns and
  `track show` show fields so, and a mix's HTML report shows a field with them.
- `roles` says which of an analyzer's fields locallibrary reads as what (`Role`), a role to a
  field: `key` (a Camelot code, `8A`: a string), `bpm`, `key_confidence` and `bpm_confidence`
  (0 to 1), `loudness` (integrated LUFS), `true_peak` (dBTP), each a number, `clipped_runs` (an
  integer), `identity` (a string, equal for one recording), `recording` and `release` (the
  MusicBrainz recording's and release's IDs, each a string: a bare lowercase UUID) and `artists`
  (a `string[]`: the MusicBrainz artist IDs, one for each name loclib-model's `artist_credits`
  splits the track's artist into, in its order), and what they were worked out from: `isrc` (a
  string, the ISRC as loclib-model's `isrc` spells it, null for none) and `credits` (a
  `string[]`, the names the artists' IDs go with). The host keeps the values by audio file but a
  track's tags can change without it, so it takes the IDs only for a track whose ISRC is `isrc`,
  and the artists' only when its credit splits into `credits`, when the analyzer answers them. A
  field of another type than its role's is refused. Doctor's tempos, keys and levels, the check before a set, ordering a set, writing keys
  and BPMs to tracks and the MusicBrainz IDs of a set published to an Atmosphere account read a
  role from the analyzer the user chose for it, else the only one, else the first. Roles need no
  feature of their own: a host that doesn't know them leaves them out.

### checks

`check/run {check, context}` -> `{"findings": [{level, message, tracks?, playlist?, paths?,
fix?}]}`: what a check finds on the drive `context.drive` (`Finding`: `level` `info`, `warning` or
`problem`, the tracks by id, a playlist by path, files relative to the drive), nothing when all is
well. A `fix` is one of the plugin's operations with its params (`{operation, params}`), which the
user can run, planned and confirmed as any is. The host names each finding `<plugin>.<check>` and
adds them to its check before a set and doctor; a problem counts as one of theirs. `plan/check
{check, operation, edits, context}` -> the same: what a guard says about a plan that edits a
library before it's applied (its operation, and each edit as `edit.plan` takes them, tracks named
by file), shown with the plan's warnings. That's an edit plan's edits (`edit`, a plugin's
operation, `edits.replay`...), a playlist restored (`playlist.restore`, its playlists made again),
a contact's playlist (`drive.contact`, a `create_playlist`) and a cue edit (`cues`, one `{"op":
"cues", "tracks": [file], "change": ...}` whose change is `{"set": {...}}`, `{"update": {...}}`
with the fields `cues.plan` takes, `{"delete": cue}`, `{"copy": {"from": file}}` or `{"carry":
{"from": file, "shift_ms": n}}`). A plan making no edit isn't asked about. Once the user makes a guard blocking, a warning or a problem it finds keeps the plan from being applied. A
check that fails or takes longer than the host gives it (10 s unless the user says) is a warning,
never a failure; of one's findings the host keeps 1,000, and of what one says (its message, its
playlist) 500 characters, without control characters. A file one names must be on the drive.

- `[[contributes.checks]]`: `name`, `title`, `default_on` (true unless it says: it runs until the
  user turns it off) and `when`: `check` (the default), `apply` (a guard) or both.

### commands

`command/run {command, params, context}`: any JSON for a headless command; for a view command a
view (`View`, see [views](#views)), which each host draws its own way.
While it works, a view command may send its rows (or items) in parts, `partial {id, rows | items}`
notifications (`id`: the host's request), for a host to show as they come; the view it answers still
has them all, first.

- `[[contributes.commands]]`: `name`, `title`, `mode` (`headless` or `view`), `where`, `params`
  (a JSON Schema for an object) and `cli`.

### destinations

A store destination keeps copies of locallibrary's store, which the host alone decides the
content of: stored files by their BLAKE3 hash (in hex) and the store's records by name
(`store.json`, `snapshots/<id>.json`, `snapshots.json`, `labels.json`, `libraries.json` last...).

- `blobs/has {destination, hashes}` (a thousand at most) -> `{missing}`: those it doesn't have.
- `blobs/put {destination, hash, path}` -> `{}`: keep the stored file at `path` (absolute; read
  it, never change it).
- `blobs/get {destination, hash}` -> `{blob}`: the file, in the request's scratch folder. One it
  doesn't have fails with kind `not_found`. The host hashes it and refuses one that isn't `hash`.
- `manifest/put {destination, name, blob}` -> `{}`: keep the record `name`, replacing any, its
  content the file `blob` names in the scratch folder.
- `manifest/get {destination, name}` -> `{blob?}`: the record, in the scratch folder; no `blob`
  when it has none.

A set destination publishes a set: `set/publish {destination, set: {id, date, title?, kind?,
venue?, city?, country?}, recording, tracklist: [{artist?, title?, start_seconds?}], dry_run}` ->
`{url?, warnings}`. `recording` is the set's audio file (absolute, to read), a track the DJ
publishes unnamed has no artist or title, and the DJ's notes never go. The host always asks for a
dry run first (which publishes nothing and may say where the set would be), shows it to the user
and asks, then publishes and keeps `url` (https) as the set's link.

- `[[contributes.destinations]]`: `name`, `title` and `kind`, `store` or `set`. Each method is
  only for a destination of its kind.

### events

`event/notify {event, drive?, payload}`, a notification from the host: something happened that
the plugin listens for. `event` is one of a closed list (`EventName`), each needing a capability:
`drive.mounted`, `drive.unmounted`, `drive.changed`, `drive.pulled` and `set.returned`
(`events:drives`), `job.finished` (`events:jobs`) and `plan.applied` (`events:plans`). `drive` is
the drive's id or name; `payload` is what the host says of it (as the sidecar's `event`
notification does), with no path saying where anything is on the computer: a drive's `root` is
left out, and other paths are said from their drive, `<store>` or `~`. Nothing answers it, and the
host never waits for it: once too many wait to be sent to a plugin, it drops the oldest.

A plugin with `events` (or `services`) may send the host `host/notify {message, operation?:
{name, params}, action?: {name, params}}`, a notification, whenever it likes: a line for the user
(cut to one line of 500 characters), and something to suggest, its `params` an object of at most
64 KiB as JSON (`MAX_SUGGESTED_PARAMS`), which the host starts (and the user confirms) only if the
user chooses to. That's one of its own operations, or one of the host's own flows (`HostAction`,
`HostNotify::action` sends one) on its own contributions, as the host starts it when the user
does. Its drive is named by id or name (never a path): the host finds it among those it knows
and names it by id (else name) as it passes it on, refusing one it doesn't know, and keeps the
tracks the drive has, each once, refusing one with none. The actions:

- `art.fetch` and `track.fetch` `{drive, tracks?, provider?}`: what its providers of that kind
  (`artwork` or `metadata`; only `provider` when it says) find for the tracks (every one unless it
  says, by id), offered and planned as any.
- `export` `{format, drive?, set?, mix?, options?}`: its exporter `format`, of the drive's library,
  a set or a mix as the exporter's `source` takes (with a drive at most for a set or a mix), its
  `options` fitting the exporter's. Where to is always the user's to pick.
- `analyze` `{analyzer, drive, tracks?}`: its analyzer on the drive's tracks.

The host refuses (and logs) a suggestion with both, an action it doesn't know or params the action
doesn't take, and one on another plugin's contributions, and tells the line alone. A host older
than actions leaves `action` out (`HostNotify` takes keys it doesn't know), so it tells the line
alone too.

- `[[contributes.events]]`: `on`, the events it listens for, each once, each granted.

### exporters

`export/write {exporter, source: {drive, playlists, history, set?, mix?}, output, options, dry_run,
context}` ->
`{"files": [{path, what, bytes}], "counts", "notes", "warnings"}`. The plugin writes the files; with
`dry_run` it only says which it would (`what` each holds, its size as far as it's known), so the
host can ask before overwriting any. `counts` (optional) are labelled numbers of what goes in, in
the order to show them (`{"tracks": 12, "cues": 40}`); `notes` (optional) say what the format can't
hold and what to do with the files, `warnings` what's wrong with this library. An export reads the
drive's whole library as locallibrary combines it; `playlists` and `history` (lists, by name) only
narrow which lists go in. A set's export is given `set` instead (the set's id, which `sets.get`
reads) and a mix's `mix` (the id of the report kept of it, which `mix.reports.get` reads), each
with the `drive` its tracks are on when the user names one (else `""`): which of the three a
request carries is what it writes out.

- `[[contributes.exporters]]`: `format` (one per exporter, whatever it writes out), `title`,
  `extension` (lowercase letters and digits, without the dot) or `folder = true`, `options`, and
  `source`: what it writes out, `drive` (a drive's library, unless it says), `set` or `mix`, or a
  list of them (`["drive", "set", "mix"]`) for an exporter that writes each out with the same
  options. The host refuses a request for a source it doesn't list; reading a set or a mix takes
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

A user can have `locallibrary refresh` run an operation too (`--then`), with only the drive in its
`context`: it's planned with the refresh, and again on the drive as the steps before left it when
it runs. A question it asks there is refused.

### panels

`panel/render {panel, context}` -> a view (see [views](#views)) of the track in `context.tracks`
(one, by id) on `context.drive`, never a question. Hosts ask for it as the user comes to a track
(the player once the selection settles, `track show` for each track it shows), so it answers
quickly; a panel that fails or answers nothing shows as such, with the track.

- `[[contributes.panels]]`: `name`, `title` and `where`, which is `track` (the default): shown
  under a track's details.

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

### services

`service/start {service, context}` runs the service until the host cancels it (`$/cancel`), and
then answers, as cancelled; while it runs it may send `host/notify` and `progress`. A host that
stays open starts it once the user turns it on (the setting `services.<name>`, which the host
keeps and no hook is given), keeps the plugin running while it does, and starts it again a while
after it stops or fails, a few times in a row at most.

- `[[contributes.services]]`: `name` and `title`.

### sources

`library/read {source, options, roots, cursor?, context}` -> `{"tracks": [...], "playlists":
[{path, tracks}], "blob"?, "next"?, "warnings"?}`: a page of a library the plugin reads from somewhere on the
computer (another program's collection, a folder of tagged files), which the host builds a drive
from. `roots` are the folders the user granted it for this read (absolute); the host asks for
pages, passing each one's `next` back as `cursor`, until one has none. A track is `{path, title,
artist, album, album_artist, genre, label, remixer, composer, comment, isrc, year, rating, key?,
bpm?, cues?, beats?, waveform?, my_tags?}`: `path` its audio file (absolute), `key` a Camelot code
or a key as rekordbox names it, `cues` `[{hot_cue?, start_ms, end_ms?, name?, color?}]` (`hot_cue`
0 for A to 7 for H; `color` as cue edits take one), `beats` a beat grid as an analyzer's `beats`
field is, `waveform` a blob as its `waveform` field is, `my_tags` `Category/Tag`. A playlist's
`path` is `Folder/Name`, its `tracks` their `path`s. A page too big for a message is a blob, `{"blob":
"page.json"}`, holding a page as JSON. `warnings` say what it couldn't read, a line each, and the
plan shows them. The host trusts no path: it copies only a regular audio file
in one of `roots` (links resolved), not on the drive written to, reading its length and format
itself and hashing it before copying, and leaves out (with a warning) the tracks that aren't and
what it can't write of the rest; cues, beat grids and waveforms go into analysis files it makes.

- `[[contributes.sources]]`: `name`, `title` and `options`. It reads files in the folders it's
  given, so it needs `fs:read`.

### themes

Data alone: no feature, no method, and the host never runs the plugin for one.

- `[[contributes.themes]]`: `name`, `title` and `tokens`, the colours it sets as tables of their
  names (`theme::TOKENS` in loclib-model): `cues.hot = "#28e214"`, `ui = { accent = "#ff8800" }`.
  They're packages/tokens/colors.schema.json's colours for a dark background (`bands`, `phrases`,
  `cues`, `trackColors`, `status.dark` and `playhead`), the player's roles (`ui.text`, `dim`,
  `faint`, `accent`, `info`, `secondary`, `selection`, `vinyl`) and the waveform widgets'
  gradients (`waveform.spectrum`, `waveform.scope`), each `#rrggbb` in lowercase, a gradient a list
  of two or more. What a theme leaves out is the default's; a host turns them round for a light
  background and leaves them out for NO_COLOR itself.

## Views

What a view command or a panel answers, `{"kind": ..., "title"?, ...}` (`View`), and what a host
that can't draw it (and the command line) shows instead, as plain text:

| kind | holds | as plain text |
| --- | --- | --- |
| `list` | `items: [{label, detail?}]` | each label, its detail beside it |
| `table` | `columns`, `rows` (cells: strings, numbers, booleans, `null`) | the headings, then each row, lined up |
| `kv` | `pairs: [{label, value}]` | each label, its value beside it |
| `text` | `text` | as it is |
| `tree` | `items: [{label, detail?, children}]` | each item indented two spaces deeper than the one it's below |
| `chart` | `style` (`line` or `bar`), `series: [{name, points: [[x, y]]}]`, `x_label?`, `y_label?` | a table: x, then a column for each series |
| `image` | `blob` (a file in the scratch folder), `alt` | `[image: alt]` |
| `sections` | `items: [{title, view}]` | each title, then its view |

A `table`'s column is its heading, or `{name, kind, unit?}`, its cells being `text`, `track` (an
id, shown by its artist and title where the host has the library), `duration` (seconds, `3:07`),
`key` (in Camelot notation where it's one) or `number` (with the unit after each). Every kind
from `tree` on, and typed columns, need the `views` feature; an `image` `blobs` too: the host
reads the file, refuses one that isn't an image (or is over 8192 pixels a side), and
gives it on as `data` (a PNG `data:` URL, 512 pixels on its longest side at most). A
view showing nothing is `nothing to show`. `choose` (a question, feature `choose`) is never
drawn, and a section can't be one.

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
answer names go in `req.context.scratch`; `host.notify(..)` tells the user something
(`host/notify`). A hook answers its answer or what makes one: an
operation an `OpPlan`, or an `OpAnswer` when it may ask first (`Choice`; `View::Choose` for a
command). `Choice::together(questions)` asks several in one round, the first with the rest as
its `more`, leaving any past `Choice::MAX_QUESTIONS` for the round after.
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
assert_eq!(reply["features"], json!(["accounts", "analysis", "commands", "blobs", "net", "choose", "views"]));
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

`examples/echo` is a whole plugin, with one of each kind of contribution but events, services and
a library source (`Hooks::on_event` handles events; a service is a hook, `StartService`, that runs
until it's cancelled).

## Licence

MIT.
