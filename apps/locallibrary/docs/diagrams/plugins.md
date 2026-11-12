# locallibrary diagrams: Plugins

Loading, hooks, what a plugin can reach, events and services. The rest are listed in the [README](../../README.md#diagrams).

- [What plugins can add](#what-plugins-can-add)
- [Built-in plugins that work on a library](#built-in-plugins-that-work-on-a-library)
- [Built-in plugins that publish a set](#built-in-plugins-that-publish-a-set)
- [Loading plugins](#loading-plugins)
- [A plugin process's life](#a-plugin-processs-life)
- [Talking to a plugin](#talking-to-a-plugin)
- [Plugin question rounds](#plugin-question-rounds)
- [What a plugin can reach](#what-a-plugin-can-reach)
- [A plugin's net/fetch](#a-plugins-netfetch)
- [How plugins change a library](#how-plugins-change-a-library)
- [Plugins that write elsewhere](#plugins-that-write-elsewhere)
- [Running an analyzer](#running-an-analyzer)
- [Running checks and guards](#running-checks-and-guards)
- [Plugin panels](#plugin-panels)
- [Plugin settings and keys](#plugin-settings-and-keys)
- [Plugin events and suggestions](#plugin-events-and-suggestions)
- [A plugin service's life](#a-plugin-services-life)

## What plugins can add

Each thing a plugin's manifest can contribute, and the protocol method that asks for it.

```mermaid
flowchart TD
    hub["A plugin's manifest"]
    hub --> commands["Commands<br/>command/run"]
    hub --> operations["Operations<br/>op/plan"]
    hub --> panels["Panels<br/>panel/render"]
    hub --> analyzers["Analyzers<br/>track/analyze"]
    hub --> checks["Checks and guards<br/>check/run, plan/check"]
    hub --> exporters["Exporters<br/>export/write"]
    hub --> importers["Importers<br/>import/read"]
    hub --> providers["Providers<br/>provide/candidates"]
    hub --> sources["Library sources<br/>library/read"]
    hub --> destinations["Destinations<br/>blobs/*, manifest/*, set/publish"]
    hub --> services["Services<br/>service/start"]
    hub --> events["Events<br/>event/notify"]
    hub --> manifest["From the manifest alone<br/>account types, themes, settings"]
    commands ~~~ checks
    operations ~~~ exporters
    panels ~~~ importers
    analyzers ~~~ providers
    checks ~~~ sources
    exporters ~~~ destinations
    importers ~~~ services
    providers ~~~ events
    sources ~~~ manifest
```

Source: [`crates/loclib-plugin/src/manifest.rs`](../../../../crates/loclib-plugin/src/manifest.rs), [`crates/loclib-plugin/src/hooks.rs`](../../../../crates/loclib-plugin/src/hooks.rs), [`crates/locallibrary/src/plugin.rs`](../../../../crates/locallibrary/src/plugin.rs)

## Built-in plugins that work on a library

The built-in plugins that read and change libraries, what each adds, and the servers it may reach.

```mermaid
flowchart LR
    sound["sound"] --> soundadds["Analyzer tempo-key-loudness<br/>events: suggests analysing new tracks"]
    musicbrainz["musicbrainz"] --> mbadds["Analyzer ids<br/>musicbrainz.org"]
    coverart["coverart"] --> coveradds["Providers covers, tags<br/>events: suggests fetching covers<br/>musicbrainz.org, coverartarchive.org,<br/>archive.org, *.archive.org"]
    discogs["discogs"] --> discogsadds["Providers covers, tags<br/>discogs account: a personal access token<br/>api.discogs.com, i.discogs.com"]
    demos["demos"] --> demoadds["Check tracks, operation clean"]
    tracklist["tracklist"] --> trackadds["Operation import: playlist import<br/>Importer and source for rekordbox XML"]
    interop["interop"] --> interopadds["Exporters traktor, serato, engine, rekordbox<br/>Importer and source for Traktor"]
```

Source: [`plugins/sound/locallibrary-plugin.toml`](../../../../plugins/sound/locallibrary-plugin.toml), [`plugins/musicbrainz/locallibrary-plugin.toml`](../../../../plugins/musicbrainz/locallibrary-plugin.toml), [`plugins/coverart/locallibrary-plugin.toml`](../../../../plugins/coverart/locallibrary-plugin.toml), [`plugins/discogs/locallibrary-plugin.toml`](../../../../plugins/discogs/locallibrary-plugin.toml), [`plugins/demos/locallibrary-plugin.toml`](../../../../plugins/demos/locallibrary-plugin.toml), [`plugins/tracklist/locallibrary-plugin.toml`](../../../../plugins/tracklist/locallibrary-plugin.toml), [`plugins/interop/locallibrary-plugin.toml`](../../../../plugins/interop/locallibrary-plugin.toml)

## Built-in plugins that publish a set

Each set destination, the account type it uses, and how that account is signed in to. Browser sign-ins need each service's app registered: the manifests' client ids are placeholders so far.

```mermaid
flowchart LR
    plyr["plyr: track<br/>api.plyr.fm"] --> plyrtype["plyr account<br/>keys: a developer token"]
    archive["archive: item<br/>archive.org, s3.us.archive.org"] --> archivetype["archive account<br/>keys: S3 keys"]
    mixcloud["mixcloud: show<br/>api.mixcloud.com"] --> mixtype["mixcloud account<br/>browser, token exchange"]
    soundcloud["soundcloud: track<br/>api.soundcloud.com"] --> soundtype["soundcloud account<br/>browser, token exchange"]
    youtube["youtube: video<br/>www.googleapis.com"] --> googletype["google account<br/>browser, token exchange"]
    youtube --> ffmpeg["FFmpeg on this computer"]
```

Source: [`plugins/plyr/locallibrary-plugin.toml`](../../../../plugins/plyr/locallibrary-plugin.toml), [`plugins/archive/locallibrary-plugin.toml`](../../../../plugins/archive/locallibrary-plugin.toml), [`plugins/mixcloud/locallibrary-plugin.toml`](../../../../plugins/mixcloud/locallibrary-plugin.toml), [`plugins/soundcloud/locallibrary-plugin.toml`](../../../../plugins/soundcloud/locallibrary-plugin.toml), [`plugins/youtube/locallibrary-plugin.toml`](../../../../plugins/youtube/locallibrary-plugin.toml), [`plugins/accounts`](../../../../plugins/accounts)

## Loading plugins

How the plugins in `plugins/` end up in a host. Only built-in plugins run. A clash is with a command, a sidecar method or another plugin, or an account nothing can meet.

```mermaid
flowchart TD
    folders["plugins/ID, plugins/accounts/ID"] --> build["build.rs writes the table:<br/>each manifest and how it serves"]
    build --> specs["Compiled in. A spec for each:<br/>run this binary, plugin serve ID"]
    specs --> check{"Debug builds<br/>only: a clash?"}
    check -->|yes| out["Left out, and said why"]
    check -->|"no, or a release"| host["The plugin host. A repeated id or<br/>cli place refuses every plugin"]
    host --> lazy["Started when first used"]
    host --> account["Account plugins:<br/>signed in from, never started"]
```

Source: [`build.rs`](../../build.rs), [`src/plugins.rs`](../../src/plugins.rs), [`crates/locallibrary/src/plugin.rs`](../../../../crates/locallibrary/src/plugin.rs)

## A plugin process's life

From first use to being stopped, and what happens when one misbehaves.

```mermaid
stateDiagram-v2
    state "Not running" as Off
    state "Starting" as Start
    state "Saying hello" as Hello
    state "Running" as Run
    state "Stopping" as Stop
    [*] --> Off
    Off --> Start: first used
    Start --> Off: won't start, or the host runs as root or an administrator
    Start --> Hello: spawned in STORE/plugins/ID/work, a temporary folder with no store
    Hello --> Off: wrong protocol, id, version, manifest or features, or no answer in 10 s
    Hello --> Run: all as its manifest says
    Run --> Off: it crashed, started again on next use
    Run --> Off: ignored a cancel for 5 s, killed
    Run --> Stop: idle 60 s, or the command ended
    Stop --> Off: input closed, killed after 5 s
```

Source: [`crates/locallibrary/src/plugin/process.rs`](../../../../crates/locallibrary/src/plugin/process.rs), [`crates/locallibrary/src/plugin.rs`](../../../../crates/locallibrary/src/plugin.rs)

## Talking to a plugin

The protocol: JSON-RPC, one message a line over the plugin's stdin and stdout.

```mermaid
sequenceDiagram
    participant H as Plugin host
    participant P as Plugin process
    H->>P: hello: protocol, host, features
    P-->>H: plugin id, version, features
    H->>P: describe
    P-->>H: Its manifest, which must match
    H->>P: A hook: track/analyze, op/plan...
    P->>H: A read, net/fetch or accounts/ensure
    H-->>P: Checked, then answered
    P->>H: progress, partial, host/notify
    opt You stop it
        H->>P: $/cancel
    end
    P-->>H: The hook's answer
```

Source: [`crates/loclib-plugin/src/protocol.rs`](../../../../crates/loclib-plugin/src/protocol.rs), [`crates/locallibrary/src/plugin/process.rs`](../../../../crates/locallibrary/src/plugin/process.rs), [`crates/locallibrary/src/plugin/callbacks.rs`](../../../../crates/locallibrary/src/plugin/callbacks.rs)

## Plugin question rounds

What happens when a hook asks a question before it can answer.

```mermaid
sequenceDiagram
    participant F as Front end
    participant H as Plugin host
    participant P as Plugin
    F->>H: Run a command or plan an operation
    H->>P: The hook
    P-->>H: A question, up to 64 in one round
    alt Someone to ask
        H-->>F: The items: a list in the player,<br/>numbered in a terminal
        F->>H: The answers
        H->>P: The same hook, its params set
        P-->>H: Its answer, or another round
    else --json, or no terminal
        H-->>F: Refused, saying how to answer with --choose
    end
```

Source: [`crates/loclib-plugin/src/choice.rs`](../../../../crates/loclib-plugin/src/choice.rs), [`src/plugin_cmd/choose.rs`](../../src/plugin_cmd/choose.rs), [`src/tui/plugins.rs`](../../src/tui/plugins.rs), [`crates/locallibrary/src/plugin/operations.rs`](../../../../crates/locallibrary/src/plugin/operations.rs), [`crates/locallibrary/src/plugin/commands.rs`](../../../../crates/locallibrary/src/plugin/commands.rs)

## What a plugin can reach

The limits the host keeps. Your system doesn't enforce them: a plugin runs as you, isn't sandboxed, and could read any file you can.

```mermaid
flowchart LR
    plugin["Plugin process"] -->|"reads its capabilities grant"| reads["Sidecar read methods: drives by name,<br/>pages bounded, paths scrubbed"]
    plugin -->|"net/fetch"| net["Servers its net: capabilities name,<br/>sending files lent to the hook"]
    plugin -->|"accounts/ensure"| ensure["More scopes of an account"]
    plugin -->|"its answers"| scratch["Its request's scratch folder"]
    plugin -->|"its files"| work["STORE/plugins/ID/work/"]
    plugin -->|"fs:read"| folders["Folders you grant for a source read"]
    plugin -->|"host/notify"| notify["A message or a suggestion"]
    plugin -.->|"no way through the host"| drive[("Writing a drive or the store")]
    plugin -.->|"no way through the host"| tokens["Tokens, keys, secret settings"]
```

Source: [`crates/locallibrary/src/plugin/callbacks.rs`](../../../../crates/locallibrary/src/plugin/callbacks.rs), [`crates/locallibrary/src/plugin/net.rs`](../../../../crates/locallibrary/src/plugin/net.rs), [`crates/locallibrary/src/plugin/blobs.rs`](../../../../crates/locallibrary/src/plugin/blobs.rs), [`crates/locallibrary/src/plugin/secrets.rs`](../../../../crates/locallibrary/src/plugin/secrets.rs), [`crates/loclib-plugin/src/manifest.rs`](../../../../crates/loclib-plugin/src/manifest.rs), [`docs/plugins.md`](../plugins.md)

## A plugin's net/fetch

What the host checks before a plugin's request leaves, and what it does with the answer.

```mermaid
flowchart TD
    host["net/fetch: public https, in its net:<br/>capabilities, no user or password?"]
    host -->|yes| account["A scope of its access covers<br/>any account it names?"]
    host -->|no| denied["Refused"]
    account -->|no| denied
    account -->|yes| fill["Fill in the token, keys or secret settings,<br/>refreshing a token about to expire"]
    fill --> send["Wait its turn under the [[net]] limit,<br/>send as locallibrary/VERSION"]
    send -->|"401, token refused"| again["Refresh it, once"]
    again --> send
    send -->|"a redirect, 5 at most"| follow["Checked again. The token stops at<br/>another server, or where no scope covers"]
    follow --> send
    follow -->|"somewhere refused"| denied
    send -->|"scopes lacking"| denied
    send -->|"any other answer"| answer["Passed on, tokens kept out of logs. A scratch<br/>file if it asks, over 8 MiB refused otherwise"]
```

Source: [`crates/locallibrary/src/plugin/net.rs`](../../../../crates/locallibrary/src/plugin/net.rs), [`crates/locallibrary/src/plugin/accounts/token.rs`](../../../../crates/locallibrary/src/plugin/accounts/token.rs), [`crates/locallibrary/src/plugin/accounts.rs`](../../../../crates/locallibrary/src/plugin/accounts.rs)

## How plugins change a library

Plugins never write to a drive. What each kind of hook answers becomes a plan the host makes.

```mermaid
flowchart TD
    op["Operation:<br/>answers edits"] --> plan["One edit plan, made by the host"]
    fix["A check's fix:<br/>an operation"] --> plan
    provider["Provider: you pick,<br/>or a clear best is picked"] --> plan
    importer["Importer:<br/>what a file says"] --> plan
    importer -->|"its plays"| sessions[("Histories, in sets.json")]
    analyzer["Analyzer: fields<br/>kept in cache/"] -->|"analyze --apply"| plan
    plan --> guards["Plugins' guards"]
    guards --> confirm["Confirmed like any change"]
    guards -->|"a blocking guard found something"| blocked["Not applied"]
    confirm --> write["The one write routine:<br/>backed up, journaled"]
```

Source: [`crates/locallibrary/src/plugin/operations.rs`](../../../../crates/locallibrary/src/plugin/operations.rs), [`crates/locallibrary/src/plugin/providers.rs`](../../../../crates/locallibrary/src/plugin/providers.rs), [`crates/locallibrary/src/plugin/imports.rs`](../../../../crates/locallibrary/src/plugin/imports.rs), [`crates/locallibrary/src/plugin/analyzers.rs`](../../../../crates/locallibrary/src/plugin/analyzers.rs), [`crates/locallibrary/src/plugin/apply.rs`](../../../../crates/locallibrary/src/plugin/apply.rs), [`crates/locallibrary/src/plugin/sources.rs`](../../../../crates/locallibrary/src/plugin/sources.rs), [`crates/locallibrary/src/plugin/exports.rs`](../../../../crates/locallibrary/src/plugin/exports.rs)

## Plugins that write elsewhere

Two hooks that write files without an edit plan, and what the host checks.

```mermaid
flowchart TD
    source["Library source:<br/>answers tracks"] --> copied["The host checks each file is in<br/>a folder you granted, and hashes it"]
    copied --> built["Written as collection export writes"]
    exporter["Exporter: writes its own<br/>files, where you say"] --> dry["A dry run first, overwrites confirmed. Never on<br/>a drive or in the store, and it must write what it said"]
```

Source: [`crates/locallibrary/src/plugin/sources.rs`](../../../../crates/locallibrary/src/plugin/sources.rs), [`crates/locallibrary/src/plugin/exports.rs`](../../../../crates/locallibrary/src/plugin/exports.rs), [`crates/locallibrary/src/collection_export.rs`](../../../../crates/locallibrary/src/collection_export.rs)

## Running an analyzer

`track/analyze` over a drive's tracks. Each audio file is handed to the plugin read only.

```mermaid
flowchart TD
    tracks["analyze: the drive's tracks"] --> each{"Each track"}
    each -->|"its audio isn't there"| failed["A warning, counted as failed"]
    each -->|"fields kept for this file<br/>and analyzer version"| kept["Left as it is"]
    each -->|"otherwise"| ask["track/analyze, a few at a time"]
    ask -->|"answered, fields as declared"| fields[("Fields in cache/,<br/>grids and waveforms beside them")]
    ask -->|"it failed"| failed
    ask -->|"a field it didn't declare"| failed
```

Source: [`crates/locallibrary/src/plugin/analyzers.rs`](../../../../crates/locallibrary/src/plugin/analyzers.rs), [`crates/locallibrary/src/fields.rs`](../../../../crates/locallibrary/src/fields.rs), [`src/plugin_cmd/analyze.rs`](../../src/plugin_cmd/analyze.rs)

## Running checks and guards

How a plugin's check is held to its time, for `check`, `doctor` and plans.

```mermaid
flowchart TD
    check["check/run on a drive"] --> within{"Answered in time?<br/>10 s, checks.timeout up to an hour"}
    guard["plan/check on a plan's edits"] --> within
    within -->|yes| found["Its findings, named PLUGIN.CHECK"]
    within -->|"late or failed: a check"| warning["A warning saying so"]
    within -->|"late or failed: a guard"| info["An info line saying so"]
    found -->|"a blocking guard, warning or worse"| blocked["The plan is blocked"]
```

Source: [`crates/locallibrary/src/plugin/checks.rs`](../../../../crates/locallibrary/src/plugin/checks.rs), [`src/plugin_cmd/checks.rs`](../../src/plugin_cmd/checks.rs)

## Plugin panels

A panel under a track's details in the player. Nothing a plugin does runs as a frame is drawn.

```mermaid
stateDiagram-v2
    state "Waiting for the selection to settle" as Wait
    state "Loading, in the room it took last" as Loading
    state "Shown" as Ready
    state "Failed, the reason in the messages" as Failed
    [*] --> Wait
    Wait --> Loading: panel/render, on a worker per panel
    Loading --> Ready: answered
    Loading --> Failed: an error
    Loading --> Wait: the selection moved, cancelled
    Ready --> Wait: the selection moved
    Failed --> Wait: the selection moved
```

Source: [`src/tui/plugins/panels.rs`](../../src/tui/plugins/panels.rs), [`crates/locallibrary/src/plugin/panels.rs`](../../../../crates/locallibrary/src/plugin/panels.rs)

## Plugin settings and keys

Where what you set for a plugin goes, from `plugin settings` or the player's form.

```mermaid
flowchart TD
    set["plugin settings ID KEY VALUE,<br/>or the player's form"] --> plain["A setting, given to each hook"]
    set --> secret["A secret, --secret"]
    set --> service["services.NAME: a switch"]
    set --> key["keys.NAME: a key in the player"]
    plain --> file[("plugins/ID/settings.json")]
    secret --> keychain["The keychain, or secrets/plugins/ID.json.<br/>Filled into net/fetch by the host, never shown"]
    service --> file
    key --> taken{"In the player's form: the player uses it,<br/>or another command has it?"}
    taken -->|yes| refused["Refused, saying why"]
    taken -->|no| file
    key -->|"plugin settings: saved, and the player<br/>says why and leaves it unbound"| file
```

Source: [`crates/locallibrary/src/plugin/settings.rs`](../../../../crates/locallibrary/src/plugin/settings.rs), [`crates/locallibrary/src/plugin/secrets.rs`](../../../../crates/locallibrary/src/plugin/secrets.rs), [`src/tui/keymap/bound.rs`](../../src/tui/keymap/bound.rs), [`src/tui/plugins.rs`](../../src/tui/plugins.rs)

## Plugin events and suggestions

What plugins are told, and how what they say back reaches you.

```mermaid
flowchart TD
    volumes["Player, sidecar, watch"] -->|"drive.mounted,<br/>unmounted, changed"| queue["A queue per plugin listening,<br/>its capability allowing, 64 at most"]
    pulls["watch, watch.start"] -->|"drive.pulled,<br/>set.returned"| queue
    sidecar["The sidecar"] -->|"job.finished"| queue
    apply["Any plan applied"] -->|"plan.applied"| queue
    queue -->|"event/notify, then host/notify"| notice["A message, maybe a suggestion"]
    notice --> player["Player: a message,<br/>review: in the palette"]
    notice --> out["Sidecar: plugin.notice.<br/>watch: says it"]
    notice -->|"suggestions, in the<br/>player and watch"| kept[("plugins/suggested.json,<br/>the newest 20")]
    kept --> player
    player -->|"reviewed or dismissed"| seen[("plugins/seen.json")]
    seen -.->|"left out if seen or waiting"| notice
```

Source: [`crates/locallibrary/src/plugin/events.rs`](../../../../crates/locallibrary/src/plugin/events.rs), [`src/plugins/events.rs`](../../src/plugins/events.rs), [`src/tui/plugins/events.rs`](../../src/tui/plugins/events.rs), [`crates/locallibrary/src/op.rs`](../../../../crates/locallibrary/src/op.rs)

## A plugin service's life

A service runs only in a host that stays open: the player, the sidecar or `watch`.

```mermaid
stateDiagram-v2
    state "Off" as Off
    state "Running" as Run
    state "Waiting to start again" as Wait
    state "Left off" as Gone
    [*] --> Off
    Off --> Run: turned on, while a host stays open
    Run --> Off: turned off, or the host closes
    Run --> Wait: it stopped or crashed
    Wait --> Run: after a second, doubling up to 60 s
    Wait --> Off: turned off
    Run --> Gone: the 6th failure in a row, a run of 60 s resets the count
    Gone --> Run: the next host start, or its settings saved
```

Source: [`crates/locallibrary/src/plugin/services.rs`](../../../../crates/locallibrary/src/plugin/services.rs), [`crates/locallibrary/src/plugin.rs`](../../../../crates/locallibrary/src/plugin.rs)
