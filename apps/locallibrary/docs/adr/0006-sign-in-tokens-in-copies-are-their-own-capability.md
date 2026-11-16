# Sign-in tokens in a copy are a capability of their own

A plugin with `collection:files` can copy rekordbox's database, to back up the collection, and the
copy leaves out the cloud agent's sign-in unless the plugin asks for it, as rekordbox's own backup
keeps it. A copy with those tokens in it signs in to rekordbox's cloud and Dropbox as the DJ from
wherever it ends up: a NAS, a cloud folder, another person's computer. So a copy that includes
sign-in tokens needs its own capability, which the person agrees to explicitly and on its own when
they trust the plugin. `collection:files` never implies it, and a plugin without it that asks for
the tokens is refused rather than given a copy without them, so it can't believe it holds a whole
backup when it doesn't.

## Considered options

- Let `collection:files` include the tokens, with its trust prompt saying so: reading rekordbox's
  files to back them up is something most people would agree to, and most wouldn't stop to think
  that agreeing also hands over a working sign-in.
- Never copy the tokens: a backup restored on a new computer then needs rekordbox signed in again,
  which a person who agreed to the whole copy should be able to avoid.
- A per-copy question each time the tokens are asked for: a backup plugin runs on its own, often
  with nobody there to answer.
