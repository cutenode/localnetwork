### Fixed

- A secret setting a plugin's request puts in the address comes back to the plugin as
  `{secret:<key>}` when the server says the address back (a next page's link, a `Location`, its
  text), as an account's token in the address already did. The plugin was handed the secret.
