# locallibrary diagrams: the player

Views, panes, threads, playback and what it remembers. The rest are listed in the [README](../../README.md#diagrams).

- [Player views](#player-views)
- [Panes and dialogs](#panes-and-dialogs)
- [Player threads](#player-threads)
- [Opening a drive in the player](#opening-a-drive-in-the-player)
- [Changing a drive from the player](#changing-a-drive-from-the-player)
- [An unplugged drive in the player](#an-unplugged-drive-in-the-player)
- [Drives view rows](#drives-view-rows)
- [Search and filter](#search-and-filter)
- [Playback](#playback)
- [Output devices](#output-devices)
- [Signing in from the player](#signing-in-from-the-player)
- [Identifying in the Mixes view](#identifying-in-the-mixes-view)
- [What the player remembers](#what-the-player-remembers)

## Player views

The player's five views and what crosses from one to another. `1` to `5` switch views, unless the pads make the digits hot cues. `o` in any view goes to the playing track, in the library or, for a set, in Mixes.

```mermaid
flowchart TD
    library["1 Library<br/>drives, playlist tree, tracks, details, crate"]
    drives["2 Drives<br/>libraries, drives, the work on each"]
    snapshots["3 Snapshots<br/>the store's snapshots"]
    profile["4 Profile<br/>DJ profile, DJ account"]
    mixes["5 Mixes<br/>recordings and their tracks"]
    drives -->|"enter opens a drive"| library
    mixes -->|"o goes to a track"| library
    snapshots -.->|"enter: a playlist back onto<br/>the open drive, staying here"| library
    library -->|"mark playlists to pack"| drives
    library -->|"m marks, a adds, t shows it"| crate["Crate: TAG LIST"]
    crate -->|"w saves it to its drive"| crate
    crate -->|"packed onto a drive"| drives
```

Source: [`src/tui/app/`](../../src/tui/app), [`src/tui/keymap.rs`](../../src/tui/keymap.rs), [`src/tui/ui/`](../../src/tui/ui), [`src/tui/drives.rs`](../../src/tui/drives.rs), [`src/tui/profile.rs`](../../src/tui/profile.rs), [`src/tui/mix.rs`](../../src/tui/mix.rs), [`src/tui/crate_list.rs`](../../src/tui/crate_list.rs)

## Panes and dialogs

Where keys go in the Library view, and the dialogs that take them first.

```mermaid
flowchart TD
    first["Ahead of the panes: help ?, messages !,<br/>a dialog, the output list d, the search box"] --> panes["Panes, tab to move: drive list,<br/>playlist tree, tracks, details, crate"]
    panes -->|"e, or the right mouse<br/>button but on the drive list"| menu["Menu"]
    panes -->|":"| pick["Pick: the palette"]
    menu --> prompt["Prompt: a name"]
    pick --> prompt
    menu --> form["Form: a plugin's params"]
    pick --> form
    pick --> report["Report: a check's or<br/>a command's results"]
    pick --> ask["Ask: a yes or no<br/>off the drive"]
    prompt --> plan["A plan, in the background"]
    form --> plan
    plan --> confirm["Confirm: y applies, esc stops"]
```

Source: [`src/tui/edits.rs`](../../src/tui/edits.rs), [`src/tui/palette.rs`](../../src/tui/palette.rs), [`src/tui/form.rs`](../../src/tui/form.rs), [`src/tui/keymap.rs`](../../src/tui/keymap.rs)

## Player threads

What runs where. Library reads and writes run off the UI thread, with one exception: reviewing a plugin's suggestion about tracks on a drive that isn't open reads it there. At exit the UI thread waits for a write under way.

```mermaid
flowchart TD
    loop["UI thread: draw, keys, mouse, tick.<br/>60 fps while moving, else 4 a second"]
    workers["Worker threads: one per job"] -->|"Msg"| channel["Channel, read each tick"]
    opener["Track opener thread:<br/>one file at a time"] -->|"Opened, TrackInfo"| channel
    plugins["Threads reading each plugin"] -->|"Notice, Suggested"| channel
    stderr["stderr capture thread"] -->|"Log"| channel
    channel --> loop
    loop -->|"spawn"| workers
    loop -->|"open a track"| opener
    loop -->|"each second"| poll["A worker polls the volumes"]
    poll --> channel
    audio["Audio thread"] -->|"playhead and tap, read without locks"| loop
    loop -->|"seek, loop, pause, through the playhead"| audio
```

Source: [`src/tui/mod.rs`](../../src/tui/mod.rs), [`src/tui/app/`](../../src/tui/app), [`src/tui/audio.rs`](../../src/tui/audio.rs), [`src/tui/stderr.rs`](../../src/tui/stderr.rs), [`src/tui/plugins/events.rs`](../../src/tui/plugins/events.rs)

## Opening a drive in the player

The reads a drive starts, each on its own worker. A result for a drive no longer open is dropped.

```mermaid
sequenceDiagram
    participant U as UI thread
    participant L as Library read
    participant B as Other reads
    U->>L: Read the library
    L-->>U: Library, tracks shown at once
    U->>B: Check files, index, sets, replay offer
    U->>B: My Tags and analyzer fields, when shown
    B-->>U: Missing: tracks with no audio file
    B-->>U: Indexed: sort ranks, Browse's groups
    B-->>U: Sets: the store's sets
    B-->>U: Replay: edits an export replaced
    B-->>U: MyTags, Fields
```

Source: [`src/tui/app/`](../../src/tui/app), [`src/tui/browse.rs`](../../src/tui/browse.rs)

## Changing a drive from the player

How a change goes from a key to the drive, and when it can still be stopped.

```mermaid
stateDiagram-v2
    state "Planning in the background" as Planning
    state "Confirm dialog" as Confirm
    state "Writing" as Writing
    state "Past stopping" as Committed
    state "Reading the drive again" as Reading
    [*] --> Idle
    Idle --> Planning: an action
    Planning --> Idle: esc, or nothing to change, or an error
    Planning --> Confirm: a plan
    Confirm --> Idle: esc or n
    Confirm --> Writing: y
    Writing --> Committed: databases being written, or files deleted
    Writing --> Reading: done, failed, or esc before it commits
    Writing --> Idle: the open drive wasn't written
    Committed --> Reading: done, or failed
    Reading --> Idle
    Reading --> Planning: a crate saved, now packed
```

Source: [`src/tui/edits.rs`](../../src/tui/edits.rs), [`src/tui/app/`](../../src/tui/app)

## An unplugged drive in the player

What the player does when the open drive goes away. It looks each second.

```mermaid
stateDiagram-v2
    state "Open" as Open
    state "Shown, marked unplugged" as Stale
    state "Reading it again" as Reading
    [*] --> Open
    Open --> Stale: gone at a poll
    Stale --> Stale: playing from it, planning any change on it and identifying against it refused
    Stale --> Reading: back
    Reading --> Open: read
```

Source: [`src/tui/app/`](../../src/tui/app), [`src/tui/ui/`](../../src/tui/ui), [`src/tui/drives.rs`](../../src/tui/drives.rs), [`src/tui/mix.rs`](../../src/tui/mix.rs)

## Drives view rows

What each row of the Drives view is, and what `enter` and `e` do on it.

```mermaid
flowchart TD
    rows["Rows, in order: each library then its<br/>drives plugged in, then drives of no library"]
    rows --> library["A library"]
    rows --> drive["A drive"]
    rows --> none["The heading of drives of no library"]
    library -->|enter| open["Opens its newest drive in the library"]
    library -->|e| clones["Sync its clones, check them for a set, eject them"]
    drive -->|enter| browse["Browses the drive"]
    drive -->|e| work["Pull, check, set prep, sync, pack, restore, replay,<br/>undo, eject, and plugins' operations for a drive"]
    none -->|e| select["Says to select one of the drives"]
```

Source: [`src/tui/drives.rs`](../../src/tui/drives.rs), [`src/tui/ui/`](../../src/tui/ui)

## Search and filter

What the search box does as you type.

```mermaid
flowchart TD
    slash["/ opens the box. Tab: this list or the whole drive"] --> typed["Typing. Over 5,000 tracks, it waits for 150 ms of quiet"]
    typed --> parse["Plain words, and terms with<br/>a field read as a filter"]
    parse -->|"plain words"| words["Matched across each track's text"]
    parse -->|"a filter"| filter["The filter language, as tracks --filter"]
    parse -->|"a filter still being typed"| last["The last good list stays"]
    filter -->|"tag: or sound.bpm:"| read["My Tags or analyzer fields read in the<br/>background: until then they match nothing"]
    words --> list["The list narrows, the selection kept if it's still there"]
    filter --> list
    read --> list
```

Source: [`src/tui/app/`](../../src/tui/app), [`src/tui/browse.rs`](../../src/tui/browse.rs), [`crates/locallibrary/src/query.rs`](../../../../crates/locallibrary/src/query.rs)

## Playback

The path from a file on a drive to the output, and what the UI reads back.

```mermaid
flowchart TD
    file["Audio file"] --> opener["Opener thread"]
    next["Next track, opened 30 s<br/>before this one ends"] --> opener
    opener --> decoder["rodio decoder"]
    decoder --> looper["Looper: counts frames,<br/>plays loops from memory"]
    looper --> tap["Tap: copies samples<br/>to a ring buffer"]
    tap --> output["Output device"]
    looper --> playhead["Playhead"]
    playhead --> ui["UI: clock, waveforms, cues"]
    tap --> vis["Visualisers"]
```

Source: [`src/tui/audio.rs`](../../src/tui/audio.rs), [`src/tui/deck.rs`](../../src/tui/deck.rs), [`src/tui/analyzer.rs`](../../src/tui/analyzer.rs), [`src/tui/app/`](../../src/tui/app)

## Output devices

The output the player plays through. Opening one never holds up the UI.

```mermaid
stateDiagram-v2
    state "No output" as None
    state "Opening in the background" as Opening
    state "Playing through it" as Open
    [*] --> None
    None --> Opening: the player starting, a track to play, or space
    Opening --> Open: opened, or the default if the picked one is missing
    Opening --> None: none opens, the track stops where it was
    Open --> Opening: d picks another, the track carries on
    Opening --> Open: a pick that fails keeps the output there is
    Open --> None: it went away, the track waits where it was
```

Source: [`src/tui/app/`](../../src/tui/app), [`src/tui/deck.rs`](../../src/tui/deck.rs), [`src/tui/audio.rs`](../../src/tui/audio.rs)

## Signing in from the player

The DJ account's sign-in on the Profile view, kept off the UI thread.

```mermaid
stateDiagram-v2
    state "Typing the handle" as Typing
    state "Finding the account's server" as Starting
    state "Waiting for the browser" as Waiting
    state "Signing out" as SigningOut
    state "Signed in" as In
    state "Signed out" as Out
    [*] --> Out
    [*] --> In: an account saved
    Out --> Typing: L
    Typing --> Out: esc
    Typing --> Starting: enter
    Starting --> Waiting: the browser was sent
    Starting --> Out: esc, or it failed
    Waiting --> In: the browser came back
    Waiting --> Out: esc, or it failed
    In --> SigningOut: L twice within a few seconds
    SigningOut --> Out: done
    SigningOut --> In: it failed
```

Source: [`src/tui/account.rs`](../../src/tui/account.rs), [`src/tui/profile.rs`](../../src/tui/profile.rs)

## Identifying in the Mixes view

What the Mixes view does with a recording, on a worker. An older run's answer is dropped.

```mermaid
stateDiagram-v2
    state "Listing recordings" as List
    state "Identifying, with progress" as Busy
    state "A report" as Report
    state "What went wrong" as Failed
    [*] --> List
    List --> List: not a recording, or no drive open, refused
    List --> Busy: a recording picked, or O
    Busy --> List: esc
    Busy --> Report: its tracks found, remembered
    Busy --> Failed: an error
    Report --> Busy: another recording
    Failed --> Busy: another recording
    Report --> List: esc
```

Source: [`src/tui/mix.rs`](../../src/tui/mix.rs), [`src/tui/app/`](../../src/tui/app)

## What the player remembers

What's saved between runs, and where it goes before there's a store.

```mermaid
flowchart TD
    change["Something changes"] --> check["Checked every 2 s, and on quitting"]
    check --> has{"Where to?"}
    has -->|"a store"| tui["tui.json in the store"]
    has -->|"no store yet"| beside["STORE-player/tui.json,<br/>in a folder beside it"]
    has -->|"inside the open drive,<br/>or its file can't be read"| none["Not saved"]
    tui --> keeps["Drive, list, view, sort, volume, panels, columns,<br/>theme, visualiser, modes, output, crate,<br/>recordings opened, queue and place"]
    beside --> keeps
    keeps -->|"preferences only"| export["settings export"]
```

Source: [`src/tui/state.rs`](../../src/tui/state.rs), [`src/tui/app/`](../../src/tui/app), [`crates/locallibrary/src/store.rs`](../../../../crates/locallibrary/src/store.rs), [`crates/locallibrary/src/settings/transfer.rs`](../../../../crates/locallibrary/src/settings/transfer.rs)
