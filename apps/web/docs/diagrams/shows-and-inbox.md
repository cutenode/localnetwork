# The website diagrams: Shows and the inbox

Shows, follows, inbox delivery and channels. The rest are listed in the [README](../../README.md#diagrams).

- [Announcing a show](#announcing-a-show)
- [Follows and the shows list](#follows-and-the-shows-list)
- [Inbox delivery](#inbox-delivery)
- [Delivery lifecycle](#delivery-lifecycle)
- [Inbox and delivery retention](#inbox-and-delivery-retention)
- [Email and DM channels](#email-and-dm-channels)

## Announcing a show

A show is two records with the same key in the manager's own repo. A confirmed booking thread can prefill the form.

```mermaid
sequenceDiagram
  participant M as Manager
  participant A as App
  participant P as Manager's PDS
  participant H as HappyView
  M->>A: Send the announce form
  A->>A: Active performer, they manage it, venue's UTC offset
  alt New show
    A->>P: applyWrites: calendar event and show together
  else Edit
    A->>H: Versions still the ones the form showed?
    A->>P: putRecord event, then show, each over its version
  end
  A->>H: Wait up to 5 s for both
  opt Not served in time
    A->>H: Re-read that collection
  end
  A->>A: Audit show.save
```

Source: [`src/lib/server/services/shows.ts`](../../src/lib/server/services/shows.ts), [`src/lib/server/forms/announce.ts`](../../src/lib/server/forms/announce.ts), [`src/routes/dashboard/projects/[slug]/shows/[rkey]/+page.server.ts`](../../src/routes/dashboard/projects/[slug]/shows/[rkey]/+page.server.ts), [`src/lib/server/venue-time.ts`](../../src/lib/server/venue-time.ts)

## Follows and the shows list

Following a performer, and what that does to `/shows`.

```mermaid
flowchart TD
  Button["Follow on a performer's page"] --> Signed{"Signed in?"}
  Signed -- "no" --> Login["/login, then back"]
  Signed -- "yes" --> Active{"Performer active?"}
  Active -- "no" --> Gone["404"]
  Active -- "yes" --> Write["computer.localnetwork.follow<br/>in the follower's repo"]
  Write --> Wait["Wait for getFollowStatus to show it"]
  Wait --> Audit["Audit follow.create"]
  Shows["/shows"] --> View{"Which view?"}
  View -- "default" --> Near["listShows near a city,<br/>kept in a cookie"]
  View -- "Following" --> Followed["listShows of followed performers"]
  Write -. "read by" .-> Followed
```

Source: [`src/lib/server/services/follows.ts`](../../src/lib/server/services/follows.ts), [`src/routes/projects/[slug]/+page.server.ts`](../../src/routes/projects/[slug]/+page.server.ts), [`src/routes/shows/+page.server.ts`](../../src/routes/shows/+page.server.ts), [`src/lib/server/shows-place.ts`](../../src/lib/server/shows-place.ts), [`lua/computer/localnetwork/directory/listShows.lua`](../../lua/computer/localnetwork/directory/listShows.lua)

## Inbox delivery

How a booking event reaches someone. Items and their deliveries are written in one transaction, and booking events are the only source so far.

```mermaid
flowchart TD
  Event["Booking record indexed,<br/>from a write or a sync"]
  Event -- "over 14 days old" --> Skip["Nothing"]
  Event --> Item["notification: one per other<br/>participant, once per record"]
  Item --> Site["/dashboard/inbox and<br/>the sidebar count"]
  Item -- "over 6 hours old, or<br/>no active channel on the topic" --> Stays["Stays on the site"]
  Item --> Delivery["notification_delivery,<br/>due after the channel's window"]
  Delivery --> Batch["Outbox: up to 20 items per person<br/>and channel, read ones left out"]
  Batch --> Email["Resend,<br/>2 minute window"]
  Batch --> Dm["Bluesky DM from the root,<br/>1 minute window"]
```

Source: [`src/lib/server/notifications/notifier.ts`](../../src/lib/server/notifications/notifier.ts), [`src/lib/server/notifications/outbox.ts`](../../src/lib/server/notifications/outbox.ts), [`src/lib/server/notifications/email.ts`](../../src/lib/server/notifications/email.ts), [`src/lib/server/notifications/dm.ts`](../../src/lib/server/notifications/dm.ts), [`src/lib/server/notifications/kinds.ts`](../../src/lib/server/notifications/kinds.ts)

## Delivery lifecycle

One item on its way out by one channel. A batch keeps its items and key through every retry, so a service that dedupes on the key sends it once.

```mermaid
stateDiagram-v2
  [*] --> pending
  pending --> skipped: read already, or channel off or paused
  pending --> sending: claimed in a batch
  sending --> sent: service took it
  sending --> pending: retryable, under 8 attempts
  sending --> failed: gave up, refused, or rejected
  sending --> pending: cut off by a restart
  sent --> [*]
  skipped --> [*]
  failed --> [*]
```

Source: [`src/lib/server/notifications/outbox.ts`](../../src/lib/server/notifications/outbox.ts), [`src/lib/server/db/schema.ts`](../../src/lib/server/db/schema.ts)

## Inbox and delivery retention

How long inbox items and deliveries last. The hourly `inbox-retention` job deletes them. An item's age counts from when it was made, and opening it or its thread marks it read.

```mermaid
flowchart LR
  Job["inbox-retention"]
  Job -- "read, over 90 days old" --> Item["Delete the item"]
  Job -- "unread, over 180 days old" --> Item
  Item --> Cascade["Its deliveries go with it"]
  Job -- "sent, failed or skipped,<br/>untouched for 30 days" --> Delivery["Delete the delivery"]
```

Source: [`src/lib/server/notifications/notifier.ts`](../../src/lib/server/notifications/notifier.ts), [`src/lib/server/notifications/outbox.ts`](../../src/lib/server/notifications/outbox.ts), [`src/lib/server/db/migrations.ts`](../../src/lib/server/db/migrations.ts)

## Email and DM channels

One person's email or DM channel. No row means it's off, and turning either off deletes the row. Waiting for the link is `paused` with the reason `email_unverified`.

```mermaid
stateDiagram-v2
  [*] --> Off
  Off --> WaitingForLink: email on
  Off --> Active: DMs on, chat can deliver
  Off --> Paused: host or chat says no
  WaitingForLink --> Active: opens the link in a day
  Active --> WaitingForLink: new address at sign-in
  Active --> Paused: refused, or host unconfirms
  Paused --> WaitingForLink: host confirms
  Paused --> Active: fixed, turned on again
  Active --> Off: off or unsubscribed
  Paused --> Off: off or unsubscribed
  WaitingForLink --> Off: off or unsubscribed
```

Source: [`src/lib/server/notifications/channels.ts`](../../src/lib/server/notifications/channels.ts), [`src/routes/dashboard/account/notifications/+page.server.ts`](../../src/routes/dashboard/account/notifications/+page.server.ts), [`src/routes/notifications/confirm/+page.server.ts`](../../src/routes/notifications/confirm/+page.server.ts), [`src/routes/notifications/unsubscribe/+page.server.ts`](../../src/routes/notifications/unsubscribe/+page.server.ts)
