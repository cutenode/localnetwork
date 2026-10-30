# Plugins

A plugin adds to locallibrary the kinds of things below (commands, operations that change a
library, analyzers that work out values for each track, export and import formats, accounts,
places to copy the store to...), each in a section of its own. It's a program of its own that
locallibrary starts when it's first used and talks to over its stdin and stdout (the protocol is in
[loclib-plugin](../../../crates/loclib-plugin)). A plugin never writes to a drive: it reads the
library through locallibrary, within what its capabilities allow, and an operation hands
locallibrary the edits it would make, which you're shown and asked about as any edit is, backed up
and journaled.

For now only plugins built into locallibrary run. There's no way to install another yet. Those
are:

- **sound**: works out each track's tempo, key and loudness from its audio (`locallibrary
  analyze`, then `sound.bpm`, `sound.key`... on the track). It's what `doctor --analyze`, the
  check before a set and ordering a set read tempos, keys and levels from (see [Roles](#roles)).
  It also works out a three-band waveform, which the player draws for a track whose drive has
  none (see [Analysis](#analysis)).
- **tracklist**: `locallibrary playlist import LIBRARY FILE --into PLAYLIST` makes a playlist
  from a tracklist (see [Editing](editing.md)), asking which track an entry is when it could be
  several. It reads rekordbox XML collections too, as an import format (`rekordbox-xml`) and a
  library source (see [Other DJ software](other-software.md#rekordbox-xml)).
- **interop**: the library for Traktor, Serato and Engine DJ, and as rekordbox XML
  (`locallibrary export LIBRARY --format traktor -o DIR`, or `locallibrary xml`), and a set or a
  mix as a playlist for the first three (`locallibrary sets export SET --format traktor --drive
  LIBRARY -o DIR`, see [Other DJ software](other-software.md#other-dj-software)); and Traktor's
  collection read back: the keys, BPMs and cues it has for a drive's tracks (`locallibrary import
  collection.nml LIBRARY`), or a whole drive built from it (`locallibrary collection export DRIVE
  --from interop:traktor --grant FOLDER --option collection=FILE`), see [From
  Traktor](other-software.md#from-traktor).
- **demos**: `locallibrary clean LIBRARY` removes rekordbox's demo tracks and their files (see
  [Editing](editing.md#clean)), and its check lists them in `check` and `doctor`, with `clean` as
  the fix. `refresh` runs it first.
- **coverart**: finds front covers on MusicBrainz and the Cover Art Archive for `locallibrary art
  fetch`, and track details on MusicBrainz for `locallibrary track fetch` (see
  [Providers](#providers)).
- **musicbrainz**: looks up a track's MusicBrainz recording, release and artist IDs by its ISRC
  (`musicbrainz.recording`, `musicbrainz.release`, `musicbrainz.artists`, with the ISRC as
  `musicbrainz.isrc` and the names the artists' go with as `musicbrainz.credits`). The recording
  is the ISRC's only one when it's as long as the track (or, with no length to go by, titled the
  same), or the only one of several with the track's title and length: an extended mix or an edit
  can carry the original's ISRC. The release is the earliest of that recording's named as the
  track's album (one of its editions), and the artists one for each name the track credits, when
  every name is a different one of the recording's. It's what `sets publish` reads a set's
  MusicBrainz IDs from, and the default for `identity` too (see [Roles](#roles)), and each track
  is looked up once: a track without an ISRC, or one MusicBrainz doesn't know, is kept as having
  none, while MusicBrainz being busy or refusing the lookup is a warning, tried again next time.
- **plyr**: a set destination, `track`: `locallibrary sets publish SET --to plyr:track` uploads
  the set's recording to [plyr.fm](https://plyr.fm) as a track of yours, titled after the set
  (else its venue and date), with its tracklist as the description (`05:30 Artist - Title` lines,
  as many as plyr.fm's 5,000 bytes hold) and tagged `dj mix`, and keeps the track's page as the
  set's link, named plyr.fm, and the track's `fm.plyr.track` record as the set's recording.
  plyr.fm takes mp3, wav, m4a, aiff, flac, webm and ogg recordings of up to 1536 MiB, and the
  plugin refuses others before anything goes. It needs a developer token from
  plyr.fm/settings#developer, kept as a secret: `locallibrary plugin settings plyr token
  --secret`. The recording goes straight from its file in the parts plyr.fm asks for (10 MiB),
  each sent again a few times when it doesn't arrive, with the time locallibrary gives a request
  sending a file (enough for about 1 Mbit/s up). An upload that stops partway is dropped by
  plyr.fm after a day, and publishing again starts over.
- **archive**: a set destination, `item`: `locallibrary sets publish SET --to archive:item`
  uploads the set's recording to the [Internet Archive](https://archive.org) as an item of yours in
  Community Audio, titled after the set (else its venue and date), by the DJ name you give it
  (`locallibrary plugin settings archive creator NAME`), with its tracklist as the description
  (`05:30 Artist - Title` lines, as many as fit in the 8,000-byte header it's sent in) and tagged
  `dj mix` and the set's kind, venue and city, and keeps the item's page as the set's link. The item
  is called `CREATOR-DATE-TITLE` (`dj-kit-2026-10-03-closing-set`), with `-2` and on when that's
  taken. It needs the pair of S3 keys from archive.org/account/s3.php, kept as secrets:
  `locallibrary plugin settings archive access --secret` and `locallibrary plugin settings archive
  secret --secret`. The recording goes in one upload, sent again a few times (waiting while the
  Archive says it's busy) when it doesn't arrive; locallibrary gives it as long as 128 KiB/s
  would take, so a 2 GB recording needs about 1 Mbit/s up. The Archive takes a few minutes, sometimes longer, to show a
  new item while it processes it, and the link works once it has. The Archive is only for what
  you have the rights to share: it takes down mixes of other people's records when the rights
  holders ask.

- **youtube**: a set destination, `video`: `locallibrary sets publish SET --to youtube:video`
  uploads the set to YouTube as a video on your channel and keeps its page as the set's link.
  YouTube takes videos, not audio, so the plugin runs [FFmpeg](https://ffmpeg.org/download.html)
  (`ffprobe` and `ffmpeg`, which have to be installed) to make one first: a still picture over the
  whole recording, the `picture` setting's (`locallibrary plugin settings youtube picture
  FILE`, fitted into a 1080p frame) or a black frame, at a frame a second, with the audio as AAC
  (copied when it's AAC already, else encoded, which takes a few minutes for a long WAV). The
  video is made in the request's folder and removed once it has gone. It's titled after the set
  (else its venue and date), in YouTube's category for music, and the tracklist is the
  description, a `05:30 Artist - Title` line a track, the first at `00:00`: YouTube makes those
  chapters when there are three or more, in order, each at least ten seconds long, so a time that
  would break that is left off its line (the dry run says how many). Who can watch it is the
  `privacy` setting, `unlisted` unless you set `private` or `public`. Sign in first with
  `locallibrary account google sign-in --for youtube`: it asks Google only to upload videos.
  The video goes in 16 MiB parts, and one that doesn't arrive is sent again from where YouTube got
  to, a few times at most; an upload that stops partway starts over when you publish again.

  What YouTube does once it has the video is up to YouTube. It processes it for a while after the
  upload, so the link can take some minutes to play, longer in HD. Its Content ID matches every
  upload against the labels' records, and a mix of other people's records is likely to be
  matched: a label can block the video (in some countries or all), put ads on it and take what
  they earn, or only track it. YouTube takes videos over 15 minutes only from verified accounts
  ([youtube.com/verify](https://www.youtube.com/verify)). And YouTube
  gives Local Network's Google client a number of uploads a day (100 for now), shared by everyone
  publishing through it: once they're used up, publishing says so, and works again the next day.

- **mixcloud**: a set destination, `show`: `locallibrary sets publish SET --to mixcloud:show`
  uploads the set's recording to Mixcloud as a show on your account and keeps its page as the
  set's link. Mixcloud takes MP3, M4A, AAC, WAV, AIFF, FLAC, OGG, MP2 and WMA recordings up to
  4 GB, and another is refused before anything goes. The show is titled after the set (else its
  venue and date), its description says where and when it was played, and its tracklist is the
  set's, a track an entry with its artist, title (`ID` for one published unnamed) and the second
  it starts at: a time no later than the one before it is left off, and the dry run says how
  many. The `tags` setting (`locallibrary plugin settings mixcloud tags "house, disco"`) tags it,
  five at most; the `picture` setting is its picture (10 MB at most); and `unlisted` keeps it
  private. Sign in first with `locallibrary account mixcloud sign-in --for mixcloud`. The
  recording goes up in one request, as Mixcloud has no upload in parts: one that doesn't arrive,
  or that Mixcloud fails, is sent again whole, a few times at most, and when Mixcloud says it's
  over its rate limit, publishing waits as long as it asks, up to 15 minutes, else says when to
  publish again.

  Mixcloud licenses the music in DJ mixes, so the artists you play get paid royalties, under
  rules for what a show may play that `sets export SET --format mixcloud` checks. A free account
  keeps 10 shows published at once: to publish another, move an older one to your drafts, or go
  Pro ([Mixcloud](https://www.mixcloud.com/blog/2026/10/21/what-are-unlimited-uploads-and-why-do-they-matter/)).
  And listeners without Premium can rewind a show only a few times an hour
  ([Mixcloud](https://help.mixcloud.com/hc/en-us/articles/360004054059)).

- **soundcloud**: a set destination, `track`: `locallibrary sets publish SET --to
  soundcloud:track` uploads the set's recording to SoundCloud as a track of yours and keeps the
  track's page as the set's link. Sign in first with `locallibrary account soundcloud sign-in --for
  soundcloud`. The track is titled after the set (else its venue and date), with its
  tracklist as the description (`05:30 Artist - Title` lines, as many as SoundCloud's 4,000
  characters hold), tagged `dj mix`, not downloadable, and private unless the `sharing` setting
  says `public`. The `genre` setting is its genre, and the `picture` setting (a JPEG or PNG under
  2 MB) its artwork, which SoundCloud's API reference lists for Pro accounts. SoundCloud takes WAV,
  FLAC, AIFF, MP3, AAC, Ogg, MP4, MP2, M4A, 3GP, 3G2, MJ2, WMA and AMR recordings of up to 4 GB,
  and the plugin refuses others before anything goes. The recording goes in one request, which
  locallibrary gives as long as a slow connection (about a megabit up) needs. SoundCloud can't
  resume an upload, so one that doesn't arrive, or that SoundCloud fails or finds one too many, is
  sent again whole, a few times at most. SoundCloud sometimes makes the track even when the upload
  fails, so before sending it again the plugin looks for it among your newest tracks, and answers
  it when it's there.

  What SoundCloud does once it has the recording is up to SoundCloud. It encodes it for a while
  after the upload, so the link can take some minutes to play. A DJ mix needs the permission of
  every track's rights holders: SoundCloud's content identification takes down a mix that matches
  music it isn't allowed to have, and closes an account after three copyright strikes (its
  [guide to DJ mixes](https://help.soundcloud.com/hc/en-us/articles/115003564568-Best-practices-for-creating-and-uploading-a-DJ-mix)).
  A free account may upload 2 hours of audio in all, so a long set may not fit
  ([Upload Requirements](https://help.soundcloud.com/hc/en-us/articles/360039171614-Upload-Requirements)).
  Local Network's SoundCloud app is registered with the redirect address
  `http://127.0.0.1:47110/callback`, so signing in needs port 47110 free.

