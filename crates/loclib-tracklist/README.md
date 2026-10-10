# loclib-tracklist

Reads tracklists from outside a DJ library (an M3U from a computer, a Spotify export, rekordbox's
playlist text or XML, a set list pasted from a web page) and finds their tracks in a
`loclib_model::Library`. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which uses it to import playlists onto
rekordbox USB drives.

`parse` detects the format and reads bytes into `Entry`s (title, artist, album, mix name,
duration, file path, ISRC, and the line each came from). `match_entries`, or a `Matcher` built
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
- `Format::RekordboxXml`: `parse` returns the `COLLECTION`'s tracks, and `xml_playlists` each
  playlist by its folder path (`Sets/2026/Warmup`). A playlist's tracks the collection lacks stay
  as entries, so they come out missing.
- `Format::Text`: one "Artist - Title" (or "Title by Artist") per line. Numbering, bullets and
  cue timestamps are dropped, a duration at the end is kept, and blank lines, comments and
  headings like "Tracklist:" are skipped.

`text_of` (the decoding) and `read_table` (the lenient CSV reader) are public too, for other
table-shaped files.

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
  doesn't end, an unquoted attribute or a missing `DJ_PLAYLISTS` element.
- Guess. A weak or tied match is `Ambiguous`, with its candidates, for a person to choose.

## Licence

MIT.
