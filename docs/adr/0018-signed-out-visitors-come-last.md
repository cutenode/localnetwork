# Signed-out visitors come last

The rule that decides a flag buckets a DID, and a signed-out visitor has none. So a percentage only ever counted accounts, and turning a flag on for everyone showed it to every signed-out visitor in the same save that gave it to the last accounts. Public pages are where that matters. Most people who see an act, party, venue or event page aren't signed in, and crawlers and directory listings see what they see.

So signed-out visitors are a switch of their own on each flag, and the last one to turn on. `computer.localnetwork.staff.flag` has an optional `signedOut` boolean, off unless staff tick Signed-out visitors on the flag's page. The website refuses it until what's saved has the flag on for every signed-in account (`everyone`, or 100%), and a save that takes the flag below that turns it off again, so the order can't be skipped or left half undone. `everyone` now means every signed-in account. A caller with no DID gets a flag when `enabled && signedOut && (everyone || percentage >= 100)`, and a caller with one is decided as before, whatever `signedOut` says. `getFlags` sends `signedOut` with the other gates, locallibrary applies the same rule when it's signed out, and `flags/vectors.json` has cases for it.

Ops flags have no setting of their own. A kill switch has to work where there's no account: xrpc routes, the server's background rounds, crawlers and locallibrary signed out all ask with no DID. An ops flag's `signedOut` is on whenever it's on for every signed-in account, `everyone` or 100%, so a kill switch saved at 100% pauses background rounds too. Saving one writes it that way, and a record without the field reads that way. A release flag's record without it reads as off.

No flag puts anything in a signed-out visitor's browser, so every signed-out visitor gets the same flags, and those pages stay shared-cacheable. The only cookie one can have is the language they picked ([0019](0019-english-stays-unprefixed-and-records-carry-their-language.md)), which the cache varies on.

## Considered options

- A random id in a first-party cookie, bucketed like a DID, so a share of signed-out browsers could get a flag. The maintainer turned it down: signed-out visitors should only get a feature when it's switched on for them, after everyone signed in has it. It also added a cookie to the privacy page, made signed-out pages private while a percentage ran, and showed crawlers a page some of the time.
- Keeping `everyone` as signed in or not: no new field, but a release flag couldn't reach every account without reaching every visitor in the same save.
- Letting a flag's `showsOn` decide whether signed-out visitors are counted: it's a list for the staff console, and a flag that changes something signed-out visitors meet without naming a kind of page would slip past it.
- The same setting on ops flags: a kill switch turned on for everyone and not for signed-out visitors would leave everything with no account running.

## Consequences

- A release flag that was on for everyone before this is off for signed-out visitors until staff tick the setting.
- `spacePages` writes directory listings only once signed-out visitors have it, since a listing sends them to the page.
- The console marks a release flag "Ready to remove" only once signed-out visitors have had it for 30 days too, since deleting a flag gives it to every caller.
- `FLAG_OVERRIDES` forcing a flag on turns it on for signed-out visitors as well.
- A feature can't be tried on a share of signed-out visitors. One that needs that would need a new decision.
