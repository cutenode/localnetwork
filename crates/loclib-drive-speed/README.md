# loclib-drive-speed

How long a write to a USB stick or SD card will take, said before it starts. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), whose plans
say how long copying a DJ library to a drive will take.

A port's speed predicts nothing: on the sticks and cards we've timed the port was never the
limit, and most of an export wasn't data at all. What fits is three costs, each measured on the
drive:

```text
time = bytes / sustained rate + files × create cost + folders × mkdir cost
```

`measure` finds them with a timed write of a scratch file and a few small files and folders, all
removed afterwards, and keeps the result (`DriveSpeed`) on the drive, so it goes with the drive
from one computer to the next and a format forgets it. `estimate` works out a `WriteEstimate` for
a write's `Work` from that result, measuring first when there's none or it's stale and the caller
allows it. `learn` corrects the rate from a real write of a gigabyte or more afterwards: it has
the numbers a short test can't get. A drive that had no state folder loses it again if the write
an estimate was for doesn't go ahead (`WriteEstimate::keep` says it does).

The caller reaches the drive through the `Drive` trait: its root and state folder, a lock, and
the writes the test and the result go through. The test makes its small files with
`Drive::small_files`, so what it times is what the caller's own writes cost.

What a short test can't see:

- **A cache region.** Many sticks take the first gigabytes in a fast mode, then drop. A rate that
  fell during the test is the rate past it, good for any size. One that stayed flat is only good
  for as many bytes as the test wrote (`DriveSpeed::holds_for`): a longer write may run into a
  cache's end, so its estimate is a best case (`WriteEstimate::best_case`).
- **Wear.** A card went from 47 to 16 MB/s over 320 GB of rewrites, so a result goes stale
  (`Stale`): with age, once a capacity's worth has been written since, or on a slower or faster
  port than it was measured on.

The port is a hint and never enters the sum. A rate near what the port can carry may be the
port's and not the drive's, which is worth saying (`WriteEstimate::port_limit_bps`).

The estimate leaves out what isn't the drive's: reading a slow source, downloads and transcoding
(`Work::plus` names them), and anything written after the files. It charges every file the same
create cost, though FAT32 and exFAT folders are unsorted lists that a new name is checked
against, so a create costs more in a fuller folder, and a long name takes more directory entries.
Modelling that wouldn't pay: on a FAT32 disk image the scan came to about 0.25 µs per entry, so a
3,000-entry folder adds under a millisecond to creates of 5 ms and more, and artwork (20 images a
folder) and analysis files (3 a folder) never fill one. It would show in a flat folder of tens of
thousands of files, which only a library that keeps all its audio in one folder has. The test's
file names are a library's mix of short and long, so long names are in the create cost it finds.

```rust
use loclib_drive_speed::{DriveSpeed, Limit, MeasuredBy, Work, WriteEstimate};

// A stick measured at 151 MB/s, 12 ms a file and 14 ms a folder.
let stick = DriveSpeed {
    bytes_per_sec: 151_000_000,
    create_us: 12_000,
    mkdir_us: 14_000,
    holds_for: None,
    measured_at: chrono::Utc::now(),
    measured_by: MeasuredBy::Test,
    link_bps: None,
    logged_bytes: None,
};
// 3,000 tracks with their analysis files and artwork.
let mut work = Work::default();
(work.bytes, work.files, work.folders) = (28_942_463_040, 15_000, 3_000);
let estimate = WriteEstimate::new(work, Some(stick), None, None);
assert_eq!(estimate.seconds, Some(414)); // about 7 minutes
assert_eq!(estimate.limit, Some(Limit::Files)); // mostly making files
```
