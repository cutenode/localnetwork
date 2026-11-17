# localtoolbox diagrams: What the page has loaded

How the content script learns tracks and users from SoundCloud's own page, and what it answers with. The rest are listed in the [README](../../README.md#diagrams).

- [From the page's answer to the markers](#from-the-pages-answer-to-the-markers)
- [Saying hello](#saying-hello)
- [Answering for download details](#answering-for-download-details)
- [Life of a details entry](#life-of-a-details-entry)
- [What an older version left](#what-an-older-version-left)
- [Answering for artist profiles](#answering-for-artist-profiles)

## From the page's answer to the markers

SoundCloud's web app loads tracks and users from its API as the page is browsed. `page-data.ts` listens to those requests in the page's main world and passes each answer on as it was received. It makes no request and changes none.

```mermaid
flowchart TD
  Open["the page calls XMLHttpRequest open"]
  Wanted{"GET to api-v2.soundcloud.com,<br/>a WANTED path?"}
  Pass["let by, untouched"]
  Load["listen for load"]
  Ok{"status 200, read as text?"}
  Held{"content.ts said hello?"}
  Hold["held, in order"]
  Event["tbx:page-data on document<br/>path, newline, body"]
  Parse{"body is JSON?"}
  Drop["let by"]
  Links{"/users/id/web-profiles?"}
  LinksSeen["linksSeen, by user id"]
  Learn["learn: walk the answer"]
  Track["a track with a permalink<br/>into detailsCache"]
  User["a user into usersSeen<br/>a short one never replaces a whole one"]
  Repaint["requestMarkers"]

  Open --> Wanted
  Wanted -->|no| Pass
  Wanted -->|yes| Load
  Load --> Ok
  Ok -->|no| Pass
  Ok -->|yes| Held
  Held -->|no| Hold
  Held -->|yes| Event
  Event --> Parse
  Parse -->|no| Drop
  Parse -->|yes| Links
  Links -->|yes| LinksSeen
  Links -->|no| Learn
  Learn --> Track
  Learn --> User
  Learn --> Repaint
```

Source: [`page-data.ts`](../../page-data.ts), [`content.ts`](../../content.ts)

The WANTED paths are `/tracks`, `/resolve`, `/playlists/<id>`, `/users/<x>`, and a user's `/likes`, `/track_likes` and `/web-profiles`. `learn` looks inside arrays and under `collection`, `tracks`, `track`, `playlist` and `user`, so a like, a listing, a playlist and every track's uploader are all reached. A playlist's bare `{ id }` stubs carry no permalink and are passed over.

## Saying hello

`page-data.ts` starts before SoundCloud's own scripts and `content.ts` after them, so the answers in between are held. Once the content script has read its storage, it learns the hydration, starts listening and says hello.

```mermaid
sequenceDiagram
  participant S as SoundCloud's scripts
  participant D as page-data.ts, main world
  participant C as content.ts, isolated world
  Note over D: document_start: wraps open
  S->>D: API answers as the page loads
  Note over D: held
  Note over C: document_idle: reads marks, queue, settings
  C->>C: learn each hydration entry
  C->>C: listen for tbx:page-data
  C->>D: tbx:page-data-hello
  D-)C: every held answer, in order
  Note over D: holds nothing from now on
  S->>D: a later answer
  D-)C: passed on as it lands
```

Source: [`page-data.ts`](../../page-data.ts), [`content.ts`](../../content.ts), [`src/manifest.json`](../../src/manifest.json)

## Answering for download details

`GET_DOWNLOAD_DETAILS` is answered at once from what the tab holds. Only the top frame answers.

```mermaid
flowchart TD
  Ask["GET_DOWNLOAD_DETAILS"]
  Extract["extract: the page's tracks"]
  Each["each track"]
  Known{"in detailsCache?"}
  Full["its details"]
  Bought{"the row drew a purchase link?<br/>Likes rows, a track page"}
  Partial["partial details:<br/>the link, downloadable null"]
  Null["null: not loaded yet"]
  Cover["no cover on the row:<br/>take the details' artwork"]
  Send["sendResponse, no wait"]

  Ask --> Extract
  Extract --> Each
  Each --> Known
  Known -->|yes| Full
  Known -->|no| Bought
  Bought -->|yes| Partial
  Bought -->|no| Null
  Full --> Cover
  Partial --> Cover
  Null --> Send
  Cover --> Send
```

Source: [`content.ts`](../../content.ts)

## Life of a details entry

One entry in `detailsCache`, which is the tab's memory and nothing else. In-app navigation keeps the document, so it keeps the entries; a reload or a closed tab ends them.

```mermaid
stateDiagram-v2
  state "Not loaded" as Unknown
  state "Known" as Known
  [*] --> Unknown
  Unknown --> Known: the hydration or a page answer carries it
  Known --> Known: a later answer carries it, replaced
  Known --> [*]: the document is gone
  Unknown --> [*]: the document is gone
```

Source: [`content.ts`](../../content.ts)

## What an older version left

Older versions kept details for a month under `details` in `chrome.storage.local`, kept each listed artist's profile on the artists list for good, and exported both. All of it is let go.

```mermaid
flowchart TD
  Install["this version installed or updated"]
  Remove["background worker, onInstalled:<br/>remove the local details key"]
  Drop["dropOldProfiles:<br/>profile: null on every stored artist"]
  File["a state file with a details map"]
  Check["parse checks it against the schema"]
  Ignore["apply leaves it out"]

  Install --> Remove
  Install --> Drop
  File --> Check
  Check --> Ignore
```

Source: [`src/background.ts`](../../src/background.ts), [`src/tools/artists/old-profiles.ts`](../../src/tools/artists/old-profiles.ts), [`state-transfer.ts`](../../state-transfer.ts)

## Answering for artist profiles

`GET_ARTIST_PROFILES` answers at once with the accounts the page has loaded, of the paths asked about. Every track names its uploader in short; a profile page loads the whole user and its links.

```mermaid
flowchart TD
  Ask["GET_ARTIST_PROFILES, paths"]
  Each["each distinct artist path"]
  Seen{"in usersSeen?"}
  Out["left out"]
  Whole{"a whole user, with a description,<br/>and its links in linksSeen?"}
  Profile["profile: name, picture, place,<br/>followers, track count, bio, links"]
  Partial["the same, marked partial:<br/>as far as the page loaded it"]
  Send["ok, profiles"]

  Ask --> Each
  Each --> Seen
  Seen -->|no| Out
  Seen -->|yes| Whole
  Whole -->|yes| Profile
  Whole -->|no| Partial
  Profile --> Send
  Partial --> Send
  Out --> Send
```

Source: [`content.ts`](../../content.ts)
