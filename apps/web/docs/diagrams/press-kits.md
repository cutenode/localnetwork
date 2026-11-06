# The website diagrams: Press kits

Drafts, grants, views, moves and deletes. The rest are listed in the [README](../../README.md#diagrams).

- [Press kit drafts and publishing](#press-kit-drafts-and-publishing)
- [Press kit access rules](#press-kit-access-rules)
- [Press kit grant lifecycle](#press-kit-grant-lifecycle)
- [Press kit views and receipts](#press-kit-views-and-receipts)
- [Press kit moves](#press-kit-moves)
- [Deleting a press kit](#deleting-a-press-kit)

## Press kit drafts and publishing

How managers change a press kit. Drafts stay in the backstage until one is published.

```mermaid
sequenceDiagram
  participant M as Manager
  participant A as App
  participant B as Backstage space
  participant R as Root's repo
  participant K as Press kit's space
  M->>A: Save, over the version<br/>the form opened on
  A->>B: epk.draft: changed fields<br/>and what they replace
  M->>A: Publish
  A->>A: Compare each field<br/>with what's published now
  alt Another manager changed the field
    A-->>M: Conflict, pick a<br/>side per field
  else
    A->>R: Copy new photos,<br/>put epk over the live CID
    A->>A: Wait up to 5 s<br/>for HappyView
    A->>K: Root writes epk.content
    A->>B: Delete the draft
    A->>A: Audit epk.save
  end
```

Source: [`src/lib/server/services/epk-drafts.ts`](../../src/lib/server/services/epk-drafts.ts), [`src/lib/server/services/epk-fields.ts`](../../src/lib/server/services/epk-fields.ts), [`src/routes/dashboard/projects/[slug]/press-kits/[rkey]/+page.server.ts`](../../src/routes/dashboard/projects/[slug]/press-kits/[rkey]/+page.server.ts)

## Press kit access rules

What happens when someone asks to see a press kit's private part. They get the most open rule of the audiences they're in.

```mermaid
flowchart TD
  Ask["/dashboard/access/[slug]/[rkey]"]
  Ask -- "manager or granted" --> See["They see it already"]
  Ask --> Audiences["Audiences: bookedWith, following,<br/>promoters, anyone"]
  Audiences -- "most open rule: none" --> Closed["Not taking requests"]
  Audiences -- "review" --> Pending["epk.request in the performer's<br/>request space, waits for a manager"]
  Audiences -- "auto" --> Auto["epk.request, then a grant at once<br/>(a legacy press kit's waits for its move)"]
  Pending -- "manager approves" --> Grant["Root adds them to the press kit's space<br/>and writes epk.grant to the request space"]
  Auto --> Grant
  Invite["A manager invites them"] --> Grant
```

Source: [`src/lib/epk.ts`](../../src/lib/epk.ts), [`src/lib/server/services/epk.ts`](../../src/lib/server/services/epk.ts), [`src/routes/dashboard/access/[slug]/[rkey]/+page.server.ts`](../../src/routes/dashboard/access/[slug]/[rkey]/+page.server.ts), [`src/routes/dashboard/projects/[slug]/press-kits/[rkey]/access/+page.server.ts`](../../src/routes/dashboard/projects/[slug]/press-kits/[rkey]/access/+page.server.ts)

## Press kit grant lifecycle

Where someone stands with one press kit. Someone declined can't ask again, but a manager can still invite them.

```mermaid
stateDiagram-v2
  [*] --> NoGrant
  NoGrant --> Pending: asks, rule is review
  NoGrant --> Granted: asks, rule is auto
  NoGrant --> Granted: a manager invites them
  Pending --> Granted: a manager approves
  Pending --> Declined: a manager declines
  Pending --> NoGrant: their request record is deleted
  Granted --> NoGrant: a manager removes them
  Declined --> Granted: a manager invites them
  Granted --> Declined: removed after a decline
```

Source: [`src/lib/server/services/epk.ts`](../../src/lib/server/services/epk.ts), [`src/lib/server/db/schema.ts`](../../src/lib/server/db/schema.ts), [`src/lib/epk.ts`](../../src/lib/epk.ts)

## Press kit views and receipts

What opening a press kit reads, and the receipt it leaves. A manager's own view leaves no receipt.

```mermaid
flowchart TD
  Open["/projects/[slug]/press-kits/[rkey]"] --> Public["Public record from HappyView"]
  Public --> State{"accessState"}
  State -- "manager" --> Content["Private part, read with the<br/>root's space credential"]
  State -- "granted" --> Receipt{"Viewed in the<br/>last 30 minutes?"}
  State -- "anyone else" --> Teaser["Public part, and how to ask"]
  Receipt -- "no" --> Audit["Audit epk.view: root as actor,<br/>HMAC of the viewer's DID under a salt"]
  Audit --> Row["epk_view: the DID and the salt"]
  Row --> Content
  Receipt -- "yes" --> Content
  Row --> Managers["Managers see it at<br/>/dashboard/projects/[slug]/press-kits"]
  Row --> Expire["Deleted by a manager, or<br/>after EPK_VIEW_RETENTION_DAYS"]
```

Source: [`src/routes/projects/[slug]/press-kits/[rkey]/+page.server.ts`](../../src/routes/projects/[slug]/press-kits/[rkey]/+page.server.ts), [`src/lib/server/services/epk-views.ts`](../../src/lib/server/services/epk-views.ts), [`src/lib/server/services/epk.ts`](../../src/lib/server/services/epk.ts)

## Press kit moves

Two moves for press kits made under older rules. Both can stop part way and finish on the next try.

```mermaid
flowchart TD
  Visit["Its maker opens the dashboard"] --> Legacy["Legacy press kit in their repo"]
  Legacy --> Copy["Copy to the root under the same key:<br/>photos, private part, grants, open requests"]
  Copy --> Public["Public record last, in the root's repo"]
  Public --> Old["Maker's session deletes the old<br/>record and space"]
  Boot["Startup"] --> Spaces["Press kit spaces under the root"]
  Spaces --> Grants["Grants move to the request space"]
  Grants --> Asks["Requests move there, as the root<br/>on the asker's behalf"]
  Asks --> Close["The press kit's space stops<br/>taking public writes"]
```

Source: [`src/lib/server/services/epk-legacy.ts`](../../src/lib/server/services/epk-legacy.ts), [`src/lib/server/services/epk-space-move.ts`](../../src/lib/server/services/epk-space-move.ts), [`src/routes/dashboard/+layout.server.ts`](../../src/routes/dashboard/+layout.server.ts)

## Deleting a press kit

Any of the performer's managers can delete a press kit the root holds. The space goes first, so a delete that fails part way can be run again.

```mermaid
flowchart TD
  Ask["Delete in the press kit's editor"] --> Check["A manager of the performer,<br/>and not a legacy press kit"]
  Check --> Space["Root deletes the press kit's space,<br/>which puts everyone out of it"]
  Space --> Records["Root deletes the requests it wrote<br/>for people, and the grant records"]
  Records --> Public["Root deletes the epk record"]
  Public --> Wait["Waits for HappyView to stop serving it"]
  Wait --> Index["Forgets the local rows"]
  Index --> Audit["Audit epk.delete"]
```

Source: [`src/lib/server/services/epk.ts`](../../src/lib/server/services/epk.ts), [`src/routes/dashboard/projects/[slug]/press-kits/[rkey]/+page.server.ts`](../../src/routes/dashboard/projects/[slug]/press-kits/[rkey]/+page.server.ts)
