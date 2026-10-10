# loclib-equipment

What each Pioneer DJ / AlphaTheta player, all-in-one system and mixer reads from a USB drive:
Device Library (`export.pdb`) or OneLibrary (`exportLibrary.db`), which file systems (and the
firmware exFAT needs), and which audio formats at which sample rates and bit depths. It was split
out of [locallibrary](https://github.com/cutenode/locallibrary), where it's the table behind the
`check` and `players` commands.

`MODELS` is the table. `find` looks one up however it's spelled, `Model::reads` says
whether it reads a file system, and `Model::plays` whether it plays a format at a sample rate and
bit depth. Every model names the manual or notice its values come from, and a value no source
confirmed is marked `inferred`, so a check built on it can say "probably".

```rust
use loclib_equipment::{Codec, FsKind, Unplayable, find};

let cdj = find("cdj 3000").unwrap(); // spelling and "NEXUS" vs "NXS" don't matter
assert!(cdj.export_pdb.value && !cdj.onelibrary.value);
assert_eq!(cdj.plays(Codec::Flac, 192_000, 24).result, Err(Unplayable::SampleRate));
assert_eq!(cdj.reads(FsKind::Exfat), None); // with firmware 1.20 or later
```

FAT32 is read by every model, FAT16 and HFS+ by everything but the mixers, and FAT12, NTFS and
APFS by none. MP3 and AAC are listed at 16 bit, which is what the decoder puts out, so only
lossless formats' bit depths are checked.

## Sources

Checked October 2026. Each model's `source` names its own:

- the model's instruction manual: the "USB devices" and "Supported file formats" (older manuals:
  "Playable music file formats") sections, from AlphaTheta's download server
  (`downloads.support.alphatheta.com/manuals/...`) unless the entry says otherwise;
- AlphaTheta's "Important Notice for Customers Using USB Devices with Our DJ Equipment" (18 March
  2026) for which models read OneLibrary and which read Device Library, along with the manuals
  that say "This unit only supports OneLibrary";
- the rekordbox FAQ "Which DJ equipment supports exFAT" for the firmware a model needs to read
  exFAT.

What the player and all-in-one manuals all say, and so isn't repeated per model: of several
partitions only the first is used (or, most add, the one holding a rekordbox library); folders
are browsed 8 levels deep, showing up to 10,000 entries per folder; the number of folders and
files is "unlimited", and none gives a limit on the number of tracks. The GUID partition table
isn't supported (AlphaTheta Help Center), which leaves MBR's 2 TiB as the largest usable drive.

## The table as JSON

`equipment.json` beside the crate is the whole table for tools in other languages, and
`equipment.schema.json` its JSON Schema. It's an array with one object per model, newest first
within each kind:

- `name` (`"CDJ-3000"`), `kind` (`"player"`, `"all_in_one"` or `"mixer"`)
- `export_pdb`, `onelibrary`: `{ "value": bool, "inferred": bool }`
- `exfat`: `{ "value": "yes" | "no" | { "firmware": "1.20" }, "inferred": bool }`
- `audio`: `[{ "codec": "mp3" | "aac" | "wav" | "aiff" | "flac" | "alac", "rates": [Hz...],
  "depths": [bits...], "inferred": bool }]`; a format that isn't listed doesn't play
- `audio_inferred`: whether the list itself is inferred
- `notes`: a list of strings; `source`: a string

A test regenerates both files and fails when they're out of date.
`LOCALLIBRARY_BLESS=1 cargo test -p loclib-equipment --all-features --test json` rewrites them.

## Features

- `schemars`: `JsonSchema` for the types.

## What it doesn't do

- Look at drives or files. It says what a model reads; you bring the file system, format, sample
  rate and bit depth.
- Cover models without a USB port, or ones nobody has added yet.

## Licence

MIT.
