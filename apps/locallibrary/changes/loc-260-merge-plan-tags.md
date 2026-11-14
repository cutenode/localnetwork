### Fixed

- A merge's plan lists tracks both drives have whose My Tags differ, and tracks only the source's
  OneLibrary has, which merge doesn't bring across. With any of the latter, the merge no longer
  counts as taking in the source, so a `sync` afterwards asks which way instead of removing them
  from the source.
