# loclib-format

Writes file systems onto disks. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which formats
USB drives for DJ players with it.

For now that's FAT32, the one file system every Pioneer DJ / AlphaTheta player reads, at any size
up to 2 TiB. Windows' own tools refuse to make FAT32 volumes above 32 GB, and macOS and Linux pick
the cluster size themselves; this writes the structures directly, following Microsoft's FAT
specification ("fatgen103"), with the cluster size you choose.

`fat32::Layout::new` works out where everything goes on a volume of a given size and refuses what
FAT32 can't describe. `fat32::write` writes an empty file system for that layout to anything that's
`Write + Seek`: a raw disk or volume handle, a file, or memory.

```rust
use std::io::Cursor;

use loclib_format::fat32::{self, Layout};
use loclib_progress::Ctx;

// A 64 MiB volume with 512-byte clusters, starting 2048 sectors into its disk.
let layout = Layout::new(64 << 20, 512, 512, 2048)?;
let mut volume = Cursor::new(Vec::new());
fat32::write(&mut volume, 0, &layout, "Club Drive", 0x1234_5678, &Ctx::silent())?;

let boot = &volume.get_ref()[..512];
assert_eq!(&boot[71..82], b"CLUB DRIVE ");
assert_eq!(&boot[510..], [0x55, 0xAA]);
# Ok::<(), loclib_format::Error>(())
```

## What `write` promises

- Every write is a whole number of sectors at a sector-aligned offset, which raw disk handles on
  Windows require.
- Nothing is written outside the volume: `[start, start + total_sectors * sector_size)`.
- The reserved area is zeroed first, so no older file system's boot record survives (exFAT keeps a
  backup in sectors 12 to 23).
- The boot sector goes last. A format that stops part way (cancelled through the `Ctx`, or the
  device failing) never leaves something that looks like a valid volume.
- Progress comes as `Event::Bytes` in `Phase::Format`, adding up to `Layout::bytes_written`.

The caller makes the result durable (`File::sync_all`, say), and on a whole disk writes the
partition table: `start` is the volume's byte offset, and `hidden_sectors` its partition's first
sector.

## Labels

`fat32::sanitize_label` turns any name into a FAT volume label: at most 11 characters of upper
case ASCII, with the characters FAT forbids replaced by `_`, and a fallback for a name that leaves
nothing.

## Licence

MIT.
