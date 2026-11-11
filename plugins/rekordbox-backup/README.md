# rekordbox-backup

A locallibrary plugin that backs up rekordbox's library on this computer the way rekordbox's own
File > Library > Backup Library does. rekordbox's Restore Library takes its backups back.

It's written the way a plugin from outside locallibrary would be. It depends only on the SDK
crates (loclib-plugin, loclib-progress, loclib-model), never on the `locallibrary` crate, and it
reads rekordbox's files through the host with the `collection:files` capability. It ships built
in, since locallibrary can't install other plugins yet.

## Commands

All three are about rekordbox's collection rather than a drive, so they take no drive.

- `locallibrary collection backup` writes `rekordbox_bak_YYYYMMDD.zip` into the backup folder.
  With `music` on, it first copies each track's audio into `rekordbox_bak/` beside the zip; a
  track whose copy fails is said and left out of the zip's map, and the zip is written last, so
  a cancel leaves the day's zip as it was. The zip is written to a hidden `.part` file and
  renamed into place once whole; one a killed backup left behind goes with the next backup, a
  day later. It shows its plan first: the files it would write, with sizes (the zip's is its size before
  compression), and what to know. That's whether rekordbox is running (what it changes after the
  backup starts isn't in it), whether today's zip is there already (it's replaced), and whether
  the backup will hold sign-in tokens. `--dry-run` stops at the plan.
- `locallibrary collection backups` lists the backups in the backup folder: date, size, tracks,
  the rekordbox version that wrote it, and whether its music folder is there.
- `locallibrary collection verify [FILE]` checks that a backup is whole. Every entry of the zip
  is read, which checks each one's CRC. Its `contents.xml` has to read and agree with the
  entries, and its `master.db` has to open through the host with as many tracks as
  `contents.xml` counts. When the music folder is beside the zip, every file `contents.xml` names
  has to be there. Without `FILE` it asks which backup.

The `schedule` service, once turned on (`services.schedule`), makes a backup without music while
the player, the sidecar or `locallibrary watch` runs. It makes one whenever the newest backup in
the folder is `every_days` old and today's isn't there yet, then removes the oldest of its own
beyond `keep`. It knows its own from a list in its data folder, so it never removes one rekordbox
or you made, or one of its own you replaced. Without a store there's no data folder, and it
removes nothing. It reads its settings when it starts.

## Settings

| Setting | Default | What it does |
| --- | --- | --- |
| `destination` | your Documents folder | The folder backups go in. It has to be there: a backup to a drive that isn't plugged in fails instead of filling the computer's disk. |
| `music` | off | Copy the music into `rekordbox_bak/` too. A copy already there with the same size and time (to 2 seconds, as FAT keeps times) is kept, so a second backup copies only what changed. |
| `credentials` | `keep` | `keep` leaves rekordbox's sign-in tokens in, as rekordbox does. `strip` leaves them out. |
| `every_days` | 0 (off) | Days between scheduled backups. |
| `keep` | 7 | How many of its own scheduled backups the schedule keeps (0 keeps all). |

## Restoring

This plugin doesn't restore. Open rekordbox and use File > Library > Restore Library, with
`rekordbox_bak/` (when there is one) in the same folder as the zip. A restore replaces the whole
library.

## Sign-in tokens

A rekordbox backup holds your sign-in tokens: rekordbox's and Dropbox's in `rekordbox3.settings`
as plain text, your AlphaTheta account in `activations.xml`, and the cloud agent's sign-in in
`master.db`, whose key is public. With `credentials = "keep"` (the default, as rekordbox does) a
restore stays signed in. On macOS the zip is written so only you can read it (mode 0600), but
keep it private anyway: don't upload it anywhere public.

With `credentials = "strip"`, the host's copy of `master.db` leaves out the cloud agent's
sign-in. Every `VALUE` in `rekordbox3.settings` whose name has "token" in it goes, and every
`rekordbox_account` element in `activations.xml`. Each is cut out where it is, with its line when
it has one to itself, so every other byte of those files is as rekordbox wrote it. A settings file
that doesn't parse fails the backup rather than going in with what it holds. After restoring a
stripped backup you sign in again.

## How close it is to rekordbox's own

It matches a rekordbox 7.2.14 backup made on macOS in these ways:

- The zip's entries, in rekordbox's order: `master.db`, the library files beside it, the
  settings, each live track's analysis, then each live track's artwork, the key mappings, and
  `contents.xml` last. Every entry is deflated, with its file's time (local, as DOS times are) and
  permissions, and there are no folder entries and no extra fields.
- Only the analysis files a track names go in (a stale `ANLZ0000.*` beside the `ANLZ0001.DAT` a
  track names stays out), with all three artwork sizes. A deleted track's files stay out.
- `contents.xml` is written as rekordbox writes it: a declaration, a blank line, two-space
  indents, CRLF, ASCII with `&amp;` and a decimal reference for each other character (one for a
  character outside the BMP). `DB_ID`, `CONTENT_NUM`, `BACKUP_RB_VER`, `BACKUP_FILE_VER`,
  `DIRECTORY_ARRAY` and the folders follow the rules seen in rekordbox's backups.
- The music copies are named `/<Artist>/<Album>/<HEXID>_<name>`: the track id in 8 hex digits,
  the artist and album without spaces, and the file's stem cut to 35 characters. Each copy keeps
  its source's modification time. A track whose file is missing is left out of the folder and of
  `contents.xml`, and still counted in `CONTENT_NUM`, as rekordbox does.

What isn't verified, or differs:

- No backup this plugin made has been restored by rekordbox yet.
- The number rekordbox writes for each music root in `contents.xml` has no known meaning. It's
  copied from an earlier backup of the same library in the backup folder (same `DB_ID`), and left
  out when there's none. `CONTENT_ID_MAX` is always 0, as in both backups seen.
- The escaping follows JUCE's `escapeIllegalXmlChars`, since rekordbox is built on JUCE.
  rekordbox's own file showed `&amp;`, decimal references and raw apostrophes. How it writes `"`,
  `<`, `>`, `^` and the backtick (`&quot;`, `&lt;`, `&gt;`, `&#94;`, `&#96;` here) is unverified.
  So is the line JUCE starts for a value whose name is longer than 60 characters, which only a
  long music root would have.
- The music folders are named from the track's artist and album as rekordbox stores them
  (`collection.tracks`'s `artist` and `album`). That fits the backup seen: sampler files under
  `~/Music/rekordbox/Sampler/GROOVE CIRCUIT/` went into `rekordbox/GROOVECIRCUITFACTORYSAMPLES/`,
  which is their tags, not their folders. A track without an artist or album goes into
  `UnknownArtist` or `UnknownAlbum`, which is a guess from rekordbox's drive exports. So is
  writing `_` for a character no folder can hold on macOS or Windows (`/ \ : * ? " < > |`) and
  for a name of dots alone, putting `_` before a name Windows keeps for a device (`CON`, `NUL`,
  `COM1`...), and cutting a folder's name to 200 bytes. The stem is cut to 35 characters, not
  bytes.
- The music is copied before the zip is written, where rekordbox writes the zip first. A file
  rekordbox removes while the backup runs (an analysis redone) is left out with a warning;
  `master.db` gone fails the backup.
- `master.db` is the host's copy: rebuilt by SQLite and encrypted as SQLCipher's export would
  be, as of one moment. rekordbox writes its own with `sqlcipher_export` after restarting its
  agent on a staging copy, which also stamps a staging path into `agentRegistry`. That quirk
  isn't copied.
- The zip's "made by" is what the zip crate writes (version 2.0, the version each entry needs),
  where rekordbox's say 6.3.
- Music copies keep their modification time but not their creation time. A second backup with
  music to the same folder adds to its `rekordbox_bak/`; what rekordbox does there is unknown.
- The files backed up are rekordbox 7.2.14's list on macOS, plus `smartlist.xml` (rekordbox
  7.2.11 had it) and `playlists3.sync` (rbx's list for 7.2.18), each only when it's there.
  Windows is untested.
- Key mappings go in the order the folder lists them, as rekordbox's did on macOS.

## What it needed from locallibrary

It uses these SDK features:

- The `collection:files` reads: `collection.locations` (where rekordbox's files are),
  `collection.status` (whether rekordbox is running, and its version), `collection.copy` (a copy
  of `master.db` into the request's scratch folder, with or without the cloud agent's sign-in) and
  `collection.tracks` (every track's files, artist and album, read from that copy or from a
  backup's `master.db`).
- Commands with `where = ["collection"]` and a place on the command line, `plan = true` (a dry
  run the user confirms) and `long_running = true` (progress, and time to stop cleanly).
- Questions (`choose`), views (tables and sections), scratch folders, `context.data_dir`, a
  service with `host/notify`, and settings.
- `test_util::FakeHost` for its tests.

Gaps still open:

- Plugins from outside locallibrary can't be installed yet. This one runs because it's built in.
- A schedule runs only while the player, the sidecar or `watch` runs. There's no scheduler of
  its own, and no autostart on Windows.
- A plugin can't upload a backup: `net/fetch` sends files from the request's scratch folder,
  which holds 512 MiB at most, and a backup's zip can be bigger (rekordbox's own for 2,050 tracks
  was 513 MiB).
- `collection.tracks` had no artist or album, which the music folders are named after. This
  plugin's branch adds them (`artist` and `album`, `None` without one) to the SDK and the host.

## Working on it

`cargo nextest run -p locallibrary-plugin-rekordbox-backup` runs its tests against the SDK's fake
host, on libraries made in temporary folders.
