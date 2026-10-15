# Plugins

A plugin adds to locallibrary: commands, operations that change a library, analyzers that work
out values for each track, export formats and settings of its own. Each is a program of its own
that locallibrary starts when it's first used and talks to over its stdin and stdout (the
protocol is in [loclib-plugin](../../../crates/loclib-plugin)). A plugin never writes to a drive:
it reads the library through locallibrary, within what its capabilities allow, and an operation
hands locallibrary the edits it would make, which you're shown and asked about as any edit is,
backed up and journaled.

For now only plugins built into locallibrary run. There's no way to install another yet.

## What you have

`locallibrary plugin` lists them, what each may do (its capabilities, in plain words) and what it
adds. `locallibrary plugin info ID` has one in full: its commands and how to run them, its
analyzers' fields, its export formats and its settings.

A plugin whose id, commands or export formats clash with locallibrary's, or with another plugin's,
is left out: every command warns which and why, and the other plugins work. Two plugins clashing
with each other are both left out.

## Where they show up

- **Commands and operations** are `locallibrary ID NAME` (`locallibrary tracklist import`), or
  where the plugin puts them (`locallibrary playlist import`). Their params are flags (`--into
  Sets`, `--append`; `KEY=VALUE` pairs for a map, repeated), or JSON with `--params`.
  `locallibrary ID NAME --help` lists them. A command shows what it answers as a list or a table,
  or as text or JSON when it answers a plain value (`--json`: as it is); an operation shows its
  plan and asks first, like any edit (`--dry-run`, `--yes`), and `locallibrary undo` takes it
  back.
- **Analyzers** run with `locallibrary analyze [LIBRARY] [--plugin ID] [--analyzer NAME]`, which
  works out each track's values once (again only for a changed audio file) and keeps them in the
  store. `locallibrary track LIBRARY --track ID` shows them under the track's details, each as
  `ID.FIELD` (`sound.bpm`).
- **Export formats** are `locallibrary export LIBRARY --format FORMAT -o PATH`, with the
  plugin's options as `--option KEY=VALUE`. `--dry-run` lists what it would write, and
  overwriting a file is asked about first unless `--replace`.
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
