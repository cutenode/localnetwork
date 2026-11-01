### Fixed

- A plugin's `net/fetch`, and a set's recording upload, stop sending a file the moment they're
  cancelled. They noticed only up to 50 ms later, and to a server reading quickly that could be
  the whole file.
