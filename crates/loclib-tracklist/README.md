# loclib-tracklist

Reads tracklists from outside a DJ library (an M3U from a computer, a Spotify export, rekordbox's
playlist text or XML, a set list pasted from a web page) and finds their tracks in a
`loclib_model::Library`. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), whose tracklist plugin
(`plugins/tracklist`) uses it to import playlists onto rekordbox USB drives.

`parse` detects the format and reads bytes into `Entry`s (title, artist, album, mix name,
duration, file path, ISRC, and the line each came from), and `read_xml_library` reads a rekordbox
XML collection with the tags, keys, tempos, beat grids and cues a library keeps (see
[below](#rekordbox-xml-as-a-library)). `match_entries`, or a `Matcher` built
once for many lists, gives each entry a `Status`: found (and how), ambiguous (with the candidates,
best first) or missing. `Report` wraps the results with their counts. Entries and results
serialise with serde.

```rust
use loclib_model::{Library, Track};
use loclib_tracklist::{MatchKind, Status, match_entries, parse};

let list = "#EXTM3U\n\
            #EXTINF:367,Bicep - Glue\n\
            /Users/me/Music/Bicep/Glue.mp3\n\
            #EXTINF:300,Burial - Archangel\n\
            /Users/me/Music/Archangel.mp3\n";
let entries = parse(list.as_bytes(), Some("set.m3u8")).unwrap();
assert_eq!((entries[0].artist.as_str(), entries[0].title.as_str()), ("Bicep", "Glue"));
assert_eq!(entries[0].duration_secs, Some(367));

let glue = Track {
    id: 1,
    artist: "Bicep".into(),
    title: "Glue".into(),
    file_path: "/Contents/Bicep/Glue.mp3".into(),
    ..Default::default()
};
let library = Library { tracks: vec![glue], ..Default::default() };
let matched = match_entries(&library, &entries);
assert_eq!(matched[0].status, Status::Found { track_id: 1, how: MatchKind::Path });
assert_eq!(matched[1].status, Status::Missing);
```

## Formats

Bytes can be UTF-8, UTF-16 (with or without a BOM) or, failing both, Windows-1252, with any line
ends. `parse` goes by unmistakable content first (an XML declaration or `DJ_PLAYLISTS`, `#EXTM3U`
or `#EXTINF`, rekordbox's tab-separated header), then the file name's extension (`m3u`, `m3u8`,
`xml`, `csv`, `tsv`), then softer evidence (Excel's `sep=` line, a header naming a title column
and one more, a list that's mostly file paths). `parse_as` skips the detection and `detect` only
does it.

- `Format::M3u`: `#EXTINF` durations and "Artist - Title", `#EXTALB` and `#EXTART`, and each
  line's file as an absolute, relative or Windows path, a `file://` URL or a classic Mac OS
  `Disk:Folder:file` path. Other URLs (`https://`, `spotify:`) become entries without a path.
  Without `#EXTINF`, artist and title come from the file name.
- `Format::Csv`: a table with a header row in its first five records, naming a title or file
  column. Columns are found by name (title, artist, album, mix or version, duration, ISRC, file
  location), which covers Exportify (Spotify), TuneMyMusic, Soundiiz, Beatport-style exports and
  Apple Music's text export. Fields are separated by `,`, `;` or tabs and may be quoted. Without
  an artist column, titles are split at a dash.
- `Format::RekordboxText`: rekordbox's "Export a playlist to a file", UTF-16 and tab-separated,
  with the browser's visible columns as the header.
- `Format::RekordboxXml`: `parse` returns the `COLLECTION`'s tracks, and `xml_playlist` the
  entries of the playlist at a folder path (`Sets/2026/Warmup`). A playlist's tracks the
  collection lacks stay as entries, so they come out missing.
- `Format::Text`: one "Artist - Title" (or "Title by Artist") per line. Numbering, bullets and
  cue timestamps are dropped, a duration at the end is kept, and blank lines, comments and
  headings like "Tracklist:" are skipped.

`text_of` (the decoding) and `read_table` (the lenient CSV reader) are public too, for other
table-shaped files.

## rekordbox XML as a library

`read_xml_library` reads more of a rekordbox XML file (rekordbox's own, or one `locallibrary
xml` writes): each collection track as an `XmlTrack`, its entry with its genre, label,
remixer, composer, comments, year, rating (0-5 stars), key (`Tonality`, as written) and tempo
(`AverageBpm`), its beat grid's tempo changes (`TEMPO`: where, the BPM, beats to the bar and which
beat of it) and its hot cues, memory cues and loops (`POSITION_MARK`: hot cue A-H or none, start
and end in milliseconds, name, colour). `XmlTrack::beats` expands the tempos into a beat grid,
from the first beat at or after 0 to the track's end (`TotalTime`). Its `playlists` hold their
entries as collection tracks by index (`XmlEntry`), so a track listed many times is never
copied: `XmlLibrary::entry` reads one, `XmlLibrary::playlist` a whole playlist.
`read_xml_collection` leaves the playlists out.

The file can say anything, so every value that sizes something is checked: a tempo, cue or track
tempo that's missing a value or out of range (times past a day, tempos outside 0.01-655.35 BPM,
hot cues past H, a loop that doesn't end after it starts) is left out and counted in the
`warnings`, a grid stops at `MAX_BEATS`, elements nested more than 64 deep are left out with
what's in them, and playlists are left out once their folder paths come to `MAX_PATH_BYTES`.
Fade-in, fade-out and load marks (`Type` 1-3) and meters past 4 beats to the bar aren't
supported: they're left out and counted apart. Track colours, `Grouping`, `Kind` and the file's
sizes, bit rates and play counts aren't read.

```rust
let xml = r#"<DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="1">
  <TRACK TrackID="1" Name="Glue" Artist="Bicep" TotalTime="2" Tonality="Fm" AverageBpm="120.00"
         Location="file://localhost/Users/me/Music/Glue.mp3">
    <TEMPO Inizio="0.000" Bpm="120.00" Metro="4/4" Battito="1"/>
    <POSITION_MARK Name="Drop" Type="4" Start="0.500" End="2.500" Num="0" Red="255" Green="0" Blue="0"/>
    <POSITION_MARK Type="0" Start="99999999" Num="-1"/>
  </TRACK>
</COLLECTION></DJ_PLAYLISTS>"#;
let library = loclib_tracklist::read_xml_library(xml.as_bytes()).unwrap();
let glue = &library.tracks[0];
assert_eq!((glue.key.as_deref(), glue.bpm), (Some("Fm"), Some(120.0)));
assert_eq!(glue.entry.path.as_deref(), Some("/Users/me/Music/Glue.mp3"));
assert_eq!((glue.marks[0].hot_cue, glue.marks[0].start_ms, glue.marks[0].end_ms), (Some(0), 500, Some(2500)));
let (beats, _) = glue.beats();
assert_eq!(beats.iter().map(|b| (b.time_ms, b.beat_number)).collect::<Vec<_>>(), [(0, 1), (500, 2), (1000, 3), (1500, 4)]);
assert_eq!(library.warnings, ["1 cue(s) (POSITION_MARK) left out: a type, time or hot cue out of range"]);
```

## How matching decides

Each entry is tried in this order:

1. By file path: tracks with the same file name, ranked by how many trailing path components
   agree, ignoring case, so a computer's `/Users/me/Music/A/x.mp3` finds the drive's
   `/Contents/A/x.mp3`. A match on the file name alone must also share a word with the entry's
   artist or title, when it has any.
2. By ISRC, ignoring case and punctuation.
3. By artist and title, normalised: case, accents, punctuation, "&" against "and", a leading
   "The", featured artists and generic version names ("Original Mix", "2011 Remaster") don't
   count, but a remix, dub or live version never matches the original. One artist list may
   contain the other ("Daft Punk" matches "Daft Punk, Pharrell Williams"). Durations within 2
   seconds make it `ArtistTitleDuration`; an entry without an artist matches on the title alone
   (`Title`).
4. Fuzzily, by word similarity (Sørensen-Dice, allowing a typo or two in long words), also with
   artist and title swapped. A score of 0.8 or more is a match; from 0.65, up to 10 of the best
   candidates are offered instead.

When several tracks fit a step equally well, the ones with exactly the entry's version words win,
then the ones within 2 seconds of its duration. One left is the match; more are
`Status::Ambiguous`. A title-only or fuzzy match whose duration disagrees with the entry's is
only offered, never found.

## Features

- `schemars`: JSON Schema (`schemars::JsonSchema`) for the serialisable types.

## What it doesn't do

- Read or write files. It takes bytes, and writing the found tracks into a playlist is up to you.
- Look anything up. A Spotify URI or a web link stays a name, and nothing reads audio tags,
  fingerprints audio or goes online.
- Read Traktor NML, Serato crates, Engine DJ libraries, PLS or XSPF.
- Validate XML. The rekordbox reader scans tags and attributes, and fails only on a tag that
  doesn't end, an unquoted attribute or a missing `DJ_PLAYLISTS` element; a value out of range
  is left out, counted.
- Guess. A weak or tied match is `Ambiguous`, with its candidates, for a person to choose.

## Licence

MIT.
