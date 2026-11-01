### Added

- **Accounts kept once per store.** Plugins ask for accounts by type (`[[accounts]]`, with the
  scopes they need and why): youtube for a google account, mixcloud and soundcloud for one of
  their own type, each to upload with. An account is signed in to once, through an account
  plugin, and kept in the store; a plugin uses it through the access you give it, which you can
  make its default for the account type or take away without signing out, and never sees its
  token: a request names the account as `{account:TYPE}`, and the token goes in only where a
  scope of the access covers the request. `account` lists them after the DJ account, `account
  TYPE sign-in` and `account TYPE:NAME sign-out` sign in and out, and `plugin access PLUGIN`
  makes an account its default or takes its access away. A plugin's destination asks in the
  terminal which account it may use when it has none; elsewhere it fails saying how to give it
  one. The player's palette signs in and out, and has a form for a plugin's access. The sidecar
  has them as `accounts.*`. Plugins no longer have accounts of their own: `plugin account` and
  the sidecar's `plugins.account*` are gone, and `plugin info` and `plugins.describe` show the
  access a plugin has instead.
