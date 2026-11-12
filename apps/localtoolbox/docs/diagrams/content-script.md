# localtoolbox diagrams: the content script

Repaints, pills, replaced buttons, walks and the likes list. The rest are listed in the [README](../../README.md#diagrams).

- [Content script start](#content-script-start)
- [The repaint pass](#the-repaint-pass)
- [Pills and the queue button](#pills-and-the-queue-button)
- [Opening session storage to the page](#opening-session-storage-to-the-page)
- [Noticing clicks on SoundCloud's buttons](#noticing-clicks-on-soundclouds-buttons)
- [Putting the replaced buttons in](#putting-the-replaced-buttons-in)
- [Pressing a replaced button](#pressing-a-replaced-button)
- [The walk behind a Download button](#the-walk-behind-a-download-button)
- [The signed-in frame](#the-signed-in-frame)
- [Pressing the List toggle](#pressing-the-list-toggle)

## Content script start

What runs when the page's scripts load in a frame. `page-data.js` is in before SoundCloud's own scripts, and `content.js` listens to it only once marks, queue and settings are in, since what it learns repaints the page.

```mermaid
flowchart TD
  PageData["page-data.js<br/>at document_start, main world, every frame"]
  Wrap["wraps XMLHttpRequest open,<br/>holds the page's answers"]
  Load["downloaded-store.js, then content.js<br/>at document_idle, isolated world, every frame"]
  Read["read marks, queue<br/>and settings together"]
  Listen["listenToPage: learn the hydration,<br/>listen, say hello"]
  Paint["first repaint"]
  Observe["MutationObserver on body"]
  Storage["chrome.storage.onChanged"]
  Hook["capture click listener,<br/>from the first moment"]
  Top{"top frame?"}
  Answer["answer the panels' messages"]
  Quiet["frame work only"]

  PageData --> Wrap
  Wrap -.->|"held answers, after hello"| Listen
  Load --> Read
  Load --> Hook
  Load --> Top
  Read --> Listen
  Listen --> Paint
  Paint --> Observe
  Paint --> Storage
  Top -->|yes| Answer
  Top -->|no| Quiet
```

Source: [`content.ts`](../../content.ts), [`page-data.ts`](../../page-data.ts), [`src/manifest.json`](../../src/manifest.json)

## The repaint pass

`applyMarkers` is idempotent, so anything that might have changed the page just runs it again.

```mermaid
flowchart TD
  Added["Element nodes added"]
  Learned["The page loaded tracks"]
  Timer["requestMarkers<br/>coalesced into 150 ms"]
  Changed["Storage changed<br/>queue, marks, settings"]
  Pass["applyMarkers"]
  Likes["applyLikesView"]
  Targets["each MARKER_TARGETS layout:<br/>playlist rows, likes rows, tiles,<br/>track hero, player bar"]
  Mark["paintMark<br/>pill added, swapped or removed"]
  Button["paintQueueButton"]
  Host["tbx-mark-host on the artwork"]
  Download["applyDownloadButtons"]

  Added --> Timer
  Learned --> Timer
  Timer --> Pass
  Changed --> Pass
  Pass --> Likes
  Likes --> Targets
  Targets --> Mark
  Targets --> Button
  Mark --> Host
  Button --> Host
  Host --> Download
```

Source: [`content.ts`](../../content.ts), [`content.css`](../../content.css)

## Pills and the queue button

Each piece of artwork carries at most one pill, with `downloaded` winning. Details last as long as the page, so a `nodl` pill only goes when the track is marked or the tab reloads.

```mermaid
stateDiagram-v2
  state "No pill" as None
  state "nodl pill" as Nodl
  state "downloaded pill" as Downloaded
  [*] --> None
  None --> Nodl: known details offer no link
  None --> Downloaded: marked
  Nodl --> Downloaded: marked
  Downloaded --> None: unmarked
  Downloaded --> Nodl: unmarked, details offer no link
```

Source: [`content.ts`](../../content.ts), [`content.css`](../../content.css)

And one queue button that says whether the track is queued. Small art has room for a plus sign alone.

```mermaid
stateDiagram-v2
  state "No button" as Off
  state "Offers + queue, on hover" as Offer
  state "Stays up reading queued, or + on small art" as Queued
  [*] --> Off
  Off --> Offer: queue reachable, row names a track
  Off --> Queued: same, track already queued
  Offer --> Queued: queued, here or anywhere
  Queued --> Offer: taken out, here or anywhere
  Offer --> Off: row names no track
  Queued --> Off: row names no track
```

Source: [`content.ts`](../../content.ts), [`content.css`](../../content.css)

## Opening session storage to the page

A content script can't touch `chrome.storage.session` until a trusted context opens it, so the page offers no queue button until the popup has run once in a browsing session. A tab already open gets its buttons at the next change to the queue, or a reload.

```mermaid
sequenceDiagram
  participant C as Content script
  participant S as chrome.storage.session
  participant P as Popup
  C->>S: read queue at start
  S--xC: refused
  Note over C: queueReachable is false, no buttons
  P->>S: setAccessLevel for content scripts
  opt something changes the queue
    P->>S: write the queue
    S--)C: onChanged for queue
    Note over C: queueReachable is true, buttons appear
  end
  Note over C: a page loaded after this reads it at start
  C->>S: read the queue on a click
  S-->>C: every entry
  C->>S: write the whole queue back
```

Source: [`src/lib/queue.svelte.ts`](../../src/lib/queue.svelte.ts), [`content.ts`](../../content.ts)

## Noticing clicks on SoundCloud's buttons

A capture listener sees each click before the page does and never stops it. This is how SoundCloud's own controls mark a track.

```mermaid
flowchart TD
  Start["Click anywhere in the document"]
  More{"inside .sc-button-more?"}
  Remember["remember that row,<br/>the menu renders detached"]
  Download{"inside .sc-button-download?"}
  MarkDl["mark the row's track,<br/>or the remembered one"]
  Buy{"a purchase link, or the<br/>Track header's outbound link?"}
  MarkBuy["mark the row's track,<br/>or the page's own"]
  Pass["nothing"]

  Start --> More
  More -->|yes| Remember
  More -->|no| Download
  Download -->|yes| MarkDl
  Download -->|no| Buy
  Buy -->|yes| MarkBuy
  Buy -->|no| Pass
```

Source: [`content.ts`](../../content.ts), [`downloaded-store.ts`](../../downloaded-store.ts)

## Putting the replaced buttons in

One pass over SoundCloud's own controls. Nothing is removed, so turning the setting off shows the page as it was drawn.

```mermaid
flowchart TD
  Native["Each SoundCloud control<br/>.sc-button-download, purchase link,<br/>Track header's outbound link"]
  Plan{"setting on, names a track,<br/>a link goes somewhere?"}
  Restore["remove ours,<br/>show SoundCloud's"]
  Ours["ours after it, theirs hidden<br/>with tbx-dl-replaced"]
  Fit["height of the controls beside it,<br/>tooltip says the domain"]

  Native --> Plan
  Plan -->|no| Restore
  Plan -->|yes| Ours
  Ours --> Fit
```

Source: [`content.ts`](../../content.ts), [`content.css`](../../content.css), [`src/lib/settings.svelte.ts`](../../src/lib/settings.svelte.ts)

## Pressing a replaced button

What a click on this extension's **Download** does, by the control it stands in for. Before the popup has run in a browsing session the buttons still draw, and queue nothing.

```mermaid
flowchart TD
  Start["Click on Download"]
  Kind{"stands in for"}
  Press["press SoundCloud's hidden button,<br/>the listener marks the track"]
  Queue["queue the track"]
  Walked{"plain click on Hypeddit,<br/>Droploud or Pump Your Sound?"}
  Follow["queue the track,<br/>the browser follows the link"]
  Save["SAVE_GATE to the worker"]
  Saved{"saved?"}
  Mark["mark the track,<br/>if pref:mark is on"]
  Gone["queue the track,<br/>a tab has opened"]

  Start --> Kind
  Kind -->|"Download file"| Press
  Press --> Queue
  Kind -->|"a purchase link"| Walked
  Walked -->|no| Follow
  Walked -->|yes| Save
  Save --> Saved
  Saved -->|yes| Mark
  Saved -->|no| Gone
```

Source: [`content.ts`](../../content.ts), [`src/background.ts`](../../src/background.ts)

## The walk behind a Download button

The same site modules the Downloader panel uses. The offscreen page has `DOMParser` and host access, and the worker has tabs and downloads.

```mermaid
sequenceDiagram
  participant C as Content script
  participant W as Background worker
  participant O as Offscreen page
  participant G as Gate site
  C->>W: SAVE_GATE, the link
  Note over W: one walk at a time
  W->>O: WALK_GATE
  O->>G: unlock, with email and name
  opt a Spotify step
    O->>W: ROUND_TRIP
    W-->>O: came back or not
  end
  alt a file, and it saves
    O-->>W: URL, folder, filename, mark
    W->>W: chrome.downloads
    W-->>C: saved, mark
  else a page, a throw, or a refused save
    Note over W: tab opens beside the page
    W-->>C: not saved
  end
  Note over W: offscreen page closes when idle
```

Source: [`src/background.ts`](../../src/background.ts), [`src/offscreen.ts`](../../src/offscreen.ts), [`src/tools/third-party-downloads/save.ts`](../../src/tools/third-party-downloads/save.ts), [`content.ts`](../../content.ts)

## The signed-in frame

Signed in, a track page puts the track in an iframe. Both scripts run in both documents, and each document does different work. Only the top one answers a panel, so what the frame's own page loads stays in the frame.

```mermaid
flowchart TD
  Tab["soundcloud.com tab, signed in"]
  Top["Top document<br/>header and player bar"]
  Frame["iframe.webiIframe<br/>/n/user/track?v2_layout=true"]
  Answers["answers the panels,<br/>with no track"]
  Bar["marks the player bar"]
  Header["section labelled Track header"]
  Cart["purchase link replaced"]
  Notice["clicks on it noticed"]
  Silent["no marker target,<br/>no answers to panels"]
  Own["page-data.ts and content.ts<br/>each document learns its own answers"]

  Tab --> Top
  Tab --> Frame
  Top --> Answers
  Top --> Bar
  Top --> Own
  Frame --> Own
  Frame --> Header
  Frame --> Silent
  Header --> Cart
  Header --> Notice
```

Source: [`content.ts`](../../content.ts), [`page-data.ts`](../../page-data.ts), [`src/manifest.json`](../../src/manifest.json)

## Pressing the List toggle

Once per arrival at `/you/likes`, and only with the setting on. Pressing Badges afterwards is left alone.

```mermaid
stateDiagram-v2
  state "Free to press" as Free
  state "Pressed for this visit" as Asked
  [*] --> Free
  Free --> Asked: setting on, grid showing, presses List
  Asked --> Free: toggle gone, the page was left
  Asked --> Free: settings changed
```

Source: [`content.ts`](../../content.ts)
