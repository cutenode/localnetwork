### Fixed

- `collection export` and `merge` put each track's analysis files in the folder rekordbox gives its
  audio path (`PIONEER/USBANLZ/Pxxx/xxxxxxxx`), not one of their own. An XDJ-RX3 looks for them
  there, so on a drive exported before this it showed no waveforms and refused BEAT SYNC. Export
  such a drive again to move its analysis.
