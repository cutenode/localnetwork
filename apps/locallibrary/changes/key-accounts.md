### Added

- **Accounts signed in to with keys.** An account type can be signed in to with the keys its
  service gives you instead of the browser (`kind = "keys"` in an account plugin, with each key's
  name, where you get them, and the `Authorization` header they make). `account TYPE sign-in`
  says where to get them and reads them from stdin, unseen in a terminal or a line each when
  piped, then the account's name when the service doesn't say whose they are. The player's
  palette opens a form for them, the sidecar takes them in `accounts.sign_in.finish {id, keys,
  name?}`, and when a plugin asks for more of such an account that's signed out, the terminal and
  the player ask for its keys. They're kept where an account's tokens are, once per account, and
  go only in the `Authorization` header, never in an address. Nothing refreshes them: a service refusing them is
  answered to the plugin as it is, the account still signed in, and keys refused at sign-in say
  to check them where you got them.

### Changed

- archive and plyr upload with an `archive` or `plyr` account instead of secret settings, and no
  longer read the S3 keys or developer token set with `plugin settings ... --secret`. Sign in
  once with `locallibrary account archive sign-in --for archive` (`account plyr sign-in --for
  plyr`).
