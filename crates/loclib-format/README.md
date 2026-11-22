# loclib-format

Finds disks and formats them. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which formats
USB drives for DJ players with it.

## Disks

`list`, `by_id` and `by_mount` describe disks as the operating system's tools see them: `diskutil`
and `ioreg` on macOS, `lsblk` on Linux, the Storage cmdlets through PowerShell on Windows. A `Disk`
has its model, serial number, size, sector size, bus, partition scheme and volumes, and whether
it's internal, removable, virtual, write-protected or holds the running system.
`Disk::problem` says why one mustn't be formatted, and `Disk::identity` fingerprints what's on it,
so a caller can check that the disk it's about to erase is still the one it showed.

`format` erases a disk into one MBR partition from 1 MiB to the end, holding FAT32, exFAT or HFS+
(`Filesystem`), with `diskutil eraseDisk` on macOS, `sfdisk` and `mkfs.*` on Linux, and the Storage
cmdlets on Windows. `steps` lists what it will run first. It checks nothing about the disk: that's
the caller's to do with `Disk::problem` and `Disk::identity`.

`Cmd` runs a system tool without a shell and returns its output, or its own words when it fails.
`windows` has two helpers for code that drives Windows' tools itself: `quote` for a PowerShell
string and `root_letter` for a drive root's letter.

## FAT32

FAT32 is the one file system every Pioneer DJ / AlphaTheta player reads, at any size up to 2 TiB.
Windows' own tools refuse to make FAT32 volumes above 32 GB, and macOS and Linux pick the cluster
size themselves; `fat32` writes the structures directly, following Microsoft's FAT specification
("fatgen103"), with the cluster size you choose. `format` uses it on Windows.

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

## Features

- `schemars`: JSON Schema (`schemars::JsonSchema`) for `Disk`, `Volume`, `Scheme` and `Filesystem`.
- `test-util`: for tests, and only ever as a dev-dependency. A sample disk, Hegel generators of
  disks and volumes, the disk image a test thread may format (`TEST_IMAGE`), the JSON readers and
  PowerShell runner the backends use, and, on Unix, `FakeTools`: stand-ins for the system's disk
  tools on `PATH`, which answer only the command lines a test sets up. With it on,
  `Disk::problem` lets through the virtual disk `TEST_IMAGE` names, and Linux's listing shows
  that loop device, so a build with it can format a disk image.

## What it doesn't do

- Run on Linux and Windows hardware yet. It builds and its tests pass there, but locallibrary has
  only ever formatted real drives on macOS.
- Make GPT or APM partition tables. Players need MBR.
- Check, repair or eject a drive. loclib-disk does that.

## Licence

MIT.
