# loclib-rekordbox-db

Reads and edits rekordbox's SQLite databases. It's a loclib crate, split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which keeps
rekordbox USB drives in sync, edits their libraries in place and compares them with rekordbox's
own collection.

- `masterdb` reads that collection: `master.db` in rekordbox's folder on a computer.
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

A `OneLibrary` is a decrypted working copy in a private folder (`scratch`, below), opened with SQLite through
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
and `LOCALLIBRARY_MASTERDB` names another file to read.

Nothing here writes to rekordbox's files or opens them with SQLite. `open` reads the database and
its WAL into memory (again, should rekordbox restart its WAL meanwhile), decrypts them there with
the WAL's committed transactions replayed, and hands SQLite that, so rekordbox can keep running and
nothing here writes the decrypted copy to disk (a plain `master.db`, and one of 2 GiB or more, are
read from a file instead, below). A `Collection` holds the library, cues by track, My Tags, the intelligent
playlists' rules (`smartlist`: read, matched against tracks, and written as a filter where one
can say them), and what Cloud Library Sync keeps in Dropbox or Google Drive (`CloudFolders`,
`Collection::audio_file`).

## Decrypted copies on disk

What SQLite needs as a file (an export's `VACUUM INTO`, a `OneLibrary` being edited, a plain
`master.db`, one too big for SQLite to take into memory) goes in a `scratch::Folder`:
`locallibrary-decrypted-*` in the temporary folder, its owner's alone (`0700` on Unix, an access
list for the current user alone on Windows), removed when dropped. For an export, and for a
`master.db` too big for memory, it's copied there as it is, still encrypted, and decrypted a page
at a time into the file SQLite opens, so neither holds the library in memory. Its process keeps a file there
locked, and the OS lets go of the lock however the process ends, so the first copy a later process
makes sweeps away folders nobody holds, with the `.tmp*` folders earlier versions decrypted
`master.db` and `exportLibrary.db` into. A sweep only looks into this user's own folders, and
removes one without a lock, or an earlier version's, only when it holds nothing but the files
those leave. Every connection to a decrypted copy keeps SQLite's temporary tables and sorts in
memory (`temp_store`), and the buffers decrypted here are zeroed once done with. Tests make their
folders in `locallibrary-scratch-tests` in the temporary folder, so they never sweep it.

## Features

- `files`: `masterdb::files`, rekordbox's files on this computer as
  [loclib-plugin](https://crates.io/crates/loclib-plugin)'s `collection:files` reads describe
  them: where they are, whether rekordbox is running and its version, a copy of `master.db` as
  rekordbox's own backup makes one (without the cloud agent's sign-in, unless asked), and its
  tracks' files.
- `schemars`: `JsonSchema` for `Collection`, `CollectionInfo`, the My Tag types and
  `CueRowChanges`.
- `test-util`: databases and drawn tracks for tests (`test_util`). Not a stable API.

## Licence

MIT.
