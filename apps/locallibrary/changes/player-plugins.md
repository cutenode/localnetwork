### Fixed

- The player heads a plugin's panel with its view's own title after the panel's, as `track show`
  does, and asks for the selected track's panels again once that plugin's settings are saved,
  where it kept showing what they answered before until the selection moved.
- Smart playlist rules, edits' track selectors (`--filter`) and sorting a playlist on the drive
  (`playlist sort`, the player's `s`) take analyzers' fields (`sound.bpm:>120`, `-sound.bpm`), as
  `tracks` does, where they were refused. A rule naming a field no plugin installed declares, or
  a value its field can't hold, is refused when it's added and when it's applied, saying so, and
  an edit whose filter names a field no track has a value of yet warns that they need analyzing.
- Plugins' commands, operations, panels, analyzers, exporters, importers, library sources and
  providers run on a folder the player (or any command) opened by path, where the player said
  "only on a plugged-in drive". The plugin may name that folder as its hook's context does, only
  while the hook runs, and is told its name rather than where it is; a path or another folder is
  still refused.
