# locallibrary asks the website for flags

Who a feature flag is on for lives in the root's staff space, which only staff can read, so locallibrary can't read those records for a DJ. The website answers instead. `computer.localnetwork.flags.getFlags` gives every flag's public gates (on for everyone, and a percentage) and one private bit, `targeted`, saying the caller is one of the flag's accounts or in one of its groups. locallibrary applies the same rule the website does to those three values, which `flags/vectors.json` holds both to. It asks when something checks a flag and the answer it has is over an hour old, and keeps the last answer for 14 days for computers that are offline. [ADR 0010](0010-flags-turn-off-and-preview-without-touching-targeting.md) adds a fourth value, `enabled`, and [Feature flag platforms](../research/feature-flag-platforms.md) compares this split with the client and server SDKs hosted platforms ship.

The call is authenticated with a service token from the account's own server, as `requestIndex` is, and works without one. Without a token nothing is targeted, but the percentage still applies, since locallibrary knows its own DID and hashes it on the computer. That matters because a session needs a new scope to get a token, so every session signed in before this has none until it signs in again.

A flag in locallibrary is for rolling something out, not for keeping anyone out. The program and its cache are on the DJ's computer, so anything that has to be enforced goes on the server.

## Considered options

- Answering with one boolean per flag: nothing to keep in step, but no answer at all for a session without the scope, and nothing for a percentage to work with offline.
- Listing the groups and accounts in the answer so locallibrary could work everything out: it would publish who's in which group.
- Copying flags into a public repo or a space DJs can read: the same leak, and a second place to keep right.
- Asking for the method through the `locallibraryPublishing` permission set: servers look a set up when it's used, and lexicons aren't published yet.
- Asking at every start, or in the background: a command that has nothing to do with a flag would wait on the network, and locallibrary runs offline at gigs.
