# The website diagrams: Mixes, sets and charts

Tracklists, sets, recordings, stats and artist pages. The rest are listed in the [README](../../README.md#diagrams).

- [Mix tracklist flow](#mix-tracklist-flow)
- [Linking a set to a mix](#linking-a-set-to-a-mix)
- [DJ stats on a page](#dj-stats-on-a-page)
- [Sets and recordings](#sets-and-recordings)
- [Charts and artist pages](#charts-and-artist-pages)

## Mix tracklist flow

How a tracklist gets from locallibrary onto a performer's page.

```mermaid
flowchart TD
  Export["locallibrary mix ... --export"] --> Form["Performer form, at most 1 MB"]
  Export --> Preview["/mixes/preview, in the browser only"]
  Export --> Profile["/dashboard/profile: blob in their own repo"]
  Form --> Blob["Blob in the requester's PDS,<br/>with the request"]
  Blob --> Review["Staff preview at<br/>/admin/requests/[rkey]/mixes/[n]"]
  Review -- "approved" --> Check["Validate and re-serialize"]
  Check -- "valid" --> Root["Blob in the root's repo,<br/>on the mix"]
  Check -- "invalid or unreadable" --> Without["Publishes the mix without it"]
  Root --> Page["/projects/[slug]/mixes/[n]:<br/>getBlob, validated, cached in memory"]
  Admin["An admin's direct edit"] --> Root
```

Source: [`src/lib/server/services/requests.ts`](../../src/lib/server/services/requests.ts), [`src/lib/server/services/tracklists.ts`](../../src/lib/server/services/tracklists.ts), [`src/lib/mix/schema.ts`](../../src/lib/mix/schema.ts), [`src/lib/server/services/project-admin.ts`](../../src/lib/server/services/project-admin.ts), [`src/routes/projects/[slug]/mixes/[n]/+page.server.ts`](../../src/routes/projects/[slug]/mixes/[n]/+page.server.ts)

## Linking a set to a mix

How a mix links a set a manager published, and what the page says afterwards. A mix on someone's own profile goes to their repo unchecked.

```mermaid
flowchart TD
  Form["A performer form"] --> Picker["Set picker: the managers' sets from HappyView,<br/>re-read every 10 minutes at most"]
  Picker --> Post["Form posts the set's URI and CID"]
  Post -- "a manager's edit" --> Review["Update request,<br/>until staff approve it"]
  Post -- "an admin's direct edit" --> Publish["Root publishes: each new link is checked"]
  Review --> Publish
  Publish -- "not a dj.set, not a manager's,<br/>or HappyView lacks it" --> Refused["Error on the staff form<br/>or the approval"]
  Publish --> Pinned["The mix keeps a strongRef,<br/>pinning that version"]
  Pinned --> Page["The page looks it up with getSets"]
  Page -- "another CID" --> Changed["Says the set changed"]
  Page -- "gone" --> Gone["Says it's gone"]
  Page -- "HappyView down" --> Links["Shows the mix's own link"]
```

Source: [`src/lib/server/services/sets.ts`](../../src/lib/server/services/sets.ts), [`src/lib/server/services/projects.ts`](../../src/lib/server/services/projects.ts), [`src/routes/dashboard/projects/[slug]/profile/+page.server.ts`](../../src/routes/dashboard/projects/[slug]/profile/+page.server.ts), [`src/routes/projects/[slug]/+page.server.ts`](../../src/routes/projects/[slug]/+page.server.ts)

## DJ stats on a page

How a performer page or a profile gets the stats and recent sets DJs publish. The app stores none of it.

```mermaid
flowchart TD
  Page["Performer page or /@[handle]"] --> Fresh["Every account cached,<br/>under 5 minutes old?"]
  Fresh -- "yes" --> Now["Rendered with the page"]
  Fresh -- "no" --> Stream["Streamed in after the page"]
  Stream --> Stats["dj.profile from getRecords"]
  Stream --> Sets["25 latest dj.set per account<br/>from listRecords"]
  Stats --> Cache["Kept in memory,<br/>200 accounts at most"]
  Sets --> Cache
```

Source: [`src/lib/server/services/dj.ts`](../../src/lib/server/services/dj.ts), [`src/lib/server/project-page.ts`](../../src/lib/server/project-page.ts), [`src/lib/server/profile-page.ts`](../../src/lib/server/profile-page.ts)

## Sets and recordings

What plays when someone presses Play. Audio the page plays itself wins over a platform's player. A mix checks its own recording first, then its linked set, then its link.

```mermaid
flowchart TD
  Publish["locallibrary publishes dj.set<br/>and dj.recording to the DJ's repo"] --> HV["HappyView keeps them"]
  Play["Play: /sets/[did]/[rkey]/player.json"] --> Set["Set record from HappyView"]
  HV --> Set
  Set --> Recording{"Its recording?"}
  Recording -- "fm.plyr.track" --> Plyr["api.plyr.fm: file on audio.plyr.fm,<br/>or plyr's player if it's gated"]
  Recording -- "dj.recording by an<br/>active performer's manager" --> Stream["/media/audio/[did]/[cid]"]
  Recording -- "none, or it won't resolve" --> Links["Its links: archive.org plays directly,<br/>others in their own players"]
  Stream --> Blob["getBlob from the DJ's PDS,<br/>Range answered by the app, nothing stored"]
```

Source: [`src/lib/server/services/recordings.ts`](../../src/lib/server/services/recordings.ts), [`src/lib/server/media/audio.ts`](../../src/lib/server/media/audio.ts), [`src/routes/media/audio/[did]/[cid]/+server.ts`](../../src/routes/media/audio/[did]/[cid]/+server.ts), [`src/routes/sets/[did]/[rkey]/player.json/+server.ts`](../../src/routes/sets/[did]/[rkey]/player.json/+server.ts), [`src/lib/server/plyr.ts`](../../src/lib/server/plyr.ts)

## Charts and artist pages

Support counts come straight from the sets HappyView keeps, and nothing stores them. A listed DJ who manages no active performer doesn't count. With HappyView down, the pages say so.

```mermaid
flowchart TD
  Sets["dj.set records of active performers' managers,<br/>each named track kept by the gate"] --> Sql["Lua queries group plays in SQL"]
  Sql --> Charts["getCharts"]
  Sql --> Artist["getArtist"]
  Sql --> Search["searchArtists"]
  Charts --> Memo["App reuses an answer for a minute"]
  Artist --> Memo
  Search --> Memo
  Memo --> P1["/charts"]
  Memo --> P2["/artists/[id], /artists/name/[[name]]"]
  Memo --> P3["/artists"]
```

Source: [`src/lib/server/services/supports.ts`](../../src/lib/server/services/supports.ts), [`src/routes/charts/+page.server.ts`](../../src/routes/charts/+page.server.ts), [`lua/computer/localnetwork/directory/getCharts.lua`](../../lua/computer/localnetwork/directory/getCharts.lua), [`lua/computer/localnetwork/directory/getArtist.lua`](../../lua/computer/localnetwork/directory/getArtist.lua)
