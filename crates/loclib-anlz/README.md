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
by baken (MIT), whose byte-level fixtures the cue tests reproduce.

## What it doesn't do

- Find a track's analysis files on a drive. locallibrary does that (`anlz::load`).
- Write `PQT2` or work out `PWV4`, whose bytes aren't all understood.
- Analyse audio. loclib-analysis works out tempo and 3-band waveforms from a track's sound.

## Licence

MIT.
