### Added

- **Plugin commands planned first.** A plugin's command with `plan = true` in its manifest is
  asked for a dry run first: it says what it would do and which files it would write, and runs
  only once you confirm. On the command line `--dry-run` stops at the plan and `--yes` doesn't
  ask; the player asks as it does for any plan; the sidecar has `plugins.command.plan` and
  `plugins.command.apply`. A plan replacing a file there already is marked destructive, and one
  whose files changed since is refused as stale.
- **Long-running plugin commands.** A command with `long_running = true` shows its progress and
  gets a minute to stop once asked, instead of five seconds, before its plugin is stopped for it.
- **Plugin commands about rekordbox's collection.** `where = ["collection"]` puts a command with
  the collection on this computer rather than on a drive: it takes no drive, the player's palette
  offers it with no drive open, and `locallibrary collection --help` lists it.
- **A folder for what a plugin keeps.** Each hook run with a store is given
  `<store>/plugins/ID/data/` as `context.data_dir`, made first and never removed.
- **Plugins suggesting their commands.** `host/notify` may suggest one of the plugin's own
  commands (the action `command`), checked against its manifest like the other actions, and
  reviewed in the player as if run from the palette.

### Changed

- The plugin protocol is at 0.2.1, and the SDK has the feature `plans`.
