# locallibrary diagrams: Accounts

The DJ account, token exchange, keys and access. The rest are listed in the [README](../../README.md#diagrams).

- [Accounts and access](#accounts-and-access)
- [Signing in to the DJ account](#signing-in-to-the-dj-account)
- [What the DJ account is allowed](#what-the-dj-account-is-allowed)
- [The DJ account's life](#the-dj-accounts-life)
- [Signing in through the token exchange](#signing-in-through-the-token-exchange)
- [Signing in with keys](#signing-in-with-keys)
- [An account's life](#an-accounts-life)
- [When a plugin needs more of an account](#when-a-plugin-needs-more-of-an-account)
- [When a destination starts](#when-a-destination-starts)
- [Managing access](#managing-access)
- [Signing in from another computer](#signing-in-from-another-computer)

## Accounts and access

How accounts, account types, scopes and access relate. The DJ account is kept apart, in `atmosphere.json`.

```mermaid
erDiagram
    ACCOUNT_PLUGIN }|--|| ACCOUNT_TYPE : "provides"
    ACCOUNT_TYPE ||--o{ SCOPE : "names"
    ACCOUNT_TYPE ||--o{ ACCOUNT : "kind of"
    ACCOUNT_PLUGIN ||--o{ ACCOUNT : "signed it in"
    PLUGIN ||--o{ ACCESS : "is given"
    ACCOUNT |o--o{ ACCESS : "used through"
    ACCESS }o--o{ SCOPE : "may use"
    ACCOUNT {
        string type
        string plugin
        string subject
        string name
        list scopes "as the service names them"
    }
    ACCESS {
        string id
        string plugin
        list scopes "as the type names them"
        bool default
        list declined
    }
```

Source: [`crates/locallibrary/src/plugin/accounts/kept.rs`](../../../../crates/locallibrary/src/plugin/accounts/kept.rs), [`crates/locallibrary/src/plugin/accounts/access.rs`](../../../../crates/locallibrary/src/plugin/accounts/access.rs), [`crates/locallibrary/src/plugin/accounts/shared.rs`](../../../../crates/locallibrary/src/plugin/accounts/shared.rs), [`GLOSSARY.md`](../../GLOSSARY.md)

## Signing in to the DJ account

The Atmosphere sign-in, which the `jacquard-oauth` client runs. The password stays with the account's server. A `--client-id` document never falls back, and `--session-file` keeps the secrets in a file.

```mermaid
sequenceDiagram
    participant L as locallibrary
    participant C as Client document
    participant S as Account's server
    participant B as Browser
    participant K as Keychain or file
    L->>C: Online, allowing<br/>these scopes?
    Note over L,C: Offline: the localhost<br/>development client.<br/>Online without them: refused
    L->>L: Find the server from<br/>a handle, DID or address
    Note over L: A handle neither DNS nor<br/>its web address names a DID for:<br/>stops, saying what each answered,<br/>and to try the server's address
    L->>S: Pushed authorization<br/>request, with PKCE
    L->>B: Open the sign-in page
    B->>S: You approve
    B->>L: Back to a one-off<br/>127.0.0.1 port
    L->>S: Trade the code for<br/>DPoP-bound tokens
    L->>K: DPoP key, refresh token
    L->>L: The rest in atmosphere.json
```

Source: [`crates/locallibrary/src/atmosphere/mod.rs`](../../../../crates/locallibrary/src/atmosphere/mod.rs), [`crates/locallibrary/src/atmosphere/store.rs`](../../../../crates/locallibrary/src/atmosphere/store.rs), [`crates/locallibrary/src/callback.rs`](../../../../crates/locallibrary/src/callback.rs), [`src/account_cmd/atmosphere.rs`](../../src/account_cmd/atmosphere.rs)

## What the DJ account is allowed

What each sign-in asks of the account's server. Each is asked for the first time it's needed.

```mermaid
flowchart TD
    plain["account login<br/>atproto: know who you are"] --> publish["Publishing, or login --publish<br/>include:computer.localnetwork.locallibraryPublishing,<br/>blob:audio/*"]
    plain --> report["bug, the first time<br/>repo:app.userinput.discussion?action=create"]
    plain --> plugin["A plugin's record write, the first time,<br/>or login --scope: repo:NSID"]
    publish --> records["Create and replace settings, profile, set, recording.<br/>Delete set, recording.<br/>A token for directory.requestIndex"]
    publish --> upload["Upload audio files"]
    report --> discussion["Create bug reports"]
    plugin --> written["Create and replace NSID records"]
```

Source: [`crates/locallibrary/src/atmosphere/mod.rs`](../../../../crates/locallibrary/src/atmosphere/mod.rs), [`crates/locallibrary/src/atmosphere/repo.rs`](../../../../crates/locallibrary/src/atmosphere/repo.rs), [`src/account_cmd/atmosphere.rs`](../../src/account_cmd/atmosphere.rs), [`lexicons/computer/localnetwork/locallibraryPublishing.json`](../../../../lexicons/computer/localnetwork/locallibraryPublishing.json)

## The DJ account's life

From signing in to signing out. The 14 days are Bluesky's limit as a constant here, counted by this computer's clock. Signing in again that fails leaves the account as it was.

```mermaid
stateDiagram-v2
    state "Signed out" as Out
    state "Waiting for the browser" as Waiting
    state "Signed in" as In
    state "Signing in again soon" as Soon
    state "Past its 14 days" as Expired
    [*] --> Out
    Out --> Waiting: account login
    Waiting --> Out: timed out, cancelled, refused
    Waiting --> In: tokens kept
    In --> Waiting: sign in again
    In --> In: refreshed under a lock
    In --> Soon: 2 days left
    Soon --> Waiting: sign in again
    Soon --> Expired: 14 days since sign-in
    Expired --> Waiting: sign in again
    In --> Out: account logout, or the server ended it, found on next use
    Soon --> Out: the same
    Expired --> Out: the same
```

Source: [`crates/locallibrary/src/atmosphere/mod.rs`](../../../../crates/locallibrary/src/atmosphere/mod.rs), [`crates/locallibrary/src/atmosphere/store.rs`](../../../../crates/locallibrary/src/atmosphere/store.rs), [`src/account_cmd/atmosphere.rs`](../../src/account_cmd/atmosphere.rs)

## Signing in through the token exchange

An account type whose service wants a client secret goes through `localnetwork.computer/api/token-exchange/SERVICE`, which adds it and keeps no tokens. Local Network can pause one service's exchange with a flag, say when a secret leaks; it then answers 503 before asking the service, and the sign-in fails.

```mermaid
sequenceDiagram
    participant H as Plugin host
    participant B as Browser
    participant S as Service
    participant X as Token exchange
    participant K as Keychain or file
    H->>B: Sign-in page, PKCE
    B->>S: You approve
    B->>H: Back to 127.0.0.1
    H->>X: The code, as JSON
    X->>S: The code and<br/>client secret
    S-->>X: Tokens
    X-->>H: Tokens
    H->>S: whoami
    H->>K: Tokens
    H->>H: The rest in<br/>accounts.json
```

Source: [`crates/locallibrary/src/plugin/accounts.rs`](../../../../crates/locallibrary/src/plugin/accounts.rs), [`crates/locallibrary/src/plugin/accounts/shared.rs`](../../../../crates/locallibrary/src/plugin/accounts/shared.rs), [`crates/locallibrary/src/plugin/accounts/whoami.rs`](../../../../crates/locallibrary/src/plugin/accounts/whoami.rs), [`plugins/accounts/google-account/locallibrary-plugin.toml`](../../../../plugins/accounts/google-account/locallibrary-plugin.toml), [`apps/web/src/lib/server/token-exchange/services.ts`](../../../../apps/web/src/lib/server/token-exchange/services.ts), [`apps/web/src/lib/server/token-exchange/exchange.ts`](../../../../apps/web/src/lib/server/token-exchange/exchange.ts)

## Signing in with keys

For a service that hands out keys instead, as the Internet Archive and plyr.fm do.

```mermaid
flowchart TD
    start["account TYPE sign-in"] --> where["Say where to get them"]
    where --> enter["Read them: unseen in a terminal,<br/>a form in the player"]
    enter --> whoami{"Does the type have a whoami?"}
    whoami -->|yes| asked["Ask the service whose they are"]
    whoami -->|no| named["You name the account"]
    asked -->|"refused"| stop["The sign-in ends, nothing kept"]
    asked --> keep["Keys in the keychain, or secrets/accounts.json,<br/>the rest in accounts.json"]
    named --> keep
    keep --> use["Sent only in the Authorization header,<br/>as the type spells it out"]
```

Source: [`crates/locallibrary/src/plugin/accounts/keys.rs`](../../../../crates/locallibrary/src/plugin/accounts/keys.rs), [`crates/locallibrary/src/plugin/accounts/shared.rs`](../../../../crates/locallibrary/src/plugin/accounts/shared.rs), [`docs/adr/0002-keys-accounts.md`](../adr/0002-keys-accounts.md), [`src/account_cmd/accounts.rs`](../../src/account_cmd/accounts.rs)

## An account's life

An account a plugin uses, from signing in to signing out. A plugin's access outlasts it. Lapsed requests fail as signed out. Sign-out revokes it if the service can. An account with keys never refreshes or expires: only sign-out ends it. A keychain that lost its tokens acts as signed out, with the account still listed.

```mermaid
stateDiagram-v2
    state "Signed in" as In
    state "Signed out" as Out
    state "Refreshing" as Refresh
    state "Lapsed, still listed" as Lapsed
    [*] --> In: signed in
    In --> Refresh: about to expire, or a 401
    Refresh --> In: new token, or the request fails
    Refresh --> Out: the service won't refresh it
    In --> Lapsed: expired, no refresh token
    Lapsed --> In: signed in again
    Lapsed --> Out: sign-out
    In --> Out: invalid_token, or sign-out
    Out --> In: signed in again
```

Source: [`crates/locallibrary/src/plugin/accounts.rs`](../../../../crates/locallibrary/src/plugin/accounts.rs), [`crates/locallibrary/src/plugin/accounts/token.rs`](../../../../crates/locallibrary/src/plugin/accounts/token.rs), [`crates/locallibrary/src/plugin/accounts/shared.rs`](../../../../crates/locallibrary/src/plugin/accounts/shared.rs), [`docs/plugins.md`](../plugins.md)

## When a plugin needs more of an account

What happens when a plugin asks for a scope it can do without.

```mermaid
flowchart TD
    ask["accounts/ensure from a plugin"] --> have{"Has the scopes?"}
    have -->|yes| go["Answers which it has"]
    have -->|"declined before"| answer["Answered as declined,<br/>the plugin carries on"]
    have -->|no| who{"Anyone to ask?"}
    who -->|"--json, --yes, no terminal,<br/>watch, sidecar"| fail["Fails, saying how to give them"]
    have -->|"no access of the type"| fail
    who -->|"terminal, player"| question["Why it wants them: may it?"]
    question -->|no| remember["Remembered as declined"]
    remember --> answer
    question -->|yes| give["Sign in again if the account needs it,<br/>give the access the scopes"]
    give --> go
```

Source: [`crates/locallibrary/src/plugin/accounts/ensure.rs`](../../../../crates/locallibrary/src/plugin/accounts/ensure.rs), [`crates/locallibrary/src/plugin/accounts/ready.rs`](../../../../crates/locallibrary/src/plugin/accounts/ready.rs), [`src/account_cmd/asking.rs`](../../src/account_cmd/asking.rs), [`src/tui/plugins/access.rs`](../../src/tui/plugins/access.rs)

## When a destination starts

What's checked before `store copy --to`, `store restore`, `drive verify --from` or `sets publish --to` sends anything. It's the command line's, and a no here isn't remembered.

```mermaid
flowchart TD
    start["A plugin's destination, named"] --> unready{"For each account type: an access to an<br/>account signed in, with what it needs,<br/>staying signed in long enough?"}
    unready -->|yes| go["Carry on. Checked again before<br/>a recording is lent to it"]
    unready -->|"no, in a terminal"| ask["Give it access, or sign in again?"]
    ask -->|yes| unready
    ask -->|no| refused["Refused: no_access, not_signed_in or missing_scope,<br/>saying what resolves it"]
    unready -->|"no, with --yes or no terminal"| refused
```

Source: [`src/account_cmd/access.rs`](../../src/account_cmd/access.rs), [`crates/locallibrary/src/plugin/accounts/ready.rs`](../../../../crates/locallibrary/src/plugin/accounts/ready.rs), [`src/sets_cmd.rs`](../../src/sets_cmd.rs), [`src/store_cmd.rs`](../../src/store_cmd.rs), [`src/drive_cmd.rs`](../../src/drive_cmd.rs)

## Managing access

`plugin access`, the player's "access for PLUGIN…" form, and the sidecar's `accounts.access.*`.

```mermaid
flowchart TD
    list["plugin access PLUGIN: its accesses"] --> give["--give ACCOUNT, --scope NAME:<br/>the scopes it needs, and those named"]
    list --> default["--default ACCOUNT: the one<br/>{account:TYPE} names"]
    list --> away["--take-away ACCOUNT"]
    give -->|"the first of its type"| isdefault["It's the default"]
    away -->|"it was the default,<br/>and one access is left"| left["That one becomes the default"]
    away --> gone["The account stays signed in"]
```

Source: [`crates/locallibrary/src/plugin/accounts/access.rs`](../../../../crates/locallibrary/src/plugin/accounts/access.rs), [`src/account_cmd/access.rs`](../../src/account_cmd/access.rs), [`src/tui/plugins/access.rs`](../../src/tui/plugins/access.rs), [`src/rpc/methods/accounts.rs`](../../src/rpc/methods/accounts.rs)

## Signing in from another computer

The paste path, for the DJ account and accounts plugins use, when the browser can't reach this computer's 127.0.0.1. `--no-browser` prints the link and still waits here. The sidecar's are `account.login.paste` and `accounts.sign_in.paste`.

```mermaid
sequenceDiagram
    participant L as locallibrary
    participant Y as You
    participant B as Browser elsewhere
    participant S as The server
    L->>Y: --paste prints the link
    Y->>B: Open it
    B->>S: You approve
    S-->>B: To 127.0.0.1,<br/>which fails there
    Y->>L: Paste the address<br/>it ended up on
    L->>S: Trade the code, as any sign-in
```

Source: [`src/browser.rs`](../../src/browser.rs), [`crates/locallibrary/src/atmosphere/mod.rs`](../../../../crates/locallibrary/src/atmosphere/mod.rs), [`crates/locallibrary/src/plugin/accounts/shared.rs`](../../../../crates/locallibrary/src/plugin/accounts/shared.rs), [`src/account_cmd/atmosphere.rs`](../../src/account_cmd/atmosphere.rs), [`src/account_cmd/accounts.rs`](../../src/account_cmd/accounts.rs)
