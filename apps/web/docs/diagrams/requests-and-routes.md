# The website diagrams: Requests and routes

The request path, errors, rate limits, and every route family. The rest are listed in the [README](../../README.md#diagrams).

- [Request path](#request-path)
- [Page loads and form actions](#page-loads-and-form-actions)
- [Response caching](#response-caching)
- [Error paths](#error-paths)
- [Rate limits](#rate-limits)
- [Route families](#route-families)
- [Public pages](#public-pages)
- [Dashboard routes](#dashboard-routes)
- [Staff console routes](#staff-console-routes)
- [Endpoints](#endpoints)
- [Smaller pages and endpoints](#smaller-pages-and-endpoints)

## Request path

What runs for each request, in order. SvelteKit makes its origin check before the `handle` hook.

```mermaid
flowchart TD
  In["Request"] --> Csrf["SvelteKit's origin check"]
  Csrf -- "form post from<br/>another origin" --> Blocked["403"]
  Csrf --> Tool["Tool host check"]
  Tool -- "a tool's own host" --> ToolSite["toolSite answers"]
  Tool --> Load["Get the app"]
  Load -- "it can't start" --> Setup["503 setup page"]
  Load --> Limit["Write limit for this address"]
  Limit -- "over it" --> Slow["429"]
  Limit --> Session["ln_session to a viewer,<br/>skipped on /xrpc/"]
  Session --> Flags["Flags for the viewer, a signed-out<br/>visitor's on /xrpc/, and a staff preview<br/>from ln_flag_preview off it"]
  Flags --> Guard["guardArea by route"]
  Guard -- "/dashboard or /admin,<br/>signed out" --> Login["303 to /login?next="]
  Guard -- "/admin, not staff" --> Refused["403"]
  Guard --> Route["Load, action or endpoint,<br/>page CSP with a nonce"]
  Route --> Out["gzip, security headers,<br/>noindex on unlisted areas,<br/>the one log line for the request"]
```

Source: [`src/hooks.server.ts`](../../src/hooks.server.ts), [`src/lib/server/http.ts`](../../src/lib/server/http.ts), [`src/lib/server/services/flags.ts`](../../src/lib/server/services/flags.ts), [`src/lib/server/compress.ts`](../../src/lib/server/compress.ts), [`src/lib/server/logger.ts`](../../src/lib/server/logger.ts), [`vite.config.ts`](../../vite.config.ts)

## Page loads and form actions

How a page gets its data, and how a form changes something. Layouts and the page load run alongside each other, unless a load awaits `parent()`, so the `handle` hook does the guarding.

```mermaid
flowchart TD
  Page["Page request"] --> Root["Root layout: a session that has to<br/>sign in again goes to /login"]
  Page --> Area["Area layout: dashboard<br/>or staff console"]
  Page --> Load["Page load"]
  Load --> Now["What the page needs at once"]
  Load --> Stream["Slow directory reads,<br/>streamed in after the page"]
  Post["Form post"] --> Action["Form action checks the viewer again,<br/>since actions skip the layouts"]
  Action --> Act["act() runs a service"]
  Act --> Write["Writes a record"]
  Write -- "space record" --> Index["Indexes it locally"]
  Write -- "public record" --> Wait["Waits for HappyView to serve it"]
  Write -- "planner record" --> Audit["Audit entry"]
  Index --> Audit
  Wait --> Audit
  Audit --> Result["Page data or a redirect"]
```

Source: [`src/routes/+layout.server.ts`](../../src/routes/+layout.server.ts), [`src/routes/dashboard/+layout.server.ts`](../../src/routes/dashboard/+layout.server.ts), [`src/lib/server/manage.ts`](../../src/lib/server/manage.ts), [`src/lib/server/http.ts`](../../src/lib/server/http.ts)

## Response caching

What each response lets a browser or a shared cache keep. Pages that show the same thing to everyone use `cacheBriefly`.

```mermaid
flowchart LR
  Responses["Cache-Control"]
  Responses -- "/charts, /artists pages: 60 s" --> Brief["cacheBriefly: public with no session<br/>cookie, else private, Vary: Cookie"]
  Responses -- "an active performer's<br/>mix page: 5 minutes" --> Brief
  Responses -- "/media, recordings, most /og cards" --> Year["public, a year, immutable"]
  Responses -- "/og artist cards, /og/site.png" --> Hour["public, an hour and a day"]
  Responses -- "public press kit photos" --> Day["public, a day, immutable"]
  Responses -- "private and draft photos, request avatars" --> Private["private, an hour"]
  Responses -- "/api/actors, /api/places" --> Api["private, 5 minutes"]
  Responses -- "player.json, chapters.json" --> Sets["public, 1 and 5 minutes"]
  Responses -- "client metadata, did.json" --> Docs["public, 10 and 5 minutes"]
  Responses -- "getFlags without a token" --> Flags["public, a minute,<br/>Vary: Authorization"]
  Responses -- "/healthz, token exchange, audit export,<br/>getFlags with a token" --> Never["no-store"]
```

Source: [`src/lib/server/http.ts`](../../src/lib/server/http.ts), [`src/routes/charts/+page.server.ts`](../../src/routes/charts/+page.server.ts), [`src/routes/projects/[slug]/mixes/[n]/+page.server.ts`](../../src/routes/projects/[slug]/mixes/[n]/+page.server.ts), [`src/routes/media/[did]/[cid]/+server.ts`](../../src/routes/media/[did]/[cid]/+server.ts), [`src/lib/server/og/serve.ts`](../../src/lib/server/og/serve.ts), [`src/routes/projects/[slug]/press-kits/[rkey]/photos/[cid]/+server.ts`](../../src/routes/projects/[slug]/press-kits/[rkey]/photos/[cid]/+server.ts), [`src/routes/api/places/+server.ts`](../../src/routes/api/places/+server.ts), [`src/routes/sets/[did]/[rkey]/player.json/+server.ts`](../../src/routes/sets/[did]/[rkey]/player.json/+server.ts), [`src/lib/server/flags/get-flags.ts`](../../src/lib/server/flags/get-flags.ts)

## Error paths

What each kind of failure turns into.

```mermaid
flowchart LR
  Thrown["Something throws"]
  Thrown -- "UserError in an action" --> Fail["fail() with its status,<br/>shown on the form"]
  Thrown -- "ReauthError in an action" --> Again["fail(401), the form<br/>offers Sign in again"]
  Thrown -- "bad record in an action" --> Invalid["fail(400)"]
  Thrown -- "HTTP error or redirect" --> Kit["SvelteKit answers it"]
  Thrown -- "HappyViewError" --> Directory["503: the directory<br/>isn't answering"]
  Thrown -- "anything else" --> Ours["500, logged with<br/>its request id"]
  Directory --> Page["Error page"]
  Ours --> Page
  Listing["HappyView down on a<br/>page that lists records"] -- "unlessOffline" --> Offline["The page says<br/>it's offline"]
```

Source: [`src/hooks.server.ts`](../../src/hooks.server.ts), [`src/lib/server/http.ts`](../../src/lib/server/http.ts), [`src/lib/server/core.ts`](../../src/lib/server/core.ts), [`src/lib/server/happyview/offline.ts`](../../src/lib/server/happyview/offline.ts)

## Rate limits

Every token bucket the app sets. They're held in memory, so a restart resets them.

```mermaid
flowchart LR
  Limits["Rate limits"]
  Limits -- "write: 30 a minute" --> W1["POST, PUT, PATCH and DELETE,<br/>except /xrpc/, per address"]
  Limits -- "write: 30 a minute" --> W2["Space joins and held record<br/>deletes, per account"]
  Limits -- "login: 10 a minute" --> L1["/login and /login/create,<br/>per address"]
  Limits -- "flags: 60, then 1 a second" --> F1["getFlags with a token,<br/>per address"]
  Limits -- "search: 60, then 2 a second" --> S1["/api/actors and /api/places,<br/>per address"]
  Limits -- "profileMisses: 30 in 10 minutes" --> M1["/@[handle] and its set pages<br/>naming no account, per address"]
  Limits -- "optIn: 5 in 10 minutes" --> O1["Turning email or DMs on,<br/>per account"]
  Limits -- "testSend: 3 an hour" --> T1["Test messages, per account"]
  Limits -- "email confirmations" --> C1["5 a day per account, 3 a day per<br/>account and email, 10 a day per<br/>email, 100 an hour overall"]
```

Source: [`src/lib/server/app.ts`](../../src/lib/server/app.ts), [`src/lib/server/rate-limit.ts`](../../src/lib/server/rate-limit.ts), [`src/lib/server/flags/get-flags.ts`](../../src/lib/server/flags/get-flags.ts), [`src/lib/server/profile-page.ts`](../../src/lib/server/profile-page.ts), [`src/lib/server/notifications/channels.ts`](../../src/lib/server/notifications/channels.ts)

## Route families

Who can reach each family of routes.

```mermaid
flowchart LR
  Routes["Routes"]
  Routes -- "anyone" --> Public["Public pages, media,<br/>previews, players"]
  Routes -- "anyone" --> Signin["/login, /oauth/callback"]
  Routes -- "signed-in account" --> Dash["/dashboard, Follow,<br/>Save, /report"]
  Routes -- "a performer's managers" --> Manage["/dashboard/projects/[slug]"]
  Routes -- "a thread's participants" --> Thread["/dashboard/bookings/[skey]"]
  Routes -- "moderators and admins" --> Console["/admin"]
  Routes -- "admins only" --> Edit["/admin/staff, /admin/system,<br/>/admin/verifications,<br/>performer edits"]
  Routes -- "moderators an admin gave<br/>a grant, and admins" --> Flags["/admin/flags"]
  Routes -- "PDSes and locallibrary" --> Xrpc["/xrpc/*, with service auth,<br/>getFlags without it too"]
  Routes -- "anyone, for locallibrary's<br/>loopback sign-ins" --> Exchange["/api/token-exchange/[service]"]
```

Source: [`src/lib/server/http.ts`](../../src/lib/server/http.ts), [`src/lib/server/manage.ts`](../../src/lib/server/manage.ts), [`src/lib/server/services/bookings.ts`](../../src/lib/server/services/bookings.ts), [`src/routes/admin/+layout.server.ts`](../../src/routes/admin/+layout.server.ts), [`src/lib/server/forms/flags.ts`](../../src/lib/server/forms/flags.ts), [`src/lib/server/flags/get-flags.ts`](../../src/lib/server/flags/get-flags.ts)

## Public pages

The pages anyone can read. A performer that isn't active shows only to its managers and staff; everyone else gets a 404.

```mermaid
flowchart LR
  Public["Public pages"]
  Public --> Home["/, /for-djs, /about, /privacy"]
  Public --> Find["/projects, /match"]
  Public --> Performer["/projects/[slug]<br/>/projects/[slug]/mixes/[n]<br/>/projects/[slug]/press-kits<br/>/projects/[slug]/press-kits/[rkey]"]
  Public --> Profile["/@[handle]<br/>/@[handle]/sets/[rkey]"]
  Public --> Shows["/shows"]
  Public --> Music["/charts, /artists,<br/>/artists/[id], /artists/name/[[name]]"]
  Public --> Tools["/tools, /tools/[tool],<br/>/tools/[tool]/docs/[page]"]
  Public --> Preview["/mixes/preview"]
  Public --> Signin["/login, /login/create"]
  Public --> Mail["/notifications/confirm,<br/>/notifications/unsubscribe"]
  Public --> Report["/report, which sends signed-out<br/>visitors to GitHub"]
```

Source: [`src/routes/projects/[slug]/+page.server.ts`](../../src/routes/projects/[slug]/+page.server.ts), [`src/lib/server/project-page.ts`](../../src/lib/server/project-page.ts), [`src/routes/@[handle]/sets/[rkey]/+page.server.ts`](../../src/routes/@[handle]/sets/[rkey]/+page.server.ts), [`src/routes/report/+page.server.ts`](../../src/routes/report/+page.server.ts), [`src/lib/server/http.ts`](../../src/lib/server/http.ts)

## Dashboard routes

Everything under `/dashboard`. The server hook turns away anyone signed out; pages narrow it from there.

```mermaid
flowchart LR
  Dash["/dashboard<br/>any signed-in account"]
  Dash --> Over["/activity, /profile, /data"]
  Dash --> Inbox["/inbox, /inbox/settings"]
  Dash --> Req["/requests, /requests/[rkey]<br/>the requester or staff"]
  Dash --> Book["/bookings, /bookings/new,<br/>/bookings/[skey] for participants"]
  Dash --> Promoter["/promoter, in the sidebar<br/>once promoting is on"]
  Dash --> Plan["/planner, /planner/shows/[rkey]<br/>needs spaces"]
  Dash --> Claim["/claim/[slug], /projects/new"]
  Dash --> Access["/access/[slug]/[rkey]"]
  Dash --> Manage["/projects/[slug]<br/>the performer's managers"]
  Manage --> Mine["/profile, /shows/[rkey]"]
  Manage --> Kits["/press-kits, /press-kits/new,<br/>/press-kits/[rkey], .../access"]
```

Source: [`src/routes/dashboard/+layout.server.ts`](../../src/routes/dashboard/+layout.server.ts), [`src/routes/dashboard/projects/[slug]/+layout.server.ts`](../../src/routes/dashboard/projects/[slug]/+layout.server.ts), [`src/routes/dashboard/requests/[rkey]/+page.server.ts`](../../src/routes/dashboard/requests/[rkey]/+page.server.ts), [`src/lib/server/planner-page.ts`](../../src/lib/server/planner-page.ts)

## Staff console routes

Everything under `/admin`, by the guard each page or action uses. Services that act for a viewer check the role again.

```mermaid
flowchart LR
  Staff["requireStaff<br/>moderators and admins"]
  Staff --> Over["/admin"]
  Staff --> Requests["/admin/requests, /[rkey],<br/>/[rkey]/mixes/[n]"]
  Staff --> Projects["/admin/projects,<br/>/admin/projects/[slug]: status"]
  Staff --> Listings["/admin/listings/[did]"]
  Staff --> Spaces["/admin/spaces, /[kind]/[skey]"]
  Staff --> Audit["/admin/audit, /[id], /export"]
  Admin["requireAdmin<br/>admins"]
  Admin --> Edit["/admin/projects/[slug]:<br/>update, managers"]
  Admin --> New["/admin/projects/new"]
  Admin --> People["/admin/staff"]
  Admin --> System["/admin/system"]
  Admin --> Verify["/admin/verifications"]
  Grant["requireGrant<br/>admins, and moderators<br/>an admin gave the grant"]
  Grant -- "flags" --> Flags["/admin/flags, /[key]"]
  Grant -- "flagGroups" --> Groups["/admin/flags/groups, /[rkey]"]
```

Source: [`src/routes/admin/+layout.server.ts`](../../src/routes/admin/+layout.server.ts), [`src/routes/admin/projects/[slug]/+page.server.ts`](../../src/routes/admin/projects/[slug]/+page.server.ts), [`src/routes/admin/system/+page.server.ts`](../../src/routes/admin/system/+page.server.ts), [`src/routes/admin/staff/+page.server.ts`](../../src/routes/admin/staff/+page.server.ts), [`src/routes/admin/verifications/+page.server.ts`](../../src/routes/admin/verifications/+page.server.ts), [`src/lib/server/forms/flags.ts`](../../src/lib/server/forms/flags.ts), [`src/routes/admin/flags/+page.server.ts`](../../src/routes/admin/flags/+page.server.ts), [`src/routes/admin/flags/groups/+page.server.ts`](../../src/routes/admin/flags/groups/+page.server.ts)

## Endpoints

Routes that answer data rather than a page. [Smaller pages and endpoints](#smaller-pages-and-endpoints) has the redirects and `/healthz`.

```mermaid
flowchart LR
  Ep["Endpoints"]
  Ep --> Media["/media/[did]/[cid]<br/>/media/audio/[did]/[cid]"]
  Ep --> Kit["Press kit photos, private ones<br/>for managers and people granted"]
  Ep --> Sets["/sets/[did]/[rkey]/<br/>player.json, chapters.json"]
  Ep --> Og["/og/*.png"]
  Ep --> Api["/api/actors, /api/places,<br/>/api/token-exchange/[service]"]
  Ep --> Docs["/oauth-client-metadata.json<br/>/.well-known/did.json"]
  Ep --> Session["/oauth/callback,<br/>POST /logout"]
  Ep --> Xrpc["/xrpc: notifyWrite,<br/>notifySpaceDeleted,<br/>directory.requestIndex,<br/>flags.getFlags"]
  Ep --> Signed["Viewer checked: dashboard<br/>photos, request avatars,<br/>/admin/audit/export"]
```

Source: [`src/routes/media/[did]/[cid]/+server.ts`](../../src/routes/media/[did]/[cid]/+server.ts), [`src/routes/media/audio/[did]/[cid]/+server.ts`](../../src/routes/media/audio/[did]/[cid]/+server.ts), [`src/routes/sets/[did]/[rkey]/player.json/+server.ts`](../../src/routes/sets/[did]/[rkey]/player.json/+server.ts), [`src/routes/og/site.png/+server.ts`](../../src/routes/og/site.png/+server.ts), [`src/routes/api/actors/+server.ts`](../../src/routes/api/actors/+server.ts), [`src/routes/xrpc/computer.localnetwork.flags.getFlags/+server.ts`](../../src/routes/xrpc/computer.localnetwork.flags.getFlags/+server.ts), [`src/routes/epk/[did]/[rkey]/+server.ts`](../../src/routes/epk/[did]/[rkey]/+server.ts), [`src/routes/healthz/+server.ts`](../../src/routes/healthz/+server.ts), [`src/routes/.well-known/did.json/+server.ts`](../../src/routes/.well-known/did.json/+server.ts)

## Smaller pages and endpoints

What the small routes do, beside the flows above.

```mermaid
flowchart LR
  Small["Small routes"]
  Small -- "/dashboard/activity" --> A["The viewer's own audit<br/>entries, 50 a page"]
  Small -- "/projects/[slug]/epk" --> B["308 to its press kits"]
  Small -- "/epk/[did]/[rkey] and its photos" --> C["Finds the press kit, 308 to<br/>its page, or 404"]
  Small -- "/sets/[did]/[rkey]/chapters.json" --> D["The set's tracklist as<br/>Podcasting 2.0 chapters"]
  Small -- "/api/actors" --> E["Bluesky's typeahead, kept<br/>10 minutes in memory"]
  Small -- "/healthz" --> F["200, with HappyView<br/>and backup status"]
  Small -- "/for-djs" --> G["Streams an example listing and<br/>the directory's totals"]
```

Source: [`src/routes/dashboard/activity/+page.server.ts`](../../src/routes/dashboard/activity/+page.server.ts), [`src/routes/projects/[slug]/epk/+server.ts`](../../src/routes/projects/[slug]/epk/+server.ts), [`src/lib/server/epk-page.ts`](../../src/lib/server/epk-page.ts), [`src/routes/sets/[did]/[rkey]/chapters.json/+server.ts`](../../src/routes/sets/[did]/[rkey]/chapters.json/+server.ts), [`src/lib/server/actor-search.ts`](../../src/lib/server/actor-search.ts), [`src/routes/healthz/+server.ts`](../../src/routes/healthz/+server.ts), [`src/routes/for-djs/+page.server.ts`](../../src/routes/for-djs/+page.server.ts)
