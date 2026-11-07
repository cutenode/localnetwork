# The website diagrams: Sign-in and roles

OAuth, scopes, sessions, token exchange, roles and capabilities. The rest are listed in the [README](../../README.md#diagrams).

- [Sign-in sequence](#sign-in-sequence)
- [Scope fallback](#scope-fallback)
- [Optional scope opt-in](#optional-scope-opt-in)
- [Who writes a record](#who-writes-a-record)
- [Browser session lifecycle](#browser-session-lifecycle)
- [Root account session](#root-account-session)
- [Token exchange flow](#token-exchange-flow)
- [Tool hosts](#tool-hosts)
- [Site roles](#site-roles)
- [Capabilities by role](#capabilities-by-role)

## Sign-in sequence

Signing in with an Atmosphere account. `/login/create` takes the same path with `prompt=create`. A rate limit or a handle that can't be found stays on the form.

```mermaid
sequenceDiagram
  participant B as Browser
  participant A as App
  participant P as Account's PDS
  participant D as SQLite
  B->>A: POST /login with a handle
  A->>A: Rate limit, resolve the handle
  A->>D: Save OAuth state
  A->>P: Start authorization with the widest scopes
  A-->>B: 303 to the PDS
  B->>P: Approve or decline
  P-->>B: Redirect to /oauth/callback
  B->>A: GET /oauth/callback
  alt Declined
    A-->>B: 303 to /login?error=denied
  else Anything else fails
    A-->>B: 303 to /login?error=login
  else Approved
    A->>P: Trade the code for DPoP-bound tokens
    A->>D: Save OAuth session, grant and account
    A->>D: Store the SHA-256 of a new web session
    A->>D: Audit auth.login
    A-->>B: Set ln_session, 303 to next
  end
```

Source: [`src/routes/login/+page.server.ts`](../../src/routes/login/+page.server.ts), [`src/routes/oauth/callback/+server.ts`](../../src/routes/oauth/callback/+server.ts), [`src/lib/server/atproto/oauth.ts`](../../src/lib/server/atproto/oauth.ts), [`src/lib/server/auth/session.ts`](../../src/lib/server/auth/session.ts)

## Scope fallback

What sign-in asks for and what it settles on. Opting in later works differently: see [Optional scope opt-in](#optional-scope-opt-in).

```mermaid
flowchart TD
  Full["Ask for full: blobs, calendar events,<br/>RSVPs, the permission set"]
  Full -- "scope refused" --> Base["Ask for base: profile, shows,<br/>follows, calendar events, blobs"]
  Base -- "scope refused" --> Minimal["Ask for atproto alone"]
  Full -- "accepted" --> Grant["The grant"]
  Base -- "accepted" --> Grant
  Minimal --> Grant
  Grant -- "covers every<br/>space write" --> Capable["spacesCapable: writes spaces<br/>with their own session"]
  Grant -- "doesn't" --> Behalf["Root writes space records<br/>on their behalf"]
  Capable --> Step["Public record and step-up scopes don't count.<br/>A public write that lacks one asks for sign-in;<br/>nothing writes with the step-up scopes yet"]
```

Source: [`src/lib/server/atproto/oauth.ts`](../../src/lib/server/atproto/oauth.ts), [`../../lexicons/computer/localnetwork/permissions.json`](../../../../lexicons/computer/localnetwork/permissions.json)

## Optional scope opt-in

How email and bug reports ask for one more scope after sign-in. It keeps what the account already granted and never asks below the tier it signed in with, so a refusal leaves the account as it was.

```mermaid
sequenceDiagram
  participant B as Browser
  participant A as App
  participant P as Account's PDS
  B->>A: Email on in settings,<br/>or Allow on /report
  opt Email, with the scope already granted
    A->>P: getSession for the address
    A-->>B: Link sent, or email on, and it stops here<br/>unless the address could not be read
  end
  A->>P: Authorize from full down to their tier,<br/>each with the scope
  alt PDS refuses every form
    A-->>B: Their server doesn't allow it
  else
    A-->>B: 303 to the PDS
    B->>P: Approve or decline
    P-->>B: Redirect to /oauth/callback
    B->>A: GET /oauth/callback
    alt Email declined
      A-->>B: 303 to settings?email=declined
    else Bug reports declined
      A-->>B: 303 to /login?error=denied
    else Approved
      A->>A: Record the grant,<br/>read the address for email
      A-->>B: 303 back where it started
    end
  end
```

Source: [`src/routes/dashboard/inbox/settings/+page.server.ts`](../../src/routes/dashboard/inbox/settings/+page.server.ts), [`src/routes/report/+page.server.ts`](../../src/routes/report/+page.server.ts), [`src/routes/oauth/callback/+server.ts`](../../src/routes/oauth/callback/+server.ts), [`src/lib/server/atproto/oauth.ts`](../../src/lib/server/atproto/oauth.ts), [`src/lib/server/notifications/channels.ts`](../../src/lib/server/notifications/channels.ts)

## Who writes a record

Which account's session makes a write. Public repos have no fallback, since nobody can write there for someone else.

```mermaid
flowchart LR
  Write["A write for someone"]
  Write -- "shared space" --> Capable["spacesCapable, with<br/>a live session?"]
  Capable -- "yes" --> Own["Their session,<br/>their repo in the space"]
  Capable -- "no" --> Root["Root's session, root's repo,<br/>onBehalfOf names them"]
  Write -- "their public repo" --> Session["Session holds<br/>the scope?"]
  Session -- "yes" --> Public["Their session"]
  Session -- "no" --> Again["Sign in again. If that didn't<br/>help: your account's server<br/>doesn't allow this yet"]
  Write -- "planner space" --> Planner["spacesCapable?"]
  Planner -- "yes" --> Mine["Their session"]
  Planner -- "no" --> Off["Planner unavailable"]
  Write -- "root's repo or<br/>root-only space" --> Only["Root's session"]
```

Source: [`src/lib/server/services/own-repo.ts`](../../src/lib/server/services/own-repo.ts), [`src/lib/server/services/planner.ts`](../../src/lib/server/services/planner.ts)

## Browser session lifecycle

A web session from sign-in to sign-out. `POST /logout` audits first, waiting up to five seconds, then revokes the OAuth session and sets `ln_signed_out`.

```mermaid
stateDiagram-v2
  [*] --> SignedOut
  SignedOut --> SignedIn: OAuth callback sets ln_session
  SignedIn --> SignedIn: signing in again replaces it
  SignedIn --> MustSignIn: the app now asks for more space scopes
  MustSignIn --> SignedIn: signs in again
  SignedIn --> SignedOut: SESSION_DAYS pass (30 by default), cookie deleted
  SignedIn --> SignedOut: POST /logout
```

Source: [`src/lib/server/auth/session.ts`](../../src/lib/server/auth/session.ts), [`src/lib/server/app.ts`](../../src/lib/server/app.ts), [`src/routes/logout/+server.ts`](../../src/routes/logout/+server.ts), [`src/hooks.server.ts`](../../src/hooks.server.ts)

## Root account session

How the root account signs in at startup. Its session lives in `kv`, tied to a slow hash of `ROOT_PASSWORD`, so changing the password forces a fresh sign-in.

```mermaid
flowchart TD
  Start["Connect the root"] --> Resolve["Resolve ROOT_IDENTIFIER to a DID"]
  Resolve --> Print["scrypt fingerprint of ROOT_PASSWORD"]
  Print --> Saved["root_session in kv, with the<br/>same fingerprint and DID?"]
  Saved -- "yes" --> Resume["Resume it"]
  Saved -- "no, or it won't resume" --> Login["Sign in with the password"]
  Resume --> Client["One root client for the process:<br/>writes, delegation tokens, chat"]
  Login --> Client
  Client -- "session created or refreshed" --> Store["Written to kv"]
  Client -- "session expired" --> Clear["Deleted from kv"]
```

Source: [`src/lib/server/atproto/root.ts`](../../src/lib/server/atproto/root.ts), [`src/lib/server/atproto/repo-client.ts`](../../src/lib/server/atproto/repo-client.ts)

## Token exchange flow

How a locallibrary plugin gets tokens from a service that wants a client secret.

```mermaid
sequenceDiagram
  participant L as locallibrary
  participant A as App
  participant S as Service token endpoint
  L->>A: POST /api/token-exchange/[service] as JSON
  alt Unknown service, over 64 KiB, or not a grant
    A-->>L: 404, 413 or 400
  else redirect_uri isn't locallibrary's listener
    A-->>L: 400 invalid_request
  else Service's variables aren't set
    A-->>L: 503 temporarily_unavailable
  else Refresh at a service that has none
    A-->>L: 400 unsupported_grant_type
  else
    A->>S: Same grant plus client id and secret
    S-->>A: Tokens or an error
    A-->>L: The service's answer, no-store
    A-->>L: 502 if it didn't answer properly
  end
```

Source: [`src/routes/api/token-exchange/[service]/+server.ts`](../../src/routes/api/token-exchange/[service]/+server.ts), [`src/lib/server/token-exchange/exchange.ts`](../../src/lib/server/token-exchange/exchange.ts), [`src/lib/server/token-exchange/services.json`](../../src/lib/server/token-exchange/services.json)

## Tool hosts

What a tool's own host answers. It runs before the app loads, so it works while the app isn't configured.

```mermaid
flowchart TD
  In["Request to a tool's host"] --> Method{"GET or HEAD?"}
  Method -- "no" --> Refused["405"]
  Method -- "yes" --> Path{"Which path?"}
  Path -- "/oauth-client-metadata.json" --> Doc["The tool's OAuth client document"]
  Path -- "/" --> Move["308 to /tools/[tool]<br/>on PUBLIC_URL"]
  Path -- "anything else" --> Missing["404"]
```

Source: [`src/lib/server/tools/site.ts`](../../src/lib/server/tools/site.ts), [`src/lib/tools/index.ts`](../../src/lib/tools/index.ts)

## Site roles

The roles the site itself knows, and how someone gets each.

```mermaid
flowchart TD
  Visitor["Visitor"] -- "signs in" --> Account["Signed-in account"]
  Account -- "create or claim approved,<br/>or an admin adds them" --> Manager["Manager of a performer:<br/>on its managers list, at most 10"]
  Account -- "sends an inquiry" --> Asker["Promoter side of a thread"]
  Account -- "grant" --> Reader["Reader of a press kit's private part"]
  Account -- "staff.role from an admin" --> Mod["Moderator: reviews requests,<br/>hides performers, lists DJs,<br/>changes who's in a space,<br/>reads the audit log"]
  Account -- "staff.role, or ADMIN_DIDS" --> Admin["Admin: all that, plus direct edits,<br/>staff, verifications, System page"]
```

Source: [`src/lib/server/services/staff.ts`](../../src/lib/server/services/staff.ts), [`src/lib/server/app.ts`](../../src/lib/server/app.ts), [`src/lib/server/services/requests.ts`](../../src/lib/server/services/requests.ts), [`src/lib/server/services/project-admin.ts`](../../src/lib/server/services/project-admin.ts)

## Capabilities by role

The starting capabilities of each role in a performer, party or venue space. `page.edit`, `presskits.manage`, `shows.manage` and `events.manage` also make someone a writing member. No page uses these roles yet except the staff console.

```mermaid
flowchart LR
  Performer["performer space"] --> PM["member, listed:<br/>page.edit, people.manage, listing.request,<br/>presskits.manage, bookings.answer, shows.manage"]
  Performer --> PN["manager:<br/>page.edit, listing.request, presskits.manage,<br/>bookings.answer, shows.manage"]
  Party["party space"] --> H["host, listed:<br/>page.edit, people.manage, listing.request, events.manage,<br/>bookings.send, guests.manage, guests.checkin"]
  Party --> C["cohost, listed:<br/>all of host's but people.manage"]
  Party --> D["door:<br/>guests.checkin"]
  Venue["venue space"] --> O["owner:<br/>page.edit, people.manage,<br/>venue.approve, venue.block"]
  Venue --> B["booker:<br/>venue.approve"]
```

Source: [`src/lib/capabilities.ts`](../../src/lib/capabilities.ts), [`src/lib/server/services/shared-spaces.ts`](../../src/lib/server/services/shared-spaces.ts)
