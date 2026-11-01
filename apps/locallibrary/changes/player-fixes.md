### Fixed

- The player opens its outputs in the background: the one picked from the outputs list (`d`), and
  the one it plays through at start and on the first play (or after the output went away). The
  player used to freeze until a slow Bluetooth or AirPlay device answered. Picking one, the track
  plays on through the output it was on meanwhile; playing a track, its file opens meanwhile.
- Loops rekordbox marks active play round on their own once playback reaches them, as on a CDJ.
  A track's analysis says which loops are active (`active_loop` on each cue, in the sidecar's
  `Cue` too).
- In a terminal that reports keys by their codes (CSI u, or xterm's modifyOtherKeys), `Q` and the
  player's other capital keys are no longer taken for their lowercase keys. `Q` quit the player
  where it should turn quantize off.
