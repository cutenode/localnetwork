### Added

- **Running a plugin you're writing.** `--dev-plugin MANIFEST:PROGRAM` (repeatable, `;` between
  the two on Windows), or `LOCALLIBRARY_DEV_PLUGINS`, runs a plugin locallibrary doesn't ship for
  that run only, on the command line, in the player and in the sidecar. It's held to a third
  party's rules: no command line places of its own, no `library:delete-unbacked`, no token
  exchange, and no id of a plugin locallibrary ships. Every run warns that it's loaded, naming its
  capabilities, and one that can't run fails the run, saying why.
- **`plugin check MANIFEST -- PROGRAM [ARGS...]`** checks such a plugin would run: its manifest by
  those rules, then its `hello` and `describe`, refusing anything it calls back for, listing the features both
  sides have, or what's wrong with exit status 4 (`--json` too).
