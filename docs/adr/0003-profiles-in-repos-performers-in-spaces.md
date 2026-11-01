# Profiles live in people's repos, acts and parties live in spaces

Every account has one profile, written to its own repo like a Bluesky profile, and being a DJ or a promoter is something the profile opts into. Anything several people share (an act, a party, a venue with owners, a party's events) lives in a space whose authority is the Local Network root account and whose write members are the people working on it. The site reads those spaces as the root to render public pages, so private parts, including members who chose to hide, never leave the space. A DJ under their own name keeps the same kind of space for backstage work, shared with their managers.

Spaces can't be read by logged-out visitors or indexed by HappyView, so when staff list an act or party in the directory, or an event is public, the root writes a public record (a listing, or a `community.lexicon.calendar.event`) with only what its public page already shows. The listing is regenerated from the space on every change and never edited directly, the same one-way copy shows already make of their calendar event.

This replaces root-owned project records with a public `managers` list. Backstage merges into the performer's space; press kits keep their own spaces from [0002](0002-shared-press-kits-in-root-owned-spaces.md) so grantees can read a kit without reading drafts.

## Considered options

- Everything root-owned in public repos, as projects were: edits needed staff review, and member lists were public, which rules out anonymous aliases.
- Acts written to the creating member's repo: the other members can't edit them.
- An account per act: every act needs its own login and PDS, for no gain over a shared space.
- No public listing, with directory search moved onto the site's own index of spaces: DJ profiles live in public repos that HappyView indexes, so "Find DJs" would merge two indexes.
- Hiding members only on the site: the data would still be public on the network, so hidden would be a promise the site can't keep.
