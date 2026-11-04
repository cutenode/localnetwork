### Fixed

- A file of plugin secrets, plugin settings, account sessions or what the player remembers that
  can't be read right now (its permissions, say) is left alone. Changing a secret or setting, or
  signing in or out, says why it can't, where it used to write over the file with only that
  change, losing the rest; forgetting a secret or signing out could delete the whole file.
  `settings import` and `settings export` stop the same way, and a write to a drive whose
  `state.json` can't be read stops before it starts, where a failed write used to delete that file.
