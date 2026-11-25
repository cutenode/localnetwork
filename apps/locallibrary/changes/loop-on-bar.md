### Fixed

- With quantize on, the player's `l` starts a new loop on the nearest downbeat, so it sits on the
  bar even when pressed a little early or late, rather than on the beat the playhead was in (a
  beat before the bar when pressed just ahead of it).
