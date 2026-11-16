# Private events open from a link token

A private event is kept in its party's space with a token, and it opens for anyone who has the link carrying that token, signed in or not. The earlier plan kept link-only events in member-only spaces that the site renders, but a member-only space can't show a flyer link to someone without an account, and flyer links are how private nights get shared. Anyone with the link can see the event; anyone without it can't find it, and it's never listed.

An event posted without a party lives in the poster's own repo and is always public, so there's no private event without a party.

## Considered options

- Member-only spaces: only signed-in members of the space could open it, which breaks sharing a link with friends.
- Encrypting the event and putting the key in the link: the server couldn't render previews or check the venue's rules.
