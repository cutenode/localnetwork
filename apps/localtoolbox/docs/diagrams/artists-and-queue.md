# localtoolbox diagrams: Artists, the queue and settings

Artist rows, the session queue and the settings sheet. The rest are listed in the [README](../../README.md#diagrams).

- [Adding artists and asking about them](#adding-artists-and-asking-about-them)
- [Life of an artist row](#life-of-an-artist-row)
- [What an artist row draws](#what-an-artist-row-draws)
- [Searching and sorting artists](#searching-and-sorting-artists)
- [Filling and reading the session queue](#filling-and-reading-the-session-queue)
- [What the queue sheet does](#what-the-queue-sheet-does)
- [What the settings sheet changes](#what-the-settings-sheet-changes)

## Adding artists and asking about them

Three sources feed the artists list, and the SoundCloud tab in front, any page of it, can answer for them.

```mermaid
sequenceDiagram
  participant P as artists panel
  participant L as artists store
  participant C as Content script
  participant A as SoundCloud API
  P->>L: add artistsOf the source's tracks
  alt a SoundCloud tab is in front
    P->>C: GET_ARTIST_PROFILES, paths with no answer
    loop four artists at a time
      C->>A: /resolve of the profile
      C->>A: /users/id/web-profiles
      C--)P: ARTIST_PROFILES_PROGRESS
      P->>L: learn, written a second later
    end
    C-->>P: profiles, failed, error
    P->>L: flush
  else any other tab
    Note over P: rows say not looked up, look up waits for a SoundCloud tab
  end
```

Source: [`src/tools/artists/Panel.svelte`](../../src/tools/artists/Panel.svelte), [`src/tools/artists/AddBar.svelte`](../../src/tools/artists/AddBar.svelte), [`src/tools/artists/artists.svelte.ts`](../../src/tools/artists/artists.svelte.ts), [`src/tools/artists/lookup.ts`](../../src/tools/artists/lookup.ts), [`content.ts`](../../content.ts)

## Life of an artist row

What `profile` holds on an entry. A `401` leaves the row as it was, and any other failed `/resolve` counts as no profile, over whatever the row held.

```mermaid
stateDiagram-v2
  state "Not looked up" as Unchecked
  state "Has a profile" as Profile
  state "No profile, remembered" as Unavailable
  [*] --> Unchecked: added
  Unchecked --> Profile: lookup found the user
  Unchecked --> Unavailable: not a public profile, or the request failed
  Unchecked --> Unchecked: 401, or the lookup stopped short
  Profile --> Profile: look up again
  Profile --> Unavailable: look up again, gone or failed
  Unavailable --> Profile: look up again
  Unchecked --> [*]: taken off
  Profile --> [*]: taken off
  Unavailable --> [*]: taken off
```

Source: [`src/tools/artists/artists.svelte.ts`](../../src/tools/artists/artists.svelte.ts), [`src/tools/artists/profiles.ts`](../../src/tools/artists/profiles.ts), [`src/tools/artists/Panel.svelte`](../../src/tools/artists/Panel.svelte), [`content.ts`](../../content.ts)

## What an artist row draws

How one profile becomes a row's badges and an opened row's fields.

```mermaid
flowchart TD
  Profile["profile<br/>bio up to 1,000 characters, up to 20 links"]
  Listed["links the profile lists"]
  Bio["addresses and emails read out of the bio"]
  Links["linksOf<br/>one per address, profile links first"]
  Network["networkOf<br/>lettered badge for each"]
  Row["the row: three badges, then +n"]
  Opened["the opened row: every address to copy, bio,<br/>place, followers, tracks, search badges"]
  Copy["copy the list as text"]

  Profile --> Listed
  Profile --> Bio
  Listed --> Links
  Bio --> Links
  Links --> Network
  Network --> Row
  Links --> Opened
  Profile --> Opened
  Links --> Copy
```

Source: [`src/tools/artists/profiles.ts`](../../src/tools/artists/profiles.ts), [`src/tools/artists/Panel.svelte`](../../src/tools/artists/Panel.svelte), [`content.ts`](../../content.ts)

## Searching and sorting artists

The order comes first and the search second, and the copy button takes whatever is left.

```mermaid
flowchart TD
  Items["artists.items"]
  Sort{"pref:artists-sort"}
  Name["by name"]
  Order["list order"]
  Most["most followed,<br/>no follower count last"]
  Search["search box<br/>every word in the name, path, source,<br/>profile, bio, links or track titles"]
  Paint["createPainting<br/>60 rows first, then 40 a frame"]
  Copy["copy the list as text"]

  Items --> Sort
  Sort --> Name
  Sort --> Order
  Sort --> Most
  Name --> Search
  Order --> Search
  Most --> Search
  Search --> Paint
  Search --> Copy
```

Source: [`src/tools/artists/Panel.svelte`](../../src/tools/artists/Panel.svelte), [`src/tools/artists/profiles.ts`](../../src/tools/artists/profiles.ts), [`src/lib/filter.ts`](../../src/lib/filter.ts), [`src/lib/painting.svelte.ts`](../../src/lib/painting.svelte.ts)

## Filling and reading the session queue

Everything that puts a track in the queue, and everything that reads it.

```mermaid
flowchart LR
  Art["+ queue on cover art"]
  Replaced["replaced Download button,<br/>unless it walks a gate"]
  Walk["a page walk that saved nothing"]
  Manager["Manager: open a link,<br/>list button, batch queue"]
  Downloader["Downloader: open on a failed row"]
  Import["importing a state file"]
  Queue[("queue<br/>chrome.storage.session")]
  Header["count in every header"]
  Sheet["the queue sheet"]
  Tools["playlist to text and<br/>Downloader, as a source"]
  Add["Manager and artists add bars"]
  Rows["Manager list button,<br/>Downloader's in the queue, hide queued"]
  Page["queue button on the page"]
  Export["the exported state file"]

  Art --> Queue
  Replaced --> Queue
  Walk --> Queue
  Manager --> Queue
  Downloader --> Queue
  Import --> Queue
  Queue --> Header
  Queue --> Sheet
  Queue --> Tools
  Queue --> Add
  Queue --> Rows
  Queue --> Page
  Queue --> Export
```

Source: [`src/lib/queue.svelte.ts`](../../src/lib/queue.svelte.ts), [`content.ts`](../../content.ts), [`src/tools/manager/Panel.svelte`](../../src/tools/manager/Panel.svelte), [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte), [`src/lib/components/StateTransfer.svelte`](../../src/lib/components/StateTransfer.svelte)

## What the queue sheet does

Each control on the sheet. Marks outlast the queue, and a queued track outlasts its mark. A tool already open under the sheet keeps its source until it next mounts.

```mermaid
flowchart TD
  Sheet["Queue sheet"]
  MarkAll["mark the batch"]
  TakeOut["take the marked out"]
  Send["send to playlist to text<br/>or the Downloader"]
  RowCheck["a row's check button"]
  RowX["a row's remove button"]
  Clear["clear, asks once more"]
  Marks[("marks")]
  Queue[("queue")]
  Tool["that tool, reading the queue<br/>when it next mounts"]

  Sheet --> MarkAll
  Sheet --> TakeOut
  Sheet --> Send
  Sheet --> RowCheck
  Sheet --> RowX
  Sheet --> Clear
  MarkAll -->|"one write"| Marks
  RowCheck -->|toggle| Marks
  TakeOut -->|"one write"| Queue
  RowX --> Queue
  Clear -->|"empties it"| Queue
  Send -->|"pref:source set to queue"| Tool
```

Source: [`src/lib/components/QueueSheet.svelte`](../../src/lib/components/QueueSheet.svelte), [`src/lib/queue.svelte.ts`](../../src/lib/queue.svelte.ts), [`src/lib/source.ts`](../../src/lib/source.ts), [`src/lib/prefs.svelte.ts`](../../src/lib/prefs.svelte.ts)

## What the settings sheet changes

Each section, where it keeps what it sets, and who obeys it. An import writes back to the marks, the details, both lists, the queue and three pref keys.

```mermaid
flowchart LR
  Page["SoundCloud pages<br/>replace buttons, likes as a list"]
  Down["downloads<br/>email, name, mark"]
  Badges["search badges"]
  Look["appearance<br/>theme, title case"]
  Data["your data"]
  About["about<br/>version, report a bug"]
  Settings[("settings<br/>chrome.storage.local")]
  Prefs[("pref keys<br/>localStorage")]
  Content["Content script, in every tab"]
  Pages["popup theme, Downloader,<br/>offscreen page, Manager<br/>and artists rows"]
  File["state file"]
  Bug["GitHub's bug form, filled in"]

  Page --> Settings
  Settings --> Content
  Down --> Prefs
  Badges --> Prefs
  Look --> Prefs
  Prefs --> Pages
  Data <--> File
  About --> Bug
```

Source: [`src/lib/components/SettingsSheet.svelte`](../../src/lib/components/SettingsSheet.svelte), [`src/lib/settings.svelte.ts`](../../src/lib/settings.svelte.ts), [`src/lib/downloader.svelte.ts`](../../src/lib/downloader.svelte.ts), [`src/lib/components/StateTransfer.svelte`](../../src/lib/components/StateTransfer.svelte), [`src/lib/bug-report.ts`](../../src/lib/bug-report.ts)
