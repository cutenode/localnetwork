### Added

- **Scopes asked for when a plugin needs them.** A plugin asks for the scopes it can do without
  as it needs them (`accounts/ensure`, `Host::ensure_account` in the SDK), and learns which it
  may use and which you declined, never a token. The terminal and the player ask you first
  (signing in again when the account wasn't granted them), and a no is remembered until `plugin
  access --give` gives the scope; with `--json`, in `watch` and in the sidecar it fails saying how
  to give it.
- **When an account signs itself out.** `account` says when each account signs itself out (its
  token expiring unrefreshed, or its refresh token running out when the service says), and in its
  last two days it's a warning there, before a destination uses it (offering to sign in again) and
  as the player starts; the Atmosphere account's "sign in again by" turns into a warning then too,
  and publishing offers to sign in again first.
