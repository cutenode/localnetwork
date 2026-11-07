# Some account types are signed in to with keys

Some services hand out keys instead of signing people in with OAuth: the Internet Archive's S3
keys, a plyr.fm developer token. An account type can say it's signed in to with keys (`kind =
"keys"`), naming each key and where the user gets them, and the user enters them once. Account
plugins stay manifests ([ADR 0001](0001-account-plugins-are-manifests.md)): the type says how the
keys make the `Authorization` header (`LOW {access}:{secret}`), and locallibrary keeps them as it
keeps tokens, once per account, and fills them in only where a plugin's access covers the
request. Keys go only in that header, never in a query, because addresses end up in logs and
history, and a key in one is what CodeQL's `rust/cleartext-transmission` flags. A type's whoami
may only be at a host its scopes' requests go to, since it's sent the keys, so a third-party
account plugin can't send someone's keys anywhere else. Nothing refreshes keys: when a service
refuses them, the plugin gets the answer as it is, and the account stays signed in.

## Consequences

- The archive and plyr plugins upload with an `archive` or `plyr` account instead of secret
  settings, so a person who set those signs in once instead.
- A service that only takes keys in the address isn't supported.
