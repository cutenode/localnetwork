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

- **sound**: works out each track's tempo, key and loudness from its audio (`locallibrary
  analyze`, then `sound.bpm`, `sound.key`... on the track). It's what `doctor --analyze`, the
  check before a set and ordering a set read tempos, keys and levels from (see [Roles](#roles)).
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
adds. `locallibrary plugin info ID` has one in full: its commands, how to run them and which of
the player's menus offer them, its analyzers' fields, its export formats and its settings.

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
  works out each track's values once (again only for a changed audio file, or a new version of
  the analyzer) and keeps them in the store. A value worked out with nothing to show (silence's
  key) shows as `-`, matches no filter and sorts last. `locallibrary track LIBRARY --track ID`
  shows them under the track's details, each as `ID.FIELD` (`sound.bpm`), and `locallibrary
  tracks` filters and sorts by them (`--filter "sound.bpm:>120" --sort -sound.bpm`), as the
  sidecar's `tracks.query` does.
- **Roles**: what locallibrary itself reads from analyzers, see [Roles](#roles). `locallibrary
  analyze LIBRARY --apply key,bpm` then writes what the analyzer heard to the tracks, as an edit
  you're shown and asked about (only where a track has none unless `--overwrite`;
  `--min-confidence 0.8` leaves out what it's less sure of; `--dry-run`), and `undo` takes it
  back.
- **Export formats** are `locallibrary export LIBRARY --format FORMAT -o PATH`, with the
  plugin's options as `--option KEY=VALUE`. `--dry-run` lists what it would write (each file,
  what goes in, what the format can't hold), and overwriting a file is asked about first unless
  `--replace`.
- **In the player** (`locallibrary tui`), commands, operations, analyzers and export formats are
  in the palette (`:`) and a drive's menu, and commands and operations a plugin offers for tracks
  or a playlist in the actions menu (`e`) too, with a form for their params or options, a param
  the plugin fills from what's selected (the tracks, the playlist, the drive) starting as it; an
  export shows what it would write and asks before writing it; analyzers' fields are columns to
  show, sort and search by, "write keys and BPMs to the tracks: …" writes what one heard, and
  each plugin's settings are a form in the palette too (see [the player](player.md)).
- **Settings**: `locallibrary plugin settings ID` shows them, `plugin settings ID KEY VALUE` sets
  one and `plugin settings ID KEY --reset` puts it back to its default. They're kept in the store
  (`plugins/ID/settings.json`, beside the plugin's own folder).

The sidecar has all of this for desktop apps as `plugins.*` methods (see [Building a
GUI](sidecar.md)). A plugin's command or operation may fill a param from what's selected: the
command line gives it the drive named, if that's what it takes, and the param's flag gives it
otherwise.

## Roles

An analyzer's manifest may say what some of its fields are to locallibrary: a track's key, its
tempo (BPM), how sure it is of each, its loudness, true peak and clipped runs, or what tells one
recording from another. Doctor's analysis, the check before a set, ordering a set (`playlist
order`) and `analyze --apply` read those from one analyzer each, the sound plugin's unless
another does too: `locallibrary plugin roles` shows which, `plugin roles key PLUGIN.ANALYZER`
chooses another that has it, and `plugin roles key --reset` puts it back (kept in the store's
`plugins/settings.json` as `analysis.roles.key`). `doctor --analyze` runs those analyzers first
on the tracks they haven't worked out; the check and ordering a set read what they kept.

## Writing one

A plugin in Rust uses [loclib-plugin](../../../crates/loclib-plugin)'s SDK: a function for each
kind of hook it has, and `host.drive(drive)` to read a drive (the library, tracks by id or by
query, playlists, analysis, artwork and `drive.info`, each read whole however locallibrary pages
it). Each family of hooks is an optional feature of the protocol, so a newer locallibrary adds
kinds of hooks without breaking older plugins.

## Safety

A plugin is refused if it says it's something other than what locallibrary knows it as. It may
call only the reads its capabilities grant, naming drives by name or id (never a path), in pages of
bounded size, and the errors it gets back don't say where files are on this computer. None is
started from a process running as an administrator or root, and each runs in a folder of its own
in the store (`plugins/ID/work/`). One that stops answering is stopped, and started again the next
time it's needed.
