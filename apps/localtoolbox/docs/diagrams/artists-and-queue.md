# localtoolbox diagrams: Artists, the queue and settings

Artist rows, the session queue and the settings sheet. The rest are listed in the [README](../../README.md#diagrams).

- [Adding artists and filling them in](#adding-artists-and-filling-them-in)
- [Life of an artist row](#life-of-an-artist-row)
- [What an artist row draws](#what-an-artist-row-draws)
- [Searching and sorting artists](#searching-and-sorting-artists)
- [Filling and reading the session queue](#filling-and-reading-the-session-queue)
- [What the queue sheet does](#what-the-queue-sheet-does)
- [What the settings sheet changes](#what-the-settings-sheet-changes)

## Adding artists and filling them in

Three sources feed the artists list. The SoundCloud tab in front, any page of it, fills in what it has loaded about each artist, as the panel opens and after every add. It answers from memory, so nothing is asked of SoundCloud.

```mermaid
sequenceDiagram
  participant P as artists panel
  participant L as artists store
  participant C as Content script
  opt adding
    P->>L: add artistsOf the source's tracks
  end
  alt a SoundCloud tab is in front, and some artists are incomplete
    P->>C: GET_ARTIST_PROFILES, their paths
    C-->>P: profiles of those the page has loaded
    P->>L: learn, keeping the better answer
    P->>L: flush: profiles to the session area,<br/>the list with profile null to the local one
  else any other tab
    Note over P: N artists without socials yet, open a SoundCloud tab to fill in socials
  end
```

Source: [`src/tools/artists/Panel.svelte`](../../src/tools/artists/Panel.svelte), [`src/tools/artists/AddBar.svelte`](../../src/tools/artists/AddBar.svelte), [`src/tools/artists/artists.svelte.ts`](../../src/tools/artists/artists.svelte.ts), [`src/tools/artists/lookup.ts`](../../src/tools/artists/lookup.ts), [`content.ts`](../../content.ts)

## Life of an artist row

What `profile` holds on an entry. A track names its uploader in short, and the bio, track count and links come with the artist's own profile page. A whole profile is never replaced by a partial one. Profiles live in the session area, so a new browser session starts every row at not loaded yet.

```mermaid
stateDiagram-v2
  state "Not loaded yet" as Unchecked
  state "Partial: name, picture, place, followers" as Partial
  state "Whole: bio and links too" as Whole
  state "No profile, from an older version" as Unavailable
  [*] --> Unchecked: added
  [*] --> Unavailable: restored from an older file
  Unchecked --> Partial: the tab loaded one of their tracks
  Unchecked --> Whole: the tab loaded their profile page
  Partial --> Whole: the tab loaded their profile page
  Partial --> Partial: a newer partial answer
  Whole --> Whole: a newer whole answer
  Unavailable --> Partial: the tab loaded one of their tracks
  Unavailable --> Whole: the tab loaded their profile page
  Partial --> Unchecked: the browser closed
  Whole --> Unchecked: the browser closed
  Unavailable --> Unchecked: the browser closed
  Unchecked --> [*]: taken off
  Partial --> [*]: taken off
  Whole --> [*]: taken off
  Unavailable --> [*]: taken off
```

Source: [`src/tools/artists/artists.svelte.ts`](../../src/tools/artists/artists.svelte.ts), [`src/tools/artists/profiles.ts`](../../src/tools/artists/profiles.ts), [`src/tools/artists/Panel.svelte`](../../src/tools/artists/Panel.svelte), [`content.ts`](../../content.ts)

## What an artist row draws

How one profile becomes a row's badges and an opened row's fields.

```mermaid
flowchart TD
  Profile["profile<br/>bio up to 1,000 characters, up to 20 links"]
  Listed["links the profile lists"]
  Bio["web addresses read out of the bio"]
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

Each section, where it keeps what it sets, and who obeys it. An import writes back to the marks, both lists, the queue and three pref keys.

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
