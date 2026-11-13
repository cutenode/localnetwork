# The website diagrams: Feature flags

Deciding a flag, flags on each request, getFlags, the staff console and the flag tables. The rest are listed in the [README](../../README.md#diagrams).

- [Deciding a flag](#deciding-a-flag)
- [Flags on a page request](#flags-on-a-page-request)
- [getFlags](#getflags)
- [Changing flags](#changing-flags)
- [Flag tables](#flag-tables)

## Deciding a flag

Whether one flag is on for one caller. Only flags in `flags/catalog/` exist: a record for a flag that's gone from it is ignored.

```mermaid
flowchart TD
  Ask["A catalog flag, for a DID<br/>or a signed-out visitor"] --> Forced{"In FLAG_OVERRIDES?"}
  Forced -- "forced on" --> On["On"]
  Forced -- "forced off" --> Off["Off"]
  Forced -- "no" --> Record{"A staff.flag record?"}
  Record -- "no" --> Off
  Record -- "yes" --> Enabled{"enabled?"}
  Enabled -- "no" --> Off
  Enabled -- "yes" --> Everyone{"everyone?"}
  Everyone -- "yes" --> On
  Everyone -- "no" --> Targeted{"The DID is in its accounts<br/>or one of its groups?"}
  Targeted -- "yes" --> On
  Targeted -- "no" --> Bucket{"Signed in, and SHA-256 of key + DID,<br/>mod 100, under percentage?"}
  Bucket -- "yes" --> On
  Bucket -- "no" --> Off
```

Source: [`src/lib/server/flags/evaluate.ts`](../../src/lib/server/flags/evaluate.ts), [`src/lib/server/services/flags.ts`](../../src/lib/server/services/flags.ts), [`src/lib/server/flags/catalog.ts`](../../src/lib/server/flags/catalog.ts), [`src/lib/server/config.ts`](../../src/lib/server/config.ts)

## Flags on a page request

Every request outside `/xrpc` gets every flag decided once, held in memory between record changes. A staff member's preview changes only their own browser.

```mermaid
flowchart TD
  Request["A request, not /xrpc"] --> Decide["Decide every flag for the viewer,<br/>or for a signed-out visitor"]
  Decide --> Cookie{"ln_flag_preview set?"}
  Cookie -- "no" --> Locals["locals.flags, passed to<br/>every page by the root layout"]
  Cookie -- "yes" --> Valid{"Signed with this session, for this viewer,<br/>under an hour old, and they hold flags?"}
  Valid -- "yes" --> Merge["Previewed flags replace theirs,<br/>with a banner on every page"]
  Merge --> Locals
  Valid -- "no" --> Drop["Delete the cookie"]
  Drop --> Locals
```

Source: [`src/hooks.server.ts`](../../src/hooks.server.ts), [`src/lib/server/flags/preview.ts`](../../src/lib/server/flags/preview.ts), [`src/lib/server/services/flags.ts`](../../src/lib/server/services/flags.ts), [`src/routes/+layout.server.ts`](../../src/routes/+layout.server.ts), [`src/routes/+layout.svelte`](../../src/routes/+layout.svelte)

## getFlags

How locallibrary learns the flags, since the staff space is private. The answer is each flag's gates, and the caller decides with the same rule; [`flags/vectors.json`](../../../../flags/vectors.json) holds both sides to it.

```mermaid
sequenceDiagram
  participant C as locallibrary, or anyone
  participant A as App
  C->>A: GET /xrpc/computer.localnetwork.flags.getFlags
  alt No Authorization header
    A-->>C: Every flag's gates, targeted false,<br/>public for 60 seconds
  else Over the flags rate limit for this IP
    A-->>C: 429, retry after 30 seconds
  else Service token expired, without lxm, or<br/>wrong on audience, lxm or signature
    A-->>C: 401
  else Verified
    A-->>C: The gates, with targeted for the<br/>token's issuer, no-store
  end
```

Source: [`src/routes/xrpc/computer.localnetwork.flags.getFlags/+server.ts`](../../src/routes/xrpc/computer.localnetwork.flags.getFlags/+server.ts), [`src/lib/server/flags/get-flags.ts`](../../src/lib/server/flags/get-flags.ts), [`src/lib/server/atproto/service-auth.ts`](../../src/lib/server/atproto/service-auth.ts), [`flags/README.md`](../../../../flags/README.md)

## Changing flags

What `/admin/flags` writes. Admins hold both grants; a moderator holds what an admin gives them on `/admin/staff`. Each write is audited, and saving a flag or a group unchanged writes nothing.

```mermaid
flowchart TD
  Flag["/admin/flags/[key]"] -- "save, needs flags" --> PutFlag["Put staff.flag in the staff space,<br/>keyed by the flag"]
  Flag -- "preview on, off or clear,<br/>needs flags" --> Preview["ln_flag_preview for an hour,<br/>audited as flag.preview"]
  Groups["/admin/flags/groups"] -- "create or edit,<br/>needs flagGroups" --> PutGroup["Put staff.group in the staff space"]
  Groups -- "delete, needs flagGroups" --> InUse{"A flag targets it?"}
  InUse -- "yes" --> Refuse["Refused: take it off those flags first"]
  InUse -- "no" --> Delete["Delete staff.group"]
  PutFlag --> Index["feature_flag and flag_group,<br/>memory cleared"]
  PutGroup --> Index
  Delete --> Index
  Sync["Sync of the root's records<br/>in the staff space"] --> Index
```

Source: [`src/routes/admin/flags/+page.server.ts`](../../src/routes/admin/flags/+page.server.ts), [`src/routes/admin/flags/[key]/+page.server.ts`](../../src/routes/admin/flags/[key]/+page.server.ts), [`src/routes/admin/flags/groups/+page.server.ts`](../../src/routes/admin/flags/groups/+page.server.ts), [`src/routes/admin/flags/groups/[rkey]/+page.server.ts`](../../src/routes/admin/flags/groups/[rkey]/+page.server.ts), [`src/lib/server/forms/flags.ts`](../../src/lib/server/forms/flags.ts), [`src/lib/server/services/flags.ts`](../../src/lib/server/services/flags.ts), [`src/lib/server/services/staff.ts`](../../src/lib/server/services/staff.ts), [`src/lib/server/sync/projections.ts`](../../src/lib/server/sync/projections.ts)

## Flag tables

The local index of the staff space's flag records, and the grants on staff roles. None of the lines are foreign keys: groups and accounts sit in JSON arrays.

```mermaid
erDiagram
  feature_flag }o--o{ flag_group : "group_rkeys"
  staff_role {
    text did PK
    text role
    text grants
  }
  feature_flag {
    text key PK
    integer enabled
    integer everyone
    integer percentage
    text group_rkeys
    text dids
    text uri
  }
  flag_group {
    text rkey PK
    text name
    text members
    text uri
  }
```

Source: [`src/lib/server/db/migrations.ts`](../../src/lib/server/db/migrations.ts), [`src/lib/server/db/schema.ts`](../../src/lib/server/db/schema.ts)
