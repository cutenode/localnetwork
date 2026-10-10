# loclib-mysettings

Read and write the "My Settings" files rekordbox puts in `PIONEER/` on USB drives, which players
and mixers load when the drive goes in:

| File | What's in it |
| --- | --- |
| `MYSETTING.DAT` | player settings: quantize, auto cue, jog mode, tempo range, language, brightness... |
| `MYSETTING2.DAT` | newer player settings: vinyl speed adjust, jog display, beat jump, waveform divisions... |
| `DJMMYSETTING.DAT` | mixer settings: fader and crossfader curves, headphones, talk over, MIDI... |
| `DEVSETTING.DAT` | waveform and key display: overview waveform, waveform colour, key format... |

rekordbox keeps its own My Settings on the computer in the same files. The crate was split out of
[locallibrary](https://github.com/cutenode/locallibrary), which sets the same My Settings on
every drive from one profile.

```rust
use loclib_mysettings::{SCHEMA, SettingsBlob, SettingsFile, validate};

// Parse a file's bytes (here, a fresh one with rekordbox's defaults).
let bytes = SettingsBlob::default_for(SettingsFile::MySetting).as_bytes().to_vec();
let mut blob = SettingsBlob::parse(SettingsFile::MySetting, bytes)?;
for def in SCHEMA.iter().filter(|d| d.file == blob.file) {
    println!("{} = {}", def.key, blob.get(def));
}

// Change a value: one byte is patched and the checksum recomputed.
let (def, choice) = validate("jog mode", "CDJ")?;
blob.set(def, choice);
assert!(blob.checksum_ok());
let new_bytes: &[u8] = blob.as_bytes();
# Ok::<(), loclib_mysettings::Error>(())
```

- `SCHEMA` lists the 46 known settings: key, label, category (player, display, mixer), file,
  byte offset, choices and rekordbox's default. `find` and `validate` are lenient about case,
  `-`/`_`/spaces, and take a choice's label too (`"Deutsch"` for `german`).
- `SettingsBlob` parses and edits one file. Edits patch single bytes and recompute the
  CRC-16/XMODEM, so bytes it doesn't understand (and anything newer firmware adds) are kept as
  they were. Values it doesn't know read as `SettingValue::Unknown`.
- `SettingsSnapshot` collects a set of files' values, checksum problems and the
  `UndecodedByte`s that differ from what rekordbox 6 writes (rekordbox 7.2 writes six more
  settings whose meaning isn't known yet).
- `changes` works out what making a set of files match some values would change, and their new
  bytes, creating files from rekordbox's defaults where needed.

## Features

- `schemars`: JSON Schema (`schemars::JsonSchema`) for the types.

## What it doesn't do

- Read or write files. It works on bytes; where the files live on a drive or a computer is up
  to you (`SettingsFile::key` gives a file's path on a drive, `PIONEER/MYSETTING.DAT`).
- Know which settings a given player or mixer model has.
- Decode the settings newer rekordbox versions added. Their bytes are kept as they were.

## Credits

The layout and values follow the reverse-engineering work in
[rekordcrate](https://github.com/Holzhaus/rekordcrate) and are checked against its fixtures (in
locallibrary's tests).

## Licence

MIT.
