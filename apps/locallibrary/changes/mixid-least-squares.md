### Fixed

- `mix` takes the speed it searched at for a stretch that's too short to measure once stray matches
  are dropped. It used to measure a slope across what was left, which could shift a play's speed
  and report a pitch ride that never happened.
