# The website diagrams: Jobs and the directory

Startup, scheduled jobs and HappyView. The rest are listed in the [README](../../README.md#diagrams).

- [Startup](#startup)
- [Scheduled jobs](#scheduled-jobs)
- [Directory sync](#directory-sync)
- [HappyView re-reads](#happyview-re-reads)
- [requestIndex](#requestindex)
- [HappyView record gates](#happyview-record-gates)
- [Directory queries](#directory-queries)

## Startup

What happens when the process starts. A failed start leaves the setup page up, and the next request tries again.

```mermaid
flowchart TD
  Init["init hook, or the first request"] --> Db["Open SQLite, run migrations"]
  Db --> Root["Connect the root account,<br/>reusing its saved session"]
  Root --> Wire["Build the services"]
  Wire --> Dir["Directory sync"]
  Dir --> Boot["bootstrap, in the background"]
  Dir --> Timers["Start the scheduled jobs,<br/>resume notification delivery"]
  Boot --> Spaces["ensureRootSpaces,<br/>list every space, watch them"]
  Spaces --> Members["Reconcile staff, manager<br/>and shared space members"]
  Members --> Sync["syncAll"]
  Sync --> Listings["listings.reconcile"]
  Listings --> Moves["moveKitSpaces"]
```

Source: [`src/hooks.server.ts`](../../src/hooks.server.ts), [`src/lib/server/app.ts`](../../src/lib/server/app.ts), [`src/lib/server/bootstrap.ts`](../../src/lib/server/bootstrap.ts), [`src/lib/server/db/migrations.ts`](../../src/lib/server/db/migrations.ts)

## Scheduled jobs

The jobs the app schedules, all in one process. A job still running skips its next turn.

```mermaid
flowchart LR
  Poll["SYNC_POLL_INTERVAL_MS,<br/>60 s by default"] --> P1["sync: syncAll, or bootstrap<br/>until that has worked"]
  Poll --> P2["directory-sync: retry a failed sync,<br/>else script re-reads"]
  Quick["15 s"] --> Q1["audit-retry: 25 due entries"]
  Quick --> Q2["notification-delivery, also<br/>woken when an item comes due"]
  Five["5 minutes"] --> F1["staff-spaces"]
  Hour["1 hour"] --> H1["oauth-state-cleanup"]
  Hour --> H2["manager-spaces"]
  Hour --> H3["shared-spaces"]
  Hour --> H4["listings"]
  Hour --> H5["epk-view-expiry"]
  Hour --> H6["inbox-retention"]
  Hour --> H7["db-backup, with a bucket:<br/>once a UTC day"]
```

Source: [`src/lib/server/app.ts`](../../src/lib/server/app.ts), [`src/lib/server/backup/backups.ts`](../../src/lib/server/backup/backups.ts), [`src/lib/server/services/epk-views.ts`](../../src/lib/server/services/epk-views.ts), [`src/lib/server/notifications/notifier.ts`](../../src/lib/server/notifications/notifier.ts)

## Directory sync

What `syncDirectory` does with `HAPPYVIEW_API_KEY` set, at boot and from the System page. A failed upload or root re-read is kept for the System page and retried. The re-reads after it only log a failure.

```mermaid
flowchart TD
  Compare["Compare scripts and lexicons<br/>with what HappyView stores"] --> Scripts["Upload changed scripts first,<br/>so the gate is in place"]
  Scripts --> Lexicons["Upload changed lexicons,<br/>one at a time"]
  Lexicons --> Root["Re-read the root's performers,<br/>wait up to 30 s"]
  Root --> Shows["Each manager's shows,<br/>one repo at a time, in the background"]
  Root --> Refresh["refreshRecords: older records,<br/>once per script version"]
  Refresh --> Kv["Version kept in kv<br/>once every re-read has started"]
```

Source: [`src/lib/server/happyview/sync.ts`](../../src/lib/server/happyview/sync.ts), [`src/lib/server/happyview/bundle.ts`](../../src/lib/server/happyview/bundle.ts), [`src/lib/server/happyview/refresh.ts`](../../src/lib/server/happyview/refresh.ts), [`src/lib/server/app.ts`](../../src/lib/server/app.ts)

## HappyView re-reads

What makes the app ask HappyView to read a repo again through `/admin/backfill`. Jetstream only brings new writes, so these fill the gaps.

```mermaid
flowchart LR
  Backfill["/admin/backfill"]
  Backfill -- "new record script" --> R1["That collection, per<br/>manager and root"]
  Backfill -- "new record script" --> R2["Follows press kits read,<br/>root's verifications and<br/>DJ listings, bug report space"]
  Backfill -- "a performer gains a manager,<br/>once HappyView serves it" --> R3["Their records and follows"]
  Backfill -- "boot" --> R4["The root's performers,<br/>each manager's shows"]
  Backfill -- "the set picker opens" --> R5["Their sets, every<br/>10 minutes at most"]
  Backfill -- "requestIndex" --> R6["The caller's<br/>four collections"]
  Backfill -- "a late write, except a performer's,<br/>an InvalidSwap on a press kit, show<br/>or verification, or a record HappyView lacks" --> R7["That collection<br/>from that account"]
  Backfill -- "a first press kit<br/>that reads follows" --> R8["Its managers' follows"]
  Backfill -- "staff list a DJ" --> R9["Their profile"]
```

Source: [`src/lib/server/happyview/refresh.ts`](../../src/lib/server/happyview/refresh.ts), [`src/lib/server/happyview/backfills.ts`](../../src/lib/server/happyview/backfills.ts), [`src/lib/server/happyview/indexed.ts`](../../src/lib/server/happyview/indexed.ts), [`src/lib/server/services/projects.ts`](../../src/lib/server/services/projects.ts), [`src/lib/server/services/sets.ts`](../../src/lib/server/services/sets.ts), [`src/lib/server/services/epk.ts`](../../src/lib/server/services/epk.ts)

## requestIndex

locallibrary asks for its own records to be read again before it replaces them.

```mermaid
sequenceDiagram
  participant L as locallibrary
  participant A as App
  participant H as HappyView
  L->>A: POST directory.requestIndex<br/>with service auth
  A->>A: Check audience,<br/>method and signature
  alt Check fails
    A-->>L: 401
  else
    par sets, recordings, stats, settings, all at once
      A->>H: Backfill the issuer's repo,<br/>unless done in the last 10 minutes
      A->>H: Wait up to 5 s
    end
    A-->>L: indexed: true when all finished,<br/>false without HAPPYVIEW_API_KEY
  end
```

Source: [`src/routes/xrpc/computer.localnetwork.directory.requestIndex/+server.ts`](../../src/routes/xrpc/computer.localnetwork.directory.requestIndex/+server.ts), [`src/lib/server/happyview/request-index.ts`](../../src/lib/server/happyview/request-index.ts), [`src/lib/server/happyview/backfills.ts`](../../src/lib/server/happyview/backfills.ts)

## HappyView record gates

What each record script keeps of what Jetstream sends. Anyone can write these collections, so the gates decide what the directory holds.

```mermaid
flowchart LR
  Jet["Jetstream"]
  Jet -- "the root's only" --> Root["project, listing.profile,<br/>verification"]
  Jet -- "every account's" --> All["profile, dj.set, dj.recording,<br/>dj.profile, locallibrary.settings"]
  Jet -- "accounts that manage<br/>the root's performers" --> Managers["calendar event, and show when<br/>it names a performer they manage"]
  Jet -- "managers, and accounts<br/>the root verified" --> Names["Bluesky display names"]
  Jet -- "press kits of the<br/>root's performers" --> Kits["The root's, and a<br/>manager's older ones"]
  Jet -- "of the root's performers" --> Follows["computer.localnetwork.follow"]
  Jet -- "managers of a performer whose<br/>press kit reads follows" --> Bsky["Bluesky follows"]
  Jet -- "USERINPUT_SPACE alone" --> Input["app.userinput.space"]
```

Source: [`lua/computer/localnetwork/project.lua`](../../lua/computer/localnetwork/project.lua), [`lua/computer/localnetwork/show.lua`](../../lua/computer/localnetwork/show.lua), [`lua/computer/localnetwork/epk.lua`](../../lua/computer/localnetwork/epk.lua), [`lua/computer/localnetwork/follow.lua`](../../lua/computer/localnetwork/follow.lua), [`lua/app/bsky/actor/profile.lua`](../../lua/app/bsky/actor/profile.lua), [`lua/app/bsky/graph/follow.lua`](../../lua/app/bsky/graph/follow.lua), [`lua/app/userinput/space.lua`](../../lua/app/userinput/space.lua), [`src/lib/server/happyview/bundle.ts`](../../src/lib/server/happyview/bundle.ts)

## Directory queries

Which query answers which page. All of them are Lua scripts on HappyView, and `getRecords` and `listRecords` also back `IndexedRepo`.

```mermaid
flowchart LR
  Pages["Pages and services"]
  Pages -- "home, /for-djs, staff overview" --> Q1["getStats"]
  Pages -- "home, /for-djs, dashboard sidebar,<br/>staff's performers" --> Q10["searchProjects"]
  Pages -- "/projects" --> Q2["searchPerformers"]
  Pages -- "/match" --> Q3["matchProjects"]
  Pages -- "performer pages, dashboard" --> Q4["getProject, getProjects"]
  Pages -- "home, /shows, performer pages" --> Q5["listShows"]
  Pages -- "waiting on a show write" --> Q11["getShows"]
  Pages -- "mixes, the set picker" --> Q6["getSets, listSets"]
  Pages -- "Follow button" --> Q7["getFollowStatus"]
  Pages -- "home, /charts, /artists" --> Q8["getCharts, getArtist,<br/>searchArtists"]
  Pages -- "stats, press kits, profiles,<br/>verifications, bug report space" --> Q9["getRecords, listRecords"]
```

Source: [`src/lib/server/happyview/client.ts`](../../src/lib/server/happyview/client.ts), [`src/lib/server/happyview/indexed.ts`](../../src/lib/server/happyview/indexed.ts), [`src/lib/server/services/projects.ts`](../../src/lib/server/services/projects.ts), [`src/lib/server/services/performers.ts`](../../src/lib/server/services/performers.ts), [`src/lib/server/services/shows.ts`](../../src/lib/server/services/shows.ts), [`src/lib/server/services/supports.ts`](../../src/lib/server/services/supports.ts), [`src/routes/+page.server.ts`](../../src/routes/+page.server.ts), [`src/routes/for-djs/+page.server.ts`](../../src/routes/for-djs/+page.server.ts)
