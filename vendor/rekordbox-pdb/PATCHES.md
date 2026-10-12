# Local patches

Vendored from [`rekordbox-pdb` 0.1.0](https://crates.io/crates/rekordbox-pdb) (MIT, see `LICENSE`).
The upstream crate is unmodified except for the additions below. They are small and self-contained
so they can be offered upstream.

1. `PdbEditor::add_artwork(path)`: appends a row to the Artwork table and returns its id.
   `NewTrack::artwork_id` / `set_track_field(.., "artwork_id", ..)` already existed upstream, but
   there was no way to create the row they point at.
2. Removed the `rekordbox-pdb` dump binary (not needed as a library dependency).
3. Page-chain walks (`table_row_offsets`, `mark_table_touched`) are bounded by the number of pages
   in the file, so a corrupt cyclic chain can't hang the process.
4. Row deletion: `PdbEditor::remove_tracks(ids)` (also drops their playlist and history entries
   and renumbers the affected playlists) and `PdbEditor::remove_rows(table, ids)` for id-keyed
   lookup tables. Rows are deleted the way rekordbox does it: the presence flag is cleared and the
   page's present-row count updated, leaving the heap bytes in place. Adds
   `PdbError::RowNotFound` and `PdbError::UnsupportedTable`.
5. History playlists: `PdbEditor::create_history_playlist(name)` and
   `PdbEditor::add_to_history(history_id, track_id)`, mirroring `create_playlist` /
   `add_to_playlist` for the HistoryPlaylists (id + name) and HistoryEntries (track id,
   playlist id, entry index) tables.
6. `PdbEditor::remove_rows` also takes `PlaylistTree` and `HistoryPlaylists` (by node / playlist
   id; their entries are left to the caller).
7. **Indexed editing.** `PdbEditor` parses the file once into an index of every row's location
   (rows never move: appends go to the end of a heap, deletions clear a presence bit) and keeps
   it current, instead of re-parsing the whole database for every row it adds or field it sets.
   Bulk edits were quadratic before; 3,000 playlist entries now take milliseconds.
8. Playlist editing: `rename_playlist` (the row is rewritten with the same id), `move_playlist` /
   `set_sort_order` (patched in place; a folder can't move into itself), `remove_playlists`
   (with their entries), `set_playlist_entries` (replace a playlist's contents, reusing entry
   rows), `repoint_track` (move playlist and history entries from one track to another) and
   `remove_history_playlists`. `create_playlist` checks that the parent is a folder, and
   `add_to_playlist` that the track exists.
9. Track editing: `set_track_strings` rewrites a track row with new strings (title, comment,
   mix name...) and the same id and fixed fields; `set_track_field` also takes `composer_id`;
   `artist_id_for`, `album_id_for`, `genre_id_for`, `label_id_for` and `key_id_for` (the
   formerly private get-or-create helpers) are public. Albums are matched by name *and* artist,
   so two artists' "Greatest Hits" no longer share a row.
10. `remove_tracks` renumbers history entries as well as playlist entries.
11. `append_row` moves to a new page when a page's slot directory is full, instead of failing
    with `TooManySlots` after writing the row.
12. `append_raw_row` / `delete_raw_row` for tables the editor doesn't model (used for
    `exportExt.pdb`).
13. `exportExt.pdb` (My Tags), checked against Deep Symmetry's crate-digger spec and rekordcrate:
    the reader takes tag tracks from table type 4 (not 8), reads `is_category` as the u32 at 0x18
    and the two-byte name offset of subtype 0x0684 rows, and now parses tag track rows
    (`ExtDatabase::tag_tracks`, with `categories`, `tags_in`, `has_table` and `row_locations`).
    `ExtEditor` adds, renames and removes categories and tags, and links and unlinks tracks, on
    top of `append_raw_row` / `delete_raw_row`. Like rekordbox, it writes every tag row again
    when any of them changes (each category followed by its tags, positions from 0, existing
    rows' unknown bytes kept): rewriting rekordcrate's `empty` export gives, byte for byte, the
    data page rekordbox wrote in `demo_tracks`. Index pages aren't updated (rekordbox's own
    exports can have empty ones). Adds `PdbError::TagNotFound` and `PdbError::IdInUse`. A name
    whose row can't fit on a page is refused (`RowTooLarge`) by `add_category`, `add_tag` and
    `rename`, and `to_bytes` checks every row's size before deleting the old ones.
14. `add_track` checks that its artist, album, genre, key and label rows fit on a page before
    writing any of them, so a name too long to store fails the whole call instead of leaving the
    lookup rows before it behind (found by a property-based model test, `props_pdb.rs`).
15. Checking an edit before making it: `Database::check_playlist_name`, `check_lookup_name` and
    `check_track_strings` fail as `create_playlist`/`rename_playlist`, the `*_id_for` lookups and
    `set_track_strings` would (`RowTooLarge`, `StringTooLong`) when the row can't fit on a page,
    and `ExtEditor::check_name` is public, so a caller can refuse an edit before making any of it.
    Both editors share one row-size rule (`check_alloc`) instead of each spelling it out.
16. `Database::check_new_track` and `check_history_name` check `add_track` (with the lookup rows
    it may add) and `create_history_playlist` the same way; `add_track` runs the same check.
17. Damaged headers: `Database::from_bytes` and `ExtDatabase::from_bytes` check that the file
    holds a header and that the table directory fits the first page (`read_header`), failing with
    the new `PdbError::Malformed` instead of panicking on a short file or reserving memory for
    four billion tables. `PdbEditor::from_bytes` no longer panics on a short buffer either; its
    first edit fails when it parses it. Patch 3's bounded walk now has a test of its own
    (`a_cyclic_page_chain_ends`).
18. ISRCs: rekordbox stores a track's ISRC (string 0) as a `0x90` string whose body is `0x03`,
    the code in ASCII and a NUL, not UTF-16LE (rekordcrate reads it the same way). Track rows
    read and write string 0 in that form (`decode_isrc`, `encode_isrc`); every ISRC rekordbox
    wrote used to read as CJK characters.
19. `artist_id_for` and `album_id_for` start a name in a long string (UTF-16, or ASCII past 126
    characters) on a 4-byte boundary, at 0x0c and 0x18 as rekordbox does, instead of at 0x0a and
    0x16. Short ASCII names stay where they were. Track and My Tag rows place their strings by
    the same rule (`name_at`).
20. `allocate_page` refuses a table's next free page that its last page doesn't lead to, or that
    lies more than 64 pages past the end of the file (`PdbError::Malformed`), instead of growing
    the file to wherever a corrupt header field points.
21. `rust-version` is the workspace's (1.91) instead of upstream's 1.73: Cargo's resolver picks
    dependency versions for the lowest `rust-version` in the workspace, so 1.73 held the
    workspace's dependencies back.
22. Track format codes: `set_track_field` takes `file_type` (the u16 at 0x5a), so a track pointed
    at a file of another format (a lossless copy of an MP3) gets the code that format has, and
    `file_type_code` is public, rekordbox's one table of codes by extension: `.flac` gets FLAC's
    (5) where `add_track` gave it MP3's (1), `.mp4` and `.aac` get 4, and an extension rekordbox
    doesn't list gets 0 rather than MP3's code.
23. Removing an album (`remove_rows`) that `album_id_for` found by its name and artist hands
    the name to another album of that name by that artist, as removing an artist, genre, label
    or key row already did; `album_id_for` used to add a third album when a second was there.
24. `add_track` on a buffer that doesn't parse fails as `Malformed`, as every other edit does,
    instead of with `RowTooLarge` against a page size of 0.
25. Damaged rows and strings: the readers (`Database::from_bytes`, `ExtDatabase::from_bytes`, so
    `ExtEditor` and `PdbEditor`'s first parse too) check every read against the file
    (`bytes_at`, `read_u8`, `read_u16`, `read_u32`) and fail as `Malformed` instead of panicking:
    a long string whose length is under its 4-byte header or runs past the end of the file
    (`decode_string`, `decode_isrc`), a row whose fields or strings do, a table linking to a page
    past the end, or a page whose row directory doesn't fit below its header. A string of an
    unknown kind is refused as `UnknownString` before its length is read. Properties
    (`long_string_lengths_are_checked`, `damaged_files_never_panic_the_readers`) feed the readers
    arbitrary, truncated and overwritten files.
