# Signed-out visitors get a rollout id

The rule that decides a flag buckets a DID, and a signed-out visitor has none, so a percentage only ever counted accounts. That was fine while new features were for signed-in DJs. Public pages are different: most people who see an act, party, venue or event page aren't signed in, so a flag like `spacePages` could only go from a staff preview straight to everyone.

So while a flag that changes a public page is on for a percentage, the website gives a signed-out browser an id of its own: a random version 4 UUID in a first-party cookie, `ln_flag_visitor`, for the site's whole path, `HttpOnly`, `SameSite=Lax`, `Secure` over https, kept for a year. The rule buckets `visitor:` followed by that UUID with the same sum it uses for a DID, so a browser gets the same answer on every request and stays in as the percentage grows. A DID starts `did:`, so a visitor's id is never an account's and is never targeted. `flags/vectors.json` has cases for visitor ids, which locallibrary's test reads as it reads any other id.

Nothing else asks differently. `getFlags` and locallibrary ask with a DID or with nothing, and an xrpc call never reads a cookie, so a caller with neither still only gets flags on for everyone. The cookie is only set for a signed-out visitor outside `/xrpc/`, and only while a flag with `showsOn` in its catalog file is enabled, not on for everyone, and above 0%. A flag without `showsOn`, like `inboxEmail` or `userinputReports`, changes only what accounts see, so rolling it out to a percentage sets no cookie and keeps signed-out pages shared-cacheable. A flag that does change a public page has to list it in `showsOn`, or signed-out visitors stay out of its percentage. The server uses the id for flags alone and never stores or logs it; a request's log line only says whether the visitor's id was new.

A signed-out page that depends on the id can't go in a shared cache. `cacheBriefly` marks signed-out pages `public`, and a visitor's first response sets the cookie, so a cache in front of the site could serve one visitor's answer and their cookie to the next. Nothing caches in front of the site today, but while a visitor id is in use the server hook turns `public` into `private` on the response. Browsers still cache their own copy.

## Considered options

- Leaving signed-out visitors out of percentages, as before: no cookie, and no gradual rollout of a public page.
- Bucketing by IP address or user agent: many people share an address, one person's changes between networks, and the hash would be built from personal data.
- An id in local storage, as LaunchDarkly's browser SDK keeps one: pages are rendered on the server, which can't read it on any request.
- Bucketing on the flag key alone, as Statsig does for an empty id: every visitor gets the same answer, so a percentage becomes a switch.
- Setting the cookie on every signed-out request: most of the time no flag needs it, and signed-out pages would never be shared-cacheable.
- Setting it while any flag is on for a percentage: a rollout of something only accounts see, like inbox email, would give every signed-out browser a cookie and make every public page private for as long as it ran.
- Relying on `Vary: Cookie`, which `cacheBriefly` already sends: a cached first response has no cookie to vary on, and carries its `Set-Cookie` to everyone after.

## Consequences

- Signing in moves someone from their browser's bucket to their account's, so a feature can appear or disappear for them at that moment. PostHog documents the same problem.
- Crawlers and anything else that drops cookies get a new id on every request, so during a percentage rollout they see the flag on at about that rate. A page that 404s while its flag is off will 404 to a crawler some of the time. `spacePages` writes no directory listing until it's on for everyone, which keeps those pages out of the directory meanwhile.
- A visitor can clear or edit the cookie to land somewhere else. Flags are for rolling something out, not for keeping anyone out, so that's allowed.
- While a flag with `showsOn` is on for a percentage, no signed-out response outside `/xrpc/` is shared-cacheable.
