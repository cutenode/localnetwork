### Fixed

- A plugin's `net/fetch`, and a set's recording upload (`sets publish --upload`), stop as soon
  as they're cancelled. A request waiting on a server that had stopped reading, or hadn't
  answered yet, went on until its time was up (30 s, and a second more for each 128 KiB it
  sent), holding its file open. A plugin killed for not stopping takes its requests with it at
  once, even when a process it started holds its output open.
