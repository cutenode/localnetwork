# Names repeat; keys and handles identify

Two acts, parties or venues can share a name, and the site shouldn't make the second one an ugly copy. Making names unique (first come, first served, then `nightshift-berlin` or `nightshift-2`) punishes whoever arrives later and gives nobody a reason to invest in their name. AT Protocol already has the answer: display names repeat, and a handle (a verified domain) is the unique name you can build a brand on.

So an act, party or venue is identified by its space key, and its name is a display name that can repeat. Its page is `/act/<key>/<name>`, `/party/<key>/<name>` or `/venue/<key>/<name>`, where the name part is decorative: it's ignored when the page is looked up, and the page redirects to the current name after a rename. An act or party that links its own Atmosphere account gets its page at `/@<handle>`, the same as a person, so a handle on its own domain is how it gets a clean address. Wherever two of the same name could be confused (search, pickers, mentions, credits), a second line shows the city and a few member avatars.

This amends [0012](0012-pages-follow-what-people-look-at-and-link-to-each-other.md)'s `/act/<slug>`, `/party/<slug>` and `/venue/<slug>`. Venues are still one per real place, but they're matched by place, not by a unique name.

## Considered options

- Unique slugs with a city or number suffix: the way other listings sites do it, and the reason second arrivals look like knock-offs.
- Each act as its own atproto account with a DID: overturns [0003](0003-profiles-in-repos-performers-in-spaces.md)'s root-owned spaces and [0004](0004-hidden-members-speak-as-the-act.md)'s hidden members, and deleting one account would delete the act.
- Names scoped to a person (`/@person/nightshift`): reveals who is behind an anonymous alias and ties a shared act to one member.
- A free `name.localnetwork.computer` handle for everyone: still first come, first served, so it's a later convenience once there's a Local Network PDS, not the answer to collisions.
