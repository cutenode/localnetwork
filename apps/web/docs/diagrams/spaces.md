# The website diagrams: Spaces

Credentials, sync, notifications, projections and member lists. The rest are listed in the [README](../../README.md#diagrams).

- [Space types](#space-types)
- [Space credentials](#space-credentials)
- [Reads outside the sync](#reads-outside-the-sync)
- [Space sync pass](#space-sync-pass)
- [Repo sync lifecycle](#repo-sync-lifecycle)
- [Write notifications](#write-notifications)
- [Record projections](#record-projections)
- [Member list upkeep](#member-list-upkeep)

## Space types

Every kind of space, what creates it, and how the app reads it.

```mermaid
flowchart LR
  App["App"]
  App -- "bootstrap" --> Four["audit, moderation,<br/>staff, industry"]
  App -- "a booking inquiry" --> Booking["booking,<br/>one per thread"]
  App -- "first press kit publish" --> Epk["epk,<br/>one per press kit"]
  App -- "first request or grant" --> Requests["epkRequests,<br/>one per performer"]
  App -- "first draft saved" --> Backstage["backstage,<br/>one per performer"]
  App -- "SharedSpaces.create or createAct,<br/>no page calls them yet" --> Shared["performer, party, venue"]
  App -- "first planner read or write" --> Planner["planner, under the<br/>person's own DID"]
  App -- "nothing yet" --> Details["bookingDetails, one per<br/>performer, root only"]
  Four --> Watch["Syncer watches it,<br/>reading with the<br/>root's credential"]
  Booking --> Watch
  Epk --> Watch
  Requests --> Watch
  Backstage --> Watch
  Shared --> Watch
  Planner --> Session["Read with their session,<br/>no copy kept"]
```

Source: [`src/lib/server/bootstrap.ts`](../../src/lib/server/bootstrap.ts), [`src/lib/server/atproto/spaces.ts`](../../src/lib/server/atproto/spaces.ts), [`src/lib/nsid.ts`](../../src/lib/nsid.ts), [`src/lib/server/services/space-members.ts`](../../src/lib/server/services/space-members.ts), [`src/lib/server/services/bookings.ts`](../../src/lib/server/services/bookings.ts), [`src/lib/server/services/planner.ts`](../../src/lib/server/services/planner.ts)

## Space credentials

How the app syncs a space. The root's session gets a delegation token, the space host trades it for a credential, and each repo host proves its answer with a MAC.

```mermaid
sequenceDiagram
  participant A as App
  participant R as Root PDS
  participant H as Space host
  participant W as Writer's repo host
  A->>R: getDelegationToken with the root's session
  R-->>A: Delegation token
  A->>A: New P-256 key, kept in memory
  A->>H: getSpaceCredential with the token and key id
  H-->>A: Credential bound to that key
  A->>H: listRepos, signed with the key
  H-->>A: Writers and their revs
  A->>W: listRepoOps, signed, with a one-time KEM share
  W-->>A: Ops and a commit with a MAC
  A->>A: Check the MAC and the LtHash
  alt MAC fails
    A->>A: Refresh the writer's DID document, read once more
  else Credential rejected
    A->>H: Mint a fresh credential, retry once
  end
```

Source: [`src/lib/server/atproto/credentials.ts`](../../src/lib/server/atproto/credentials.ts), [`src/lib/server/sync/engine.ts`](../../src/lib/server/sync/engine.ts), [`src/lib/server/atproto/repo-client.ts`](../../src/lib/server/atproto/repo-client.ts)

## Reads outside the sync

What the app reads from a space directly, without going through the local copy.

```mermaid
flowchart LR
  Reads["Direct space reads"]
  Reads -- "a press kit's private part" --> R1["getRecord with the root's space<br/>credential, checked against its MAC"]
  Reads -- "private and draft photos, proposed<br/>avatars and tracklists" --> R2["getBlob with the root's<br/>space credential"]
  Reads -- "the root's own records: grants,<br/>people, opening records" --> R3["Root session: listSpaceRecords,<br/>getSpaceRecord"]
  Reads -- "who's in a space" --> R4["Root session: listMembers"]
  Reads -- "planners" --> R5["The person's OAuth session"]
  Reads -- "a legacy press kit's records" --> R6["Its maker's OAuth session"]
```

Source: [`src/lib/server/app.ts`](../../src/lib/server/app.ts), [`src/lib/server/atproto/credentials.ts`](../../src/lib/server/atproto/credentials.ts), [`src/lib/server/services/epk.ts`](../../src/lib/server/services/epk.ts), [`src/lib/server/services/requests.ts`](../../src/lib/server/services/requests.ts), [`src/lib/server/services/manager-spaces.ts`](../../src/lib/server/services/manager-spaces.ts), [`src/lib/server/services/planner.ts`](../../src/lib/server/services/planner.ts), [`src/lib/server/services/epk-legacy.ts`](../../src/lib/server/services/epk-legacy.ts)

## Space sync pass

One pass over one space. A second request while it runs queues one more pass.

```mermaid
flowchart TD
  Trigger["Poll, notifyWrite, or watchSpace"] --> List["registerNotify if it's due, then<br/>listRepos from spaceRev, 500 a page"]
  List -- "authority says<br/>it's gone" --> Drop["Drop the local copy<br/>and what came from it"]
  List --> Ops["Each repo that's behind, 4 at a time<br/>across all spaces: listRepoOps since local rev"]
  List -- "never synced" --> Star["getRepo: verified STAR archive"]
  Ops -- "fails, not on a MAC<br/>or credential" --> Star
  Ops -- "writer taken down,<br/>suspended or deactivated" --> Skip["Skip, retry on backoff"]
  Star -- "same errors" --> Skip
  Ops --> Apply["Transaction: space_record, pending_projection,<br/>synced_repo"]
  Star --> Apply
  Apply --> Project["Project each change.<br/>A failure stays in pending_projection"]
  Project --> Due["Repos and projections whose retry<br/>is due, then save spaceRev"]
```

Source: [`src/lib/server/sync/engine.ts`](../../src/lib/server/sync/engine.ts)

## Repo sync lifecycle

One writer's repo in one space. A failure stays in `sync_repo_retry`, since `listRepos` won't list that repo again until it changes.

```mermaid
stateDiagram-v2
  [*] --> Unsynced
  Unsynced --> Synced: full download verifies
  Unsynced --> Failing: read fails
  Synced --> Synced: oplog matches the commit
  Synced --> Synced: oplog fails, full download verifies
  Synced --> Failing: read fails
  Failing --> Failing: fails again, wait doubles from 1 minute to 1 hour
  Failing --> Synced: retry works
  Synced --> [*]: space dropped
  Failing --> [*]: space dropped
```

Source: [`src/lib/server/sync/engine.ts`](../../src/lib/server/sync/engine.ts), [`src/lib/server/db/schema.ts`](../../src/lib/server/db/schema.ts)

## Write notifications

How a write reaches the app without waiting for the poll, with `SYNC_NOTIFY` on. Without it, or if registering fails, writes wait for the poll.

```mermaid
sequenceDiagram
  participant W as Writer's PDS
  participant H as Space host
  participant A as App
  A->>H: registerNotify with the space credential
  W->>H: notifyWrite, service auth from the writer
  H->>A: GET /.well-known/did.json to find the syncer
  H->>A: POST notifyWrite, service auth from the authority
  A->>A: Check audience, method and issuer
  alt Bad space
    A-->>H: 400
  else Check fails
    A-->>H: 401
  else Space isn't watched
    A-->>H: 404
  else Watched
    A-->>H: 200
    A->>A: Sync the space in the background
  end
  H->>A: POST notifySpaceDeleted when a space goes
  A->>A: Drop the local copy
```

Source: [`src/routes/xrpc/com.atproto.space.notifyWrite/+server.ts`](../../src/routes/xrpc/com.atproto.space.notifyWrite/+server.ts), [`src/routes/xrpc/com.atproto.space.notifySpaceDeleted/+server.ts`](../../src/routes/xrpc/com.atproto.space.notifySpaceDeleted/+server.ts), [`src/routes/.well-known/did.json/+server.ts`](../../src/routes/.well-known/did.json/+server.ts), [`src/lib/server/atproto/service-auth.ts`](../../src/lib/server/atproto/service-auth.ts)

## Record projections

Where each synced record lands. Every record is checked against its lexicon first, and an invalid one is skipped. A delete takes its row with it, except that a deleted open request reads as withdrawn and a deleted audit copy marks the entry failed.

```mermaid
flowchart LR
  Sync["Synced change"]
  Sync -- "audit.entry" --> A1["audit_copy, and marks the<br/>entry verified or failed"]
  Sync -- "moderation.request" --> A2["request"]
  Sync -- "moderation.decision,<br/>root only" --> A3["request's decision"]
  Sync -- "staff.role, root only" --> A4["staff_role"]
  Sync -- "project.details, root only" --> A5["project_details"]
  Sync -- "booking.inquiry, message, status,<br/>participants and the root" --> A6["booking_thread, booking_event,<br/>inbox items"]
  Sync -- "epk.request, and epk.grant<br/>from the authority only" --> A7["epk_request, epk_grant"]
  Sync -- "epk.draft in a backstage" --> A8["epk_draft"]
  Sync -- "people.*, listing.status,<br/>performer, root only" --> A9["shared space tables and<br/>performer, then its listing"]
  Sync -- "staff.flag, staff.group,<br/>root only" --> A10["feature_flag, flag_group"]
```

Source: [`src/lib/server/sync/projections.ts`](../../src/lib/server/sync/projections.ts), [`src/lib/server/services/bookings.ts`](../../src/lib/server/services/bookings.ts), [`src/lib/server/services/epk.ts`](../../src/lib/server/services/epk.ts), [`src/lib/server/services/flags.ts`](../../src/lib/server/services/flags.ts)

## Member list upkeep

What keeps each space's member list right. Every run compares with the live list, so a run that failed part way is finished by the next.

```mermaid
flowchart TD
  Publish["A publish that changes managers"] --> Managers["ManagerSpaces.sync"]
  Hourly["Startup, hourly, System page"] --> All["reconcileManagerSpaces,<br/>from the root's own repo"]
  All --> Managers
  Managers --> Threads["Booking spaces: leavers go,<br/>new managers join live threads,<br/>the promoter stays"]
  Managers --> Back["backstage: read and write"]
  Managers --> Asks["epkRequests: read"]
  Managers --> Kits["epk spaces: read,<br/>granted people stay"]
  Hourly --> Shared["SharedSpaces:<br/>members follow people records"]
  People["A change to a space's people"] --> Shared
  Boot["Startup, every 5 minutes,<br/>System page"] --> Staff["audit, moderation, staff and<br/>industry spaces: staff join,<br/>leavers go"]
  Threads -. "only with a root record<br/>naming the performer" .-> Root["Opening record or<br/>booking.create audit entry"]
```

Source: [`src/lib/server/services/manager-spaces.ts`](../../src/lib/server/services/manager-spaces.ts), [`src/lib/server/services/space-members.ts`](../../src/lib/server/services/space-members.ts), [`src/lib/server/services/shared-spaces.ts`](../../src/lib/server/services/shared-spaces.ts), [`src/lib/server/services/staff.ts`](../../src/lib/server/services/staff.ts)
