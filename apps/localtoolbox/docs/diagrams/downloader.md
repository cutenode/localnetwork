# localtoolbox diagrams: the downloader

Gate links, row life, walks, gates done on their own page, and gate sites. The rest are listed in the [README](../../README.md#diagrams).

- [Gathering gate links](#gathering-gate-links)
- [Narrowing the Downloader list](#narrowing-the-downloader-list)
- [Life of a Downloader row](#life-of-a-downloader-row)
- [What a walk can answer](#what-a-walk-can-answer)
- [Gate sites as data](#gate-sites-as-data)
- [A gate done on its own page](#a-gate-done-on-its-own-page)
- [The Spotify step on a Hypeddit gate](#the-spotify-step-on-a-hypeddit-gate)

## Gathering gate links

Where the third-party downloader's rows come from. Reopening the popup keeps the list, and only the source picker empties it. A send from the queue sheet adds the queue's links to what is listed.

```mermaid
flowchart TD
  Open["Panel mounts, on opening or a send<br/>from the queue sheet: the kept list stays"]
  Switch["Source picker switched<br/>the list is emptied, outcomes kept"]
  Collect["collect"]
  Source{"source"}
  Queued["queue.items<br/>each track's purchase link"]
  Tab{"what the tab is"}
  Gate["that one link"]
  Sc["SoundCloud page<br/>GET_DOWNLOAD_DETAILS, one read:<br/>purchase links the page loaded or drew"]
  Hand["links pasted earlier"]
  Claim["addRows<br/>only links a site claims,<br/>a known link comes back as it was"]
  Paste["Pasted text<br/>extractGateUrls"]

  Open --> Collect
  Switch --> Collect
  Collect --> Source
  Source -->|"session queue"| Queued
  Source -->|"this page"| Tab
  Tab -->|"a gate site"| Gate
  Tab -->|soundcloud.com| Sc
  Queued --> Claim
  Gate --> Claim
  Sc --> Claim
  Collect -->|"after the source"| Hand
  Hand --> Claim
  Paste --> Claim
```

Source: [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte), [`src/tools/third-party-downloads/walked.svelte.ts`](../../src/tools/third-party-downloads/walked.svelte.ts), [`src/tools/third-party-downloads/sites/index.ts`](../../src/tools/third-party-downloads/sites/index.ts), [`src/lib/components/QueueSheet.svelte`](../../src/lib/components/QueueSheet.svelte)

## Narrowing the Downloader list

The two toggles choose what **get all** runs, and the check button marks a row by hand. A row with no track behind it is never hidden.

```mermaid
flowchart TD
  Rows["The rows gathered"]
  Done{"hide downloaded on,<br/>track marked?"}
  Queued{"hide queued on,<br/>track queued?"}
  Hidden["row hidden"]
  Visible["visible rows"]
  GetAll["get all<br/>walks the visible Hypeddit rows not yet settled,<br/>then opens the visible page gates one by one"]
  Check["check button, on a row with a track"]
  Marks[("marks")]

  Rows --> Done
  Done -->|yes| Hidden
  Done -->|no| Queued
  Queued -->|yes| Hidden
  Queued -->|no| Visible
  Visible --> GetAll
  Visible --> Check
  Check -->|toggle| Marks
```

Source: [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte), [`src/lib/filter.ts`](../../src/lib/filter.ts), [`src/lib/downloaded.svelte.ts`](../../src/lib/downloaded.svelte.ts)

## Life of a Downloader row

The `RowState` values. Walked rows run one at a time, and a run dies with the popup that started it. A page gate's row moves on through what the background worker reports, read when the popup next opens. A link site's row is an Open button in any state.

```mermaid
stateDiagram-v2
  [*] --> queued
  queued --> working: get, or get all, on a walked site
  failed --> working: get again
  working --> saving: unlock gave a file
  working --> failed: unlock threw
  saving --> done: handed to chrome.downloads
  saving --> failed: save threw
  done --> failed: transfer interrupted, panel open
  working --> queued: popup closed mid-walk
  saving --> queued: popup closed mid-walk
  queued --> opened: get on a page site, the worker opened it
  opened --> done: the page's own download was filed
  opened --> sent: the sign-up form was submitted
  opened --> opened: get again
  done --> [*]
  sent --> [*]
```

Source: [`src/tools/third-party-downloads/walked.svelte.ts`](../../src/tools/third-party-downloads/walked.svelte.ts), [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte), [`src/tools/third-party-downloads/on-page.ts`](../../src/tools/third-party-downloads/on-page.ts)

## What a walk can answer

A walked site's walk ends one of three ways.

```mermaid
flowchart TD
  Unlock["walk, from walks/index.ts<br/>url, email, name, connect"]
  File["the file, with a note<br/>after a Spotify follow"]
  Thrown["throws Error"]
  Wanting["throws Wanting"]
  Saved["save, in the site's folder when the name is known,<br/>mark the track if it has one and pref:mark is on"]
  OpenGate["open, queues the track and<br/>opens the gate or the file"]
  Settings["add email,<br/>opens the settings sheet"]

  Unlock --> File
  Unlock --> Thrown
  Unlock --> Wanting
  File --> Saved
  Thrown --> OpenGate
  Wanting --> Settings
```

Source: [`src/tools/third-party-downloads/sites/index.ts`](../../src/tools/third-party-downloads/sites/index.ts), [`src/tools/third-party-downloads/walks/index.ts`](../../src/tools/third-party-downloads/walks/index.ts), [`src/tools/third-party-downloads/walks/hypeddit.ts`](../../src/tools/third-party-downloads/walks/hypeddit.ts), [`src/tools/third-party-downloads/next-step.ts`](../../src/tools/third-party-downloads/next-step.ts), [`src/tools/third-party-downloads/save.ts`](../../src/tools/third-party-downloads/save.ts), [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte)

## Gate sites as data

`SITES` is the whole table. `siteFor` matches a link's host, without `www.`, and a site's `how` says what **get** does with it.

```mermaid
flowchart TD
  Link["A link"]
  SiteFor["siteFor<br/>hosts and aliases of SITES"]
  Unclaimed["no site: not a row"]
  Walk["how walk: fetched and saved here<br/>Hypeddit, hypeddit.com, folder Hypeddit<br/>its host is the one host permission"]
  Page["how page: done on its own page, with help<br/>Droploud, droploud.com, folder Droploud<br/>Pump Your Sound, pumpyoursound.com, folder PumpYourSound<br/>Laylo, laylo.com, a sign-up<br/>their hosts are gate-page.js's matches"]
  Only["how link: only opened, with a note<br/>ToneDen, toneden.io<br/>FanLink, fanlink.tv, alias fanlink.to<br/>Feature.fm, ffm.to"]

  Link --> SiteFor
  SiteFor --> Unclaimed
  SiteFor --> Walk
  SiteFor --> Page
  SiteFor --> Only
```

Source: [`src/tools/third-party-downloads/sites/index.ts`](../../src/tools/third-party-downloads/sites/index.ts), [`src/tools/third-party-downloads/sites/hypeddit.ts`](../../src/tools/third-party-downloads/sites/hypeddit.ts), [`src/tools/third-party-downloads/sites/droploud.ts`](../../src/tools/third-party-downloads/sites/droploud.ts), [`src/tools/third-party-downloads/sites/pumpyoursound.ts`](../../src/tools/third-party-downloads/sites/pumpyoursound.ts), [`src/tools/third-party-downloads/sites/laylo.ts`](../../src/tools/third-party-downloads/sites/laylo.ts), [`src/tools/third-party-downloads/sites/toneden.ts`](../../src/tools/third-party-downloads/sites/toneden.ts), [`src/tools/third-party-downloads/sites/featurefm.ts`](../../src/tools/third-party-downloads/sites/featurefm.ts), [`src/manifest.json`](../../src/manifest.json)

## A gate done on its own page

Get on a Droploud, Pump Your Sound or Laylo row, Get all with some listed, or the page's Download button on a link to one. The person does the steps; the extension opens the page, helps on it and files the download.

```mermaid
sequenceDiagram
  participant P as Panel, or content.ts
  participant W as Background worker
  participant O as Offscreen page
  participant T as Gate tab
  participant G as gate-page.js
  P->>W: OPEN_GATES, or SAVE_GATE for a page site
  W->>T: open the first gate
  W->>W: the run, and opened, in session storage
  G->>W: GATE_PAGE_HELLO
  W->>O: READ_PREFS
  O-->>W: the address and the mark setting
  W-->>G: what to say and outline, the address if the page has an email field
  G->>T: bar, outline, email filled while empty
  Note over T: the person does the steps and presses the page's own button
  alt the page starts its download
    T->>W: onDeterminingFilename, from the gate's site
    W-->>T: the site's folder, the site's name for the file
    W->>W: done, in session storage
    W->>G: GATE_PAGE_SAVED
    G->>G: marks the track, if settings say so
    W->>T: the next gate, after a moment
  else the sign-up form is submitted
    G->>W: GATE_PAGE_SIGNED_UP
    W->>W: sent, in session storage
    G->>W: GATE_PAGE_NEXT, on next
    W->>T: the next gate
  end
```

Source: [`src/tools/third-party-downloads/on-page.ts`](../../src/tools/third-party-downloads/on-page.ts), [`src/background.ts`](../../src/background.ts), [`src/offscreen.ts`](../../src/offscreen.ts), [`gate-page.ts`](../../gate-page.ts), [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte), [`content.ts`](../../content.ts)

Which downloads are the gate's. Anything not claimed keeps the browser's own name.

```mermaid
flowchart TD
  Item["A download starts"]
  Ours{"started by the extension?"}
  Run{"a page gate open?"}
  From{"referrer, URL or final URL<br/>on the gate's site?"}
  Leave["no suggestion: the browser's name"]
  File["the site's folder,<br/>the browser's name for it made safe"]

  Item --> Ours
  Ours -->|yes| Leave
  Ours -->|no| Run
  Run -->|no| Leave
  Run -->|yes| From
  From -->|no| Leave
  From -->|yes| File
```

Source: [`src/tools/third-party-downloads/on-page.ts`](../../src/tools/third-party-downloads/on-page.ts), [`src/tools/third-party-downloads/save.ts`](../../src/tools/third-party-downloads/save.ts), [`src/background.ts`](../../src/background.ts)

## The Spotify step on a Hypeddit gate

The one step that needs a tab. A no throws before the download post goes out, and in a panel run the first no stops the trip for the rest of that run.

```mermaid
sequenceDiagram
  participant W as The walk
  participant H as hypeddit.com
  participant T as Background tab
  participant S as accounts.spotify.com
  W->>H: GET the gate page
  H-->>W: hidden fields, CSRF token, steps
  W->>H: POST /gate/ge, the visit
  opt the gate has an email step
    W->>H: POST /verifyEmailAddress
  end
  opt the gate has a Spotify step
    W->>T: open the step's authorize URL
    T->>S: OAuth, as a person would
    S-->>T: back to hypeddit.com, if allowed before
    T-->>W: arrived, or no after 12 s
  end
  W->>H: POST /gate/download/ul
  H-->>W: download_status and a signed URL
  opt it went through Spotify
    W->>W: the row says whose Spotify was followed
  end
```

Source: [`src/tools/third-party-downloads/walks/hypeddit.ts`](../../src/tools/third-party-downloads/walks/hypeddit.ts), [`src/lib/chrome.ts`](../../src/lib/chrome.ts), [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte)
