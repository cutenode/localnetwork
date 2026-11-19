### Fixed

- A write to a FAT or exFAT drive on macOS no longer leaves a `._` file beside every file it
  wrote (macOS makes one for each new file). It removes those in `PIONEER/` and `Contents/` when
  it finishes, so players don't find a `._ANLZ0000.DAT` beside each track's analysis.
