### Fixed

- `pack` leaves out a playlist whose folder is missing from the database (a damaged or
  hand-edited export can hold one), and its tracks, unless it was chosen. Before, it was always
  packed.
