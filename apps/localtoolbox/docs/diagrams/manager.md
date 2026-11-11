# localtoolbox diagrams: the Manager

Adding, filing and acting on rows, filters and search badges. The rest are listed in the [README](../../README.md#diagrams).

- [Adding to the Manager list](#adding-to-the-manager-list)
- [Filing a Manager row](#filing-a-manager-row)
- [What a Manager row can do](#what-a-manager-row-can-do)
- [Filtering and painting the Manager list](#filtering-and-painting-the-manager-list)
- [Search badges](#search-badges)

## Adding to the Manager list

Opening the panel only reads the page to label the button. Pressing **add** puts rows up first and asks SoundCloud second.

```mermaid
sequenceDiagram
  participant P as Manager panel
  participant L as listed store
  participant C as Content script
  participant A as SoundCloud API
  P->>C: EXTRACT_TRACKS, warmed at boot
  C-->>P: the page's tracks
  alt add from the tab
    P->>L: add the page's tracks
    P->>C: GET_DOWNLOAD_DETAILS
    loop until the rendered rows settle
      C->>A: what the cache can't answer
      C--)P: DOWNLOAD_DETAILS_PROGRESS
    end
    alt the lookup answered
      C-->>P: tracks with details
      P->>L: add again, better answer wins
    else no tab, no client id, or a 401
      Note over P: rows stay, a note says so
    end
  else add from the session queue
    P->>L: add the queue's tracks, no lookup
  end
  Note over L: each add is one write
```

Source: [`src/tools/manager/Panel.svelte`](../../src/tools/manager/Panel.svelte), [`src/tools/manager/AddBar.svelte`](../../src/tools/manager/AddBar.svelte), [`src/tools/manager/listed.svelte.ts`](../../src/tools/manager/listed.svelte.ts), [`src/lib/tools.ts`](../../src/lib/tools.ts)

## Filing a Manager row

How a listed track turns into links, and which section it lands in.

```mermaid
flowchart TD
  Track["A listed track"]
  Dl{"details.downloadable<br/>is true?"}
  Sc["SoundCloud entry<br/>opens the track page"]
  Buy{"details.purchaseUrl?"}
  Ext["external entry<br/>gate.sc unwrapped"]
  Sort{"pref:sort"}
  Site["by site<br/>grouped by domain,<br/>soundcloud.com first"]
  None["no download links<br/>search badges"]
  Unchecked["not checked<br/>search badges, a spinner<br/>while a lookup runs"]
  Order["list order<br/>every track once, best link leads"]

  Track --> Dl
  Dl -->|yes| Sc
  Track --> Buy
  Buy -->|yes| Ext
  Sc --> Sort
  Ext --> Sort
  Track -->|"no entries"| Sort
  Sort -->|site| Site
  Sort -->|"site, known to have none"| None
  Sort -->|"site, not settled"| Unchecked
  Sort -->|page| Order
```

Source: [`src/tools/manager/links.ts`](../../src/tools/manager/links.ts), [`src/tools/manager/Panel.svelte`](../../src/tools/manager/Panel.svelte)

## What a Manager row can do

Every action on a row and on a ticked batch, the header's **unmark**, and what each one writes.

```mermaid
flowchart LR
  Link["download link"]
  List["list button"]
  Check["check button"]
  Art["cover art"]
  Chevron["chevron"]
  Badge["search badge"]
  Select["Select, then the bar"]
  Header["unmark in the header,<br/>asks once more"]
  Queue[("session queue")]
  Marks[("marks")]
  Listed[("the Manager's list")]
  Tab["a new tab"]
  Clip["clipboard"]

  Link -->|"queues the track"| Queue
  Link -->|"then opens"| Tab
  List -->|toggle| Queue
  Check -->|toggle| Marks
  Art -->|"click, 1080 cover"| Clip
  Art -->|"command or ctrl click"| Tab
  Chevron -->|"facts, Title Case if on"| Clip
  Chevron -->|"link arrow, queues nothing"| Tab
  Chevron -->|"off the list"| Listed
  Badge --> Tab
  Select -->|"queue, unqueue"| Queue
  Select -->|"mark, unmark"| Marks
  Select -->|remove| Listed
  Header -->|"every mark off"| Marks
```

Source: [`src/tools/manager/Panel.svelte`](../../src/tools/manager/Panel.svelte), [`src/lib/selection.svelte.ts`](../../src/lib/selection.svelte.ts), [`src/lib/artwork.ts`](../../src/lib/artwork.ts), [`src/lib/title-case.svelte.ts`](../../src/lib/title-case.svelte.ts)

## Filtering and painting the Manager list

Which rows reach the screen. The search is kept for the session and **hide downloaded** for good, under a key the Downloader shares.

```mermaid
flowchart TD
  Sections["The rows of every section"]
  Hide{"hide downloaded on,<br/>track marked?"}
  Gone["row hidden"]
  Search{"every typed word in title, artist,<br/>source or link hosts?"}
  Visible["visible rows, a section left<br/>empty goes with them"]
  Paint["createPainting<br/>60 rows first, then 40 a frame"]
  Dom["rows in the DOM"]

  Sections --> Hide
  Hide -->|yes| Gone
  Hide -->|no| Search
  Search -->|no| Gone
  Search -->|yes| Visible
  Visible --> Paint
  Paint --> Dom
```

Source: [`src/lib/list-filter.svelte.ts`](../../src/lib/list-filter.svelte.ts), [`src/lib/filter.ts`](../../src/lib/filter.ts), [`src/lib/painting.svelte.ts`](../../src/lib/painting.svelte.ts), [`src/tools/manager/links.ts`](../../src/tools/manager/links.ts), [`src/tools/manager/Panel.svelte`](../../src/tools/manager/Panel.svelte)

## Search badges

Sites are data, and adding one to the JSON file, or in settings, adds its badge and its switch.

```mermaid
flowchart TD
  Json["src/lib/search-sites.json"]
  Own["added in settings<br/>pref:search-sites"]
  Read["readSites<br/>drops entries it can't use, and repeats"]
  Switch["one switch per site<br/>pref:search:id, set in settings"]
  Manager["Manager row with no links,<br/>unless a lookup covers it"]
  Artists["opened artist row"]
  Url["searchUrl<br/>fills artist, title, query"]
  Tab["opens a tab, never a fetch"]

  Json --> Read
  Own --> Read
  Read --> Switch
  Switch --> Manager
  Switch --> Artists
  Manager --> Url
  Artists --> Url
  Url --> Tab
```

Source: [`src/lib/search-sites.json`](../../src/lib/search-sites.json), [`src/lib/search-sites.ts`](../../src/lib/search-sites.ts), [`src/lib/search-sites.svelte.ts`](../../src/lib/search-sites.svelte.ts), [`src/lib/components/SearchSiteForm.svelte`](../../src/lib/components/SearchSiteForm.svelte), [`src/tools/manager/Panel.svelte`](../../src/tools/manager/Panel.svelte)
