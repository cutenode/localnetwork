### Changed

- `format` always refuses disk images, loop devices and virtual disks:
  `LOCALLIBRARY_FORMAT_VIRTUAL`, which let it format any of them, is gone. The weekly scheduled
  run tests `format` and `eject` on a disk image it attaches (hdiutil, a loop device, a VHD), and
  only a test build lets that one image through.
