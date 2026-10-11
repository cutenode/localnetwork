# Drives

## Your library's drives

Each drive holds a copy of a library: it's a clone of it. `status` shows each library with its
drives under it, those not plugged in too, where each stands against the library's latest
snapshot, and the drives of no library last:

```text
Home  3 of 4 clones plugged in · not identical · changes not saved
  REKORDBOX  primary  up to date
  BACKUP     backup   up to date
  SD         set      back from a set: 2 new sessions (pull saves them)
  Spare      backup   not plugged in · up to date
No library
  LOOSE               not in the store (locallibrary pull)
```

`status --all` adds the libraries with none of their drives plugged in, and drives of no library
that aren't plugged in; `drive ls` groups the mounted drives the same way. A drive's name is its
volume name unless you gave it another (`setup --name`, `drive rename`), and it matters less than
the library's: most commands take the library's name and mean its lead clone (see
[Commands](commands.md#commands)).

- `drive rename <drive> <name> [--label]` (`--label` changes the volume label too; experimental on
  Linux and Windows, see [Platforms](install.md#platforms)), `drive forget <drive>`,
  `drive retire <drive> [--undo]`, `drive ls --known` (every registered drive).
- **Roles:** `drive role <drive> primary|backup|set|archive` (`gig`, the old name for `set`, works
  too). A library has one primary: a primary that joins a library which has one becomes a backup.
  Archive and retired drives are never written to (by a sync, an edit, a format or anything else)
  unless `--overwrite-archive` is given (`--force` also does for push); locallibrary still keeps
  its own notes in their `.locallibrary/` (the file index, what it last saw). `locallibrary sync`
  with no drives syncs each library's drives plugged in (`drive groups`); it, a change copied onto
  a library's other drives, `smart apply` and `settings apply` leave archive and retired drives
  out.
- **Logbook:** `drive log <drive>`: data written by sync, push and repair, clean ejects, the last
  check of its files and of its file system, and problems found.
- **A drive that lost its identity** (erased elsewhere, or cleaned of `.locallibrary/`) is matched
  to the drive it was by its contents or its disk's serial number (on macOS, a USB disk's, from the
  IORegistry): `drive recognise <drive>`,
  then `drive adopt <drive> --as <name>` gives it back its id, name, library and history.
- **Copies:** copying a drive in Finder or with a cloning tool copies its identity file, so two
  drives claim one identity. `status` says so, and which is the original when the disk serial
  number recorded with the identity tells. `drive reidentify <path>` gives the copy its own, named
  `<name> (copy)` unless `--name`, after showing the plan and asking. It's refused when nothing
  shows the drive is a copy (no other drive mounted has its identity, and it's on the disk
  recorded with it) unless `--force`; a role stays with the original.

### Health

```bash
locallibrary drive verify REKORDBOX               # re-read every file and compare it with a known-good copy
locallibrary drive verify REKORDBOX --repair      # rewrite what's damaged or missing (from the store, or --against another drive)
locallibrary drive fsck REKORDBOX [--repair]      # the operating system's own file-system check
locallibrary drive format disk4 --check-capacity  # catch a fake-capacity stick before using it
```

- `drive verify` reads every byte under `PIONEER/` and `Contents/`, bypassing the hash cache (which
  would vouch for a damaged file by its size and modification time), and compares each file with the
  best reference there is: a stored snapshot of exactly this state, the drive's own hash records
  when the file's size and modification time haven't changed, or an older snapshot holding the same
  version of the file. A file a player or rekordbox rewrote has a new modification time and counts
  as unverified, not damaged. Unreadable files are reported too, with the tracks and playlists
  affected. `--repair` checks each file again, copies a good copy with every byte verified (keeping
  the modification time), and reads it back. A file that is still wrong after that means the flash
  itself is failing: replace the drive.
- `drive fsck` runs `diskutil verifyVolume`/`repairVolume` on macOS (no administrator rights; the
  drive is unmounted while it runs), `fsck.vfat`/`fsck.exfat` on Linux as root (the drive is
  unmounted with `udisksctl` first and mounted again afterwards; if it can't be unmounted, nothing
  runs), and `chkdsk` on Windows from an elevated prompt (`--repair` dismounts the drive while it
  runs). FAT's "dirty" flag isn't used: macOS clears the clean bit as soon as it mounts a volume,
  and repairs a dirty volume before mounting it. Only the macOS path has run; on Linux and Windows
  `fsck` is experimental (see [Platforms](install.md#platforms)).
- `--check-capacity` writes a 1 MiB pattern to about 256 regions across the disk, highest first,
  then reads them back from the device (f3probe's approach). A fake stick wraps writes past its
  real size onto what it already holds, so regions come back holding other regions' data, and the
  format stops. The first megabyte (partition table and boot sector) is wiped before the test
  writes anything, so a test that stops part way leaves a disk that no longer mounts and has to
  be formatted. It only runs as part of a format (it overwrites data), needs administrator rights
  on a real disk, and isn't available on Windows yet. On Linux it's experimental: it has only run
  on macOS disk images.
- `status` flags write-protected drives (players can't save history or cues to them) and drives
  with less than 200 MB free, and sync plans warn before leaving less.

### Space

```bash
locallibrary drive space REKORDBOX                  # what's using the drive, its growth, whether it fits on the others
locallibrary drive tidy REKORDBOX [--empty-trash]   # remove ._*, .DS_Store, Spotlight's index...
locallibrary drive recordings SD --copy-to ~/Music/Sets --tracklist latest [--delete]
```

- `drive space` sorts every file on the volume: audio, analysis, artwork, databases, settings,
  locallibrary's undo backups, recordings made on the player, OS clutter, trash, and audio outside
  the library (players list it when browsing folders, with no analysis). From the library's
  snapshots it fits a growth rate ("grows about 2.1 GB a month; full around March 2027"), and it
  checks whether the library still fits on the other drives of its library.
- `drive tidy` removes AppleDouble `._*` files (only real ones, by their magic number; other devices
  list them as broken tracks), `.DS_Store`, `Thumbs.db`, `.TemporaryItems`, `.Spotlight-V100`,
  `.fseventsd`, and the `.locallibrary-tmp-*` files a cut-off write left (up to a whole track each).
  `--empty-trash` also deletes what's in the Finder or Explorer trash, for good. `System Volume
  Information` and locallibrary's own folder are left alone.
- `drive recordings` lists the WAV files MASTER REC writes on the XDJ-AZ (`ALPHATHETA REC`) and
  OPUS-QUAD (`PIONEER REC`), with their length. `--copy-to` copies them (verified), names them by
  when they started, writes a tracklist from a history list next to them (`--tracklist latest`, or
  a list's name), and with `--delete` frees the drive.

### Setting up and handing over

```bash
locallibrary provision /Volumes/NEW --like Home --role backup --player-name "BACKUP 2" --eject
locallibrary provision disk4 --format --check-capacity --name Spare   # erase and test it, then the latest snapshot
locallibrary eject                                                    # your library's drives; or: locallibrary sync --eject
locallibrary drive firmware REKORDBOX ~/Downloads/CDJ3000Xv150.UPD [--replace]  # afterwards: --remove
```

- `provision` is one plan: format, give it an identity (under its volume name, or `--name`), make it
  another copy of a library from `--like` (a library's name or a drive) or write the latest
  snapshot, write the settings profile, set the role and player name, read everything back, eject.
  It stops at the first step that fails, and says which.
- `eject` unmounts a drive and powers it off (`diskutil eject`, `udisksctl`, Explorer's Eject),
  which takes the disk's other volumes too: it names them first. It's refused while a
  locallibrary operation is writing to the drive, and for a disk that also holds the store or
  locallibrary itself. Only `diskutil` has run; on Linux and Windows it's experimental (see
  [Platforms](install.md#platforms)).
- `drive firmware` checks an update file's name against the model it's for (CDJ-3000X, CDJ-3000,
  XDJ-AZ, OPUS-QUAD, DJM-A9, DJM-V10) and the drive's partition scheme and file system against that
  model's rules (every model needs MBR; the DJM-A9 and DJM-V10 take FAT32 only; the CDJ-3000 and
  DJM-A9 want one update file on the drive; the CDJ-3000X takes exFAT too), then copies it to the
  drive's root. It never downloads firmware.

### What players show

```bash
locallibrary drive player BACKUP                                # the name and colour players show
locallibrary drive player BACKUP --name "BACKUP B" --colour 6
locallibrary drive contact REKORDBOX "sam@example.com" --playlist
```

- Players show the Device Name set in rekordbox, not the volume label (a CDJ-3000 shows "NO
  DEVICE NAME" until one is set). rekordbox keeps it, with a background colour code, in
  OneLibrary's `property` table and in its own record of the device
  (`PIONEER/DeviceLibBackup/rbDevLibBaInfo_*.json`, never copied between drives). `drive player`
  sets OneLibrary's, which the newer players read; where players reading `export.pdb` find the
  name isn't documented. Sync, push and pack keep each drive's own name and colour (a run that
  was cut off leaves them for the one that finishes it), and OneLibrary's fingerprint leaves them
  out, so drives with different names still compare as identical. rekordbox's next export writes
  rekordbox's values again; `drive player` remembers its own on the drive (in `.locallibrary/`),
  and `locallibrary refresh` puts them back. Undoing `drive player` remembers the ones it puts
  back instead.
- `drive contact` writes `IF FOUND.txt` in the drive's root and, with `--playlist`, a top-level
  playlist named after the contact, the first thing a player shows. The playlist is a recorded
  edit, so `edits replay` puts it back after rekordbox's next export.

### Back from a set

Players write to the drive: a history list every session, memory cues, and on the CDJ-3000X
playlist edits and TAG LIST playlists. If the primary got a new export in the meantime, both
drives changed since they were last in sync, and a plain sync has to refuse. `sync` looks at what
each drive changed since the last state both held, which must be in the store (pull before
sets). When one drive's changes are only things players do (new history lists, rewritten analysis
files of tracks that were already there, player state files, TAG LIST playlists), it saves that
drive as a snapshot, so its history lists and cues stay in the store, copies the newer library
over it, and saves the result as the newest snapshot. Any other change still counts, and the sync
refuses as before. On a drive with both databases it looks at both, since a CDJ-3000X writes
only to OneLibrary: its playlist edits count as changes of the drive's own.

## Formatting a drive

```bash
locallibrary drive disks                          # what's connected, and how players will get on with it
locallibrary drive format BACKUP --dry-run        # the plan: what gets erased, what gets created
locallibrary drive format BACKUP                  # ...then type the drive's name to confirm
locallibrary drive format disk4 --fs exfat        # a disk with no mounted volume: disk4 / /dev/sdb / PhysicalDrive2
locallibrary drive format disk4 --yes --erase disk4       # in a script: the name is given, not typed
```

A drive to format is named by its path (`./NAME` for a folder here), the disk's identifier, or
the name of a drive locallibrary adopted; never by a volume label, which an unrelated disk could
carry while the drive you meant isn't plugged in. The confirmation asks for that adopted name, or
the disk's identifier when the disk holds no adopted drive (on Linux, which doesn't mount the new
volume, always the disk's identifier). `--yes` doesn't skip typing it (for
`drive format` and `provision --format`): without a terminal, `--erase NAME` gives it, and a name
that doesn't match erases nothing.

The whole disk is repartitioned: one MBR partition from 1 MiB to the end, holding FAT32 with the
cluster size Windows and macOS would choose (32 KB above 32 GB). That's what AlphaTheta's own
documentation points to (checked October 2026 against the operating manuals, the AlphaTheta
Help Center and the rekordbox 7 manual), and how AlphaTheta's SanDisk DJ drives ship:

| | FAT32 (default) | exFAT (`--fs exfat`) | HFS+ (`--fs hfs+`) |
| --- | --- | --- | --- |
| CDJ / XDJ / OPUS / OMNIS | every model | CDJ-3000 (firmware 1.20+), CDJ-3000X, CDJ-1500X, OPUS-QUAD, OMNIS-DUO, XDJ-AZ, XDJ-AN, XDJ-XZ (1.23+), XDJ-RX3 (1.11+) | every model |
| Not readable on | | CDJ-2000NXS2 and earlier, CDJ-900NXS, XDJ-1000MK2, XDJ-700, XDJ-RX2, XDJ-RR | |
| DJM-V10 / DJM-A9 (settings, firmware) | yes | no | no |
| Player firmware updates | yes | CDJ-3000X only | no |
| rekordbox can export to it from | Windows and Mac | Windows and Mac (rekordbox 6.6.2+) | Mac only |
| Largest file | 4 GB | no limit | no limit |

- **MBR, not GPT.** The Help Center pages for the CDJ-3000, CDJ-3000X, CDJ-2000NXS2, XDJ-XZ,
  XDJ-RX3, XDJ-AZ, OPUS-QUAD and OMNIS-DUO all say the GUID partition map isn't supported (macOS
  Disk Utility uses it by default). Players read the first partition, unless another holds a
  rekordbox library (the NXS2, NXS, XDJ-XZ, OPUS-QUAD and OMNIS-DUO manuals give that one priority).
  The drive gets exactly one, so there's no doubt which. MBR tops out at 2 TiB, so larger drives are
  refused.
- **Cluster size** has no official guidance and no measured effect on loading or browsing. It
  decides how much space small files take: rekordbox writes three analysis files per track, so
  exFAT's default 128 KB clusters use about 0.5 MB per track where `--cluster-size 32K` uses
  about 0.1 MB. locallibrary never makes FAT32 clusters larger than 32 KB: many FAT
  implementations can't handle larger ones.
- **HFS+** is created non-journaled and case-insensitive (case-sensitive HFS+ may not be
  recognised). Users report players mounting it more slowly than FAT32.
- **Players show the Device Name set in rekordbox**, not the volume label.

How each OS does it (only macOS's has run; Linux and Windows are experimental, see
[Platforms](install.md#platforms)):

- **macOS**: `diskutil eraseDisk ... MBR` (no administrator rights needed). A non-default
  `--cluster-size` re-creates the file system with `newfs_msdos`/`newfs_exfat`, which needs `sudo`.
- **Linux** (never run): `sfdisk` and `mkfs.vfat` (dosfstools), `mkfs.exfat` (exfatprogs) or
  `mkfs.hfsplus` (hfsprogs), as root. The new volume isn't mounted: reconnect the drive.
- **Windows** (never run, nor compiled): the Storage cmdlets (`Clear-Disk`, `Initialize-Disk
  -PartitionStyle MBR`, `New-Partition`, `Format-Volume`) from an elevated terminal. Windows won't
  create FAT32 above 32 GB, so locallibrary writes the FAT32 structures itself (the writer is
  checked in tests against an independent FAT implementation, the `fatfs` crate). HFS+ isn't
  available.

Only external or removable, physical, writable disks can be formatted; the system disk, the disk
holding the store (the one `--store` names, even when it can't be opened) and the one holding
locallibrary itself are refused, and so is an archive or retired drive unless `--overwrite-archive`
is given. Under `sudo`, the default store is root's on most systems, so `format` and `provision` ask
for `--store` unless the default one is yours. The plan lists every volume that will be erased, the
drive and role it was, and whether a rekordbox library on it is safely in the store (`locallibrary
pull` it first if not). Just before erasing, the disk is looked up again and must still be the one
planned. On macOS and Windows, a drive locallibrary knew keeps its name and id, so `locallibrary
sync REKORDBOX BACKUP` works right after.

## Doctor

`locallibrary doctor` (or `doctor Home`) reports problems in a library and changes nothing
(`--hash-audio` keeps the hashes it makes in a named drive's `.locallibrary/` file index):
duplicates (several rows for one file, Sync Manager's `Name-1.mp3` copies, the same
artist/title/length, and with `--hash-audio` identical audio) with a suggested keeper, missing or
cut-short audio, missing analysis and artwork, orphaned files, tracks in no playlist, drift between
the two databases, files of 4 GB or more, and how players read the drive. `track dedupe` replaces
the duplicates; `--fix-orphans` deletes audio under `Contents/`, analysis under `PIONEER/USBANLZ/`
and images under `PIONEER/Artwork/` that no track in either database uses, after backing them up.
Files other than audio under `Contents/` are never touched. `--audio` adds audio files that don't
open, are cut short or don't decode at the end (see [Before a set](sets.md#before-a-set)). `--only
duplicates,unfiled` runs just those sections, `--all` lists every group and file instead of the
first few, and `--fix-orphans --dry-run` shows what it would delete.
