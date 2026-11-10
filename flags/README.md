# Feature flags

The website's feature flags, kept here so locallibrary can use the same ones. Each one is a file in `catalog/`, named for its key, which `flag.schema.json` checks. To add a flag, add a file. To remove one, delete its file, and the website stops compiling wherever it's still used.

Every flag has a `type`. `release` and `experiment` flags are meant to go, so they need a `removeBy` date (`YYYY-MM-DD`): once it passes, the website's tests fail until the flag is deleted or the date moves. `ops` and `permission` flags stay, and can't have one.

Who a flag is on for lives in the staff space and is set at `/admin/flags`, not here. Every flag starts off. The list there marks a flag "Ready to remove" once it's been on for everyone, or every account, for 30 days.

```jsonc
{
	"$schema": "../flag.schema.json",
	"key": "newPlanner",
	"description": "What the flag gates, for the staff console.",
	"type": "release",
	"removeBy": "2027-03-01"
}
```

Check a flag on the website with `locals.flags.newPlanner` on the server or `data.flags?.newPlanner` in a page.

## How a flag is decided

```text
bucket(key, did) = the first 4 bytes of sha256(key + did), big-endian, mod 100
on = enabled && (everyone || targeted || (did is known && bucket(key, did) < percentage))
```

`enabled` is false once someone turns the flag off for everyone, which keeps the other gates for when it's back on. A record that doesn't say is enabled. `targeted` means the account is one of the flag's accounts or in one of its groups. Only the website can know that, since those lists are private; it says so through `computer.localnetwork.flags.getFlags`, for clients that can't read them. Raising a percentage only adds accounts. Signed-out visitors have no DID, so they only get flags on for everyone.

`vectors.json` holds computed answers. The website's tests run them, and anything else that decides a flag has to give the same answers. A case without `enabled` is enabled. If a case has to change, the rule changed, and accounts will move in or out of every rollout.

## Trying a flag

- In development and tests, `FLAG_OVERRIDES=newPlanner=on,parityCanary=off` forces flags on or off for every request to that server. The website won't start with it set in production.
- Staff with the flags grant can turn a flag on or off for their own browser from its page at `/admin/flags`, for an hour. A banner says so on every page until it's cleared, and nobody else sees a difference.

[ADR 0010](../docs/adr/0010-flags-turn-off-and-preview-without-touching-targeting.md) covers turning a flag off and the overrides, and [Feature flag platforms](../docs/research/feature-flag-platforms.md) compares the design with hosted flag platforms.
