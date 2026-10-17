# loclib-model

The plain data model of a DJ library: tracks, playlists and folders, the differences between two
exports, Camelot keys and mix costs, and a track's analysis (beat grid, cues, waveforms,
phrases). It was split out of [locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which
reads and writes these from rekordbox USB exports, so the fields follow what rekordbox keeps.

- `model`: `Library`, `Track`, `Playlist` and `TrackColor`, finding playlists by path, searching
  tracks, the part of a library an export of some playlists writes, numbering tracks only
  OneLibrary has for files (`ONELIBRARY_ONLY`), `LibraryDiff` between two exports of one library,
  and `untangle` for a playlist tree read from a damaged file.
- `harmony`: keys on the Camelot wheel in any notation rekordbox shows, their relative and
  parallel keys, which keys mix, mix costs (going easier on keys an analyzer isn't sure of),
  harmonic suggestions and set ordering (cancellable through a loclib-progress `Ctx`).
- `analysis`: `TrackAnalysis` and its parts, and rekordbox's hot cue palette.
- `paths`: the normalised path keys tracks are matched by, finding them below a folder, names
  made safe for FAT32 and exFAT, and `PathStyle` (how an export writes paths).
- `labelled_tuple!`: a struct that serialises as a JSON array but names its elements in its
  schema.

```rust
use loclib_model::harmony::{Camelot, Relation};
use loclib_model::model::LibraryDiff;
use loclib_model::{Library, Playlist, Track};

let track = |id, artist: &str, title: &str, key: &str| Track {
    id,
    artist: artist.into(),
    title: title.into(),
    key: key.into(),
    bpm: 124.0,
    file_path: format!("/Contents/{artist}/{title}.mp3"),
    ..Default::default()
};
let warmup = Playlist {
    id: 1,
    parent_id: None,
    name: "Warmup".into(),
    is_folder: false,
    sort_order: 0,
    track_ids: vec![1, 2],
};
let old = Library {
    tracks: vec![track(1, "Bicep", "Glue", "Am"), track(2, "Floating Points", "Ratio", "9A")],
    playlists: vec![warmup],
    ..Default::default()
};
assert_eq!(old.find_playlist("warmup").unwrap().track_ids, [1, 2]);

let glue = Camelot::parse(&old.tracks[0].key).unwrap();
assert_eq!(glue.code(), "8A");
assert_eq!(glue.relation(Camelot::parse("9A").unwrap()), Some(Relation::Adjacent));

// Another export of the same library: row ids change, file paths don't.
let mut new = old.clone();
new.tracks[0].id = 7;
new.tracks[0].rating = 5;
new.playlists[0].track_ids = vec![7, 2];
let diff = LibraryDiff::between(&old, &new);
assert_eq!(diff.changed_tracks[0].fields[0].field, "rating");
assert!(diff.changed_playlists.is_empty());
```

## Features

- `schemars`: JSON Schema (`schemars::JsonSchema`) for the serialisable types.
- `test-util`: `test_util::track`, a ready-made track for tests. Not a stable API.

## What it doesn't do

- Read or write any file. Parsing `export.pdb`, OneLibrary, ANLZ files or rekordbox XML, and
  writing them back, is locallibrary's job.
- Analyse audio. `analysis` holds what rekordbox worked out; nothing here decodes or measures
  anything.
- Keep a library consistent. The fields are public: nothing allocates ids or checks that a
  playlist's tracks exist, and a library built by hand can hold a folder loop (walks stop at
  `model::MAX_DEPTH`).

## Licence

MIT.
