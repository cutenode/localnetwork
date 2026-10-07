# Local Network

Promoters find local DJs who fit their shows. DJs (called _projects_: a solo DJ, a b2b duo, a group, or a collective) get a profile built around what promoters book on: sound, distance, slot, room size, and budget.

All data lives on the AT Protocol. Public profiles are ordinary atproto records; everything private (requests, staff roles, the audit log, booking threads, personal planners) lives in [atproto spaces](https://atproto.com/blog/atproto-spaces-alpha), the protocol's permissioned-data extension. Published projects are indexed by [HappyView](https://happyview.dev), an AppView whose Lua scripts answer directory search and the show matcher. The app keeps its own SQLite index only for space data, and can rebuild it from the network.

> Atproto spaces is an alpha (released 2026). This app is built against `@atproto/*@0.0.0-spaces-alpha-20261001173819` and the [0016 spaces proposal](https://github.com/bluesky-social/proposals/tree/main/0016-permissioned-data). Expect breaking changes upstream; all versions are pinned.

## Features

### Promoters

- Directory search by text, genre family (a search for "house" finds deep house and afro house), distance from a venue, fee tier, slot, format, and type.
- Show matcher: describe the night and get a ranked shortlist, scored on sound (40%), distance (25%), budget (15%), room size, slot, and booth setup, with the reasons for each match.
- Booking inquiries open a private thread (its own space) shared only with the project's managers, with show details up front and a status flow (open → negotiating → confirmed / declined / cancelled).
- A private planner (saved projects and shows with working lineups) in a personal space on the promoter's own PDS.

### DJs (projects)

- List a new project, or claim one an admin added. Several managers per project (crews, agents).
- Profiles cover genres, city and travel radius, set types and lengths, formats, room sizes, fee tier and range, links, playable SoundCloud/Mixcloud/YouTube mixes, highlights, photo.
- A mix can carry a tracklist: a mix report exported from [locallibrary](../locallibrary) (`locallibrary mix … --export`), shown at `/projects/<slug>/mixes/<n>` with the waveform and track lanes, tempo, key moves on the Camelot wheel, and every transition. `/mixes/preview` renders an export locally without uploading it.
- Private booking details (email, phone, rates, tech rider, hospitality) visible only to signed-in members.
- Every claim and change is reviewed by a moderator, so profiles can't be hijacked.

### Staff

- Admins add and edit projects directly, change visibility, manage managers, and grant moderator/admin roles.
- Moderators review creates, claims, and changes with field-level diffs and stale-revision warnings. Nobody can review their own request.
- A complete audit log of every account's actions, with filters, per-entry detail, hash-chain verification, and JSONL export.
- System page: root spaces and policies, per-space and per-repo sync status, audit delivery backlog, HappyView status and script sync, one-click bootstrap/resync.

## How data is stored

| Data                                                  | Where                                                             | Who writes                           | Who reads             |
| ----------------------------------------------------- | ----------------------------------------------------------------- | ------------------------------------ | --------------------- |
| Published projects (`computer.localnetwork.project`)  | Root account's public repo                                        | Root, after admin action or approval | Everyone              |
| Requests (`computer.localnetwork.moderation.request`) | Requester's repo in the root's moderation space                   | The requester                        | Staff (via the app)   |
| Decisions                                             | Root's repo in the moderation space                               | Root                                 | Staff                 |
| Staff roles (`computer.localnetwork.staff.role`)      | Root's repo in the staff space                                    | Root                                 | Staff                 |
| Private booking details                               | Root's repo in the industry space                                 | Root                                 | Any signed-in account |
| Audit entries (`computer.localnetwork.audit.entry`)   | Both the actor's repo and the root's repo in the audit space      | Actor and root                       | Staff                 |
| Booking threads                                       | One booking space per thread; each party writes in their own repo | Promoter and managers                | Thread members only   |
| Planner                                               | A personal planner space under the promoter's own DID             | The promoter                         | The promoter only     |

Space policies (`com.atproto.simplespace`):

| Space                | Read policy                        | Write policy                                |
| -------------------- | ---------------------------------- | ------------------------------------------- |
| audit                | member list (staff)                | public (everyone records their own entries) |
| moderation           | member list (staff)                | public (everyone can file requests)         |
| staff                | member list (staff)                | member list (root)                          |
| industry             | public (any authenticated account) | member list (root)                          |
| booking (per thread) | member list (promoter + managers)  | member list                                 |
| planner (personal)   | member list (owner)                | member list                                 |

```mermaid
flowchart LR
  subgraph User PDS
    UR[user's space repos]
    PL[(planner space)]
  end
  subgraph Root PDS
    PUB[(public repo: projects)]
    RS[root's space repos]
    SH{{space host}}
  end
  App[Local Network app]
  App -- OAuth: createRecord in spaces --> UR
  App -- root session: putRecord / spaces --> PUB & RS
  UR -- notifyWrite --> SH
  SH -- notifyWrite --> App
  App -- space credential: listRepos / listRepoOps / getRepo --> SH & UR
  PUB -- relay + Jetstream --> HV[(HappyView)]
  App -- directory queries --> HV
```

### The audit log

Every action (sign-in/out, submissions, revisions, withdrawals, decisions, direct edits, visibility changes, manager and role changes, booking messages and status changes, planner changes, system bootstrap) is recorded:

1. Locally first, in an outbox row with a SHA-256 hash chain (`hash = sha256(canonical(entry ∪ {prev}))`).
2. In the actor's own repo inside the root's audit space, written with the actor's OAuth session.
3. In the root account's repo in the same space, with the same record key and the chain fields (`seq`, `prev`, `hash`).

Failed writes retry with exponential backoff. The syncer reads both copies back from the network and marks them _verified_ only if their content matches the chained entry; a copy edited later is flagged _failed_. If the local database is lost, the log is rebuilt from the root's chained copies. Denied attempts (e.g. a non-moderator trying to approve) are recorded with outcome `denied`. Booking message contents are never copied into the audit log, only metadata.

Accounts whose PDS doesn't run the spaces alpha can still sign in: the root files their requests and messages on their behalf (`onBehalfOf`), and their audit entries are marked "no spaces support".

### Mix tracklists

A tracklist is a JSON blob (`application/json`, at most 1 MB) on a mix in the project record, following the `locallibrary.mix-report` format, version 1 or 2 (schema in `src/lib/mix/report.ts`). It moves like the photo: uploaded to the requester's PDS with the request, previewed by moderators from there (`/admin/requests/<rkey>/mixes/<n>`), then validated, re-serialized (dropping unknown fields), and uploaded to the root's repo on approval. A tracklist that is already published is kept by CID without being fetched again. Published tracklists are read with `com.atproto.sync.getBlob` from the root's PDS, validated, and cached in memory. The format carries no file paths, comments, or ratings; cover art is embedded as small `data:` images.

### Sync

The app is a space syncer. For each watched space it mints a space credential (delegation token from the root's PDS, exchanged at the space host with an RFC 9421 HTTP message signature bound to a fresh P-256 key), enumerates writers with `listRepos`, pulls each repo's oplog with `listRepoOps`, and verifies the result against the author's signed LtHash commit, falling back to a full verified CAR download (`getRepo`) when needed. It registers for `notifyWrite` (verified with inter-service auth) and also polls. Records are validated against the lexicons, and authority rules are enforced: only root-authored decisions, roles, and details count, and booking records from non-participants are ignored.

### Directory

Published projects are served by [HappyView](https://happyview.dev) 2.16, not the app. HappyView follows Jetstream for `computer.localnetwork.project` records and answers the `computer.localnetwork.directory.*` queries (`lexicons/computer/localnetwork/directory/`) with Lua scripts in `lua/`:

- `project.lua` runs on every incoming record and keeps only the root account's, since Jetstream filters by collection and anyone can write one.
- `searchProjects`, `matchProjects`, `getProject`, `getProjects`, and `getStats` do the search, ranking, and lookups. Match results carry reason codes, which the app turns into text.
- The genre taxonomy and fee tiers are generated into the scripts from `src/lib/taxonomy/`, so TypeScript stays the source of truth.

With `HAPPYVIEW_API_KEY` set, the app uploads whichever scripts and lexicons changed when it boots (scripts first, so the record gate is in place before ingestion starts), then has HappyView re-read the root's repo and waits for that to finish. After publishing a project it waits (up to 5 seconds) for HappyView to index the new version, so the next page shows it.

## Getting started

Requirements: Node.js 24 (see `.node-version`), pnpm 12, and Docker (or OrbStack) for the local HappyView.

```bash
pnpm install
```

Start a local, spaces-enabled AT Protocol network (PLC directory + alpha PDS, root and demo accounts, published lexicons, 12 demo projects) and a HappyView container fed by a local Jetstream stand-in. It writes `.env.local`:

```bash
pnpm dev:network
```

In another terminal:

```bash
pnpm dev
```

Open http://127.0.0.1:5173 and sign in with a handle. The PDS asks for the password:

| Handle      | Password    | Role                                       |
| ----------- | ----------- | ------------------------------------------ |
| `ada.test`  | `ada-pass`  | admin (via `ADMIN_DIDS`)                   |
| `milo.test` | `milo-pass` | regular; make them a moderator under Staff |
| `neon.test` | `neon-pass` | DJ, manages two demo projects              |
| `pat.test`  | `pat-pass`  | promoter                                   |

The network is temporary; restarting it resets everything (and the dev database).

## Scripts

| Script                              | What it does                                                          |
| ----------------------------------- | --------------------------------------------------------------------- |
| `pnpm dev` / `pnpm dev:network`     | Dev server / local atproto network and HappyView                      |
| `pnpm build` / `pnpm start`         | Production build (adapter-node) / run it                              |
| `pnpm check`                        | `svelte-check` with TypeScript 6                                      |
| `pnpm lint`                         | Oxlint with type-aware rules (tsgolint) and the anti-slop plugin      |
| `pnpm format` / `pnpm format:check` | Oxfmt                                                                 |
| `pnpm test`                         | Unit tests (Node) + component tests (Vitest browser mode, Chromium)   |
| `pnpm test:integration`             | Full flows against an in-process spaces PDS and a HappyView container |
| `pnpm test:coverage`                | Unit + component + integration with `@vitest/coverage-v8`             |
| `pnpm test:e2e`                     | Playwright against an isolated network and dev server                 |
| `pnpm knip`                         | Unused files, exports, and dependencies                               |
| `pnpm lexicons:publish`             | Publish lexicons from the root account and check their DNS records    |
| `pnpm verify`                       | Everything CI runs, in order                                          |

Git hooks (lefthook) format and lint staged files on commit and run type checks and unit tests on push.

## Configuration

See [`.env.example`](.env.example) for every option. The essentials:

- `PUBLIC_URL`: the app's origin. OAuth client metadata (`/oauth-client-metadata.json`) and the syncer's `did:web` (`/.well-known/did.json`) are derived from it. Loopback URLs use an atproto loopback OAuth client.
- `ROOT_IDENTIFIER` / `ROOT_PASSWORD`: the Local Network root account. **Use a dedicated account and its account password.** In the alpha, `com.atproto.space.getDelegationToken` requires a full-access session, so app passwords cannot mint space credentials (the app explains this in its logs if you try).
- `ADMIN_DIDS`: accounts that are always admins. Everyone else is managed in-app.
- `HAPPYVIEW_URL`: the HappyView instance that serves the directory. Use its public URL: HappyView only answers the host its own `PUBLIC_URL` names. If HappyView is down, the app still starts, and pages that need projects say the directory is offline.
- `HAPPYVIEW_API_KEY`: a HappyView API key with `lexicons:read`, `lexicons:create`, `scripts:read`, `scripts:manage`, `backfill:create`, and `backfill:read`. Optional; without it, upload the lexicons and scripts yourself.
- `LOCALLIBRARY_HOST`: the host that serves [locallibrary](#locallibrarys-site)'s site (default `locallibrary.localnetwork.computer`).

## Deploying

1. Create the root account on a PDS that runs the spaces alpha (e.g. `ghcr.io/bluesky-social/atproto:pds-spaces-alpha`).
2. Publish the lexicons and add the printed `_lexicon` TXT records to the `localnetwork.computer` zone. The lexicons are `computer.localnetwork.*` (`src/lib/nsid.ts` and `lexicons/`), so there's one record per sub-namespace (`_lexicon.localnetwork.computer`, `_lexicon.space.localnetwork.computer`, and so on). PDSes resolve the space type declarations while authorizing OAuth requests, so sign-in fails with `invalid_scope` until they resolve. `computer.localnetwork.locallibrary.*` belongs to locallibrary; keep this app's lexicons out of it.
   ```bash
   pnpm lexicons:publish
   ```
3. Run HappyView 2.16 (`ghcr.io/gamesgamesgamesgamesgames/happyview:2.16.0`, SQLite on a volume) at `https://happyview.localnetwork.computer`, log in to its dashboard first (the first login becomes its admin), and create an API key with the permissions above.
4. Run the container at `https://localnetwork.computer` (`PUBLIC_URL`) with a persistent volume for `/data`:
   ```bash
   docker build -t localnetwork .
   ```
   ```bash
   docker run -p 3000:3000 -v localnetwork-data:/data --env-file .env localnetwork
   ```
5. Run **one** instance (SQLite and in-process background jobs). Back up `/data`: it holds OAuth sessions and the audit outbox; everything else can be re-synced from atproto.
6. Point `locallibrary.localnetwork.computer` at the same deployment (a DNS record, plus the custom domain on your host) for [locallibrary's site](#locallibrarys-site).

`/healthz` reports database, root-account, and HappyView status.

### locallibrary's site

[locallibrary](../locallibrary), the DJ drive tool's desktop app and CLI, signs people in with their Atmosphere accounts as `https://locallibrary.localnetwork.computer/oauth-client-metadata.json`. This app serves that host from `src/lib/server/locallibrary/` before anything else runs (so it works even while the app isn't configured): the client document at `/oauth-client-metadata.json` and a short page about locallibrary at `/`. Every other path on that host is a 404. It's separate from this app's own `/oauth-client-metadata.json`, which describes the Local Network web client; a desktop app needs a native client that redirects to `127.0.0.1`.

locallibrary writes the document; after changing its scopes, regenerate it here before releasing the locallibrary version that asks for them:

```bash
locallibrary account client-metadata -o src/lib/server/locallibrary/oauth-client-metadata.json
```

Until the host answers, locallibrary signs in as a local development app instead (servers ask about it every time).

## Security notes

- OAuth uses DPoP-bound tokens and granular scopes: write-only access to the user's own repos in the audit, moderation, and booking spaces, a self-authority planner space, and blob uploads limited to PNG, JPEG, and WebP avatars and JSON tracklists. Accounts without spaces support are asked for plain `atproto` only; they, and anyone who approves only part of the request, have the root account write on their behalf. Sessions granted before the app asked for a new scope are sent back through sign-in to approve it. Nothing lets the app read other users' data with a user's session; shared spaces are read with credentials issued to the root authority.
- Browser sessions are opaque random tokens stored as SHA-256 hashes; cookies are `HttpOnly`, `SameSite=Lax`, and `Secure` on HTTPS.
- CSP with per-request nonces, `frame-ancestors 'none'`, `nosniff`, strict referrer policy, HSTS on HTTPS, and SvelteKit's CSRF origin checks.
- Service endpoints from DID documents are checked against private address ranges in production (SSRF guard).
- Uploaded images are type- and magic-byte-checked and capped at 1 MB; images are served from a dedicated route with a locked-down CSP.
- Rate limits on sign-in, writes, and city search.
- Spaces are access control, not end-to-end encryption: the hosting PDSes and this app can read the data they handle.

## Project layout

```
lexicons/computer/localnetwork/ Lexicons: project record, space types, space records, permission set, directory queries
lua/                            HappyView scripts: the project record gate and the directory queries
scripts/                        dev-network (local atproto network), publish-lexicons, seed data
scripts/lib/                    Local HappyView container and Jetstream stand-in
src/lib/                        Shared code: matching, genre taxonomy, geo, formatting, components
src/lib/server/atproto/         Identity, OAuth, root session, repo clients, space credentials, service auth
src/lib/server/audit/           Hash-chained audit log with dual atproto delivery
src/lib/server/happyview/       HappyView client, script bundling, and boot sync
src/lib/server/locallibrary/    locallibrary's site: its OAuth client document and page, served on its own host
src/lib/server/services/        Projects, requests, staff, bookings, planner
src/lib/server/sync/            Space syncer and record projections
src/routes/                     SvelteKit 3 pages, form actions, and endpoints
tests/integration/              Flows against an in-process spaces PDS and a HappyView container
tests/e2e/                      Playwright
tools/oxlint/anti-slop/         Vendored anti-slop Oxlint plugin (provenance in its UPSTREAM.md)
```

## Known limitations

- Oxfmt doesn't format `.svelte` files yet; it formats everything else.
- The spaces alpha has no relay: sync load falls on PDSes, mitigated by notifications and incremental oplog sync.
- Account deactivation/deletion events arrive on the firehose, which this app doesn't subscribe to yet.
- The directory scripts query HappyView's SQLite directly, so the HappyView instance must use SQLite, not Postgres.
- HappyView drops a Jetstream cursor older than 72 hours. The boot backfill recovers new and changed projects after a longer outage, but not deletions (nothing deletes projects today).
