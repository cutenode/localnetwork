# localtoolbox diagrams: State

Every store, marks, export and import. The rest are listed in the [README](../../README.md#diagrams).

- [State kept for good](#state-kept-for-good)
- [State kept for the session](#state-kept-for-the-session)
- [Options and mirrors in localStorage](#options-and-mirrors-in-localstorage)
- [State held in memory](#state-held-in-memory)
- [Everything hangs off a path](#everything-hangs-off-a-path)
- [What makes a mark](#what-makes-a-mark)
- [Exporting the state file](#exporting-the-state-file)
- [Importing the state file](#importing-the-state-file)
- [Checking a state file by hand](#checking-a-state-file-by-hand)

## State kept for good

The `chrome.storage.local` keys, what each holds, and its limit. All of it goes with the extension. Everything but `settings` can be exported first.

```mermaid
flowchart LR
  Content["Content script"]
  Popup["Popup"]
  Downloaded["downloaded<br/>path to time marked"]
  Details["details<br/>path to answer, 20,000 newest,<br/>10,000 if storage refuses that"]
  Listed["listed<br/>5,000 tracks, then refuses"]
  Artists["artists<br/>2,000 artists, then refuses"]
  Settings["settings<br/>replaceDownloadButtons on,<br/>likesListView off"]

  Content <--> Downloaded
  Content <--> Details
  Settings --> Content
  Popup <--> Downloaded
  Popup <--> Details
  Popup <--> Listed
  Popup <--> Artists
  Popup <--> Settings
```

Source: [`downloaded-store.ts`](../../downloaded-store.ts), [`content.ts`](../../content.ts), [`src/tools/manager/listed.svelte.ts`](../../src/tools/manager/listed.svelte.ts), [`src/tools/artists/artists.svelte.ts`](../../src/tools/artists/artists.svelte.ts), [`src/lib/settings.svelte.ts`](../../src/lib/settings.svelte.ts), [`state-transfer.ts`](../../state-transfer.ts)

## State kept for the session

The `chrome.storage.session` keys. They end when the browser closes, and a run in progress is never resumed.

```mermaid
flowchart LR
  Content["Content script,<br/>once the popup has opened the area"]
  Popup["Popup"]
  Queue["queue<br/>entries, in the order added"]
  View["view<br/>open tool, which sheet is up, and per scope:<br/>scroll, query, selecting, picked, opened,<br/>drawer, paste, import"]
  Downloader["downloader<br/>origin, listed URLs,<br/>known rows and their outcomes"]
  Worker["Background worker"]
  Run["gate-run<br/>the page gate open and the ones after it,<br/>its tab, how many in all"]
  Outcomes["gate-outcomes<br/>per page gate: opened, done or sent"]

  Content <--> Queue
  Popup <--> Queue
  Popup <--> View
  Popup <--> Downloader
  Worker <--> Run
  Worker --> Outcomes
  Outcomes --> Popup
```

Source: [`src/lib/queue.svelte.ts`](../../src/lib/queue.svelte.ts), [`src/lib/view.svelte.ts`](../../src/lib/view.svelte.ts), [`src/tools/third-party-downloads/walked.svelte.ts`](../../src/tools/third-party-downloads/walked.svelte.ts), [`src/tools/third-party-downloads/on-page.ts`](../../src/tools/third-party-downloads/on-page.ts)

## Options and mirrors in localStorage

The extension origin's own `localStorage`. The popup and the offscreen page share it, and a content script can't see it.

```mermaid
flowchart LR
  Popup["Popup"]
  Offscreen["Offscreen page"]
  File["In the state file<br/>pref:background, pref:sort, pref:hide-downloaded,<br/>pref:search-sites"]
  Stay["On this install only<br/>pref:format, pref:source, pref:split-title, pref:dedupe,<br/>pref:artists-sort, pref:hide-queued, pref:theme,<br/>pref:title-case, pref:search:id"]
  Gate["For gates, on this install only<br/>pref:email, pref:name, pref:mark"]
  Notice["Seen once, on this install only<br/>pref:notice-seen"]
  Mirror["Mirrors, corrected by the session<br/>view:open, queue:items"]

  Popup <--> File
  Popup <--> Stay
  Popup <--> Gate
  Popup <--> Mirror
  Popup <--> Notice
  Gate --> Offscreen
```

Source: [`src/lib/prefs.svelte.ts`](../../src/lib/prefs.svelte.ts), [`src/lib/downloader.svelte.ts`](../../src/lib/downloader.svelte.ts), [`src/lib/components/FirstRun.svelte`](../../src/lib/components/FirstRun.svelte), [`src/lib/view.svelte.ts`](../../src/lib/view.svelte.ts), [`src/lib/queue.svelte.ts`](../../src/lib/queue.svelte.ts), [`state-transfer.ts`](../../state-transfer.ts)

## State held in memory

What dies with the document, the popup or the worker.

```mermaid
flowchart LR
  Doc["Each SoundCloud document"]
  Popup["Each opening of the popup"]
  Worker["Background worker"]
  Page["hydration, parsed once<br/>/resolve answers, by playlist or user<br/>likes cursor, per user<br/>measured button heights<br/>the last row whose More was pressed"]
  Jobs["lookup jobs and waiters<br/>mirrors of marks, queue and details"]
  Chunks["loaded Panel and sheet chunks<br/>warmed reads<br/>a Downloader run"]
  Turn["the offscreen errand queue<br/>how many are waiting"]

  Doc --> Page
  Doc --> Jobs
  Popup --> Chunks
  Worker --> Turn
```

Source: [`content.ts`](../../content.ts), [`src/lib/tools.ts`](../../src/lib/tools.ts), [`src/lib/soundcloud.ts`](../../src/lib/soundcloud.ts), [`src/background.ts`](../../src/background.ts)

## Everything hangs off a path

A track's permalink path is its identity in every store, and an artist's is the first segment of it. A details entry is either an answer or a remembered failure, which holds `unavailable` and `at` alone.

```mermaid
erDiagram
  TRACK_PATH ||--o| MARK : "downloaded"
  TRACK_PATH ||--o| DETAILS : "details"
  TRACK_PATH ||--o| LISTED_ENTRY : "listed"
  TRACK_PATH ||--o| QUEUE_ENTRY : "queue"
  ARTIST_ENTRY ||--o{ TRACK_PATH : "up to 50 kept"
  LISTED_ENTRY ||--o| TRACK_DETAILS : "own copy"
  QUEUE_ENTRY ||--o| TRACK_DETAILS : "own copy"
  MARK {
    str permalink PK
    str time
  }
  DETAILS {
    str permalink PK
    num id
    bool downloadable
    bool hasDownloadsLeft
    str purchaseUrl
    str purchaseTitle
    str genre
    str artworkUrl
    bool unavailable
    num at
  }
  LISTED_ENTRY {
    str permalink PK
    str title
    str artist
    str artworkUrl
    str genre
    bool liked
    bool reposted
    str domPurchaseTitle
    str from
    num at
  }
  QUEUE_ENTRY {
    str permalink PK
    str title
    str artist
    str artworkUrl
    str genre
    bool liked
    bool reposted
  }
  ARTIST_ENTRY {
    str permalink PK
    str name
    obj profile
    str from
    num at
  }
  TRACK_DETAILS {
    str permalink
    bool partial
  }
```

Source: [`src/lib/types.d.ts`](../../src/lib/types.d.ts), [`docs/manager-state.schema.json`](../manager-state.schema.json)

## What makes a mark

What puts a mark on a track. Clicking SoundCloud's own purchase link marks it on the spot, while opening a link from the Manager or through a replaced button queues it instead.

```mermaid
flowchart TD
  Native["SoundCloud's Download file<br/>or purchase link clicked"]
  Walked["Page walk saved a file"]
  Saved["Downloader saved a file"]
  OnPage["A gate's own page handed its file over,<br/>gate-page.js marks"]
  Row["A row's check button"]
  Batch["Manager batch, queue sheet"]
  Import["Importing a state file"]
  Setting{"pref:mark on?"}
  Store["TbxDownloaded"]
  Key[("downloaded<br/>chrome.storage.local")]
  Pills["pills on every SoundCloud tab"]
  Rows["panels, Hide downloaded"]

  Walked --> Setting
  Saved --> Setting
  OnPage --> Setting
  Setting -->|yes| Store
  Native --> Store
  Row --> Store
  Batch --> Store
  Import --> Store
  Store --> Key
  Key --> Pills
  Key --> Rows
```

Source: [`downloaded-store.ts`](../../downloaded-store.ts), [`src/lib/downloaded.svelte.ts`](../../src/lib/downloaded.svelte.ts), [`content.ts`](../../content.ts), [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte), [`src/offscreen.ts`](../../src/offscreen.ts), [`gate-page.ts`](../../gate-page.ts)

## Exporting the state file

What the popup holds goes in the file, so a queue kept only in memory is exported too.

```mermaid
sequenceDiagram
  participant T as StateTransfer
  participant O as Popup stores
  participant S as TbxState
  participant B as storage.local
  participant L as localStorage
  T->>O: queue, listed, artists, search sites
  O-->>T: plain lists
  T->>S: snapshot
  S->>B: marks, details
  S->>L: three options
  S-->>T: the document
  Note over T: downloads manager-date.json
```

Source: [`src/lib/components/StateTransfer.svelte`](../../src/lib/components/StateTransfer.svelte), [`state-transfer.ts`](../../state-transfer.ts), [`src/lib/clipboard.ts`](../../src/lib/clipboard.ts)

## Importing the state file

Adding is the default. Replacing is a switch that comes back off every time the popup does.

```mermaid
flowchart TD
  Parse["Chosen file or pasted text,<br/>through TbxState.parse as it lands"]
  Refused["says what is wrong,<br/>writes nothing"]
  Holds["says what the file holds,<br/>Import is pressed"]
  Switch{"replace switch"}
  Add["Adds<br/>marks: merged, the earlier time kept<br/>details: only newer ones<br/>listed, artists: appended, the better answer kept,<br/>past the cap refused and counted<br/>queue: appended, a held track left alone<br/>the three options: only where unset<br/>search sites: added unless already a badge"]
  Replace["Replaces<br/>marks and details: the file's<br/>listed, artists: the file's, past the cap<br/>refused and counted<br/>queue: the file's<br/>the three options: the file's, or back to default<br/>search sites: the file's"]
  Reload["panels re-read their options"]

  Parse -->|refused| Refused
  Parse -->|reads| Holds
  Holds --> Switch
  Switch -->|off| Add
  Switch -->|on| Replace
  Add --> Reload
  Replace --> Reload
```

Source: [`state-transfer.ts`](../../state-transfer.ts), [`src/lib/components/StateTransfer.svelte`](../../src/lib/components/StateTransfer.svelte), [`downloaded-store.ts`](../../downloaded-store.ts), [`src/lib/queue.svelte.ts`](../../src/lib/queue.svelte.ts), [`src/tools/manager/listed.svelte.ts`](../../src/tools/manager/listed.svelte.ts), [`src/tools/artists/artists.svelte.ts`](../../src/tools/artists/artists.svelte.ts), [`src/lib/search-sites.svelte.ts`](../../src/lib/search-sites.svelte.ts), [`docs/manager-state.schema.json`](../manager-state.schema.json)

## Checking a state file by hand

`pnpm validate:state` holds a file to the same schema outside the browser. The popup's parser enforces that schema by hand, since a classic script can't import a validator.

```mermaid
flowchart TD
  Run["pnpm validate:state files"]
  Any{"any files named?"}
  Usage["usage line, exit 2"]
  Parse{"JSON?"}
  Ajv{"valid against<br/>docs/manager-state.schema.json,<br/>its sites against search-sites.schema.json?"}
  Good["a tick for that file"]
  Bad["a cross, and the read error<br/>or each Ajv error"]
  Exit["exit 0 if all passed, else 1"]

  Run --> Any
  Any -->|no| Usage
  Any -->|yes| Parse
  Parse -->|no| Bad
  Parse -->|yes| Ajv
  Ajv -->|yes| Good
  Ajv -->|no| Bad
  Good --> Exit
  Bad --> Exit
```

Source: [`scripts/validate-state.mts`](../../scripts/validate-state.mts), [`docs/manager-state.schema.json`](../manager-state.schema.json), [`src/lib/search-sites.schema.json`](../../src/lib/search-sites.schema.json), [`state-transfer.ts`](../../state-transfer.ts)
