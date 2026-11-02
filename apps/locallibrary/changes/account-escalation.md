### Added

- **More of an account when a plugin needs it.** Signing in again asks the service for every
  scope the account's plugins use plus what's added, as the same account (`account TYPE:NAME
  sign-in` works for a signed-out account plugins still have access to), and keeps the refresh
  token when the service sends no new one. A plugin gets only the scopes the service granted, and
  is told which it declined; `plugin access PLUGIN --give ACCOUNT --scope S` gives one later.
  Before a plugin destination starts, and again before a publish goes out, its accounts are
  checked: signed in, with the scopes it needs, and a token that lasts the upload. A token a
  service refuses is refreshed once and the request tried again, the account signed out only
  when the service says the token is no good, and a service saying the token lacks a scope fails asking you to sign in again for it. google-account asks Google for offline
  access, so its accounts stay signed in.
