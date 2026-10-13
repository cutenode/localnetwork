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
and replace the three record types it publishes
(`repo:computer.localnetwork.locallibrary.settings?action=create&action=update`, and the same for
`computer.localnetwork.dj.profile` and `computer.localnetwork.dj.set`), not to delete them, and
nothing else in the account. `account login --publish` asks for it up front.

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
  document from `https://locallibrary.localnetwork.computer/oauth-client-metadata.json`, show that
  name when you approve, and remember the approval. The Local Network web app
  ([`apps/web`](../../web)) serves that host: the document (in its `src/lib/tools/locallibrary/`;
  `locallibrary account client-metadata -o …` regenerates it), and a redirect from the host's
  front page to locallibrary's page on localnetwork.computer. Until it's online, sign-in says so
  and falls back to the `http://localhost` development client, which servers show as "a local app"
  and ask about every time (`account` says when a session is the development client's). The
  document lists every scope a sign-in may ask for, publishing's included, and servers refuse a
  sign-in that asks for more: a release that asks for new scopes needs the hosted document updated
  first. Until it allows them, sign-in stops and says so. `--dev-client` chooses that client on
  purpose; `--client-id` (or `LOCALLIBRARY_OAUTH_CLIENT_ID`) another document.

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
| `profile publish` | `computer.localnetwork.dj.profile`, key `self` | the [profile document](sets.md#dj-profile), in whole numbers: shares as percents, BPM whole |
| `sets publish <set>` | `computer.localnetwork.dj.set`, keyed by the set's id | the day, title and kind (club, festival, radio, livestream, mix, or your own word), the venue with its city and country, the event it was part of, who you played with (`playedWith`), where to listen (`links`), what the tracklist came from (`sources`: `history`, `mix`, `added`), and the tracks in order: artist, title, the artists one by one, release, ISRC, MusicBrainz IDs, BPM, Camelot key, start time, and how each came in |

A set's notes, its recording, the sessions it came from and the files' paths stay on this
computer. Tracks you marked unnamed (`sets edit <set> --unnamed 3`), tracks your profile hides
(`profile hide`) and the positions you give with `--hide 3,7` are published as unidentified:
only their place in the set and when it starts. The venue, city and country also go out as a
`community.lexicon.location.address`, which needs a country: without one, the city stays out. A
set built from a single history list that an earlier version published with `plays publish`
takes that list's key as its id the first time it's published, and the time the old record says
it first went out. The old record stays where `plays publish` wrote it
(`computer.localnetwork.locallibrary.set`), since publishing may only write today's set records.

A track's artists go out one by one (split as the profile counts them) when its credit names
several, and its MusicBrainz IDs (`mbid:<uuid>`, as teal.fm writes them) when its file is tagged
with them, as MusicBrainz Picard tags files: rekordbox keeps none, so they're read from the files
on the mounted drives when a set is made or edited. An artist's ID goes with their name only when
the tags have one for each artist the credit names. For a set built from a recording, each track
says how it came in (`transitionIn`), as the recording's [mix report](sets.md#mixes) heard it,
wherever the set still has the two tracks one after the other: a `blend` when both were heard
together for 8 seconds or more, otherwise `unknown` (a fade the fingerprints miss can look like a
cut), with how long they overlapped, how the keys heard relate and how much the tempo changed.
Next to an unidentified track it says only how long they overlapped.

A set record holds up to 500 tracks: `sets publish` refuses a longer set, so drop some first
(`sets edit --drop`). A track with every field filled is under 1.5 KB even with names as long as the
Lexicon takes, so 500 fit the 1 MB a data server takes in a record. A data server not updated
since May 2026 takes 150 KB: about 160 fully tagged tracks with long names, or 290 with typical
ones. The plan says when a record is bigger than that.

The record types are Lexicons in the repo's [`lexicons/`](../../../lexicons/computer/localnetwork/):
My Settings is locallibrary's own (`locallibrary/`), and the profile and set are DJ records Local
Network's tools share (`dj/`, with the definitions both use in `dj/defs.json`). The address Lexicon
they use is copied from [Lexicon Community](https://github.com/lexicon-community/lexicon). Each
record is checked against its Lexicon before it's written. Everything published is public and copied around the network:
deleting a record later doesn't delete the copies.
