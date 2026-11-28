# loclib-rekordbox-db

Reads and edits rekordbox's SQLite databases. It's a loclib crate, split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which keeps
rekordbox USB drives in sync, edits their libraries in place and compares them with rekordbox's
own collection.

- `masterdb` reads that collection, `master.db` in rekordbox's folder on a computer, and writes
  it (`masterdb::write`) only while rekordbox is closed and over a fresh backup.
- `onelibrary` reads and edits OneLibrary (rekordbox's name since 7.2; "Device Library Plus"
  before): the `PIONEER/rekordbox/exportLibrary.db` rekordbox 6.8 and 7 write to a USB drive. The
  CDJ-3000X, OPUS-QUAD, XDJ-AZ and other newer players read only this file, so a change to a
  drive's library has to land here too.

Both are SQLite encrypted with SQLCipher 4
([loclib-sqlcipher](https://crates.io/crates/loclib-sqlcipher)) under passphrases every rekordbox
installation shares, and both read into [loclib-model](https://crates.io/crates/loclib-model)'s
`Library`. A drive's older `export.pdb` isn't read here: locallibrary-core reads it through a
vendored fork of rekordbox-pdb that isn't published, which would keep this crate off crates.io too.

```rust
use loclib_rekordbox_db::onelibrary::OneLibrary;

let mut one = OneLibrary::create()?;
let set = one.create_playlist("Friday", 0, false)?;
// What a drive's PIONEER/rekordbox/exportLibrary.db holds.
let bytes = one.to_encrypted()?;
let library = OneLibrary::from_encrypted(&bytes, None)?.library()?;
assert_eq!(library.find_playlist("Friday").map(|p| p.id), Some(set));
# Ok::<(), loclib_rekordbox_db::Error>(())
```

## OneLibrary

rekordbox leaves the file in WAL mode, often with its latest changes only in
`exportLibrary.db-wal`, so `OneLibrary::from_encrypted` takes the WAL too and replays its committed
transactions.

A `OneLibrary` is a decrypted working copy in a temporary folder, opened with SQLite through
[rusqlite](https://github.com/rusqlite/rusqlite). Its key is public, so anyone can write the
file: one whose schema holds views or triggers (rekordbox makes neither), or that SQLite finds
damaged, is refused before anything reads it. It reads the tracks, playlists and history lists,
and edits:

- playlists, history lists, folders and their entries (`create_playlist`, `rename_node`,
  `set_playlist_entries`...);
- tracks: adding them from another database or a `Track` (`ContentRecord`), their details
  (`set_track_fields`, with loclib-model's `TrackFields`), their file (`set_track_path`), their
  artwork, and removing them with the rows only they used;
- cues (`cue_rows`, `replace_cues`), keeping the hot cue bank lists that point at them;
- My Tags (`my_tag_rows`, `add_my_tag`, `link_my_tag`...).

`to_encrypted` checks the result (`PRAGMA integrity_check`, and that it decrypts back to exactly
what was edited) and encrypts it with the original file's salt, as one checkpointed file. Whoever
writes it to a drive removes the old `exportLibrary.db-wal` first: SQLite would replay it into the
new file.

`content_hash` tells whether two files hold the same library whatever their bytes: a checkpoint,
re-encryption and the drive's name and colour (`PER_DRIVE_COLUMNS`) all leave it as it was.

Deriving a file's keys takes 256,000 rounds of PBKDF2, about 50 ms. Name a folder with
`onelibrary::cipher::keep_keys_in` and the keys derived are kept there under the file's salt,
readable by the user alone, so opening that file again skips deriving them. Nothing is kept until
it's called.

## master.db

rekordbox 6 and 7 keep the collection (tracks, playlists, cues, My Tags, history) in `master.db`:
`~/Library/Pioneer/rekordbox/` on macOS, `%APPDATA%\Pioneer\rekordbox\` on Windows, or wherever
rekordbox's settings say the library moved (`default_path`). The
[pyrekordbox](https://github.com/dylanljones/pyrekordbox) project documents the format and the
passphrase (`DEFAULT_KEY`); `LOCALLIBRARY_MASTERDB_KEY`, or a key passed to `open`, overrides it,
`LOCALLIBRARY_MASTERDB` names another file to read, and `LOCALLIBRARY_REKORDBOX_SETTINGS` another
Pioneer settings folder.

A test never reads the collection of whoever runs it, nor their settings beyond where that
collection is. In a debug build, or any process nextest starts, `default_path` gives only what
`LOCALLIBRARY_MASTERDB` names (else `None`), the settings (`settings_dir`,
`CloudFolders::this_computer`) come only from `LOCALLIBRARY_REKORDBOX_SETTINGS`, and opening
rekordbox's own `master.db` panics unless `LOCALLIBRARY_MASTERDB` names that file. To tell which
file is rekordbox's own, that check reads `masterDbDirectory` and the agent's `db-path` from
rekordbox's settings and nothing else, and keeps neither. To read your own collection from a debug
build, name it: `LOCALLIBRARY_MASTERDB=~/Library/Pioneer/rekordbox/master.db`. A plain
`cargo test --release` counts as a release build.

Nothing but `masterdb::write` writes to rekordbox's files, and nothing opens them with SQLite.
`open` copies the database and its WAL to a temporary folder (again, should rekordbox restart its WAL meanwhile), decrypts the
copy there with the WAL's committed transactions replayed, and reads only that, so rekordbox can
keep running. A `Collection` holds the library, cues by track, My Tags, the intelligent
playlists' rules (`smartlist`: read, matched against tracks, and written as a filter where one
can say them), and what Cloud Library Sync keeps in Dropbox or Google Drive (`CloudFolders`,
`Collection::audio_file`).

`masterdb::write` (the `files` feature) writes the collection as ADR 0007 in locallibrary decides.
It refuses while rekordbox or its agent runs, while a WAL beside `master.db` holds anything,
while Cloud Library Sync is on or might be, while another write holds the collection's lock, on a
drive that may not replace a file in one rename, and for a rekordbox version whose layout hasn't
been checked (`write::VERSIONS`). Otherwise it backs `master.db` up and reads the backup back,
makes the edit on that decrypted copy in one transaction, and stamps what it changes with
rekordbox's update counter. It then checks the copy, encrypts it again with the original's salt,
reads it back and swaps it in with one rename, if `master.db` hasn't changed meanwhile. A track's
cues are written as rekordbox lays them out, changing only the rows an edit changes, with the
`contentCue` list made again from the rows and `CueUpdated` raised. Each edit answers what it
replaced, which a later write puts back as a new change.

## Features

- `files`: `masterdb::files`, rekordbox's files on this computer as
  [loclib-plugin](https://crates.io/crates/loclib-plugin)'s `collection:files` reads describe
  them: where they are, whether rekordbox is running and its version, a copy of `master.db` as
  rekordbox's own backup makes one (without the cloud agent's sign-in, unless asked), and its
  tracks' files; and `masterdb::write`, which needs to know whether rekordbox and its agent run.
- `schemars`: `JsonSchema` for `Collection`, `CollectionInfo`, the My Tag types and
  `CueRowChanges`.
- `test-util`: databases and drawn tracks for tests (`test_util`). Not a stable API.

## Licence

MIT.
