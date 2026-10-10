# Test fixtures

Real rekordbox device-export files, used to test locallibrary against what rekordbox actually
writes. No audio is included; tests generate placeholder audio files at the paths the
databases reference.

| Directory | Source | License |
| --- | --- | --- |
| `rekordcrate/` | [Holzhaus/rekordcrate](https://github.com/Holzhaus/rekordcrate) `data/`: complete exports, a large real `export.pdb` (3,886 tracks), and `*SETTING.DAT` samples with one setting changed each | [MPL-2.0](https://mozilla.org/MPL/2.0/) |
| `onelibrary/` | Made here by the `sqlcipher` CLI (`make.sh`): rekordbox 7's OneLibrary schema and default menu rows (`src/onelibrary/schema.sql`), an empty `exportLibrary.db`, and one in WAL mode with a track only in its WAL (`wal.sql`). The tracks are synthetic. | this project's |

The tests also use the vendored rekordbox-pdb's own exports (one track, and twelve tracks with a
playlist) in `vendor/rekordbox-pdb/tests/data/`
([MIT](../../../../vendor/rekordbox-pdb/LICENSE)).

The settings sample directories are named `<setting>_<value>`. Each `.DAT` file inside differs
from rekordbox's defaults only in that one setting.
