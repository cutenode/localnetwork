### Changed

- **Publishing says when it isn't open yet, before the browser.** Until Local Network publishes
  the permission publishing asks for, no account's server can grant it. `account login
  --publish`, `profile publish`, `sets publish` and `settings publish` now check for it first
  (its `_lexicon.localnetwork.computer` DNS record) and say publishing isn't open yet, instead of
  asking you to sign in again and failing in the browser with `invalid_scope`. `--dry-run` still
  shows what would be published. It opens by itself once the permission is published, and a
  server's `invalid_scope` refusal for it is explained the same way.
