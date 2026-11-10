### Changed

- Writing a library to a drive is faster: on macOS and Linux, a new file goes straight to its name
  instead of through a temp file and a rename, which cost FAT and exFAT as much as making the file.
  Windows keeps the previous temp-file route. Analysis files
  and artwork go several at a time. Audio is still written one file at a time, so each stays in one
  piece on the drive.
- Exporting or packing again when little has changed takes seconds: a plan reads rekordbox's
  analysis files side by side, and building the databases reads the drive's the same way.
- A drive's first scan hashes its analysis files and artwork side by side.
- Copying a big file reads and hashes ahead of the writes, so a drive-to-drive sync no longer waits
  for one drive at a time.
- `collection export` downloads online-only files a few at a time ahead of the copy. A cancelled
  or failed copy stops queued downloads without waiting for an unrelated cloud read to finish.
- Copies on one APFS, Btrfs or XFS volume are copy-on-write, taking no space or writing until one
  side changes: exports to a folder, imports into the store, Serato's tagged copies and the copy
  embedding artwork works on. FAT and exFAT drives can't do this and still get bytes.
- A write notes the new files it's making on the drive, so what a cut-off write left part written
  goes with the next write or `drive tidy`.

### Fixed

- The progress bar counts analysis files as they're written, so it reaches the end.
