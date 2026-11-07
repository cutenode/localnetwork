# Test fixtures

Real rekordbox device-export files, used to test locallibrary against what rekordbox actually
writes. Tests generate the audio they need (WAV, AIFF, FLAC and MP3), and placeholder files at
the paths the databases reference; only audio no test can make is committed.

| Directory | Source | License |
| --- | --- | --- |
| `rekordcrate/` | [Holzhaus/rekordcrate](https://github.com/Holzhaus/rekordcrate) `data/`: the `demo_tracks` export, the databases from the `empty` one, a large real `export.pdb` (3,886 tracks), and `*SETTING.DAT` samples with one setting changed each | [MPL-2.0](https://mozilla.org/MPL/2.0/) |
| `onelibrary/` | Made here by the `sqlcipher` CLI (`remake_sqlcipher_fixtures` in `tests/it/onelibrary.rs`): rekordbox 7's OneLibrary schema and default menu rows (`src/onelibrary/schema.sql`), an empty `exportLibrary.db`, and one in WAL mode with a track only in its WAL (`wal.sql`). The tracks are synthetic. | this project's |
| `audio/` | Made here by FFmpeg 9.0.2: `silence.m4a`, 20 s of silence as 8 kHz mono AAC at 8 kbit/s with its index first (`ffmpeg -f lavfi -i anullsrc=r=8000:cl=mono -t 20 -c:a aac -b:a 8k -movflags +faststart -map_metadata -1 -fflags +bitexact -flags:a +bitexact silence.m4a`), and `silence-alac.m4a`, 1 s of it as ALAC (`ffmpeg -f lavfi -i anullsrc=r=8000:cl=mono -t 1 -c:a alac -movflags +faststart -map_metadata -1 -fflags +bitexact -flags:a +bitexact silence-alac.m4a`), and `two-tracks.m4a`, 2 s of it as two ALAC tracks (the same with `-t 2 -map 0 -map 0`), and `silence-opus.mp4`, 1 s of 48 kHz silence as Opus at 6 kbit/s, a codec symphonia has no decoder for (`-c:a libopus -b:a 6k -f mp4`) | this project's |

The tests also use the vendored rekordbox-pdb's own exports (one track, and twelve tracks with a
playlist) in `vendor/rekordbox-pdb/tests/data/`
([MIT](../../../../vendor/rekordbox-pdb/LICENSE)).

The settings sample directories are named `<setting>_<value>`. Each `.DAT` file inside differs
from rekordbox's defaults only in that one setting.
