### Added

- **Your own keys in the player.** "change the keys" in the palette, or its row in the help, opens
  the Keys screen: pick any action, plugins' commands included, and press the key you want. It says
  what that key would do everywhere before keeping it, and offers a swap when another action has
  it. A key can be a capital, `ctrl-` and a letter, `shift-` and a named key, or `f1` to `f12`.
  The footer, the help, the palette, the tabs and the actions menu show your keys. "all keys back
  to their defaults" puts them back.
- **Your keys follow your account.** Signed in, the player publishes them, and signing in on
  another computer loads them, showing what differs first when that computer has its own.
  "import someone's keys…" takes anyone's by their handle, and "keys for an AZERTY keyboard" moves
  the views and hot cues onto the digit row's unshifted keys. Signing in from the player now asks
  to publish your keys.
- `settings export` carries the keys you set (`keybinds.json`).

### Changed

- A plugin command's key is set on the Keys screen or in `keybinds.json`, as
  `plugin.ID.NAME`, instead of in the plugin's settings. The player moves `keys.NAME` settings over
  the first time it starts.
