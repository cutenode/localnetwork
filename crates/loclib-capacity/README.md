# loclib-capacity

Catches fake-capacity USB sticks and SD cards before anything is written to them. It was split
out of [locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which
runs it before formatting a drive for DJ players.

A fake stick reports a large size (often 1 to 2 TB) and quietly wraps writes past its real
capacity onto what it already holds, or drops them. A library on it works until it grows past the
real size, then breaks. In 2023 GRC bought 12 cheap 1 to 2 TB sticks on Amazon; all 12 were fake,
typically with about 64 GB that worked.

The probe follows [f3probe](https://github.com/AltraMayor/f3)'s idea. It writes a distinct 1 MiB
pattern to regions spread over the whole disk (one per MiB, at most 256), highest first, so on a
drive that wraps, the low regions written last overwrite the high ones that alias them. It then
reads every region back, from the device rather than the operating system's cache, and checks
each pattern. A real drive returns them all; a fake one returns other regions' patterns, or
nothing, past its real size. The `ProbeReport` says how many regions failed, the offset below
which every region held its data (`works_up_to`), and up to 8 `Alias`es: where data that came
back at the wrong place had been written.

```rust
use std::io;

use loclib_capacity::{RawDisk, probe};
use loclib_progress::Ctx;

/// A stick that keeps the first `.0.len()` bytes and wraps every address past them.
struct Wrapping(Vec<u8>);

impl RawDisk for Wrapping {
    fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<()> {
        let n = self.0.len();
        for (i, b) in buf.iter().enumerate() {
            self.0[(offset as usize + i) % n] = *b;
        }
        Ok(())
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let n = self.0.len();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = self.0[(offset as usize + i) % n];
        }
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// Claims 8 MiB, keeps 2.
let mut stick = Wrapping(vec![0; 2 << 20]);
let report = probe(&mut stick, 8 << 20, 512, &Ctx::silent()).unwrap();
assert!(report.is_fake());
assert_eq!(report.works_up_to, Some(2 << 20));
```

On a real disk, `DeviceFile::open_to_overwrite("/dev/rdisk4")` (macOS; `/dev/sdX` on Linux) opens
the whole device, and `probe` takes what it returns. The disk's size and sector size come from the
caller (`diskutil info`, `lsblk`).

## It destroys what's on the disk

The probe overwrites the start of the disk and every test region, so run it only on a disk you're
about to erase anyway. A check over the free space of a drive that already holds data would be
unsafe on exactly the drives it's meant to catch: the test writes land on the data.

What the crate does about that:

- Nothing touches a device until it's opened with `DeviceFile::open_to_overwrite`, and `probe`
  only takes a disk that's already open. Opening never creates a file, so a mistyped path is an
  error rather than a new file, and it needs write access: without it, the error says
  administrator rights are needed.
- The first region (the partition table and the boot sector) is zeroed and flushed before any
  pattern is written. A probe that stops part way (cancelled, an I/O error, the stick pulled out)
  leaves a disk that no longer mounts, rather than the old volume mounting with damaged files.
- A disk smaller than one region isn't written at all.
- Every region ends at or before the size given, is aligned to the sector size (at least 512
  bytes), and no two overlap.
- Cancellation is checked before each region is written and before each is read back. The first
  write or flush that fails stops the probe, with nothing written after it.
- Each run's patterns carry a fresh random nonce, so data an earlier run left never passes for
  this run's, and only this run's data counts as an alias. A read that fails is never taken for
  an alias.
- Reads come from the device. `DeviceFile` syncs its writes first (macOS's raw `/dev/rdisk`
  devices aren't cached and refuse fsync, which is let through there) and, on Linux, drops the
  kernel's page cache through `/proc/sys/vm/drop_caches`, which needs root.

What the caller has to do, as locallibrary's format does: pick the disk the user meant, unmount
all its volumes, use the raw `/dev/rdiskN` on macOS, and check the disk is still the same one
before writing a file system on it afterwards.

## Features

- `schemars`: JSON Schema (`schemars::JsonSchema`) for `ProbeReport` and `Alias`. An `Alias`
  serialises as `[written_at, read_at]`, and its schema names both elements.

## What it doesn't do

- Find, unmount or format disks, or tell their size.
- Run on Windows. It builds there, but locallibrary refuses the check on Windows and it has never
  been tried.
- Measure speed. `pattern` is public so that a speed test can write data a controller can't
  compress, but timing is up to it.
- Test in place. There's no mode that keeps what's on the disk.

## Licence

MIT.
