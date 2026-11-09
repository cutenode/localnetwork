# Feature flags

The website's feature flags, kept here so locallibrary can use the same ones. Each one is a file in `catalog/`, named for its key, which `flag.schema.json` checks. To add a flag, add a file. To remove one, delete its file, and the website stops compiling wherever it's still used.

Who a flag is on for lives in the staff space and is set at `/admin/flags`, not here. Every flag starts off.

```jsonc
{
	"$schema": "../flag.schema.json",
	"key": "newPlanner",
	"description": "What the flag gates, for the staff console."
}
```

Check a flag on the website with `locals.flags.newPlanner` on the server or `data.flags?.newPlanner` in a page.

## How a flag is decided

```text
bucket(key, did) = the first 4 bytes of sha256(key + did), big-endian, mod 100
on = everyone || targeted || (did is known && bucket(key, did) < percentage)
```

`targeted` means the account is one of the flag's accounts or in one of its groups. Only the website can know that, since those lists are private; it says so through `computer.localnetwork.flags.getFlags`, for clients that can't read them. Raising a percentage only adds accounts. Signed-out visitors have no DID, so they only get flags on for everyone.

`vectors.json` holds computed answers. The website's tests run them, and anything else that decides a flag has to give the same answers. If a case has to change, the rule changed, and accounts will move in or out of every rollout.
