# Atmosphere account

Signing in and publishing are alpha: so far they've only been tested against a mock server, so
expect rough edges and changes.

```bash
locallibrary account login dj.example.com    # sign in in the browser
locallibrary account                         # who's signed in, and until when
locallibrary account status --check          # ask the account's server (refreshes the session)
locallibrary account logout                  # revoke the session and remove it from this computer
```

Your Atmosphere account is the one you use for Bluesky and other AT Protocol apps, hosted by
Bluesky or any other server. Signing in happens in your browser, on your account's server:
locallibrary never sees your password. Give your handle, a DID, or your server
(`locallibrary account login https://bsky.social`, which also lets you pick an account or make
one). Signing in only asks to know who you are: locallibrary can't post or change anything.
[Publishing](#publishing) asks for more the first time, by signing in again: permission to create
and replace locallibrary's own records
(`repo:computer.localnetwork.locallibrary.settings?action=create&action=update`, and the same for
`.profile` and `.set`), not to delete them, and nothing else in the account. `account login
--publish` asks for it up front.

- **The browser comes back to locallibrary** on a one-off `127.0.0.1` port, and only the address
  that carries this sign-in's `state` counts. When the browser runs on another computer (over
  SSH), `--paste` takes the address it ends up on instead; `--no-browser` only prints the link.
  Only `https://` sign-in pages are opened.
- **The session is kept in the system keychain** (macOS Keychain, Windows Credential Manager, the
  Secret Service on Linux), one item per store folder and account: the key every token is tied to
  and the refresh token. The rest is in `atmosphere.json` in the store folder. Without a keychain,
  `--session-file` (or `LOCALLIBRARY_SESSION_STORE=file`, which the player and the sidecar go by)
  keeps it all in files only you can read. Signing out revokes both tokens at the server and removes
  them here; if the keychain doesn't answer, it says what's left and the next `account logout` tries
  again.
- **Servers found from a handle or DID are only reached at public `https://` addresses**, never
  an address on this computer or its local network, and redirects too (a name that leads there
  fails the certificate check before any request is sent); certificates are checked against the
  system's trusted roots as well as the bundled ones.
- **Sessions last up to two weeks** on Bluesky's servers, however often they're refreshed; then
  sign in again. `account` shows the date. One account is signed in at a time.
- **Servers know locallibrary as `locallibrary.localnetwork.computer`**: they fetch its client
  document from `https://locallibrary.localnetwork.computer/oauth-client-metadata.json`, show
  that name when you approve, and remember the approval. The Local Network web app (a separate
  project) serves that host: the document (in its `src/lib/server/locallibrary/`; `locallibrary
  account client-metadata -o …` regenerates it) and a short page about locallibrary. Until it's
  online, sign-in says so and falls back to the `http://localhost` development client, which
  servers show as "a local app" and ask about every time (`account` says when a session is the
  development client's). The document lists every scope a sign-in may ask for, publishing's
  included, and servers refuse a sign-in that asks for more: a release that asks for new scopes
  needs the hosted document updated first. Until it allows them, sign-in stops and says so.
  `--dev-client` chooses that client on purpose; `--client-id` (or
  `LOCALLIBRARY_OAUTH_CLIENT_ID`) another document.

The OAuth itself (finding your server, the authorization request, PKCE, DPoP-bound tokens, the
checks on the server's answer, refresh and revocation) is the
[jacquard-oauth](https://crates.io/crates/jacquard-oauth) client's.

## Publishing

```bash
locallibrary settings publish                 # your My Settings
locallibrary profile publish --dry-run        # your DJ profile: look first
locallibrary sets publish last --hide 3,7     # your latest set, without naming tracks 3 and 7
```

Publishing writes a record to your account's repository, where anyone can read it and other
apps can show it. Each command shows exactly what it will publish (`--dry-run` stops there,
and works signed out; `--json` prints the record) and asks first, then publishes exactly that,
to the account it named: if the record was published from somewhere else in between, or
another account signed in, nothing is replaced. Like every write, it exits with status 3 when
it wasn't confirmed (see `locallibrary --help`). The first time, it asks you to sign in again to
allow publishing (`--paste`, `--no-browser` and `--timeout` work as for `account login`), then
shows the plan again for the account you approved. Publishing again replaces the record.

| Command | Record | Holds |
| --- | --- | --- |
| `settings publish` | `computer.localnetwork.locallibrary.settings`, key `self` | the settings profile's base values |
| `profile publish` | `computer.localnetwork.locallibrary.profile`, key `self` | the [profile document](sets.md#dj-profile), in whole numbers: shares as percents, BPM whole |
| `sets publish <set>` | `computer.localnetwork.locallibrary.set`, keyed by the set's id | the day, title and kind (club, festival, radio, livestream, mix, or your own word), the venue with its city and country, the event it was part of, who you played with, where to listen, what the tracklist came from (`history`, `mix`, `added`), and the tracks in order: artist, title, ISRC, BPM, Camelot key, start time |

A set's notes, its recording, the sessions it came from and the files' paths stay on this
computer. Tracks you marked unnamed (`sets edit <set> --unnamed 3`), tracks your profile hides
(`profile hide`) and the positions you give with `--hide 3,7` are published as unidentified:
only their place in the set and when it starts. The venue, city and country also go out as a
`community.lexicon.location.address`, which needs a country: without one, the city stays out. A
set built from a single history list that an earlier version published with `plays publish`
replaces that record the first time it's published, and takes its key as its id.

The record types are Lexicons in [`lexicons/`](../lexicons/computer/localnetwork/locallibrary/),
next to the address Lexicon they use, copied from
[Lexicon Community](https://github.com/lexicon-community/lexicon). Each record is checked against
its Lexicon before it's written. Everything published is public and copied around the network:
deleting a record later doesn't delete the copies.
