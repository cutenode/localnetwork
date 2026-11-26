# loclib-disk

Checks, repairs and ejects drives with the operating system's own tools. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which looks
after the USB drives DJs play from.

A drive pulled out without ejecting it, from a computer or from a player whose USB indicator is
still blinking, can leave its file system inconsistent. `fscheck` checks it, and repairs it when
asked, with the system's checker:

| System  | Check                                    | Around it                                           |
| ------- | ---------------------------------------- | --------------------------------------------------- |
| macOS   | `diskutil verifyVolume` / `repairVolume` | diskutil unmounts and mounts the volume itself      |
| Linux   | `fsck.vfat` / `fsck.exfat`, `-n` or `-a` | `udisksctl unmount` before, `udisksctl mount` after |
| Windows | `chkdsk`, `/f /x` to repair              | `/x` dismounts the volume for a repair              |

`eject` unmounts a drive and powers it off: `diskutil eject` on macOS, `udisksctl unmount` for
every mounted partition and then `power-off` on Linux (`umount` alone without udisks), and
Explorer's own Eject on Windows.

Both work in two steps: one says what will run, so it can be shown first, and the other runs it.

```rust,no_run
use std::path::Path;

use loclib_disk::{eject, fscheck};

let root = Path::new("/Volumes/DJ");
// Linux checks the partition; macOS and Windows go by the mount point.
let steps = fscheck::steps(root, Some("/dev/sdb1"), Some("vfat"), false)?;
println!("will run: {}", steps.display());
let found = fscheck::run(&steps, Some("/dev/sdb1"))?;
println!("consistent: {}, {:?}", found.ok, found.output);

// Linux unmounts every partition of the disk: find it with loclib-format.
let disk = loclib_format::by_mount(root)?;
let ejected = eject::run(root, &eject::commands(root, Some(&disk))?)?;
println!("ran {:?}, powered off: {}", ejected.commands, ejected.powered_off);
# Ok::<(), loclib_format::Error>(())
```

Errors are [loclib-format](https://crates.io/crates/loclib-format)'s, which finds the disks and
runs the tools: a tool that's missing says which package has it, a failing one comes back in its
own words, and a check that needs administrator rights says so before anything is unmounted.

## What it doesn't do

- Decide whether a drive may be checked or ejected. locallibrary first makes sure none of its
  own work is writing to the drive.
- Read FAT's or exFAT's "dirty" flag. It isn't a useful signal on a mounted drive: macOS clears
  FAT's clean-shutdown bit as soon as it mounts a volume writable, Windows and Linux set the
  boot sector's dirty bit while a volume is mounted, and macOS runs `fsck -y` on a dirty volume
  before mounting it anyway. The check reports what's inconsistent instead.
- Run on Linux and Windows hardware yet. It builds and its tests pass there, but locallibrary
  has only ever checked and ejected real drives on macOS.

## Licence

MIT.
