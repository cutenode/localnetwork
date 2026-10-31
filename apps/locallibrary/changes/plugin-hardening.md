### Fixed

- A plugin's export whose files' sizes add up to 2^64 bytes or more is the plugin failing.
  Debug builds crashed on it, and release builds showed a wrong total in the plan.

### Security

- Everything a plugin tells the host (its manifest, its answers and the files it answers in, a
  store destination's records, its errors, its rows sent in parts, its progress and what it tells
  the user) has control characters and escape sequences removed as the host takes it in, and a
  newline, carriage return or tab becomes a space (a text view's text, and a command's answer that's
  only text, keep their lines). Only check findings, notices and the plugin's
  log were cleaned, so the rest could move the cursor, retitle the window or recolour the screen.
  A secret the host filled in for the plugin is still left out of its errors, log, notices and
  findings when it has a control character in it.
- A plugin's `net/fetch` refuses an address with a user name or password, also as a redirect's
  target, as account endpoints in a manifest already were.
