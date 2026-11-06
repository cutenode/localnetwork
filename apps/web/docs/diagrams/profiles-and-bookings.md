# The website diagrams: Profiles, requests and bookings

Listings, moderation, search, the planner and booking threads. The rest are listed in the [README](../../README.md#diagrams).

- [Profiles and DJ listings](#profiles-and-dj-listings)
- [Moderation requests](#moderation-requests)
- [Request lifecycle](#request-lifecycle)
- [Performer status](#performer-status)
- [Search and the show matcher](#search-and-the-show-matcher)
- [Planner flow](#planner-flow)
- [Booking inquiry](#booking-inquiry)
- [Booking thread messages](#booking-thread-messages)
- [Booking lifecycle](#booking-lifecycle)

## Profiles and DJ listings

How someone gets a profile, and how a DJ gets into Find DJs.

```mermaid
flowchart TD
  First["No profile yet: a card on /dashboard,<br/>filled from their Bluesky profile"] --> Edit["/dashboard/profile"]
  Edit --> Save["Writes computer.localnetwork.profile<br/>to their repo over the version the form opened"]
  Save --> Wait["Waits up to 5 s for HappyView to serve it"]
  Wait --> Page["/@[handle] shows it,<br/>sections decide the sidebar"]
  Edit -- "DJ section on" --> Ask["list request in the moderation space"]
  Ask --> Review["Staff review the profile as it is now"]
  Review -- "approved" --> Listing["Root writes listing.profile,<br/>HappyView re-reads the profile"]
  Listing --> Find["searchPerformers shows them"]
  Find --> Out["/admin/listings/[did] can take them out"]
  Find --> Off["Turning the DJ section off hides<br/>them too, the listing stays"]
```

Source: [`src/lib/server/services/profiles.ts`](../../src/lib/server/services/profiles.ts), [`src/routes/dashboard/profile/+page.server.ts`](../../src/routes/dashboard/profile/+page.server.ts), [`src/lib/server/services/dj-listings.ts`](../../src/lib/server/services/dj-listings.ts), [`src/routes/admin/listings/[did]/+page.server.ts`](../../src/routes/admin/listings/[did]/+page.server.ts)

## Moderation requests

A request from filing to decision. `create`, `claim` and `update` publish a performer; `list` lists a DJ.

```mermaid
sequenceDiagram
  participant R as Requester
  participant A as App
  participant S as Moderation space
  participant T as Staff
  participant P as Root's repo
  R->>A: Send the form
  A->>S: Request in their repo,<br/>files as blobs
  Note over A,S: Without spaces, the root files it<br/>with onBehalfOf
  A->>A: Index it,<br/>audit request.submit
  T->>A: Open /admin/requests/[rkey]
  A->>S: Read proposed files<br/>with the space credential
  T->>A: Decide
  alt Approved
    A->>S: Copy and check<br/>avatar and tracklists
    A->>P: Publish the performer<br/>or the DJ listing
  end
  A->>S: Root writes the decision
  A->>A: Audit the decision
```

Source: [`src/lib/server/services/requests.ts`](../../src/lib/server/services/requests.ts), [`src/routes/admin/requests/[rkey]/+page.server.ts`](../../src/routes/admin/requests/[rkey]/+page.server.ts), [`src/routes/dashboard/projects/new/+page.server.ts`](../../src/routes/dashboard/projects/new/+page.server.ts), [`src/routes/dashboard/claim/[slug]/+page.server.ts`](../../src/routes/dashboard/claim/[slug]/+page.server.ts)

## Request lifecycle

The status of a moderation request. Nobody can decide their own, and only staff decide. A rejection or a call for changes needs a reason. A `list` request can't be revised, so changes requested on one end in a withdrawal.

```mermaid
stateDiagram-v2
  [*] --> pending: submitted
  pending --> pending: requester revises (not a list request)
  pending --> approved: staff approve
  pending --> rejected: staff reject
  pending --> changes_requested: staff ask for changes
  changes_requested --> pending: requester revises (not a list request)
  pending --> withdrawn: requester withdraws, or their record is deleted
  changes_requested --> withdrawn: requester withdraws, or their record is deleted
  approved --> [*]
  rejected --> [*]
  withdrawn --> [*]
```

Source: [`src/lib/server/services/requests.ts`](../../src/lib/server/services/requests.ts), [`src/lib/server/sync/projections.ts`](../../src/lib/server/sync/projections.ts), [`src/lib/server/db/schema.ts`](../../src/lib/server/db/schema.ts)

## Performer status

The `status` on a `computer.localnetwork.project` record. Moderators can hide and show a performer; only admins can archive.

```mermaid
stateDiagram-v2
  [*] --> active: request approved, or an admin adds it
  active --> hidden: staff
  hidden --> active: staff
  active --> archived: admin
  hidden --> archived: admin
  archived --> active: staff
  archived --> hidden: staff
```

Source: [`src/lib/server/services/project-admin.ts`](../../src/lib/server/services/project-admin.ts), [`src/lib/types.ts`](../../src/lib/types.ts), [`src/routes/admin/projects/[slug]/+page.server.ts`](../../src/routes/admin/projects/[slug]/+page.server.ts)

## Search and the show matcher

How a promoter finds a performer and starts a booking.

```mermaid
flowchart TD
  Find["/projects: Find DJs"] --> Search["searchPerformers:<br/>active performers and listed DJs"]
  Match["/match: describe the night"] --> Rank["matchProjects: ranked,<br/>with reason codes"]
  City["City search"] --> Places["/api/places, cached in SQLite"]
  Places --> Find
  Places --> Match
  Search --> Dj["/@[handle] for a listed DJ"]
  Search --> Page["/projects/[slug]"]
  Rank --> Page
  Rank --> Dated["Inquire from a result:<br/>/dashboard/bookings/new with the night's date"]
  Rank --> Night["Save the night and ticked DJs<br/>as a planned show"]
  Page --> Save["Save to the planner"]
  Page --> Inquire["/dashboard/bookings/new"]
```

Source: [`src/routes/projects/+page.server.ts`](../../src/routes/projects/+page.server.ts), [`src/routes/match/+page.server.ts`](../../src/routes/match/+page.server.ts), [`src/lib/server/services/performers.ts`](../../src/lib/server/services/performers.ts), [`src/lib/server/geocode.ts`](../../src/lib/server/geocode.ts), [`lua/lib/directory.lua`](../../lua/lib/directory.lua)

## Planner flow

The promoter's private planner. The space is under their own DID, so it needs a PDS with spaces.

```mermaid
flowchart TD
  Save["Save on a performer's page"] --> Client{"spacesCapable,<br/>with a session?"}
  Night["/match: Save show to planner"] --> Client
  Show["/dashboard/planner/shows/[rkey]"] --> Client
  Show --> Inquire["Inquire from a planned show,<br/>filled in from its details"]
  Client -- "no" --> Off["Planner unavailable"]
  Client -- "yes" --> Ensure["Create the planner space if it's missing"]
  Ensure --> Records["planner.save and planner.show<br/>in their own repo"]
  Records --> Audit["Audit planner.*"]
  Page["/dashboard/planner"] --> Read["Reads them with their session"]
  Read --> Names["Names performers from HappyView"]
```

Source: [`src/lib/server/services/planner.ts`](../../src/lib/server/services/planner.ts), [`src/lib/server/planner-page.ts`](../../src/lib/server/planner-page.ts), [`src/routes/dashboard/bookings/new/+page.server.ts`](../../src/routes/dashboard/bookings/new/+page.server.ts)

## Booking inquiry

Opening a booking thread. It reads the performer from the root's own repo, so a new manager is already in the thread.

```mermaid
sequenceDiagram
  participant B as Promoter
  participant A as App
  participant R as Root's repo
  participant S as Thread's space
  participant D as SQLite
  B->>A: Send the inquiry
  A->>R: Performer record
  A->>S: Root creates the space,<br/>adds promoter, managers
  A->>D: Thread and<br/>participant rows
  A->>S: Root writes the<br/>opening status record
  A->>S: Inquiry in the<br/>promoter's repo
  Note over A,S: Without spaces, the root<br/>writes it with onBehalfOf
  alt A write fails
    A->>S: Delete the space
    A-->>B: Not sent
  else Sent
    A->>D: Index the inquiry,<br/>inbox items
    A->>A: Watch the space,<br/>audit booking.create
    A-->>B: The thread
  end
```

Source: [`src/lib/server/services/bookings.ts`](../../src/lib/server/services/bookings.ts), [`src/routes/dashboard/bookings/new/+page.server.ts`](../../src/routes/dashboard/bookings/new/+page.server.ts), [`src/routes/dashboard/bookings/[skey]/+page.server.ts`](../../src/routes/dashboard/bookings/[skey]/+page.server.ts)

## Booking thread messages

What happens in a thread once it's open.

```mermaid
sequenceDiagram
  participant P as Participant
  participant A as App
  participant S as Thread's space
  participant D as SQLite
  P->>A: Open /dashboard/bookings/[skey]
  A->>D: Mark their inbox items<br/>for it read
  alt Not a participant
    A-->>P: 404
  else Participant
    A->>D: Thread and events,<br/>leaving out the root's
    A-->>P: The thread
  end
  P->>A: Send a message
  A->>S: Message in their repo, or<br/>the root's with onBehalfOf
  A->>D: Index it, inbox items for<br/>the other participants
  A->>D: Audit booking.message,<br/>its length, never its text
  Note over P,A: A confirmed thread offers<br/>managers Announce this show
```

Source: [`src/lib/server/services/bookings.ts`](../../src/lib/server/services/bookings.ts), [`src/routes/dashboard/bookings/[skey]/+page.server.ts`](../../src/routes/dashboard/bookings/[skey]/+page.server.ts), [`src/routes/dashboard/bookings/[skey]/+page.svelte`](../../src/routes/dashboard/bookings/[skey]/+page.svelte), [`src/routes/dashboard/+layout.server.ts`](../../src/routes/dashboard/+layout.server.ts)

## Booking lifecycle

The status of a booking thread. The site refuses any other change with a 409 and audits it as denied. A status record a participant writes to the space from elsewhere is indexed without that check.

```mermaid
stateDiagram-v2
  [*] --> open: inquiry sent
  open --> negotiating: manager
  open --> confirmed: manager, or promoter once a manager wrote
  open --> declined: manager
  open --> cancelled: promoter withdraws
  negotiating --> confirmed: either side
  negotiating --> declined: manager
  negotiating --> cancelled: promoter withdraws
  confirmed --> cancelled: either side
  declined --> [*]
  cancelled --> [*]
```

Source: [`src/lib/bookings.ts`](../../src/lib/bookings.ts), [`src/lib/server/services/bookings.ts`](../../src/lib/server/services/bookings.ts)
