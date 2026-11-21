# Feature flags

The flags the website and locallibrary share. Each one is a file in `catalog/`, named for its key, which `flag.schema.json` checks. To add a flag, add a file. To remove one, delete its file, and the website and locallibrary stop compiling wherever it's still used.

Every flag has a `type`, `release` or `ops`. A `release` flag is meant to go, so it needs a `removeBy` date (`YYYY-MM-DD`): once it passes, the website's tests fail until the flag is deleted or the date moves. An `ops` flag stays, and can't have one.

A flag nobody has saved reads as off, so name a flag for the state you can switch it to. A kill switch, for something that already works, is named for the paused state, like `pauseRecordingStream`, and is `ops`: the deploy that adds it changes nothing, and turning it on stops the feature. Something that's dark today and should stay switchable after it ships, like inbox email, is named for the on state and is `ops` too, so it has no `removeBy`. Use `release` only for flags that go once the rollout is done.

A flag can list the kinds of page it changes in `showsOn`: `profile`, `act`, `party`, `venue`, `event`, `set`, `mix` or `pressKit`, or `global` for every page. An act's page is still `/projects/<slug>` until acts move. The flag's page in the staff console lists them under Shows on. Leave it out when the flag changes no public page, like a kill switch for the token exchange, something in the dashboard, or anything only signed-in accounts see. It also decides whether signed-out visitors are counted in a percentage: only a flag with `showsOn` gives them a rollout id.

Who a flag is on for lives in the staff space and is set at `/admin/flags`, not here. Every flag starts off. The list there marks a release flag "Ready to remove" once it's been on for everyone, or every account, for 30 days, and its page shows when it should be gone by.

```jsonc
{
	"$schema": "../flag.schema.json",
	"key": "newPlanner",
	"description": "What the flag gates, for the staff console.",
	"type": "release",
	"removeBy": "2027-03-01",
	"showsOn": ["profile"]
}
```

Check a flag on the website with `locals.flags.newPlanner` on the server or `data.flags?.newPlanner` in a page. An xrpc route never reads a cookie, so it only gets flags on for everyone: targeting and percentages don't reach it. A route that has checked a service token can call `app.flags.evaluate(did)` with that token's DID to get them.

In locallibrary, use `locallibrary::flags::check(root, flags::keys::NEW_PLANNER)`. A flag in locallibrary is for rolling something out. Anything that has to be enforced belongs on the server, since anyone can change what's on their own computer.

## How a flag is decided

```text
bucket(key, id) = the first 4 bytes of sha256(key + id), big-endian, mod 100
on = enabled && (everyone || targeted || (id is known && bucket(key, id) < percentage))
```

`enabled` is false once someone turns the flag off for everyone, which keeps the other gates for when it's back on. A record that doesn't say is enabled. `targeted` means the account is one of the flag's accounts or in one of its groups. Only the website can know that, since those lists are private; it says so through `computer.localnetwork.flags.getFlags`. `id` is the caller's DID. A signed-out visitor has none, so while a flag with `showsOn` is on for a percentage the website gives their browser a random id in a first-party cookie, `ln_flag_visitor`, and buckets `visitor:` followed by it. They get the same answer on every request until they clear it or sign in, when their account's bucket takes over. A caller with neither, like an xrpc call, only gets flags on for everyone. Raising a percentage only adds accounts and browsers. [ADR 0018](../docs/adr/0018-signed-out-visitors-get-a-rollout-id.md) covers why, and what it means for caching and crawlers.

`vectors.json` holds computed answers, for DIDs, visitor ids and callers with neither. The website's and locallibrary's tests both run them, so the two can't come to disagree. A case without `enabled` is enabled. If a case has to change, the rule changed, and accounts will move in or out of every rollout.

## Trying a flag

- In development and tests, `FLAG_OVERRIDES=newPlanner=on,parityCanary=off` forces flags on or off for every request to that server. The website won't start with it set in production.
- Staff with the flags grant can turn a flag on or off for their own browser from its page at `/admin/flags`, for an hour. A banner says so on every page until it's cleared, and nobody else sees a difference.

[ADR 0010](../docs/adr/0010-flags-turn-off-and-preview-without-touching-targeting.md) covers turning a flag off and the overrides, and [Feature flag platforms](../docs/research/feature-flag-platforms.md) compares the design with hosted flag platforms.
