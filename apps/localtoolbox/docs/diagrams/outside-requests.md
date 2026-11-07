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

One link at a time, from a panel or the offscreen page. Each host below is in `host_permissions`.

```mermaid
flowchart LR
  Walk["A walk"]
  Hypeddit["hypeddit.com<br/>GET gate page, POST /gate/ge,<br/>/verifyEmailAddress, /gate/download/ul"]
  Droploud["droploud.com, GET gate page<br/>api.droploud.com, POST /api/gate/id/step,<br/>/api/gate/id/download,<br/>/api/soundcloud/verify-repost"]
  Pump["pumpyoursound.com<br/>GET fangate page, POST its form"]
  Toneden["www.toneden.io<br/>GET posts, attachment download,<br/>gateMetadata"]
  Fanlink["fanlink.tv, GET link page<br/>ffm.to, GET link page"]
  Laylo["laylo.com, GET drop page<br/>two cloudfront.net CDNs, GET drop and artist JSON<br/>events.laylo.com, POST rsvp"]

  Walk --> Hypeddit
  Walk --> Droploud
  Walk --> Pump
  Walk --> Toneden
  Walk --> Fanlink
  Walk --> Laylo
```

Source: [`src/manifest.json`](../../src/manifest.json), [`src/tools/third-party-downloads/sites/hypeddit.ts`](../../src/tools/third-party-downloads/sites/hypeddit.ts), [`src/tools/third-party-downloads/sites/droploud.ts`](../../src/tools/third-party-downloads/sites/droploud.ts), [`src/tools/third-party-downloads/sites/pumpyoursound.ts`](../../src/tools/third-party-downloads/sites/pumpyoursound.ts), [`src/tools/third-party-downloads/sites/toneden.ts`](../../src/tools/third-party-downloads/sites/toneden.ts), [`src/tools/third-party-downloads/sites/laylo.ts`](../../src/tools/third-party-downloads/sites/laylo.ts), [`src/tools/third-party-downloads/sites/featurefm.ts`](../../src/tools/third-party-downloads/sites/featurefm.ts)

## What else leaves the browser

The other ways out, and two addresses that are read but never requested.

```mermaid
flowchart LR
  Ext["The extension"]
  File["the file a walk found, from wherever<br/>its link points, through chrome.downloads"]
  Art["SoundCloud's artwork URL<br/>one fetch per cover click"]
  Images["cover and avatar images<br/>drawn in the popup"]
  Tabs["Tabs a person opens: links, track<br/>pages, search sites, artists' links"]
  Bug["github.com issue form,<br/>opened as a link"]
  Spotify["accounts.spotify.com,<br/>only through a tab"]
  GateSc["gate.sc<br/>unwrapped, never requested"]
  Tracker["api.ffm.to<br/>decoded, never requested"]

  Ext --> File
  Ext --> Art
  Ext --> Images
  Ext --> Tabs
  Ext --> Bug
  Ext --> Spotify
  Ext -.-> GateSc
  Ext -.-> Tracker
```

Source: [`src/tools/third-party-downloads/save.ts`](../../src/tools/third-party-downloads/save.ts), [`src/lib/clipboard.ts`](../../src/lib/clipboard.ts), [`src/lib/components/Artwork.svelte`](../../src/lib/components/Artwork.svelte), [`src/lib/chrome.ts`](../../src/lib/chrome.ts), [`src/lib/bug-report.ts`](../../src/lib/bug-report.ts), [`src/tools/third-party-downloads/sites/index.ts`](../../src/tools/third-party-downloads/sites/index.ts), [`src/tools/third-party-downloads/sites/featurefm.ts`](../../src/tools/third-party-downloads/sites/featurefm.ts)
