# Feature flags

The flags the website and locallibrary share. Each one is a file in `catalog/`, named for its key, which `flag.schema.json` checks. To add a flag, add a file. To remove one, delete its file, and the website and locallibrary stop compiling wherever it's still used.

Every flag has a `type`, `release` or `ops`. A `release` flag is meant to go, so it needs a `removeBy` date (`YYYY-MM-DD`): once it passes, the website's tests fail until the flag is deleted or the date moves. An `ops` flag stays, and can't have one.

A flag nobody has saved reads as off, so name a flag for the state you can switch it to. A kill switch, for something that already works, is named for the paused state, like `pauseRecordingStream`, and is `ops`: the deploy that adds it changes nothing, and turning it on stops the feature. Something that's dark today and should stay switchable after it ships, like inbox email, is named for the on state and is `ops` too, so it has no `removeBy`. Use `release` only for flags that go once the rollout is done.

A flag can list the kinds of page it changes in `showsOn`: `home`, `profile`, `act`, `party`, `venue`, `event`, `set`, `mix` or `pressKit`, or `global` for every page. An act's page is still `/projects/<slug>` until acts move. The flag's page in the staff console lists them under Shows on. Leave it out when the flag changes no public page, like a kill switch for the token exchange, something in the dashboard, or anything only signed-in accounts see.

Who a flag is on for lives in the staff space and is set at `/admin/flags`, not here. Every flag starts off. The list there marks a release flag "Ready to remove" once every account and signed-out visitors have had it for 30 days, and its page shows when it should be gone by.

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

Check a flag on the website with `locals.flags.newPlanner` on the server or `data.flags?.newPlanner` in a page. An xrpc route gets the flags a signed-out visitor does, since the website never reads the sign-in cookie there: targeting and percentages don't reach it. A route that has checked a service token can call `app.flags.evaluate(did)` with that token's DID to get them.

In locallibrary, use `locallibrary::flags::check(root, flags::keys::NEW_PLANNER)`. A flag in locallibrary is for rolling something out. Anything that has to be enforced belongs on the server, since anyone can change what's on their own computer.

## How a flag is decided

```text
bucket(key, did) = the first 4 bytes of sha256(key + did), big-endian, mod 100
with a DID:  on = enabled && (everyone || targeted || bucket(key, did) < percentage)
with no DID: on = enabled && signedOut && (everyone || percentage >= 100)
```

`enabled` is false once someone turns the flag off for everyone, which keeps the other gates for when it's back on. A record that doesn't say is enabled. `everyone` means every signed-in account. `targeted` means the account is one of the flag's accounts or in one of its groups. Only the website can know that, since those lists are private; it says so through `computer.localnetwork.flags.getFlags`. Raising a percentage only adds accounts.

A caller with no DID comes last: a signed-out visitor, an xrpc call, background work on the server, or locallibrary signed out. A release flag is on for them only once staff tick Signed-out visitors on its page, which sets `signedOut`, and only while every account has the flag. The console refuses it before then, and a save that takes the flag below every account turns it off again. A record that doesn't say has it off. An ops flag has no setting of its own: its `signedOut` is on whenever every account has it (`everyone` or 100%), so a kill switch turned on for every account also stops what runs with no account. There's no percentage of signed-out visitors and no cookie for them, so signed-out pages stay shared-cacheable. [ADR 0018](../docs/adr/0018-signed-out-visitors-come-last.md) covers why.

`vectors.json` holds computed answers. The website's and locallibrary's tests both run them, so the two can't come to disagree. A case without `enabled` is enabled, and one without `signedOut` is a flag signed-out visitors don't have. If a case has to change, the rule changed, and accounts will move in or out of every rollout.

## Trying a flag

- In development and tests, `FLAG_OVERRIDES=newPlanner=on,parityCanary=off` forces flags on or off for every request to that server. The website won't start with it set in production.
- Staff with the flags grant can turn a flag on or off for their own browser from its page at `/admin/flags`, for an hour. A banner says so on every page until it's cleared, and nobody else sees a difference.

[ADR 0010](../docs/adr/0010-flags-turn-off-and-preview-without-touching-targeting.md) covers turning a flag off and the overrides, and [Feature flag platforms](../docs/research/feature-flag-platforms.md) compares the design with hosted flag platforms.
