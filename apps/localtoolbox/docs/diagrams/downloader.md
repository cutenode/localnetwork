# localtoolbox diagrams: the downloader

Gate links, row life, walks and gate sites. The rest are listed in the [README](../../README.md#diagrams).

- [Gathering gate links](#gathering-gate-links)
- [Narrowing the Downloader list](#narrowing-the-downloader-list)
- [Life of a Downloader row](#life-of-a-downloader-row)
- [What a walk can answer](#what-a-walk-can-answer)
- [Gate sites as data](#gate-sites-as-data)
- [Gates that end in a file](#gates-that-end-in-a-file)
- [Account gates and smart links](#account-gates-and-smart-links)
- [Laylo drops](#laylo-drops)
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
  GetAll["get all<br/>runs the visible rows not yet settled"]
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

The `RowState` values. Rows walk one at a time, and a run dies with the popup that started it.

```mermaid
stateDiagram-v2
  [*] --> queued
  queued --> working: get, or get all
  failed --> working: get again
  working --> saving: unlock gave a file
  working --> link: unlock gave a page
  working --> sent: Laylo took the address
  working --> failed: unlock threw
  saving --> done: handed to chrome.downloads
  saving --> failed: save threw
  done --> failed: transfer interrupted, panel open
  working --> queued: popup closed mid-walk
  saving --> queued: popup closed mid-walk
  done --> [*]
  link --> [*]
  sent --> [*]
```

Source: [`src/tools/third-party-downloads/walked.svelte.ts`](../../src/tools/third-party-downloads/walked.svelte.ts), [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte)

## What a walk can answer

Every site's `unlock` ends one of five ways. A page and a signup both offer **open**.

```mermaid
flowchart TD
  Unlock["site.unlock<br/>url, email, name, connect"]
  File["kind file"]
  Link["kind link"]
  Signup["kind signup"]
  Thrown["throws Error"]
  Wanting["throws Wanting"]
  Saved["save, in the site's folder when the name is known,<br/>mark the track if it has one and pref:mark is on"]
  Open["open, opens the page"]
  OpenGate["open, queues the track and<br/>opens the gate or the file"]
  Settings["add email or add name,<br/>opens the settings sheet"]

  Unlock --> File
  Unlock --> Link
  Unlock --> Signup
  Unlock --> Thrown
  Unlock --> Wanting
  File --> Saved
  Link --> Open
  Signup --> Open
  Thrown --> OpenGate
  Wanting --> Settings
```

Source: [`src/tools/third-party-downloads/sites/index.ts`](../../src/tools/third-party-downloads/sites/index.ts), [`src/tools/third-party-downloads/next-step.ts`](../../src/tools/third-party-downloads/next-step.ts), [`src/tools/third-party-downloads/save.ts`](../../src/tools/third-party-downloads/save.ts), [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte)

## Gate sites as data

`SITES` is the whole table. `siteFor` matches a link's host, without `www.`, and the diagrams below take these groups in turn.

```mermaid
flowchart TD
  Link["A link"]
  SiteFor["siteFor<br/>hosts and aliases of SITES"]
  Unclaimed["no site: not a row"]
  File["yields file, walked from the page too<br/>Hypeddit, hypeddit.com, folder Hypeddit, wants email<br/>Droploud, droploud.com, folder Droploud<br/>Pump Your Sound, pumpyoursound.com, folder PumpYourSound"]
  Account["yields mixed, ToneDen's backend<br/>ToneDen, toneden.io, folder ToneDen<br/>FanLink, fanlink.tv, alias fanlink.to, folder ToneDen"]
  Signup["yields mixed, signup<br/>Laylo, laylo.com, folder Laylo, wants email"]
  Smart["yields mixed, smart link<br/>Feature.fm, ffm.to, folder FeatureFM"]

  Link --> SiteFor
  SiteFor --> Unclaimed
  SiteFor --> File
  SiteFor --> Account
  SiteFor --> Signup
  SiteFor --> Smart
```

Source: [`src/tools/third-party-downloads/sites/index.ts`](../../src/tools/third-party-downloads/sites/index.ts), [`src/tools/third-party-downloads/sites/hypeddit.ts`](../../src/tools/third-party-downloads/sites/hypeddit.ts), [`src/tools/third-party-downloads/sites/droploud.ts`](../../src/tools/third-party-downloads/sites/droploud.ts), [`src/tools/third-party-downloads/sites/pumpyoursound.ts`](../../src/tools/third-party-downloads/sites/pumpyoursound.ts), [`src/tools/third-party-downloads/sites/toneden.ts`](../../src/tools/third-party-downloads/sites/toneden.ts), [`src/tools/third-party-downloads/sites/laylo.ts`](../../src/tools/third-party-downloads/sites/laylo.ts), [`src/tools/third-party-downloads/sites/featurefm.ts`](../../src/tools/third-party-downloads/sites/featurefm.ts)

## Gates that end in a file

The `yields: 'file'` family. Each fetches its gate page, reads what the page carries, and posts back what pressing Download would.

```mermaid
flowchart TD
  Page["GET the gate page, with cookies"]
  Kind{"site"}
  HRead["Hypeddit<br/>hidden fields, CSRF token, step list"]
  DRead["Droploud<br/>gateData in the Next.js payload"]
  PRead["Pump Your Sound<br/>the free download form"]
  HPost["POST the visit, then email and<br/>Spotify if asked, then /gate/download/ul"]
  DPost["POST each step, give a repost step up,<br/>then /gate/id/download"]
  PPost["POST the form"]
  Look{"looks like a file?"}
  File["kind file"]
  Page2["kind link"]
  Refused["throws, saying why"]

  Page -->|"bad status, or no gate"| Refused
  Page --> Kind
  Kind --> HRead
  Kind --> DRead
  Kind --> PRead
  HRead --> HPost
  DRead --> DPost
  PRead --> PPost
  HPost -->|"signed URL"| File
  HPost -->|refused| Refused
  DPost -->|refused| Refused
  DPost -->|"download_url"| Look
  PPost -->|"file URL, or a file link in the page"| File
  PPost -->|"nothing"| Refused
  Look -->|yes| File
  Look -->|no| Page2
```

Source: [`src/tools/third-party-downloads/sites/hypeddit.ts`](../../src/tools/third-party-downloads/sites/hypeddit.ts), [`src/tools/third-party-downloads/sites/droploud.ts`](../../src/tools/third-party-downloads/sites/droploud.ts), [`src/tools/third-party-downloads/sites/pumpyoursound.ts`](../../src/tools/third-party-downloads/sites/pumpyoursound.ts), [`src/tools/third-party-downloads/parse.ts`](../../src/tools/third-party-downloads/parse.ts), [`src/tools/third-party-downloads/save.ts`](../../src/tools/third-party-downloads/save.ts)

## Account gates and smart links

The `yields: 'mixed'` sites other than Laylo. First, how a smart link picks a destination.

```mermaid
flowchart TD
  FanLink["FanLink page<br/>fanlink.to rewritten to fanlink.tv,<br/>reads window.preloadLink"]
  Attached{"unlock attached?"}
  Target{"target is a ToneDen post?"}
  Feature["Feature.fm page<br/>destUrl decoded from each button,<br/>the tracker is never called"]
  Best["best destination: a file, then the shops,<br/>then FanLink's target, else the page"]
  Post["the post's unlock,<br/>next diagram"]
  Throws["throws, saying why"]
  Result["kind file, or kind link"]

  FanLink -->|"bad status, or no link"| Throws
  FanLink --> Attached
  Attached -->|"yes, post in hand"| Post
  Attached -->|no| Target
  Target -->|"yes, after the lookup"| Post
  Target -->|no| Best
  Feature -->|"bad status, or no services"| Throws
  Feature --> Best
  Best --> Result
```

Source: [`src/tools/third-party-downloads/sites/toneden.ts`](../../src/tools/third-party-downloads/sites/toneden.ts), [`src/tools/third-party-downloads/sites/featurefm.ts`](../../src/tools/third-party-downloads/sites/featurefm.ts), [`src/tools/third-party-downloads/parse.ts`](../../src/tools/third-party-downloads/parse.ts)

How a ToneDen post unlocks. A post attached to a FanLink starts at the redirect check.

```mermaid
flowchart TD
  Link["ToneDen post link"]
  Lookup["GET /api/v1/posts<br/>no account needed"]
  Redirect{"redirect with a URL?"}
  Kind{"a download with a file?"}
  NoFile["throws: no file behind it"]
  Dl["GET the attachment's download<br/>with the browser's session"]
  Why["gateMetadata says which account<br/>it wants, then throws"]
  Result["kind file, or kind link"]

  Link --> Lookup
  Lookup --> Redirect
  Redirect -->|yes| Result
  Redirect -->|no| Kind
  Kind -->|no| NoFile
  Kind -->|yes| Dl
  Dl -->|refused| Why
  Dl -->|"a URL"| Result
```

Source: [`src/tools/third-party-downloads/sites/toneden.ts`](../../src/tools/third-party-downloads/sites/toneden.ts)

## Laylo drops

A drop is a signup, so the walk is reading the drop and leaving the address when it can.

```mermaid
flowchart TD
  Json["GET the drop page, then the drop's and<br/>the artist's JSON from the CDNs it names"]
  HasLink{"drop has a link?"}
  Goes["kind file, or kind link"]
  Can{"takes email, at most a name?"}
  OpenIt["kind link, the drop to open"]
  Have{"settings has them?"}
  Want["throws Wanting, email or name"]
  Rsvp["POST rsvp to events.laylo.com"]
  Sent["kind signup"]
  Refused["throws: open the drop"]

  Json --> HasLink
  HasLink -->|yes| Goes
  HasLink -->|no| Can
  Can -->|no| OpenIt
  Can -->|yes| Have
  Have -->|no| Want
  Have -->|yes| Rsvp
  Rsvp -->|success| Sent
  Rsvp -->|"no"| Refused
```

Source: [`src/tools/third-party-downloads/sites/laylo.ts`](../../src/tools/third-party-downloads/sites/laylo.ts), [`src/tools/third-party-downloads/next-step.ts`](../../src/tools/third-party-downloads/next-step.ts)

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
```

Source: [`src/tools/third-party-downloads/sites/hypeddit.ts`](../../src/tools/third-party-downloads/sites/hypeddit.ts), [`src/lib/chrome.ts`](../../src/lib/chrome.ts), [`src/tools/third-party-downloads/Panel.svelte`](../../src/tools/third-party-downloads/Panel.svelte)
