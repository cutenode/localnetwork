# localtoolbox diagrams: Outside requests

What is read from SoundCloud's own page, what each gate host is asked, and what else leaves the browser. The rest are listed in the [README](../../README.md#diagrams).

- [What is read from SoundCloud's page](#what-is-read-from-soundclouds-page)
- [What the gate hosts are asked](#what-the-gate-hosts-are-asked)
- [What else leaves the browser](#what-else-leaves-the-browser)

## What is read from SoundCloud's page

The extension sends SoundCloud's API nothing. The content script reads the JSON the page was served with, and `page-data.ts` passes on the answers to requests SoundCloud's own web app makes as it is browsed. Dotted lines are requests the page makes, listened to and never repeated.

```mermaid
flowchart LR
  Content["Content script"]
  PageData["page-data.ts<br/>in the page's main world"]
  Hydration["window.__sc_hydration<br/>a script element, no request"]
  Page["SoundCloud's web app"]
  Api["api-v2.soundcloud.com, GET<br/>/tracks, /resolve, /playlists/id,<br/>/users/x, its likes and track_likes,<br/>its web-profiles"]

  Content --> Hydration
  Page -.-> Api
  Api -.->|"the answer, as the page got it"| PageData
  PageData -->|"tbx:page-data event"| Content
```

Source: [`content.ts`](../../content.ts), [`page-data.ts`](../../page-data.ts)

## What the gate hosts are asked

Only Hypeddit is fetched, one link at a time, from a panel or the offscreen page; its host is the one in `host_permissions`. The other gate sites are opened as tabs a person works in.

```mermaid
flowchart LR
  Walk["A walk"]
  Hypeddit["hypeddit.com<br/>GET gate page, POST /gate/ge,<br/>/verifyEmailAddress, /gate/download/ul"]
  Opened["A tab the person works in"]
  Pages["droploud.com, pumpyoursound.com, laylo.com<br/>the page and what the person presses on it,<br/>with gate-page.js helping"]
  Only["toneden.io, fanlink.tv, ffm.to<br/>opened, nothing more"]

  Walk --> Hypeddit
  Opened --> Pages
  Opened --> Only
```

Source: [`src/manifest.json`](../../src/manifest.json), [`src/tools/third-party-downloads/walks/hypeddit.ts`](../../src/tools/third-party-downloads/walks/hypeddit.ts), [`src/tools/third-party-downloads/sites/index.ts`](../../src/tools/third-party-downloads/sites/index.ts), [`src/tools/third-party-downloads/on-page.ts`](../../src/tools/third-party-downloads/on-page.ts), [`gate-page.ts`](../../gate-page.ts)

## What else leaves the browser

The other ways out, and an address that is read but never requested.

```mermaid
flowchart LR
  Ext["The extension"]
  File["the file a walk found, from wherever<br/>its link points, through chrome.downloads"]
  Email["the address in settings, typed into<br/>a Droploud or Laylo page's field"]
  Art["SoundCloud's artwork URL<br/>one fetch per cover click"]
  Images["cover and avatar images<br/>drawn in the popup"]
  Tabs["Tabs a person opens: links, track<br/>pages, search sites, artists' links"]
  Bug["github.com issue form,<br/>opened as a link"]
  Spotify["accounts.spotify.com,<br/>only through a tab"]
  GateSc["gate.sc<br/>unwrapped, never requested"]

  Ext --> File
  Ext --> Email
  Ext --> Art
  Ext --> Images
  Ext --> Tabs
  Ext --> Bug
  Ext --> Spotify
  Ext -.-> GateSc
```

Source: [`src/tools/third-party-downloads/save.ts`](../../src/tools/third-party-downloads/save.ts), [`src/lib/clipboard.ts`](../../src/lib/clipboard.ts), [`src/lib/components/Artwork.svelte`](../../src/lib/components/Artwork.svelte), [`src/lib/chrome.ts`](../../src/lib/chrome.ts), [`src/lib/bug-report.ts`](../../src/lib/bug-report.ts), [`src/tools/third-party-downloads/sites/index.ts`](../../src/tools/third-party-downloads/sites/index.ts), [`gate-page.ts`](../../gate-page.ts)
