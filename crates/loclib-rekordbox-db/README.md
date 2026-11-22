# loclib-rekordbox-db

Reads and edits rekordbox's SQLite databases. It's a loclib crate, split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which keeps
rekordbox USB drives in sync and edits their libraries in place.

`onelibrary` is OneLibrary (rekordbox's name since 7.2; "Device Library Plus" before): the
`PIONEER/rekordbox/exportLibrary.db` rekordbox 6.8 and 7 write to a USB drive. The CDJ-3000X,
OPUS-QUAD, XDJ-AZ and other newer players read only this file, so a change to a drive's library
has to land here too.

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

The file is SQLite encrypted with SQLCipher 4
([loclib-sqlcipher](https://crates.io/crates/loclib-sqlcipher)) under a passphrase every rekordbox
installation shares. rekordbox leaves it in WAL mode, often with its latest changes only in
`exportLibrary.db-wal`, so `OneLibrary::from_encrypted` takes the WAL too and replays its committed
transactions.

A `OneLibrary` is a decrypted working copy in a temporary folder, opened with SQLite through
[rusqlite](https://github.com/rusqlite/rusqlite). Its key is public, so anyone can write the
file: one whose schema holds views or triggers (rekordbox makes neither), or that SQLite finds
damaged, is refused before anything reads it. It reads the tracks, playlists and history lists
into [loclib-model](https://crates.io/crates/loclib-model)'s `Library`, and edits:

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

## Features

- `schemars`: `JsonSchema` for `CueRowChanges`.
- `test-util`: databases built for tests (`test_util`). Not a stable API.

## Licence

MIT.
