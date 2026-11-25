# loclib-anlz

Reads and writes the analysis files rekordbox puts next to each track on a USB drive
(`PIONEER/USBANLZ/.../ANLZ0000.DAT`, `.EXT` and `.2EX`), which CDJs and XDJs load to show a
track's beat grid, cues, waveforms and phrases. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which edits
cues, grids and phrases on drives, relocates tracks and makes analysis files for tracks without any.

| Section | What it holds | Read | Written |
| --- | --- | --- | --- |
| `PPTH` | the audio file's path on the drive | `read_path` | `rewrite_path`, `new_analysis_files` |
| `PQTZ` | the beat grid | `beat_grid` | `write_beat_grid` (`PQT2`, the `.EXT`'s extended grid, only removed: `without_extended_grid`) |
| `PCOB`, `PCO2` | hot cues, memory cues and loops, with names and colours | `cues`, `track_cues`, `read_cue_points` | `write_cue_points` |
| `PWAV`, `PWV2`...`PWV7` | blue, colour and 3-band waveforms, overview and detail | `preview`, `detail`, `TrackWaves::read`, `waveform_entries` | `write_waveform`, `waveform_from_bands` |
| `PSSI` | phrases and the track's mood, masked or not | `song_structure` | `write_song_structure` |

What the readers decode to is [loclib-model](https://github.com/cutenode/localnetwork/tree/main/crates/loclib-model)'s `analysis` types (`Beat`, `Cue`,
`Waveform`, `Bands`, `SongStructure`, `TrackAnalysis`), re-exported here. `CuePoint` is a cue with
everything the files store about it, and keeps the entries it was read from so writing it back
leaves the bytes this crate doesn't model as they were. `shift_times` moves every time in a file
(grid, cues, detail waveforms) by some milliseconds, for a track pointed at another file of the
same recording.

```rust,no_run
use loclib_anlz::{CueFile, CuePoint, beat_grid, read_cue_points, write_cue_points};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let dat = std::fs::read("PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT")?;
let ext = std::fs::read("PIONEER/USBANLZ/P016/0000875E/ANLZ0000.EXT")?;
println!("{} beats", beat_grid(&dat)?.len());

// Add hot cue A at 15 s. Each file holds the cues its lists take (`CueFile`).
let mut cues = read_cue_points(&dat, Some(&ext))?;
cues.push(CuePoint { hot_cue: Some(0), start_ms: 15_000, comment: "Drop".into(), ..Default::default() });
std::fs::write("ANLZ0000.DAT", write_cue_points(&dat, CueFile::Dat, &cues)?)?;
std::fs::write("ANLZ0000.EXT", write_cue_points(&ext, CueFile::Ext, &cues)?)?;
# Ok(())
# }
```

## Where the files go

`AnalysisFolder` is the folder rekordbox puts a track's files in, from the track's path as
`export.pdb` stores it: `AnalysisFolder::of("/Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3")`
writes as `PIONEER/USBANLZ/P016/0000875E`. It's a hash of the path's UTF-16 code units, and it
places every analysed track of the rekordbox exports it was checked on. It stays two numbers until
it's written out, and `of_parts` hashes a path from its parts without joining them. An XDJ-RX3 only
looks there, so files written anywhere else show no waveforms or beat grid on it. When two paths hash alike, rekordbox
puts the second track's files in the same folder as `ANLZ0001.*`, and so on.

## How it writes

A file is a `PMAI` header followed by tagged sections, each starting with `tag[4] header_len:u32
total_len:u32`, all integers big-endian. A writer replaces, adds or removes its own section and
copies every other one byte for byte, then checks that it did. Unchanged data keeps the file as it
was, byte for byte. A new section goes where rekordbox puts it, with the header fields rekordbox 6
writes. Every writer reads what it wrote back with the readers here and fails when that isn't what
it was given. These layouts are checked against rekordbox's own files, but only real players show
what they make of them.

A file holding a section twice (never seen, but for the cue lists) is refused rather than guessed
at. Counts and lengths come from the file, so none is trusted for an allocation or an offset: a
damaged file reads as an error or as less. A fuzz target in the repository checks every reader and
the rewrites for panics.

## Compared with rekordcrate

[rekordcrate](https://github.com/Holzhaus/rekordcrate) parses the same files (and `export.pdb`
and the settings files) into structs that mirror their layout. This crate's tests read what its
writers write with rekordcrate, as a reader written independently of this one. The differences:
this crate writes too, changing the sections it's asked to and keeping every other byte, including
the ones it doesn't model; and it decodes into loclib-model's types (a waveform's columns as heights
and colours, phrases named for the track's mood) rather than the raw fields.

## Credits

The layouts follow Deep Symmetry's
[analysis file documentation](https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html),
part of the DJ Link ecosystem analysis that began with
[dysentery](https://github.com/Deep-Symmetry/dysentery), and their parser in
[crate-digger](https://github.com/Deep-Symmetry/crate-digger), which is where the mask rekordbox 6
puts over phrases comes from. rekordcrate is a second, independent reading of those layouts,
which the tests check this one against. How `PWV6` and `PWV7` heights scale follows
[beat-link](https://github.com/Deep-Symmetry/beat-link). The rekordbox 7 cue lists were observed
by baken (MIT), whose byte-level fixtures the cue tests reproduce. The analysis folder hash was
reported by [rekordbox_converter](https://github.com/ModeAxe/rekordbox_converter/pull/2) and
[rbxport](https://github.com/chrisle/rbxport/issues/616), and checked here against rekordbox's own
exports.

## What it doesn't do

- Find a track's analysis files on a drive, or pick a free `ANLZ000n` for new ones. locallibrary
  does that (`anlz::load`, `paths::analysis_stem`).
- Write `PQT2` or work out `PWV4`, whose bytes aren't all understood.
- Analyse audio. loclib-analysis works out tempo, beat grids and 3-band waveforms from a track's
  sound.

## Licence

MIT.
