# localtoolbox diagrams: Lookups

RunJob, the likes listing, API failures and the details cache. The rest are listed in the [README](../../README.md#diagrams).

- [Free answers first](#free-answers-first)
- [One lookup step by step](#one-lookup-step-by-step)
- [Picking up the likes listing](#picking-up-the-likes-listing)
- [Joining a running lookup](#joining-a-running-lookup)
- [Asking the API and failing](#asking-the-api-and-failing)
- [Life of a details entry](#life-of-a-details-entry)
- [Writing the details cache back](#writing-the-details-cache-back)

## Free answers first

What `GET_DOWNLOAD_DETAILS` does before it asks SoundCloud anything, and how each track's answer is filled in afterwards.

```mermaid
flowchart TD
  Extract["extract, once start has run"]
  Any{"any tracks?"}
  Done["answer as it stands"]
  Client{"client id on the page?"}
  NoId["error: no-client-id"]
  Harvest["harvest the hydration"]
  Wanted{"cache lacks any?"}
  Look["lookUp"]
  Fill["fill in each track"]
  Known["known details"]
  Partial["partial, from the page's<br/>purchase link"]
  Nothing["null"]

  Extract --> Any
  Any -->|no| Done
  Any -->|yes| Client
  Client -->|no| NoId
  Client -->|yes| Harvest
  Harvest --> Wanted
  Wanted -->|yes| Look
  Wanted -->|no| Fill
  Look --> Fill
  Fill --> Known
  Fill --> Partial
  Fill --> Nothing
```

Source: [`content.ts`](../../content.ts)

## One lookup step by step

`runJob`, starting with the bulk source for the page's kind.

```mermaid
flowchart TD
  Kind{"page kind"}
  Playlist["playlistDetails<br/>hydration or one /resolve,<br/>then /tracks?ids="]
  Worth{"listing cheaper than<br/>a request per track?"}
  Listing["likesDetails<br/>/users/id/likes"]
  Next["what is left goes on below"]

  Kind -->|playlist| Playlist
  Kind -->|likes| Worth
  Kind -->|"anything else"| Next
  Worth -->|yes| Listing
  Worth -->|no| Next
  Playlist --> Next
  Listing --> Next
```

Source: [`content.ts`](../../content.ts)

Then what it left. A page the anonymous API can't see stops after five rows.

```mermaid
flowchart TD
  Nothing{"bulk ran, settled none,<br/>more than 5 left?"}
  Probe["resolve 5 of them"]
  Seen{"what came back"}
  Each["resolveDetails<br/>/resolve for the rest, 4 at a time"]
  Unauth["stop: unauthorized"]
  Unseen["stop: unseen"]

  Nothing -->|no| Each
  Nothing -->|yes| Probe
  Probe --> Seen
  Seen -->|"a 401"| Unauth
  Seen -->|"no details at all"| Unseen
  Seen -->|"at least one"| Each
  Each -->|"a 401"| Unauth
```

Source: [`content.ts`](../../content.ts)

## Picking up the likes listing

`likesDetails` carries on from where the last walk of a user's likes stopped, as long as every row it wants lies past that point.

```mermaid
flowchart TD
  Cursor{"cursor for this user, and<br/>every wanted row past it?"}
  Resume["next_href from the cursor,<br/>200 items"]
  Top["from the top, what the page<br/>shows plus 5, 200 at most"]
  Get["GET the page of the listing"]
  Failed["cursor dropped if the walk got anywhere,<br/>/resolve per track takes over"]
  Learn["learn its tracks, keep any<br/>next_href and the count as the cursor"]
  More{"next_href, rows unsettled,<br/>under 10 pages?"}
  Done["stop"]

  Cursor -->|yes| Resume
  Cursor -->|no| Top
  Resume --> Get
  Top --> Get
  Get -->|fails| Failed
  Get --> Learn
  Learn --> More
  More -->|yes| Get
  More -->|no| Done
```

Source: [`content.ts`](../../content.ts)

## Joining a running lookup

One lookup runs at a time per tab. A request whose tracks are already covered joins and is answered as soon as they settle.

```mermaid
flowchart TD
  Ask["A request for its wanted tracks"]
  Cover{"a job covers them all?"}
  Join["wait on that job"]
  New["new job behind the others"]
  Waiter["a waiter holds the request"]
  Change["cache changed"]
  Done{"all its tracks settled?"}
  Answer["answer it, the job goes on"]
  Tell["newest waiter gets<br/>DOWNLOAD_DETAILS_PROGRESS"]
  Over["job finished"]
  Release["answer its waiters, flush the cache"]

  Ask --> Cover
  Cover -->|yes| Join
  Cover -->|no| New
  Join --> Waiter
  New --> Waiter
  Waiter --> Change
  Change --> Done
  Done -->|yes| Answer
  Done -->|no| Tell
  Tell --> Change
  Waiter --> Over
  Over --> Release
```

Source: [`content.ts`](../../content.ts)

## Asking the API and failing

`apiGet` is the one way to SoundCloud's API. What happens to a failure depends on who asked.

```mermaid
flowchart TD
  Get["GET api-v2.soundcloud.com<br/>the page's client id, no cookies"]
  Status{"answer"}
  Json["JSON"]
  Wait["wait 1.5 s, then 3 s"]
  Error["ApiError with the status"]
  Who{"who asked"}
  Bulk["/tracks or the likes listing:<br/>/resolve per track takes over"]
  Is401{"a 401?"}
  Stop["stop the rest,<br/>error: unauthorized"]
  TrackFail["a track: remembered as failed, a day"]
  Is404{"an artist: a 404?"}
  ArtistMissing["in the missing list,<br/>no profile, stored"]
  ArtistFail["in the failed list,<br/>the row keeps what it had"]
  Links["web-profiles: a profile with no links"]

  Get --> Status
  Status -->|ok| Json
  Status -->|"429, two retries"| Wait
  Wait --> Get
  Status -->|"anything else"| Error
  Error --> Who
  Who -->|bulk| Bulk
  Who -->|"one track or artist"| Is401
  Is401 -->|yes| Stop
  Is401 -->|no| TrackFail
  Is401 -->|no| Is404
  Is404 -->|yes| ArtistMissing
  Is404 -->|no| ArtistFail
  Is401 -->|no| Links
```

Source: [`content.ts`](../../content.ts)

## Life of a details entry

One entry under `details` in `chrome.storage.local`. Age isn't the only way an entry goes off.

```mermaid
stateDiagram-v2
  state "Not in the cache" as Unknown
  state "Known, 30 days" as Known
  state "Remembered failure, 1 day" as Failed
  [*] --> Unknown
  Unknown --> Known: hydration, API answer or an import
  Unknown --> Failed: lookup failed, not a 401
  Failed --> Known: hydration, API answer or an import
  Failed --> Unknown: a day passed
  Known --> Known: a bulk answer carries it, stamped again
  Known --> Unknown: 30 days passed
  Known --> Unknown: a field this version keeps is missing
  Known --> Unknown: past the newest 20,000, or 10,000 if refused
```

Source: [`content.ts`](../../content.ts), [`state-transfer.ts`](../../state-transfer.ts)

## Writing the details cache back

Answers go into memory as they arrive and reach storage a moment later, merged with what other tabs have written.

```mermaid
flowchart TD
  Learn["learn or learnFailure<br/>into the in-memory cache"]
  Timer["scheduleSave<br/>a second after the first change"]
  Finished["a lookup ends: flushSave"]
  Read["read what storage holds,<br/>keep what is still fresh"]
  Merge["this tab's entries over it"]
  Write["write the newest 20,000"]
  Half["storage refused:<br/>write the newest 10,000"]
  Tabs["other tabs, onChanged:<br/>take only newer entries"]

  Learn --> Timer
  Timer --> Read
  Finished --> Read
  Read --> Merge
  Merge --> Write
  Write -->|refused| Half
  Write --> Tabs
  Half --> Tabs
```

Source: [`content.ts`](../../content.ts)
