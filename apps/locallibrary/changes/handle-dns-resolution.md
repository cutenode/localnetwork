### Fixed

- Signing in to the Atmosphere account, and finding an account by handle, work with a handle set
  up only in DNS (an `_atproto` TXT record). Its DNS record was never looked up, so the handle
  wasn't found unless its website served `/.well-known/atproto-did`.
