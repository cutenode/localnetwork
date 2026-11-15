### Changed

- **Signing out removes plugins' access, and signing in again restores it.** Signing out of an
  account, or its session ending, removes every plugin's access to it, and this computer remembers
  it. Signing in again asks: Restore (enter) gives it all back, Choose picks plugins and scopes,
  and Reset forgets it, in the terminal and the player. `account TYPE sign-in --restore` or
  `--reset` answers without asking; with neither and no terminal it stays off, and `account
  TYPE:NAME restore` gives it back later. `account [TYPE:NAME] --restore-automatically on` restores
  it without asking, saying in a line what came back. The sidecar's `accounts.sign_in.begin` takes
  `restore`, and `accounts.access.restore` and `accounts.restore_automatically` are new.
- Signing out of a plugin's default account leaves it with no default until you pick one, and
  `plugin access --give` refuses an account that's signed out, saying to sign in first.
