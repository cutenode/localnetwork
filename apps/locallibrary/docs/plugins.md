# Plugins

A plugin adds to locallibrary: commands, operations that change a library, analyzers that work
out values for each track, export formats and settings of its own. Each is a program of its own
that locallibrary starts when it's first used and talks to over its stdin and stdout (the
protocol is in [loclib-plugin](../../../crates/loclib-plugin)). A plugin never writes to a drive:
it reads the library through locallibrary, within what its capabilities allow, and an operation
hands locallibrary the edits it would make, which you're shown and asked about as any edit is,
backed up and journaled.

For now only plugins built into locallibrary run. There's no way to install another yet. Those
are:

- **sound**: works out each track's tempo, key and loudness from its audio, as `doctor
  --analyze` does (`locallibrary analyze`, then `sound.bpm`, `sound.key`... on the track).
- **tracklist**: `locallibrary playlist import LIBRARY FILE --into PLAYLIST` makes a playlist
  from a tracklist (see [Editing](editing.md)), and `locallibrary tracklist match LIBRARY FILE`
  shows what it finds in the list without changing anything.
- **interop**: the library for Traktor, Serato and Engine DJ (`locallibrary export LIBRARY
  --format traktor -o DIR`, see [Other DJ software](other-software.md#other-dj-software)).
- **coverart**: finds front covers for tracks without artwork on MusicBrainz and the Cover Art
  Archive (`locallibrary coverart fetch LIBRARY`, `--replace` for tracks with artwork too). It's
  the only one that goes online, and only when you run it.

## What you have

`locallibrary plugin` lists them, what each may do (its capabilities, in plain words) and what it
adds. `locallibrary plugin info ID` has one in full: its commands and how to run them, its
analyzers' fields, its export formats and its settings.

A plugin whose id, commands or export formats clash with locallibrary's, or with another plugin's,
is left out: every command warns which and why, and the other plugins work. Two plugins clashing
with each other are both left out.

## Where they show up

- **Commands and operations** are `locallibrary ID NAME` (`locallibrary coverart fetch`), or
  where the plugin puts them (`locallibrary playlist import`). Their params are flags (`--into
  Sets`, `--append`; `KEY=VALUE` pairs for a map, repeated), or JSON with `--params`.
  `locallibrary ID NAME --help` lists them. A command shows what it answers as a list or a table,
  or as text or JSON when it answers a plain value (`--json`: as it is); an operation shows its
  plan and asks first, like any edit (`--dry-run`, `--yes`), and `locallibrary undo` takes it
  back.
- **Analyzers** run with `locallibrary analyze [LIBRARY] [--plugin ID] [--analyzer NAME]`, which
  works out each track's values once (again only for a changed audio file) and keeps them in the
  store. `locallibrary track LIBRARY --track ID` shows them under the track's details, each as
  `ID.FIELD` (`sound.bpm`), and `locallibrary tracks` filters and sorts by them (`--filter
  "sound.bpm:>120" --sort -sound.bpm`), as the sidecar's `tracks.query` does.
- **Export formats** are `locallibrary export LIBRARY --format FORMAT -o PATH`, with the
  plugin's options as `--option KEY=VALUE`. `--dry-run` lists what it would write (each file,
  what goes in, what the format can't hold), and overwriting a file is asked about first unless
  `--replace`.
- **In the player** (`locallibrary tui`), commands, operations and analyzers are in the palette
  (`:`) and a drive's menu, operations in the actions menu (`e`) too, with a form for their
  params; analyzers' fields are columns to show, sort and search by, and each plugin's settings
  are a form in the palette too (see [the player](player.md)).
- **Settings**: `locallibrary plugin settings ID` shows them, `plugin settings ID KEY VALUE` sets
  one and `plugin settings ID KEY --reset` puts it back to its default. They're kept in the store
  (`plugins/ID/settings.json`).

The sidecar has all of this for desktop apps as `plugins.*` methods (see [Building a
GUI](sidecar.md)).

## Safety

A plugin is refused if it says it's something other than what locallibrary knows it as. It may
call only the reads its capabilities grant, naming drives by name or id (never a path), in pages of
bounded size, and the errors it gets back don't say where files are on this computer. None is
started from a process running as an administrator or root. One that stops answering is stopped,
and started again the next time it's needed.
