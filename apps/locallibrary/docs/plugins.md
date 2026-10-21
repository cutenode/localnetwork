# Plugins

A plugin adds to locallibrary the kinds of things below (commands, operations that change a
library, analyzers that work out values for each track, export formats...), each in a section of
its own. It's a program of its own that locallibrary starts when it's first used and talks to over
its stdin and stdout (the protocol is in [loclib-plugin](../../../crates/loclib-plugin)). A plugin
never writes to a drive: it reads the library through locallibrary, within what its capabilities
allow, and an operation hands locallibrary the edits it would make, which you're shown and asked
about as any edit is, backed up and journaled.

For now only plugins built into locallibrary run. There's no way to install another yet. Those
are:

- **sound**: works out each track's tempo, key and loudness from its audio (`locallibrary
  analyze`, then `sound.bpm`, `sound.key`... on the track). It's what `doctor --analyze`, the
  check before a set and ordering a set read tempos, keys and levels from (see [Roles](#roles)).
- **tracklist**: `locallibrary playlist import LIBRARY FILE --into PLAYLIST` makes a playlist
  from a tracklist (see [Editing](editing.md)), asking which track an entry is when it could be
  several.
- **interop**: the library for Traktor, Serato and Engine DJ (`locallibrary export LIBRARY
  --format traktor -o DIR`, see [Other DJ software](other-software.md#other-dj-software)).
- **coverart**: finds front covers on MusicBrainz and the Cover Art Archive for `locallibrary art
  fetch` (see [Providers](#providers)). It's the only one that goes online, and only when you run
  it, through locallibrary (see [Safety](#safety)), a request a second to MusicBrainz as it asks.

## What you have

`locallibrary plugin` lists them, what each may do (its capabilities, in plain words) and what it
adds. `locallibrary plugin info ID` has one in full: its commands, how to run them and which of
the player's menus offer them, its analyzers' fields, its checks, its export formats, its
providers and its settings.

A plugin whose id, commands or export formats clash with locallibrary's, or with another plugin's,
is left out: every command warns which and why, and the other plugins work. Two plugins clashing
with each other are both left out.

## Where they show up

On the command line, what a plugin adds runs as commands of its own or of locallibrary's
(`analyze`, `export --format`, `art fetch`). In the player (`locallibrary tui`) it's in the
palette (`:`), and a drive's menu or the actions menu (`e`) where it works on a drive, tracks or a
playlist, with a form for what it takes, a param the plugin fills from what's selected (the
tracks, the playlist, the drive) starting as it (see [the player](player.md)). The sidecar has all of it for desktop apps as
`plugins.*` methods (see [Building a GUI](sidecar.md)). Each kind is below, in a section of its
own.

## Analyzers

`locallibrary analyze [LIBRARY] [--plugin ID] [--analyzer NAME]` runs them, which works out each
track's values once (again only for a changed audio file, or a new version of the analyzer) and
keeps them in the store. A value worked out with nothing to show (silence's key) shows as `-`,
matches no filter and sorts last. `locallibrary track LIBRARY --track ID` shows them under the
track's details, each as `ID.FIELD` (`sound.bpm`), and `locallibrary tracks` filters and sorts by
them (`--filter "sound.bpm:>120" --sort -sound.bpm`), as the sidecar's `tracks.query` does. In
the player, analyzers are in the palette and a drive's menu, and their fields are columns to
show, sort and search by. What locallibrary itself reads from them is their [roles](#roles).

## Checks

A plugin's check looks for something on a drive, and what it finds is in `locallibrary check` and
`locallibrary doctor` (`--only checks` for theirs alone), named `PLUGIN.CHECK`. A problem it finds
is a problem (`check` exits 4), a warning a warning; when a fix goes with it, the command line
that runs it follows (the plugin's operation, planned and asked about as any is). A check that
fails or takes longer than 10 seconds is a warning saying so, never a failure. A guard is a check
run before changes to a library are made (edits, cues, a playlist restored, a contact's
playlist): what it says is among the plan's warnings, and once you make it
blocking (`locallibrary plugin checks PLUGIN.CHECK --blocking`), a warning or a problem it finds
stops the changes. `locallibrary plugin checks` lists them, whether each runs and blocks, and how
long one has; `--on`, `--off`, `--no-blocking`, `--reset` and `--timeout SECS` (an hour at
most) change that (kept
in the store's `plugins/settings.json` as `checks.PLUGIN.CHECK.on`, `.blocking` and
`checks.timeout`). In the player, a check's results have plugins' findings too, their fixes are
in the palette until the next check ("fix on DRIVE: ..."), and "check settings…" there is a form
for which run.

## Commands and operations

They're `locallibrary ID NAME`, or where the plugin puts them (`locallibrary playlist import`, the
tracklist plugin's `import`). Their params are flags (`--into Sets`, `--append`; `KEY=VALUE` pairs
for a map, repeated), or JSON with `--params`. `locallibrary ID NAME --help` lists them. A command
shows what it answers as a list or a table (its rows as they come, when the plugin sends them so),
or as text or JSON when it answers a plain value (`--json`: as it is); an operation shows its plan
and asks first, like any edit (`--dry-run`, `--yes`), and `locallibrary undo` takes it back.
Either may ask you something first (which track an entry is, say): in a terminal you pick from a
numbered list; otherwise, or with `--json`, it stops and says how to answer with `--choose
PARAM=VALUE`. In the player they're in the palette, and in a drive's menu or the actions menu
where the plugin offers them, and a question one asks is a list to pick from. On the command
line, a param filled from what's selected is given the drive named, if that's what it takes, and
its flag gives it otherwise.

## Export formats

They're `locallibrary export LIBRARY --format FORMAT -o PATH`, with the plugin's options as
`--option KEY=VALUE`. `--dry-run` lists what it would write (each file, what goes in, what the
format can't hold), and overwriting a file is asked about first unless `--replace`. In the player
they're in the palette and a drive's menu: an export shows what it would write and asks before
writing it.

## Providers

A provider finds cover art or details (tags) for tracks, online usually: `locallibrary art fetch
[LIBRARY]` cover art, and `locallibrary track fetch [LIBRARY]` track details (title, artist,
album, year, genre, label, ISRC). They ask every provider of that kind about a library's tracks
(`--track`, `--playlist`, `--filter`... for some), only those without artwork, or missing a tag,
unless `--replace`; `--from PLUGIN` (or `PLUGIN.PROVIDER`) asks only that one. What they find for
each track is listed best first, with how sure each provider is. One alone scoring `plugin
providers`' auto-pick or more (0.9 unless `plugin providers --auto-pick 0.8`, or "provider
settings…" in the player's palette, says otherwise, kept in the store as `providers.auto_pick`) is
chosen for you; you pick the others, in a terminal, or with `--choose TRACK=N` (`--choose
812=null` for none). Then locallibrary plans the changes itself, cover art set on the tracks or
the tags they lack filled in (over what they have, with `--replace`), shown and asked about as any
edit is, and `undo` takes them back. In the player, "fetch cover art…" and "fetch track details…"
in the actions menu ask the providers about the tracks selected. A provider never changes a
library itself, and needs no capability to: it's given the tracks' tags (`library:read`) and
answers what it found. Of what one answers, locallibrary keeps 25 candidates a track and 50,000 in
all, and images coming to 512 MiB; past that it leaves the rest out and says so.

## Roles

An analyzer's manifest may say what some of its fields are to locallibrary: a track's key, its
tempo (BPM), how sure it is of each, its loudness, true peak and clipped runs, or what tells one
recording from another. Doctor's analysis, the check before a set, ordering a set (`playlist
order`) and `analyze --apply` read those from one analyzer each, the sound plugin's unless
another does too: `locallibrary plugin roles` shows which, `plugin roles key PLUGIN.ANALYZER`
chooses another that has it, and `plugin roles key --reset` puts it back (kept in the store's
`plugins/settings.json` as `analysis.roles.key`). `doctor --analyze` runs those analyzers first
on the tracks they haven't worked out; the check and ordering a set read what they kept.

`locallibrary analyze LIBRARY --apply key,bpm` writes what the analyzer heard to the tracks, as an
edit you're shown and asked about (only where a track has none unless `--overwrite`;
`--min-confidence 0.8` leaves out what it's less sure of; `--dry-run`), and `undo` takes it back.
In the player, "write keys and BPMs to the tracks: …" does the same.

## Settings

`locallibrary plugin settings ID` shows them, `plugin settings ID KEY VALUE` sets one and `plugin
settings ID KEY --reset` puts it back to its default. They're kept in the store
(`plugins/ID/settings.json`, beside the plugin's own folder). A secret one (a password, a key) is
set with `plugin settings ID KEY --secret`, which reads it from stdin without showing it, and is
kept in the system's keychain (or with `LOCALLIBRARY_SESSION_STORE=file` in
`secrets/plugins/ID.json`, which only you can read, apart from the plugin's folder): it's only
ever shown as set or not set (or unknown, with why, when the keychain can't say), and never
copied off the computer. In the player, each plugin's settings are a form in the palette.

## Writing one

A plugin in Rust uses [loclib-plugin](../../../crates/loclib-plugin)'s SDK: a function for each
kind of hook it has, `host.drive(drive)` to read a drive (the library, tracks by id or by
query, playlists, analysis, artwork and `drive.info`, each read whole however locallibrary pages
it), `host.fetch(..)` to go online and `host.partial_rows(..)` to send a view's rows as they're
ready. A command or an operation may answer a question for you (`Choice`) instead, and is run
again with your answer as one of its params. Each family of hooks is an optional feature of the
protocol, so a newer locallibrary adds kinds of hooks without breaking older plugins.

## Safety

A plugin is refused if it says it's something other than what locallibrary knows it as. It may
call only the reads its capabilities grant, naming drives by name or id (never a path), in pages of
bounded size, and what it gets back, errors too, doesn't say where things are on this computer:
a drive is said by its name, where the store's copies, a set's recording and rekordbox's
collection are is left out, and any other path is said from its drive, the store (`<store>`) or
your home folder (`~`). An exporter alone is told where the drive it exports is, as the files it
writes name the audio there. None is started from a process running as an administrator or
root, and each runs in a folder of its own in the store (`plugins/ID/work/`). One that stops
answering is stopped, and started again the next time it's needed.

A plugin goes online through locallibrary, which fetches only from the servers its `net:`
capabilities name (`plugin info` lists them), at public https addresses, following redirects only
to those, as `locallibrary/VERSION` with the plugin's contact setting if it has one, as often as
its manifest says each server allows. A secret setting goes only where such a request names it,
filled in by locallibrary, never on to another server a redirect leads to, and is left out of the
plugin's log and of the errors it's told (what a server answers is passed on as it is).
Images and other large answers come back as files in a folder locallibrary makes for each request
(`plugins/ID/scratch/`), which it reads itself, never past the folder (nor a link, or a file with
another name) or more than 512 MiB of them, and removes; `store gc` removes those a locallibrary
that crashed left.

A plugin runs as you and isn't sandboxed, so this is what locallibrary does, not a wall your
system keeps: a plugin could still read any file you can.
