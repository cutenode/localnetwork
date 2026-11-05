# localtoolbox diagrams: Reading pages

What the panels ask a tab, and playlist to text. The rest are listed in the [README](../../README.md#diagrams).

- [Reading the active tab](#reading-the-active-tab)
- [Telling which page is showing](#telling-which-page-is-showing)
- [Reading tracks by page kind](#reading-tracks-by-page-kind)
- [Turning tracks into text](#turning-tracks-into-text)

## Reading the active tab

What `readPage` hands a panel: the page's answer, or the sentence that says why there is none. A warmed read is this same read, started before the panel exists.

```mermaid
flowchart TD
  IsSc{"SoundCloud tab?"}
  Open["status: open a page"]
  Reached{"delivered?"}
  Reload["status: reload the tab"]
  Shape{"tracks list?"}
  Failed["status: couldn't read the page"]
  Empty{"any tracks?"}
  Why["status by page type"]
  Data["the page's answer"]

  IsSc -->|no| Open
  IsSc -->|yes| Reached
  Reached -->|no| Reload
  Reached -->|yes| Shape
  Shape -->|no| Failed
  Shape -->|yes| Empty
  Empty -->|no| Why
  Empty -->|yes| Data
```

Source: [`src/lib/soundcloud.ts`](../../src/lib/soundcloud.ts), [`src/lib/chrome.ts`](../../src/lib/chrome.ts)

## Telling which page is showing

`detectPage` asks the rendered page in this order: the playlist layout, the track page, an active likes tab, the Library's view toggle. The URL is the last resort. A signed-in track page is the `other` page in the top document, because only its iframe holds the track.

```mermaid
flowchart TD
  Hero{"playlist?"}
  Listen{"track page?"}
  Tab{"likes tab?"}
  Toggle{"view toggle?"}
  Url["the URL: /sets/ is a playlist,<br/>/name/likes is likes, else other"]
  Playlist["playlist<br/>key from the rows' links, else the URL"]
  Track["track<br/>key is the URL, if the uploader agrees"]
  Likes["likes<br/>key is the tab's link"]
  You["likes<br/>key is /you/likes"]

  Hero -->|yes| Playlist
  Hero -->|no| Listen
  Listen -->|yes| Track
  Listen -->|no| Tab
  Tab -->|yes| Likes
  Tab -->|no| Toggle
  Toggle -->|yes| You
  Toggle -->|no| Url
```

Source: [`content.ts`](../../content.ts)

## Reading tracks by page kind

What `EXTRACT_TRACKS` reads from each kind of page. It asks SoundCloud nothing. A signed-in track page comes back with no track, since the top frame has no hero and the iframe does not answer.

```mermaid
flowchart TD
  Extract["extract"]
  Kind{"page kind"}
  Sound["soundList rows<br/>purchase link and genre on the row"]
  Tile["badge tiles<br/>no purchase link, no genre tag"]
  Skip["liked playlists skipped and counted"]
  Named["title from the page's heading,<br/>or your likes"]
  Hero["the classic hero, one track<br/>purchase link from the engagement bar"]
  Rows["trackItem rows<br/>any .trackItem as a fallback on a playlist"]
  Meta["title and curator from the hero"]
  Answer["url, pageType, pageKey, pageDetectedBy,<br/>title, curator, tracks, skippedPlaylists"]

  Extract --> Kind
  Kind -->|likes| Sound
  Kind -->|likes| Tile
  Sound --> Skip
  Tile --> Skip
  Skip --> Named
  Kind -->|track| Hero
  Kind -->|"playlist or other"| Rows
  Hero --> Meta
  Rows --> Meta
  Named --> Answer
  Meta --> Answer
```

Source: [`content.ts`](../../content.ts), [`src/lib/types.d.ts`](../../src/lib/types.d.ts)

## Turning tracks into text

The whole of the playlist to text tool. All of it runs in the popup, and the page is read on every opening, whichever source is chosen.

```mermaid
flowchart TD
  Read["readPage EXTRACT_TRACKS"]
  Queue["queue.items"]
  Source{"pref:source picks one"}
  Process["processTracks<br/>pref:split-title, pref:dedupe"]
  Render["format's render<br/>pref:format: artist - title, title - artist,<br/>title only, numbered list, CSV, TSV, JSON"]
  Copy["Copy<br/>clipboard"]
  Save["Download<br/>file named after the page, or session-queue"]

  Read --> Source
  Queue --> Source
  Source --> Process
  Process --> Render
  Render --> Copy
  Render --> Save
```

Source: [`src/tools/soundcloud-playlist-to-text/Panel.svelte`](../../src/tools/soundcloud-playlist-to-text/Panel.svelte), [`src/tools/soundcloud-playlist-to-text/formats.ts`](../../src/tools/soundcloud-playlist-to-text/formats.ts), [`src/lib/clipboard.ts`](../../src/lib/clipboard.ts)
