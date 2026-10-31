# loclib-interop

Writes a DJ library for other DJ software: Traktor's `collection.nml`, Serato's crates (and its
cue tag in copies of the audio), a new Engine DJ library, and rekordbox XML; and reads
Traktor's `collection.nml` back. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), whose interop plugin
(`plugins/interop`) uses it to export rekordbox USB drives and the sets played from them, and to
build drives from a Traktor collection.

You bring a `loclib_model::Library`, the folder its audio will be in when the other software reads
it, and optionally each track's analysis. Each writer hands back the files to write (paths below
your output folder, and their bytes) and notes on what the format left out.

- `traktor`: `collection.nml`, for Traktor's "Import another Collection". Carries every track's
  details (title, artist, album, genre, label, comment, rating, colour, dates...), tempo and key,
  hot cues on pads A to H, memory cues, loops, and playlists and folders. A beat grid becomes one
  grid marker on the first downbeat with one tempo, locked so Traktor keeps it. Leaves out cue
  colours (Traktor colours cues by type), hot cues past H, tempo changes within a track, My Tags,
  artwork and waveforms. Two tracks with one file are one Traktor track. `traktor::read` reads a
  collection back, Traktor's own or one written here: each track's location (`Location::path`
  makes it a path on this computer), title, artist, album, genre, label, comment, remixer, key,
  tempo, length, rating and release year, its markers (cues, fade and load markers, grid markers
  and loops, with their pads, names and lengths), and the playlists with the folders above them.
  `Entry::beats` lays a track's grid markers out as a beat grid, for a track up to a day long.
  Smart playlists, tempos past 655.35 BPM and markers further from the start than a track reaches
  (`u32::MAX` milliseconds) are left out.
- `serato`: a `.crate` file per playlist and folder in `_Serato_/Subcrates` (a folder becomes a
  crate holding its crates). Crates only list tracks: Serato reads each file's own tags and
  analyses it again, so beat grids aren't carried. With `serato::Copies`, the audio is copied for
  the crates to list, and MP3, AIFF and WAV copies get rekordbox's hot cues and loops (and the
  track colour) in a `Serato Markers2` tag, written with `serato::write_markers`. Memory cues,
  cues past the eighth pad or loop slot, and the cues of FLAC and MP4 files are left out.
- `engine`: `Engine Library/Database2/m.db`, schema 3.0.2 (Engine DJ 4.5 and later), with tracks
  and playlists (a folder becomes a playlist holding playlists, and a playlist holds a track
  once). Beat grids, cues, loops and waveforms aren't written: Engine analyses the tracks when it
  first reads them. Nor are artwork and colours.
- `rekordbox`: a `DJ_PLAYLISTS` XML file, for rekordbox's Imported Library (Preferences >
  Advanced > rekordbox xml), which turns a USB drive back into a computer library. Carries every
  track's details, beat grid (a `TEMPO` per tempo change), hot cues with their colours, memory
  cues, loops, and playlists and folders. Leaves out My Tags, artwork and waveforms.
- `encode`: the XML escaping the formats are written with.
- `with_history`: a library with its history lists as playlists, in a `History` folder, for the
  formats that keep no history (Traktor, Serato, Engine DJ). The writers themselves leave history
  lists out.

```rust
use std::path::Path;

use loclib_interop::{Content, Source, traktor};
use loclib_model::{Library, Playlist, Track};
use loclib_progress::Ctx;

let library = Library {
    tracks: vec![Track {
        id: 1,
        artist: "Bicep".into(),
        title: "Glue".into(),
        key: "Am".into(),
        bpm: 130.0,
        file_path: "/Contents/Bicep/Glue.mp3".into(),
        ..Default::default()
    }],
    playlists: vec![Playlist {
        id: 1,
        parent_id: None,
        name: "Warmup".into(),
        is_folder: false,
        sort_order: 0,
        track_ids: vec![1],
    }],
    ..Default::default()
};
// The audio is on a drive Traktor will see at /Volumes/USB. No beat grids or cues.
let source = Source { library: &library, root: "/Volumes/USB".into(), analysis: None };
let export = traktor::build(&source, Path::new("for-traktor"), &Ctx::silent()).unwrap();
assert_eq!(export.files[0].path, Path::new("for-traktor/collection.nml"));
let Content::Bytes(nml) = &export.files[0].content else { panic!("an NML file is bytes") };
assert!(String::from_utf8_lossy(nml).contains(r#"FILE="Glue.mp3" VOLUME="USB""#));
// std::fs::write(&export.files[0].path, nml) puts it in place.

// Read back, as a collection Traktor wrote would be.
let collection = traktor::read(nml).unwrap();
let glue = &collection.entries[0];
let audio = glue.location.path(&traktor::startup_disk());
assert_eq!(audio, Path::new("/Volumes/USB/Contents/Bicep/Glue.mp3"));
assert_eq!((glue.key.map(|k| k.code()), glue.bpm), (Some("8A".to_string()), Some(130.0)));
assert_eq!(collection.lists[0].path, ["Warmup"]);
```

## Features

- `traktor`, `serato`, `engine`, `rekordbox` (all default): one per format. `engine` brings
  SQLite (built in, through rusqlite), `serato` lofty for the tags, `traktor` quick-xml for
  reading NML and `rekordbox` percent-encoding for its file URLs, so a build for one format
  brings only what it needs.
- `schemars`: JSON Schema (`schemars::JsonSchema`) for `Software`.

## What it doesn't do

- Read a rekordbox library. Reading rekordbox drives (and their analysis files) is
  locallibrary's job.
- Write the files, or copy the audio. You get paths and bytes, and for Serato copies, where to
  copy from; `serato::write_markers` tags a copy you made. The Engine database is built in a
  temporary file and handed back as bytes.
- Check the audio is where the files say, or that a file is already at a path you'd write.
- Read Serato, Engine or rekordbox XML libraries back.

## Licence

MIT.
