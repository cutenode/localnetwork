# Account plugins are manifests, and locallibrary runs every sign-in

Each account type comes from an account plugin that anyone can publish. An account plugin is only
a manifest: endpoints, client, scopes and the requests each scope covers. locallibrary runs every
sign-in from it, keeps the tokens, and fills them into a plugin's requests only where its access
covers them. This makes account plugins safe to install from anyone before plugin code can be
installed, and no plugin ever holds a token. Because tokens only refresh through the client that
issued them, an account belongs to the account plugin that signed it in, so the same person signed
in through two account plugins is two accounts. Local Network's token exchange adds Local
Network's own client secret, so anything using it signs people in as Local Network; only built-in
account plugins may use it.

## Considered options

- Accounts declared by each plugin (the old model): every plugin signs in again and keeps its own
  tokens.
- Account plugins with code: custom flows like OAuth 1 become possible, but the plugin handles
  tokens, and third-party code can't be installed yet.
- Any account plugin using the token exchange: third parties could ask for scopes under Local
  Network's name and use its quota.
