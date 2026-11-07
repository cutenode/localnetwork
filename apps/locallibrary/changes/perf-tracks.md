### Changed

- Listing, searching, sorting and comparing tracks do less work: table rows go out in one write
  rather than one for each track, and text that's ASCII skips its Unicode checks.
