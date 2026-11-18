# locallibrary-plugin-discogs

locallibrary's Discogs plugin, built into `locallibrary`. It has two providers:

- `discogs.covers` finds front covers for `locallibrary art fetch`
- `discogs.tags` finds an album's title, year, record label and style for `locallibrary track fetch`

Discogs only answers searches for a signed-in account, so make a personal access token at
https://www.discogs.com/settings/developers and sign in with it:

```bash
locallibrary account discogs sign-in --for discogs
```

The `discogs` account type comes from the discogs-account plugin
(`plugins/accounts/discogs-account/`). The token goes only into searches of
`api.discogs.com/database/search`, filled in by locallibrary: the plugin never sees it. Covers come
from `i.discogs.com` without it.

How it looks albums up, scores what it finds and keeps to Discogs' limit is in
[Providers](../../apps/locallibrary/docs/plugins.md#providers).

## Working on it

Its tests run it against `loclib_plugin::test_util::FakeHost`, with Discogs' answers scripted from
the search results in `fixtures/` (shaped as Discogs answers, with made-up ids and image
addresses), so nothing goes online:

```bash
cargo nextest run -p locallibrary-plugin-discogs
```