- **google-account**: an account plugin (see [Account plugins](#account-plugins)), the `google`
  account type, signed in to through Local Network's token exchange. Its scope `youtube-upload`
  uploads videos to YouTube.
- **mixcloud-account**: an account plugin, the `mixcloud` account type, signed in to through the
  token exchange. Its scope `upload` uploads shows to Mixcloud.
- **soundcloud-account**: an account plugin, the `soundcloud` account type, signed in to through
  the token exchange. Its scopes are `upload`, for uploading tracks to SoundCloud, and `read`, for
  reading from SoundCloud as you.

coverart, musicbrainz, plyr, archive, youtube, mixcloud and soundcloud are the only ones that go
online, and
only for these, through locallibrary (see [Safety](#safety)), a request a second to MusicBrainz
between the first two, as it asks: `art fetch` and `track fetch`; `sets publish` when it's about to
publish (signed in, with `--yes` or a terminal to ask in), for the tracks it names without IDs,
unless `--no-lookup` (or `plugin roles recording --none`, for good); `analyze --plugin
musicbrainz`; and `sets publish --to plyr:track`, `--to archive:item`, `--to youtube:video`,
`--to mixcloud:show` or `--to soundcloud:track` once you confirm it. `analyze` alone leaves out the
analyzers that go online, and says so. youtube is the only one that runs another program
(FFmpeg), as you, like any plugin.

## What you have

`locallibrary plugin` lists them, what each may do (its capabilities, in plain words) and what it
adds. `locallibrary plugin info ID` has one in full: its commands, how to run them and which of
the player's menus offer them, the events it listens for, its analyzers' fields, its checks, its
export and import formats, its providers, its services, its library sources and its settings.

A plugin whose id, commands or export or import formats clash with locallibrary's, or with another
plugin's, is left out: every command warns which and why, and the other plugins work. Two plugins
clashing with each other are both left out.

## Where they show up

On the command line, what a plugin adds runs as commands of its own or of locallibrary's
(`analyze`, `export --format`, `import`, `art fetch`). In the player (`locallibrary tui`) it's in the
palette (`:`), and a drive's menu or the actions menu (`e`) where it works on a drive, tracks or a
playlist, with a form for what it takes, a param the plugin fills from what's selected (the
tracks, the playlist, the drive) starting as it (see [the player](player.md)). The sidecar has all of it for desktop apps as
`plugins.*` methods (see [Building a GUI](sidecar.md)). Each kind is below, in a section of its
own.

## Account plugins

An account plugin provides one account type (Google, say) and nothing else. It's a manifest
alone, its `[[contributes.account_types]]`: how to sign in to the service, how to ask it who
signed in, and the scopes a plugin may be given, each with the requests it covers. It has no
code, so locallibrary never starts one: it runs every sign-in from the manifest and keeps the
tokens itself (see [the decision](adr/0001-account-plugins-are-manifests.md)). Only the ones
locallibrary ships may use Local Network's token exchange. Those are google-account,
mixcloud-account and soundcloud-account, in `plugins/accounts/`. `locallibrary plugin` lists the
account type each adds, and `plugin info` its scopes.

A plugin asks for accounts of a type with `[[accounts]]`, naming the scopes it needs and why:
youtube asks for a google account to upload videos with, mixcloud and soundcloud for an account
of their own type to upload with.

## Accounts

You sign in to an account once, through an account plugin, and the store keeps it. A plugin uses
it only through the access you give it: the scopes it may use, on that account. `locallibrary
account` lists your accounts after the DJ account, each with whether it's signed in and the
plugins that have access to it. `locallibrary account TYPE sign-in` opens the service's sign-in
page in the browser and waits for it to come back to this computer (`--no-browser` prints the
link, `--paste` takes the address the browser ended up on when it runs on another computer).
`--for PLUGIN` gives a plugin access to it, `--with PLUGIN` picks the account plugin when several
provide the type, and `--scope NAME` asks for one of the type's scopes besides.
`locallibrary account TYPE:NAME sign-out` forgets its tokens and tells the service when it can;
plugins keep their access, unused until it's signed in again.

`locallibrary plugin access PLUGIN` lists a plugin's access, `--default ACCOUNT` makes an account
the one it uses for that type, and `--take-away ACCOUNT` takes its access away. Before a plugin's
destination is used, a plugin with no access to a type it needs asks in the terminal which of your
accounts it may use, or to sign in to another. With `--json`, `--yes` or no terminal, and in the
sidecar, nothing is asked: it fails and says how to give it access.

In the player's palette, "sign in to a TYPE account…" signs in to one, "sign out of NAME" signs
out of one, and "access for PLUGIN…" opens a form choosing which account of each type the plugin
uses, saying what for, or signing in to another.

A plugin may also have accounts of its own. `locallibrary plugin account PLUGIN ACCOUNT sign-in`
signs in to one as above; `status` says whether you're signed in and until when, and `sign-out`
forgets the tokens. In the player, "sign in to ACCOUNT…" and "sign out of ACCOUNT" are in the
palette, beside the plugin's settings.

The tokens are kept in the system's keychain, as secret settings are (see [Settings](#settings)),
and refreshed a minute before they expire. The plugin never sees them: a request it makes names
the account (`{account:ID}`) where its token goes, the `Authorization` header unless the service
only takes it in the address (Mixcloud), and locallibrary fills in the token once it has checked
the request, never sending it on to another server a redirect leads to.

Some services (SoundCloud) only take the browser back at the exact address their app was
registered with, port included. A plugin's account on one of those names its port, and its
service's app is registered with `http://127.0.0.1:PORT/callback`. Signing in then needs that port
free on this computer, and says so when another program has it.

Some services (Mixcloud, SoundCloud, Google) want a client secret when the code is traded for
tokens, or when they're refreshed. A secret can't ship inside locallibrary, so a plugin's account
on one of those is signed in to through Local Network's token exchange
(`localnetwork.computer/api/token-exchange/<service>`), which adds the secret and passes the
tokens back. The website never keeps them, and the exchange is the only server besides the
service that a sign-in's code goes to. Until the website is set up for a service, signing in
says so.

## Analysis

An analyzer may also work out a track's beat grid, phrases and waveform (fields of type `beats`,
`phrases` and `waveform`). They're not columns: locallibrary keeps them in the store beside the
analyzer's other values, and the player draws them for a track whose drive has none, marked as not
on the drive. `locallibrary analyze [LIBRARY] --apply grid,phrases,waveform` writes them into the
tracks' analysis files, planned and confirmed as any edit is (`undo` takes it back, and `edits
replay` makes it again after rekordbox exports): only where a track has none, unless
`--overwrite`. A grid sets the track's BPM too, and its cues stay where they are (the plan says
how many sat on beats of the old grid). Phrases count the analyzer's own beats, so they're written
only with its grid, or onto the same one. A track rekordbox never analysed has no analysis files
to take them, and a waveform can't go where rekordbox's colour waveforms are (locallibrary can't
work out every byte of those). An operation can plan the same edit (`set_analysis`), with the
`analysis:plan` capability.

What locallibrary writes reads back as given, and rewriting what rekordbox wrote leaves its files
byte for byte as they were, but that's checked against rekordbox's files, not on players: try a
drive on yours before you rely on it.

## Analyzers

`locallibrary analyze [LIBRARY] [--plugin ID] [--analyzer NAME]` runs them, which works out each
track's values once (again only for a changed audio file, or a new version of the analyzer) and
keeps them in the store. A value worked out with nothing to show (silence's key) shows as `-`,
matches no filter and sorts last. `locallibrary track LIBRARY --track ID` shows them under the
track's details, each as `ID.FIELD` (`sound.bpm`), and `locallibrary tracks` filters and sorts by
them (`--filter "sound.bpm:>120" --sort -sound.bpm`), as the sidecar's `tracks.query` does. In
the player, analyzers are in the palette and a drive's menu, and their fields are columns to
show, sort and search by. An analyzer may say how its fields are shown: a unit (`LUFS`), how many
places, as a key, a duration (`3:07`), decibels, a percentage or a meter, and how wide its column
is in the player; `track` and the player show them so, and the rest as they are, and a mix's
HTML report shows those fields of each track it played. What
locallibrary itself reads from them is their [roles](#roles).

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
labelled values, a tree, a chart (as a table of its points), a picture (said in words) or
sections of those, or as text or JSON when it answers a plain value (`--json`: as it is, a
picture as a PNG `data:` URL); an operation shows its plan
and asks first, like any edit (`--dry-run`, `--yes`), and `locallibrary undo` takes it back.
Either may ask you something first (which track an entry is, say), or several things at once
(which track each of a tracklist's entries is): in a terminal you pick from a numbered list for
each; otherwise, or with `--json`, it stops and says how to answer each with `--choose
PARAM=VALUE`. In the player they're in the palette, and in a drive's menu or the actions menu
where the plugin offers them, and a question one asks is a list to pick from, several one after
another (titled `2 of 5` and so on, esc cancelling them all). On the command
line, a param filled from what's selected is given the drive named, if that's what it takes, and
its flag gives it otherwise. `refresh --then 'PLUGIN.OPERATION KEY=VALUE...'` runs an operation
as a step of a refresh, after the steps that put the drive right and before it's saved (see
[After rekordbox exports](editing.md#after-rekordbox-exports)); one that asks something first is
refused there.

## Destinations

A destination is somewhere a plugin keeps things for you.

A store destination keeps a copy of the store, as a copy in a folder does (`store copy DIR`):
`locallibrary store copy --to PLUGIN:DESTINATION` copies only what it lacks, in the same order
(stored files, then snapshots once all their files are there, then the libraries last), and
`--dry-run` says what would go. The account session never goes. `locallibrary store restore
--from PLUGIN:DESTINATION` brings back what the copy has that the store lacks (`--library L` for
some, `--dry-run`), after losing this computer say, and `locallibrary drive verify DRIVE --from
PLUGIN:DESTINATION --repair` rewrites damaged files from it when nothing nearer has them. Every
file that comes back is checked against its BLAKE3 hash before it goes in the store, and only
locallibrary writes the store.

A set destination publishes a set's recording with its tracklist (tracks marked unnamed, tracks
your profile hides and those `--hide 3,7` names go without their names, notes stay private):
`locallibrary sets publish SET --to PLUGIN:DESTINATION` asks the plugin what would happen first,
shows it with which tracks go unnamed and why, always asks before anything goes (`--dry-run` stops
there), and keeps where the set can be heard as its link, with the name the plugin gives it
(`plyr.fm`) and the Atmosphere record holding the recording when it keeps one (a plyr.fm track).
The plugin uploads the recording through locallibrary, whole, in parts or in a form, which reads
it as it sends it and shows how much has gone; the plugin may send it only while it's publishing,
never in the dry run. Signed in to your account, it then offers to publish the set's record with
them (see [Sets](sets.md)).

## Events

A plugin may listen for what locallibrary notices happen: a drive plugged in (`drive.mounted`),
unplugged (`drive.unmounted`) or changed (`drive.changed`), saved into the store (`drive.pulled`,
with the history lists the store lacked) or back from a set (`set.returned`), one of the sidecar's
requests finishing (`job.finished`) and a plan applied (`plan.applied`). Each needs a capability,
shown as any is: `events:drives`, `events:jobs` or `events:plans`. The player, the sidecar and the
`watch` agent tell it as they notice them, and any command tells it of the plans it applies. It's
told by name or id, never where a drive is mounted, and whatever path it's told is said from its
drive, the store or `~`. Events wait in a queue for each plugin (64 at most: past that the oldest
is dropped, and its log says so), so a slow plugin never holds up the player, the sidecar or
`watch`.

A plugin may tell you something back, from an event or a service, and suggest one of its own
operations, or one of locallibrary's own flows on what it contributes: fetching cover art or track
details from its providers (`art.fetch`, `track.fetch`), exporting in one of its formats
(`export`) or running one of its analyzers (`analyze`), on a drive known here it names (never a
path: one locallibrary doesn't know, the suggestion is left out) and the tracks of it that drive
has. The player shows it as a message, and "review: ..." in the palette plans the operation,
shown and asked about as any is, or opens the flow as if you'd started it, filled in: the fetch
form only from its providers to start with, saying which drive and how many tracks; the export
form with its options and a set's own name for the file (where it goes is yours to pick, as
always); or a question before analyzing. The player doesn't export mixes, so it says so for a
mix's format. "dismiss: ..." lets it go. Suggestions are kept in the store until you
do either (the newest 20), so they're still there when the player opens again; `watch` says what
it's told and keeps its suggestions for the player the same way. The sidecar sends it as a
`plugin.notice` event. A plugin can't suggest another plugin's work, and nothing it suggests is
planned or applied unless you choose to.

## Export formats

They're `locallibrary export LIBRARY --format FORMAT -o PATH`, with the plugin's options as
`--option KEY=VALUE`. `--dry-run` lists what it would write (each file, what goes in, what the
format can't hold), and overwriting a file is asked about first unless `--replace`. In the player
they're in the palette and a drive's menu: an export shows what it would write and asks before
writing it.

A format may write out a set or a recorded mix instead (`plugin info` says which): `locallibrary
sets export SET --format FORMAT -o PATH [--drive LIBRARY]`, and `locallibrary mix RECORDING
--format FORMAT --export PATH` (one recording, its report kept in the store for the plugin to
read), with `--option`, `--dry-run` and `--replace` as above. In the player, a set's formats are
in the palette while the set is open (Browse › Sets). One format can write out more than one of
them, with the same options: interop's `traktor`, `serato` and `engine` take a drive's library, a
set or a mix (see [Other DJ software](other-software.md#sets-and-mixes)).

## Import formats

An importer reads another program's file (a collection, a play log) for what it says about tracks:
their keys and BPMs, cues, My Tags, and the plays it logged. `locallibrary import FILE [LIBRARY]
--format FORMAT` (the format a plugin reads files ending in the file's extension with, unless
`--format` says; `--option KEY=VALUE` for its options) finds those tracks on the drive the way
`playlist import` finds a tracklist's, by file, ISRC, then artist and title, and says how many it
couldn't (not on the drive, or could be several). It only adds to what they have: a key and a BPM
where a track has none (a key written as rekordbox names it, a BPM to hundredths), a cue in a hot
cue slot (or at a place) a track has none in (one a cue edit can't set, say a colour or a loop it
doesn't take, is left out, saying why), and the My Tags a track lacks. It's one edit, shown
and asked about as any is (`--dry-run`), and `undo` takes it back. The plays it logged are kept in
the store as sessions, as Beat Link Trigger's are (`plays`), each play found on the drive or kept
as the log names it; the same log imported again replaces its session. In the player, a drive's
menu and the palette have "import ...", a form asking for the file. An importer plans nothing
itself and needs no capability to: it only answers what it read.

## Library sources

A library source is a library a plugin reads from somewhere on this computer (another DJ
program's collection, such as Traktor's with the interop plugin's `interop:traktor`, or a folder
of tagged files), which locallibrary builds a drive from as it
writes rekordbox's collection: `locallibrary collection export DRIVE --from PLUGIN:SOURCE --grant
FOLDER` (`--grant` again for more folders, the source's options as `--option KEY=VALUE`,
`--dry-run` to see the plan). The plugin is given only the folders you grant it, for that build,
and locallibrary copies only audio files players play that are in them (their links resolved),
not on the drive itself: it reads each file's length and format itself, hashes it before copying
it, and checks the copy. What it leaves out (a file elsewhere, one that isn't audio, a cue in a
slot taken) is listed as a warning, and so is what the plugin says it couldn't read. Cues, beat
grids and waveforms a track comes with go into analysis files made for it, each read back first.
A read that comes to more than 64 million beats and cues across its pages (50,000 ten-minute
tracks at 128 BPM have that many) is stopped, saying so. Building again copies only what changed;
a library on the drive that wasn't built from that source is only replaced with `--replace`. In the
player, "build from …" in the palette and a drive's menu asks for the folders and options first.

## Panels

A panel shows something about a track under its details: `locallibrary track LIBRARY --track ID`
prints each plugin's panel after the track's details and fields (with `--json`, each panel's
view, or why it couldn't show one), and the player shows them under the selected track's details,
asked for once the selection settles on a track. A panel that fails says so, and the others show
all the same. The sidecar's `plugins.panel` renders one for a desktop app.

## Providers

A provider finds cover art or details (tags) for tracks, online usually: `locallibrary art fetch
[LIBRARY]` cover art, and `locallibrary track fetch [LIBRARY]` track details (title, artist,
album, year, genre, label, ISRC). They ask every provider of that kind about a library's tracks
(`--track`, `--playlist`, `--filter`... for some), only those without artwork, or missing a tag,
unless `--replace`; `--from PLUGIN` (or `PLUGIN.PROVIDER`) asks only that one. What they find for
each track is listed best first, with how sure each provider is, leaving out tags that would
change nothing (what the track has, or a tag it has without `--replace`). One alone scoring `plugin
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

The coverart plugin has one of each, and looks each album up on MusicBrainz once for either: by
a track's ISRC first, else by the album's name and artist. `coverart.covers` offers the front cover
of each release group it could be. `coverart.tags` offers each one's album title and year, and
its label only when it was found by name (MusicBrainz's ISRC search doesn't name labels). When
it's the only release group named so, a second lookup adds its top genre and the year it first
came out; otherwise the year is the earliest of its releases the search listed. A track whose
ISRC found it also gets that recording's title and artist; the album's other tracks get the
album's details alone. A track without an album is looked up by its title, and gets details only
from a single of that name, never from an album it's the title track of. One release group found
by an ISRC scores 0.95, by name 0.9, and one of several 0.5, for you to choose.

## Roles

An analyzer's manifest may say what some of its fields are to locallibrary: a track's key, its
tempo (BPM), how sure it is of each, its loudness, true peak and clipped runs, what tells one
recording from another, or its MusicBrainz IDs: the recording's (`recording`) and the release's
(`release`), each a bare lowercase UUID, and the artists' (`artists`), a list with one for each
artist its credit names, in order, split as a published set splits them (at commas, semicolons and
"feat."), with the ISRC they were looked up by (`isrc`) and the names the artists' go with
(`credits`). Doctor's analysis, the check before a set, ordering a set (`playlist order`),
`analyze --apply` and the MusicBrainz IDs a set is published with read those from one analyzer
each, the sound plugin's and the musicbrainz plugin's unless another does too: `locallibrary
plugin roles` shows which, `plugin roles key PLUGIN.ANALYZER` chooses another that has it,
`plugin roles recording --none` reads one from none (`sets publish` then looks nothing up), and
`plugin roles key --reset` puts it back (kept in the store's `plugins/settings.json` as
`analysis.roles.key`). `doctor --analyze` runs those analyzers first on the tracks they haven't
worked out, and so does `sets publish` for the MusicBrainz IDs of the tracks it names (see
[Publishing](account.md#publishing)); the check and ordering a set read what they kept. A
recording or release ID that isn't a UUID is left out, and so are the artists' unless every one
is a UUID and there's one for each name. What an analyzer keeps goes with the audio file, which a
library can retag without changing, so a track takes the IDs only when they were looked up by its
ISRC, and the artists' only for its credit, as far as the analyzer says (`isrc`, `credits`);
`sets publish` looks those tracks up again.

`locallibrary analyze LIBRARY --apply key,bpm` writes what the analyzer heard to the tracks, as an
edit you're shown and asked about (only where a track has none unless `--overwrite`;
`--min-confidence 0.8` leaves out what it's less sure of; `--dry-run`), and `undo` takes it back.
In the player, "write keys and BPMs to the tracks: …" does the same.

## Services

A service is something a plugin keeps running in the background (watching a folder, say). It's
off until you turn it on, `locallibrary plugin settings ID services.NAME on` (or its switch in the
player's settings form), and runs while the player, the sidecar or `watch` (but not `watch
--once`) do, never for a command run once. A plugin with a service running isn't stopped as idle.
One that stops or crashes is started again a second later, then twice that, and so on, and left
off (saying so) after failing five times in a row. `plugin info` and `plugin list` show each
plugin's services, and whether they're on.

## Settings

`locallibrary plugin settings ID` shows them, `plugin settings ID KEY VALUE` sets one and `plugin
settings ID KEY --reset` puts it back to its default. They're kept in the store
(`plugins/ID/settings.json`, beside the plugin's own folder). A secret one (a password, a key) is
set with `plugin settings ID KEY --secret`, which reads it from stdin without showing it, and is
kept in the system's keychain (or with `LOCALLIBRARY_SESSION_STORE=file` in
`secrets/plugins/ID.json`, which only you can read, apart from the plugin's folder): it's only
ever shown as set or not set (or unknown, with why, when the keychain can't say), and never
copied off the computer. In the player, each plugin's settings are a form in the palette.

## Themes

A theme is the colours the player draws in, a plugin's as data: the player's own (text, accents,
the selection), the waveform's gradients, cue and phrase colours, rekordbox's track colours and
good, warning and critical. "Night theme" and the others in the player's palette (`:`) draw in
one from then on, and "default theme" puts the default back; the one chosen is kept in the
player's `tui.json`, and the next run starts in it. What a theme leaves out is the default's. On a
light terminal its colours are turned round as the default's are, and with `NO_COLOR` there are
none.

## Writing one

A plugin in Rust uses [loclib-plugin](../../../crates/loclib-plugin)'s SDK: a function for each
kind of hook it has, `host.drive(drive)` to read a drive (the library, tracks by id or by
query, playlists, analysis, artwork and `drive.info`, each read whole however locallibrary pages
it), `host.fetch(..)` to go online and `host.partial_rows(..)` to send a view's rows as they're
ready. A command or an operation may answer a question for you (`Choice`) instead, or several
at once (`Choice::together`), and is run again with your answers as its params. A view (a
command's, a panel's) is any of loclib-plugin's kinds, each with a plain text form for a host
that can't draw it, and a theme is a table of colours in the manifest. `Hooks::on_event`
handles events, a service is a hook that runs until it's cancelled, and `host.notify(..)` tells
you something. Each family of hooks is an optional feature of the protocol, so a newer
locallibrary adds kinds of hooks without breaking older plugins.

## Safety

A plugin is refused if it says it's something other than what locallibrary knows it as. It may
call only the reads its capabilities grant, naming drives by name or id (never a path), in pages of
bounded size, and what it gets back, errors too, doesn't say where things are on this computer:
a drive is said by its name, where the store's copies, a set's or a mix's recording and
rekordbox's collection are is left out, and any other path is said from its drive, the store
(`<store>`) or your home folder (`~`). An exporter alone is told where the drive it exports is, as
the files it writes name the audio there, and an importer only where the file it reads is. None is started from a process running as an administrator or
root, and each runs in a folder of its own in the store (`plugins/ID/work/`). One that stops
answering is stopped, and started again the next time it's needed.

A plugin goes online through locallibrary, which fetches only from the servers its `net:`
capabilities name (`plugin info` lists them), at public https addresses, following redirects only
to those, as `locallibrary/VERSION` with the plugin's contact setting if it has one, as often as
its manifest says each server allows. A secret setting, or an account's token, goes only where
such a request names it (an account's token, only where its manifest says it goes), filled in by
locallibrary, never on to another server a redirect leads to, and is left out of the plugin's log
and of the errors it's told. What a server answers is passed on as it is, but for an account's
token sent in the address, which comes back as the placeholder that named it. The files a plugin
sends are those in its request's folder (see below) and a set's recording while it publishes the
set, nothing else. A destination is given stored files to read, never the store's folder to
write: what it gives back is checked before locallibrary keeps it.
Images and other large answers come back as files in a folder locallibrary makes for each request
(`plugins/ID/scratch/`), which it reads itself, never past the folder (nor a link, or a file with
another name) or more than 512 MiB of them, and removes; `store gc` removes those a locallibrary
that crashed left. A library source names audio files by path, and locallibrary copies a file
only when it's in a folder you granted for that build, its links resolved (see
[Library sources](#library-sources)).

A plugin runs as you and isn't sandboxed, so this is what locallibrary does, not a wall your
system keeps: a plugin could still read any file you can.
