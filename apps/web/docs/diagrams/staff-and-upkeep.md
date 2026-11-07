# The website diagrams: Staff and upkeep

Bug reports, the audit log, staff actions, listings and backups. The rest are listed in the [README](../../README.md#diagrams).

- [Bug report flow](#bug-report-flow)
- [Audit delivery](#audit-delivery)
- [Audit copy lifecycle](#audit-copy-lifecycle)
- [Staff actions](#staff-actions)
- [Bluesky verifications](#bluesky-verifications)
- [Records the root holds](#records-the-root-holds)
- [People in shared spaces](#people-in-shared-spaces)
- [Public listings](#public-listings)
- [Backup and restore](#backup-and-restore)
- [Link previews and page heads](#link-previews-and-page-heads)

## Bug report flow

Where "Report a bug" goes. A report that goes to userinput.app comes from the person's own account.

```mermaid
flowchart TD
  Link["/report"] --> Ready{"Signed in, with<br/>USERINPUT_SPACE set?"}
  Ready -- "no" --> Github["GitHub's bug form, prefilled"]
  Ready -- "yes" --> Scope{"Session allows<br/>posting?"}
  Scope -- "no" --> Allow["Sign in again for<br/>app.userinput.discussion"]
  Allow --> Form["The form"]
  Scope -- "yes" --> Form
  Form --> Pin["Current space version from HappyView"]
  Pin --> Post["app.userinput.discussion in their repo,<br/>tagged bug and web"]
  Post -- "posted" --> There["Link to it on userinput.app"]
  Post -- "failed" --> Github
```

Source: [`src/routes/report/+page.server.ts`](../../src/routes/report/+page.server.ts), [`src/lib/server/services/bug-reports.ts`](../../src/lib/server/services/bug-reports.ts), [`src/lib/bug-report.ts`](../../src/lib/bug-report.ts)

## Audit delivery

How one action becomes a chained entry and two copies.

```mermaid
sequenceDiagram
  participant S as Service
  participant L as AuditLog
  participant D as SQLite
  participant R as Root's copy
  participant U as Actor's copy
  S->>L: record
  L->>D: Next seq, hash over<br/>the previous hash
  L->>R: Write to the audit space<br/>with the chain fields
  alt Actor has spaces, and isn't the root
    L->>U: Write with their session,<br/>same key
  end
  L->>D: Copy statuses, next attempt
  loop Every 15 s for what's due
    L->>R: Retry
    L->>U: Retry
    Note over L,U: Each pending copy, from 5 s doubling<br/>to an hour, 12 attempts by default
  end
  R-->>D: Sync reads the copy<br/>back and compares it
  U-->>D: Sync reads the copy<br/>back and compares it
```

Source: [`src/lib/server/audit/log.ts`](../../src/lib/server/audit/log.ts), [`src/lib/server/audit/chain.ts`](../../src/lib/server/audit/chain.ts), [`src/lib/server/sync/projections.ts`](../../src/lib/server/sync/projections.ts)

## Audit copy lifecycle

The status of one copy of an entry. If the database is lost, entries come back from the root's copies: the root's copy as verified, the actor's as pending until sync finds it.

```mermaid
stateDiagram-v2
  [*] --> pending: the root's copy, or an actor with spaces
  [*] --> self: the actor is the root
  [*] --> unsupported: the actor has no spaces
  pending --> written: write works
  pending --> pending: retry
  pending --> failed: 12 attempts
  pending --> verified: sync finds a match
  written --> verified: sync finds a match
  failed --> verified: sync finds a match
  written --> failed: copy differs or is deleted
  verified --> failed: copy differs or is deleted
```

Source: [`src/lib/server/audit/log.ts`](../../src/lib/server/audit/log.ts), [`src/lib/server/db/schema.ts`](../../src/lib/server/db/schema.ts), [`src/lib/server/sync/projections.ts`](../../src/lib/server/sync/projections.ts)

## Staff actions

What each part of the staff console changes. Changes to records go in the audit log.

```mermaid
flowchart LR
  Console["Staff console"]
  Console -- "/admin/requests" --> T1["Decision, and the performer<br/>or listing it publishes"]
  Console -- "/admin/projects" --> T2["Performer records in the root's repo,<br/>booking details in the industry space"]
  Console -- "/admin/listings/[did]" --> T3["Deletes listing.profile"]
  Console -- "/admin/spaces" --> T4["People records,<br/>list and unlist"]
  Console -- "/admin/staff" --> T5["staff.role, and membership<br/>of the four root spaces"]
  Console -- "/admin/verifications" --> T6["app.bsky.graph.verification<br/>in the root's repo"]
  Console -- "/admin/system" --> T7["Bootstrap, resync,<br/>retry audit delivery"]
  Console -- "/admin/audit" --> T8["Read only: filter, verify<br/>the chain, export JSONL"]
```

Source: [`src/routes/admin/+layout.server.ts`](../../src/routes/admin/+layout.server.ts), [`src/lib/server/services/staff.ts`](../../src/lib/server/services/staff.ts), [`src/lib/server/services/verifications.ts`](../../src/lib/server/services/verifications.ts), [`src/lib/server/services/project-admin.ts`](../../src/lib/server/services/project-admin.ts), [`src/lib/server/services/requests.ts`](../../src/lib/server/services/requests.ts)

## Bluesky verifications

How an admin verifies a manager's account from the root. Apps that trust the root show a badge while the handle and display name still match.

```mermaid
flowchart TD
  Claim["An approved claim from the act's<br/>own account or a member"] -- "link" --> Verify["/admin/verifications"]
  Manager["A manager on /admin/projects/[slug]"] -- "link" --> Verify
  Verify --> Admin["requireAdmin: 403 for anyone else"]
  Admin --> Who["Manages a performer,<br/>or is verified already?"]
  Who -- "no" --> Refused["Only project managers can be verified"]
  Who -- "yes" --> Live["Handle from their DID document,<br/>display name from HappyView"]
  Live -- "either is missing,<br/>or nothing changed" --> Stop["Error on the form"]
  Live --> Write["Root writes the verification,<br/>a re-issue over the version HappyView serves"]
  Write --> Wait["Waits for HappyView, audits it"]
  Admin -- "revoke" --> Delete["Root deletes each one it issued<br/>for them, audits it"]
```

Source: [`src/lib/server/services/verifications.ts`](../../src/lib/server/services/verifications.ts), [`src/routes/admin/verifications/+page.server.ts`](../../src/routes/admin/verifications/+page.server.ts), [`src/routes/admin/requests/[rkey]/+page.server.ts`](../../src/routes/admin/requests/[rkey]/+page.server.ts), [`src/routes/admin/projects/[slug]/+page.server.ts`](../../src/routes/admin/projects/[slug]/+page.server.ts)

## Records the root holds

How someone deletes what the root wrote for them at `/dashboard/data`. Audit entries and booking status changes aren't offered.

```mermaid
flowchart TD
  Pick["One record, or a kind at once"] --> Limit["Per-account write limit"]
  Limit --> Check["Do the index and the record's<br/>onBehalfOf both name them?"]
  Check -- "no" --> No["That isn't one of yours"]
  Check -- "yes" --> Delete["Root deletes the record"]
  Delete -- "a decided request" --> Keep["Row stays without its content,<br/>so the decision stands"]
  Delete -- "anything else" --> Gone["Row goes"]
  Keep --> Audit["Audit record.delete"]
  Gone --> Audit
```

Source: [`src/lib/server/services/held-records.ts`](../../src/lib/server/services/held-records.ts), [`src/routes/dashboard/data/+page.server.ts`](../../src/routes/dashboard/data/+page.server.ts)

## People in shared spaces

How people join a performer, party or venue space. `SpaceJoins` and the self-service parts of `SharedSpaces` exist without pages; today only the staff console adds, changes and removes people. No page starts a space yet either: `SharedSpaces.create`, which adds its first person, has no caller.

```mermaid
flowchart TD
  Staff["Staff, at /admin/spaces"] --> Person["people.person, written by the root"]
  Invite["Invite to an account, 30 days"] -- "accepted" --> Person
  Link["Invite link, 7 days, token hashed,<br/>never people.manage"] -- "opened" --> Person
  Ask["Request to join"] -- "people.manage approves" --> Person
  Person --> Members["Members follow people:<br/>everyone reads, writers write"]
  Person --> Listing["Public listing refreshed"]
  Person --> Audit["Audit people.*"]
```

Source: [`src/lib/server/services/shared-spaces.ts`](../../src/lib/server/services/shared-spaces.ts), [`src/lib/server/services/space-joins.ts`](../../src/lib/server/services/space-joins.ts), [`src/routes/admin/spaces/[kind]/[skey]/+page.server.ts`](../../src/routes/admin/spaces/[kind]/[skey]/+page.server.ts)

## Public listings

How a listed act, party or venue would reach the directory. No kind has a public page yet, so nothing is listed.

```mermaid
flowchart TD
  Status["Staff list or unlist:<br/>listing.status in the space"] --> Refresh["ListingWriter.refresh"]
  People["A people record syncs"] --> Refresh
  Hourly["Startup and hourly reconcile"] --> Refresh
  Refresh --> Source{"Listed, with a<br/>page to show?"}
  Source -- "no" --> Delete["Delete any listing"]
  Source -- "yes" --> Build["Public fields, plus people in a<br/>listed role who haven't hidden"]
  Build --> Same{"Differs from<br/>the published one?"}
  Same -- "no" --> Skip["No write"]
  Same -- "yes" --> Put["Root writes computer.localnetwork.listing"]
```

Source: [`src/lib/server/services/listings.ts`](../../src/lib/server/services/listings.ts), [`src/lib/server/sync/projections.ts`](../../src/lib/server/sync/projections.ts), [`../../docs/adr/0003-profiles-in-repos-performers-in-spaces.md`](../../../../docs/adr/0003-profiles-in-repos-performers-in-spaces.md)

## Backup and restore

The database's daily backup and how it comes back. See [Backups](../../README.md#backups).

```mermaid
flowchart TD
  Check["Startup and hourly: runIfDue"] --> Due{"None yet this UTC day?"}
  Due -- "no" --> Wait["Next hour"]
  Due -- "yes" --> Copy["SQLite online backup to a temp file"]
  Copy --> Integrity["Integrity check, gzip"]
  Integrity --> Upload["localnetwork-db/[time].sqlite.gz"]
  Upload --> Prune["Delete past BACKUP_KEEP_DAILY<br/>and BACKUP_KEEP_WEEKLY"]
  Copy -- "any step fails" --> Status["Logged, shown on the System page,<br/>tried again next hour"]
  Restore["scripts/db-backup.ts restore"] --> Download["Download, gunzip, integrity check"]
  Download --> Keep["Keep the old one as<br/>localnetwork.db.before-restore"]
  Keep --> Write["Write in with online backup,<br/>then restart the app"]
```

Source: [`src/lib/server/backup/backups.ts`](../../src/lib/server/backup/backups.ts), [`src/lib/server/backup/retention.ts`](../../src/lib/server/backup/retention.ts), [`scripts/db-backup.ts`](../../scripts/db-backup.ts)

## Link previews and page heads

What a public page tells crawlers and link previews.

```mermaid
flowchart TD
  Load["A public page's load"] --> Meta["meta: title, description,<br/>preview image"]
  Load -- "listed in search" --> Records["meta-records: at:canonical,<br/>at:alternate, at:author"]
  Load -- "listed in search" --> Json["JSON-LD: performer, press kit,<br/>profile, mix, artist"]
  Records --> Head["crawlerHead"]
  Json --> Head
  Head --> Hook["Server hook swaps it for the<br/>marker in app.html"]
  Meta -- "a profile's picture" --> Avatar["/media/[did]/[cid]"]
  Meta -- "other pages" --> Card["/og/... PNG"]
  Card --> Cached{"Kept in memory?"}
  Cached -- "yes" --> Png["Served"]
  Cached -- "no" --> Draw["Drawn from a Svelte card, then kept,<br/>or 503 when too many are waiting"]
  Draw --> Png
```

Source: [`src/lib/server/meta.ts`](../../src/lib/server/meta.ts), [`src/lib/server/meta-records.ts`](../../src/lib/server/meta-records.ts), [`src/lib/server/structured-data.ts`](../../src/lib/server/structured-data.ts), [`src/lib/server/crawler-head.ts`](../../src/lib/server/crawler-head.ts), [`src/lib/server/og/serve.ts`](../../src/lib/server/og/serve.ts), [`src/hooks.server.ts`](../../src/hooks.server.ts)
