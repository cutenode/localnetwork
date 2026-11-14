# localtoolbox diagrams: Outside requests

What each host is asked and what else leaves the browser. The rest are listed in the [README](../../README.md#diagrams).

- [What SoundCloud is asked](#what-soundcloud-is-asked)
- [What the gate hosts are asked](#what-the-gate-hosts-are-asked)
- [What else leaves the browser](#what-else-leaves-the-browser)

## What SoundCloud is asked

Every API request goes out from the content script, anonymous and `GET` only, because the API takes calls from `soundcloud.com` alone.

```mermaid
flowchart LR
  Content["Content script"]
  Hydration["window.__sc_hydration<br/>client id and full tracks, no request"]
  Tracks["/tracks?ids=<br/>50 ids a batch, at most 4 at once"]
  Likes["/users/id/likes<br/>up to 200 a page, 10 pages a lookup"]
  Resolve["/resolve?url=<br/>a playlist or user once a document,<br/>a track 4 at a time"]
  Web["/users/id/web-profiles<br/>an artist's links, 4 artists at a time"]

  Content --> Hydration
  Content --> Tracks
  Content --> Likes
  Content --> Resolve
  Content --> Web
```

Source: [`content.ts`](../../content.ts)

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
